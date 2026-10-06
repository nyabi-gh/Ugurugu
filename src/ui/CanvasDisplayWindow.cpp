// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/CanvasDisplayWindow.hpp"

#include "ui/CanvasViewport.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/Theme.hpp"

#include <QCoreApplication>
#include <QExposeEvent>
#include <QFile>
#include <QPainter>
#include <QPlatformSurfaceEvent>

#include <spdlog/spdlog.h>

#include <cstring>

namespace ugurugu
{

namespace
{

QShader loadShader(const QString &path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
    {
        return {};
    }
    return QShader::fromSerialized(file.readAll());
}

// std140 layout of the "buf" uniform block shared by both shader stages.
struct FrameUniforms
{
    float mvp[16];
    float checkerOrigin[2];
    float checkerSize;
    float frameValid;
};

QSurface::SurfaceType platformSurfaceType()
{
#if defined(Q_OS_WIN)
    return QSurface::Direct3DSurface;
#elif defined(Q_OS_MACOS)
    return QSurface::MetalSurface;
#else
    return QSurface::RasterSurface;
#endif
}

QRhi *createRhi()
{
#if defined(Q_OS_WIN)
    QRhiD3D11InitParams params;
    return QRhi::create(QRhi::D3D11, &params);
#elif defined(Q_OS_MACOS)
    QRhiMetalInitParams params;
    return QRhi::create(QRhi::Metal, &params);
#else
    return nullptr;
#endif
}

// The pipelines keep a pointer to the bindings object they were created with,
// so a new texture is swapped into the same object rather than a new one.
void bindResources(QRhi &rhi,
    std::unique_ptr<QRhiShaderResourceBindings> &bindings,
    std::initializer_list<QRhiShaderResourceBinding> resources)
{
    if (!bindings)
    {
        bindings.reset(rhi.newShaderResourceBindings());
        bindings->setBindings(resources);
        bindings->create();
        return;
    }
    bindings->setBindings(resources);
    bindings->updateResources();
}

}

CanvasDisplayWindow::CanvasDisplayWindow(CanvasWidget *canvas)
    : m_canvas(canvas)
{
    setSurfaceType(platformSurfaceType());
#ifdef Q_OS_MACOS
    // Cocoa drops hover moves over a child window that is transparent for
    // input, so the canvas would only see moves while a button is held.
    setFlags(Qt::WindowDoesNotAcceptFocus);
#else
    setFlags(Qt::WindowTransparentForInput | Qt::WindowDoesNotAcceptFocus);
#endif
}

CanvasDisplayWindow::~CanvasDisplayWindow()
{
    releaseResources();
}

void CanvasDisplayWindow::releaseResources()
{
    m_overlayPipeline.reset();
    m_overlayBindings.reset();
    m_overlayTexture.reset();
    m_overlayVertexBuffer.reset();
    m_overlayImage = {};
    m_pipeline.reset();
    m_bindings.reset();
    m_frameTexture.reset();
    m_sampler.reset();
    m_uniformBuffer.reset();
    m_vertexBuffer.reset();
    m_renderPass.reset();
    m_swapChain.reset();
    m_rhi.reset();
}

void CanvasDisplayWindow::fail()
{
    if (m_failed)
    {
        return;
    }
    m_failed = true;
    releaseResources();
    emit renderFailed();
}

void CanvasDisplayWindow::bindFrameResources()
{
    bindResources(*m_rhi,
        m_bindings,
        {
            QRhiShaderResourceBinding::uniformBuffer(0,
                QRhiShaderResourceBinding::VertexStage
                    | QRhiShaderResourceBinding::FragmentStage,
                m_uniformBuffer.get()),
            QRhiShaderResourceBinding::sampledTexture(1,
                QRhiShaderResourceBinding::FragmentStage,
                m_frameTexture.get(),
                m_sampler.get()),
        });
}

void CanvasDisplayWindow::bindOverlayResources()
{
    bindResources(*m_rhi,
        m_overlayBindings,
        {
            QRhiShaderResourceBinding::uniformBuffer(0,
                QRhiShaderResourceBinding::VertexStage,
                m_uniformBuffer.get()),
            QRhiShaderResourceBinding::sampledTexture(1,
                QRhiShaderResourceBinding::FragmentStage,
                m_overlayTexture.get(),
                m_sampler.get()),
        });
}

bool CanvasDisplayWindow::initialize()
{
    m_rhi.reset(createRhi());
    if (!m_rhi)
    {
        return false;
    }
    spdlog::info("Canvas display: GPU ({})", m_rhi->backendName());

    m_swapChain.reset(m_rhi->newSwapChain());
    m_swapChain->setWindow(this);
    m_renderPass.reset(m_swapChain->newCompatibleRenderPassDescriptor());
    m_swapChain->setRenderPassDescriptor(m_renderPass.get());
    if (!m_swapChain->createOrResize())
    {
        return false;
    }

    m_vertexBuffer.reset(m_rhi->newBuffer(
        QRhiBuffer::Dynamic, QRhiBuffer::VertexBuffer, sizeof(float) * 4 * 4));
    m_vertexBuffer->create();
    m_uniformBuffer.reset(m_rhi->newBuffer(
        QRhiBuffer::Dynamic, QRhiBuffer::UniformBuffer, sizeof(FrameUniforms)));
    m_uniformBuffer->create();
    // Nearest sampling matches the QPainter path, which draws the frame with
    // SmoothPixmapTransform disabled.
    m_sampler.reset(m_rhi->newSampler(QRhiSampler::Nearest,
        QRhiSampler::Nearest,
        QRhiSampler::None,
        QRhiSampler::ClampToEdge,
        QRhiSampler::ClampToEdge));
    m_sampler->create();
    // Placeholder so the bindings stay valid before the first frame arrives;
    // the shader skips sampling while frameValid is zero.
    m_frameTexture.reset(m_rhi->newTexture(QRhiTexture::BGRA8, QSize(1, 1)));
    m_frameTexture->create();
    bindFrameResources();
    m_overlayVertexBuffer.reset(m_rhi->newBuffer(
        QRhiBuffer::Dynamic, QRhiBuffer::VertexBuffer, sizeof(float) * 4 * 4));
    m_overlayVertexBuffer->create();
    m_overlayTexture.reset(m_rhi->newTexture(QRhiTexture::BGRA8, QSize(1, 1)));
    m_overlayTexture->create();
    bindOverlayResources();

    const QShader vertexShader =
        loadShader(QStringLiteral(":/shaders/canvas_frame.vert.qsb"));
    const QShader fragmentShader =
        loadShader(QStringLiteral(":/shaders/canvas_frame.frag.qsb"));
    const QShader overlayVertexShader =
        loadShader(QStringLiteral(":/shaders/canvas_overlay.vert.qsb"));
    const QShader overlayFragmentShader =
        loadShader(QStringLiteral(":/shaders/canvas_overlay.frag.qsb"));
    if (!vertexShader.isValid() || !fragmentShader.isValid()
        || !overlayVertexShader.isValid() || !overlayFragmentShader.isValid())
    {
        return false;
    }

    QRhiVertexInputLayout inputLayout;
    inputLayout.setBindings({{4 * sizeof(float)}});
    inputLayout.setAttributes({
        {0, 0, QRhiVertexInputAttribute::Float2, 0},
        {0, 1, QRhiVertexInputAttribute::Float2, 2 * sizeof(float)},
    });

    m_pipeline.reset(m_rhi->newGraphicsPipeline());
    m_pipeline->setTopology(QRhiGraphicsPipeline::TriangleStrip);
    m_pipeline->setShaderStages({
        {QRhiShaderStage::Vertex, vertexShader},
        {QRhiShaderStage::Fragment, fragmentShader},
    });
    m_pipeline->setVertexInputLayout(inputLayout);
    m_pipeline->setShaderResourceBindings(m_bindings.get());
    m_pipeline->setRenderPassDescriptor(m_renderPass.get());

    m_overlayPipeline.reset(m_rhi->newGraphicsPipeline());
    m_overlayPipeline->setTopology(QRhiGraphicsPipeline::TriangleStrip);
    m_overlayPipeline->setShaderStages({
        {QRhiShaderStage::Vertex, overlayVertexShader},
        {QRhiShaderStage::Fragment, overlayFragmentShader},
    });
    m_overlayPipeline->setVertexInputLayout(inputLayout);
    m_overlayPipeline->setShaderResourceBindings(m_overlayBindings.get());
    m_overlayPipeline->setRenderPassDescriptor(m_renderPass.get());
    // QPainter output is premultiplied.
    QRhiGraphicsPipeline::TargetBlend blend;
    blend.enable = true;
    blend.srcColor = QRhiGraphicsPipeline::One;
    blend.dstColor = QRhiGraphicsPipeline::OneMinusSrcAlpha;
    blend.srcAlpha = QRhiGraphicsPipeline::One;
    blend.dstAlpha = QRhiGraphicsPipeline::OneMinusSrcAlpha;
    m_overlayPipeline->setTargetBlends({blend});

    return m_pipeline->create() && m_overlayPipeline->create();
}

bool CanvasDisplayWindow::event(QEvent *event)
{
    switch (event->type())
    {
    case QEvent::UpdateRequest:
        renderFrame();
        return true;
    case QEvent::PlatformSurface:
        // The swap chain must go before the native window it presents to.
        if (static_cast<QPlatformSurfaceEvent *>(event)->surfaceEventType()
            == QPlatformSurfaceEvent::SurfaceAboutToBeDestroyed)
        {
            releaseResources();
        }
        break;
    case QEvent::TabletPress:
    case QEvent::TabletMove:
    case QEvent::TabletRelease:
    case QEvent::MouseButtonPress:
    case QEvent::MouseButtonRelease:
    case QEvent::MouseButtonDblClick:
    case QEvent::MouseMove:
    case QEvent::Wheel:
    case QEvent::TouchBegin:
    case QEvent::TouchUpdate:
    case QEvent::TouchEnd:
    case QEvent::TouchCancel:
    case QEvent::NativeGesture:
    case QEvent::Enter:
    case QEvent::Leave:
        // The window covers the canvas widget exactly, so the event's local
        // positions are already the widget's.
        QCoreApplication::sendEvent(m_canvas, event);
        return true;
    default:
        break;
    }
    return QWindow::event(event);
}

void CanvasDisplayWindow::exposeEvent(QExposeEvent *event)
{
    Q_UNUSED(event);
    if (isExposed())
    {
        renderFrame();
    }
}

void CanvasDisplayWindow::updateOverlay(
    QRhiResourceUpdateBatch *batch, const QSize &pixelSize)
{
    const qreal ratio = devicePixelRatio();
    QRegion dirty = m_canvas->takeOverlayDirtyRegion();
    if (m_overlayImage.size() != pixelSize
        || m_overlayImage.devicePixelRatio() != ratio)
    {
        m_overlayTexture.reset(
            m_rhi->newTexture(QRhiTexture::BGRA8, pixelSize));
        m_overlayTexture->create();
        bindOverlayResources();
        m_overlayImage = QImage(pixelSize, QImage::Format_ARGB32_Premultiplied);
        m_overlayImage.setDevicePixelRatio(ratio);
        dirty = QRect(QPoint(), size());
    }
    const QRectF dirtyBounds(dirty.boundingRect());
    const QRect pixelBounds =
        QRectF(dirtyBounds.topLeft() * ratio, dirtyBounds.size() * ratio)
            .toAlignedRect()
            .intersected(m_overlayImage.rect());
    if (pixelBounds.isEmpty())
    {
        return;
    }
    // Whole device pixels, so the clear and the clip cover the same pixels
    // the upload sends.
    const QRectF bounds(QPointF(pixelBounds.topLeft()) / ratio,
        QSizeF(pixelBounds.size()) / ratio);
    QPainter painter(&m_overlayImage);
    painter.setClipRect(bounds);
    painter.setCompositionMode(QPainter::CompositionMode_Source);
    painter.fillRect(bounds, Qt::transparent);
    painter.setCompositionMode(QPainter::CompositionMode_SourceOver);
    m_canvas->paintOverlay(painter, QRegion(bounds.toAlignedRect()));
    painter.end();

    const qsizetype stride = m_overlayImage.bytesPerLine();
    const char *base =
        reinterpret_cast<const char *>(m_overlayImage.constBits())
        + qsizetype(pixelBounds.y()) * stride + qsizetype(pixelBounds.x()) * 4;
    const qsizetype length = stride * (pixelBounds.height() - 1)
                             + qsizetype(pixelBounds.width()) * 4;
    QRhiTextureSubresourceUploadDescription upload(
        QByteArray::fromRawData(base, length));
    upload.setDataStride(quint32(stride));
    upload.setSourceSize(pixelBounds.size());
    upload.setDestinationTopLeft(pixelBounds.topLeft());
    batch->uploadTexture(
        m_overlayTexture.get(), QRhiTextureUploadEntry(0, 0, upload));
}

bool CanvasDisplayWindow::renderFrame(QRhiReadbackResult *readback)
{
    if (m_failed || !isExposed())
    {
        return false;
    }
    if (!m_rhi && !initialize())
    {
        fail();
        return false;
    }
    if (m_swapChain->currentPixelSize() != m_swapChain->surfacePixelSize())
    {
        if (m_swapChain->surfacePixelSize().isEmpty())
        {
            return false;
        }
        if (!m_swapChain->createOrResize())
        {
            fail();
            return false;
        }
    }
    QRhi::FrameOpResult begun = m_rhi->beginFrame(m_swapChain.get());
    if (begun == QRhi::FrameOpSwapChainOutOfDate)
    {
        if (!m_swapChain->createOrResize())
        {
            fail();
            return false;
        }
        begun = m_rhi->beginFrame(m_swapChain.get());
    }
    if (begun != QRhi::FrameOpSuccess)
    {
        if (begun == QRhi::FrameOpDeviceLost || begun == QRhi::FrameOpError)
        {
            fail();
        }
        return false;
    }

    QRhiCommandBuffer *cb = m_swapChain->currentFrameCommandBuffer();
    QRhiRenderTarget *target = m_swapChain->currentFrameRenderTarget();
    const QSize outputSize = target->pixelSize();
    const CanvasWidget::DisplayedFrame frame =
        m_canvas->resolveDisplayedFrame();
    QRhiResourceUpdateBatch *batch = m_rhi->nextResourceUpdateBatch();

    // The upload descriptions reference these bytes in place until endFrame
    // below submits them.
    QImage source = frame.image;
    bool frameValid = false;
    if (!source.isNull())
    {
        if (source.format() != QImage::Format_ARGB32_Premultiplied
            && source.format() != QImage::Format_RGB32)
        {
            source =
                source.convertToFormat(QImage::Format_ARGB32_Premultiplied);
        }
        const bool recreate = m_frameTexture->pixelSize() != source.size();
        if (recreate)
        {
            m_frameTexture.reset(
                m_rhi->newTexture(QRhiTexture::BGRA8, source.size()));
            m_frameTexture->create();
            bindFrameResources();
        }
        const QRect uploadBounds =
            (recreate ? source.rect() : frame.dirtyBounds)
                .intersected(source.rect());
        if (!uploadBounds.isEmpty())
        {
            // ARGB32 scanlines are BGRA bytes in memory, so the rows can feed
            // a BGRA8 texture without a per-pixel conversion.
            const qsizetype stride = source.bytesPerLine();
            const char *base =
                reinterpret_cast<const char *>(source.constBits())
                + qsizetype(uploadBounds.y()) * stride
                + qsizetype(uploadBounds.x()) * 4;
            const qsizetype length = stride * (uploadBounds.height() - 1)
                                     + qsizetype(uploadBounds.width()) * 4;
            QRhiTextureSubresourceUploadDescription upload(
                QByteArray::fromRawData(base, length));
            upload.setDataStride(quint32(stride));
            upload.setSourceSize(uploadBounds.size());
            upload.setDestinationTopLeft(uploadBounds.topLeft());
            batch->uploadTexture(
                m_frameTexture.get(), QRhiTextureUploadEntry(0, 0, upload));
        }
        frameValid = true;
    }

    const QTransform transform = m_canvas->documentTransform();
    const QSizeF documentSize(m_canvas->m_controller->document().size);
    const QPointF topLeft = transform.map(QPointF(0.0, 0.0));
    const QPointF topRight = transform.map(QPointF(documentSize.width(), 0.0));
    const QPointF bottomLeft =
        transform.map(QPointF(0.0, documentSize.height()));
    const QPointF bottomRight =
        transform.map(QPointF(documentSize.width(), documentSize.height()));
    const float vertices[16] = {
        float(topLeft.x()),
        float(topLeft.y()),
        0.0f,
        0.0f,
        float(topRight.x()),
        float(topRight.y()),
        1.0f,
        0.0f,
        float(bottomLeft.x()),
        float(bottomLeft.y()),
        0.0f,
        1.0f,
        float(bottomRight.x()),
        float(bottomRight.y()),
        1.0f,
        1.0f,
    };
    batch->updateDynamicBuffer(
        m_vertexBuffer.get(), 0, sizeof(vertices), vertices);
    const float overlayVertices[16] = {
        0.0f,
        0.0f,
        0.0f,
        0.0f,
        float(width()),
        0.0f,
        1.0f,
        0.0f,
        0.0f,
        float(height()),
        0.0f,
        1.0f,
        float(width()),
        float(height()),
        1.0f,
        1.0f,
    };
    batch->updateDynamicBuffer(m_overlayVertexBuffer.get(),
        0,
        sizeof(overlayVertices),
        overlayVertices);
    updateOverlay(batch, outputSize);

    QMatrix4x4 mvp = m_rhi->clipSpaceCorrMatrix();
    mvp.ortho(0.0f, float(width()), float(height()), 0.0f, -1.0f, 1.0f);
    const QRectF canvasRect =
        transform.mapRect(QRectF(QPointF(0.0, 0.0), documentSize));
    FrameUniforms uniforms;
    std::memcpy(uniforms.mvp, mvp.constData(), sizeof(uniforms.mvp));
    uniforms.checkerOrigin[0] = float(canvasRect.left());
    uniforms.checkerOrigin[1] = float(canvasRect.top());
    uniforms.checkerSize = float(canvas_detail::checkerSize);
    uniforms.frameValid = frameValid ? 1.0f : 0.0f;
    batch->updateDynamicBuffer(
        m_uniformBuffer.get(), 0, sizeof(uniforms), &uniforms);

    const QRhiViewport viewport(
        0, 0, float(outputSize.width()), float(outputSize.height()));
    cb->beginPass(target, Theme::canvasBackground(), {1.0f, 0}, batch);
    cb->setGraphicsPipeline(m_pipeline.get());
    cb->setViewport(viewport);
    cb->setShaderResources(m_bindings.get());
    const QRhiCommandBuffer::VertexInput vertexInput(m_vertexBuffer.get(), 0);
    cb->setVertexInput(0, 1, &vertexInput);
    cb->draw(4);
    cb->setGraphicsPipeline(m_overlayPipeline.get());
    cb->setViewport(viewport);
    cb->setShaderResources(m_overlayBindings.get());
    const QRhiCommandBuffer::VertexInput overlayInput(
        m_overlayVertexBuffer.get(), 0);
    cb->setVertexInput(0, 1, &overlayInput);
    cb->draw(4);
    QRhiResourceUpdateBatch *readbackBatch = nullptr;
    if (readback)
    {
        readbackBatch = m_rhi->nextResourceUpdateBatch();
        readbackBatch->readBackTexture(QRhiReadbackDescription(), readback);
    }
    cb->endPass(readbackBatch);
    m_rhi->endFrame(m_swapChain.get());
    return true;
}

QImage CanvasDisplayWindow::grabFramebuffer()
{
    QRhiReadbackResult readback;
    if (!renderFrame(&readback))
    {
        return {};
    }
    m_rhi->finish();
    QImage::Format format = QImage::Format_RGBA8888_Premultiplied;
    if (readback.format == QRhiTexture::BGRA8)
    {
        format = QImage::Format_ARGB32_Premultiplied;
    }
    QImage image(reinterpret_cast<const uchar *>(readback.data.constData()),
        readback.pixelSize.width(),
        readback.pixelSize.height(),
        format);
    image = m_rhi->isYUpInFramebuffer() ? image.flipped() : image.copy();
    image.setDevicePixelRatio(devicePixelRatio());
    return image;
}

}
