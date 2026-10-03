// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/CanvasFrameView.hpp"

#include "ui/CanvasViewport.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/Theme.hpp"

#include <QFile>
#include <QPainter>

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

}

CanvasFrameView::CanvasFrameView(CanvasWidget *canvas)
    : QRhiWidget(canvas)
    , m_canvas(canvas)
{
    setAttribute(Qt::WA_TransparentForMouseEvents);
    setFocusPolicy(Qt::NoFocus);
}

CanvasFrameView::~CanvasFrameView() = default;

void CanvasFrameView::releasePipeline()
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
    m_uploadSourceFrame = {};
}

void CanvasFrameView::rebuildShaderResourceBindings()
{
    m_bindings.reset(m_rhi->newShaderResourceBindings());
    m_bindings->setBindings({
        QRhiShaderResourceBinding::uniformBuffer(0,
            QRhiShaderResourceBinding::VertexStage
                | QRhiShaderResourceBinding::FragmentStage,
            m_uniformBuffer.get()),
        QRhiShaderResourceBinding::sampledTexture(1,
            QRhiShaderResourceBinding::FragmentStage,
            m_frameTexture.get(),
            m_sampler.get()),
    });
    m_bindings->create();
}

void CanvasFrameView::rebuildOverlayShaderResourceBindings()
{
    m_overlayBindings.reset(m_rhi->newShaderResourceBindings());
    m_overlayBindings->setBindings({
        QRhiShaderResourceBinding::uniformBuffer(
            0, QRhiShaderResourceBinding::VertexStage, m_uniformBuffer.get()),
        QRhiShaderResourceBinding::sampledTexture(1,
            QRhiShaderResourceBinding::FragmentStage,
            m_overlayTexture.get(),
            m_sampler.get()),
    });
    m_overlayBindings->create();
}

void CanvasFrameView::initialize(QRhiCommandBuffer *cb)
{
    Q_UNUSED(cb);
    if (m_rhi != rhi())
    {
        releasePipeline();
        m_rhi = rhi();
        if (m_rhi)
        {
            spdlog::info("Canvas display: GPU ({})", m_rhi->backendName());
        }
    }
    if (!m_rhi || m_pipeline)
    {
        return;
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
    rebuildShaderResourceBindings();
    m_overlayVertexBuffer.reset(m_rhi->newBuffer(
        QRhiBuffer::Dynamic, QRhiBuffer::VertexBuffer, sizeof(float) * 4 * 4));
    m_overlayVertexBuffer->create();
    m_overlayTexture.reset(m_rhi->newTexture(QRhiTexture::BGRA8, QSize(1, 1)));
    m_overlayTexture->create();
    rebuildOverlayShaderResourceBindings();

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
        releasePipeline();
        m_rhi = nullptr;
        emit renderFailed();
        return;
    }

    m_pipeline.reset(m_rhi->newGraphicsPipeline());
    m_pipeline->setTopology(QRhiGraphicsPipeline::TriangleStrip);
    m_pipeline->setShaderStages({
        {QRhiShaderStage::Vertex, vertexShader},
        {QRhiShaderStage::Fragment, fragmentShader},
    });
    QRhiVertexInputLayout inputLayout;
    inputLayout.setBindings({{4 * sizeof(float)}});
    inputLayout.setAttributes({
        {0, 0, QRhiVertexInputAttribute::Float2, 0},
        {0, 1, QRhiVertexInputAttribute::Float2, 2 * sizeof(float)},
    });
    m_pipeline->setVertexInputLayout(inputLayout);
    m_pipeline->setShaderResourceBindings(m_bindings.get());
    m_pipeline->setRenderPassDescriptor(renderTarget()->renderPassDescriptor());
    m_pipeline->setSampleCount(renderTarget()->sampleCount());

    m_overlayPipeline.reset(m_rhi->newGraphicsPipeline());
    m_overlayPipeline->setTopology(QRhiGraphicsPipeline::TriangleStrip);
    m_overlayPipeline->setShaderStages({
        {QRhiShaderStage::Vertex, overlayVertexShader},
        {QRhiShaderStage::Fragment, overlayFragmentShader},
    });
    m_overlayPipeline->setVertexInputLayout(inputLayout);
    m_overlayPipeline->setShaderResourceBindings(m_overlayBindings.get());
    m_overlayPipeline->setRenderPassDescriptor(
        renderTarget()->renderPassDescriptor());
    m_overlayPipeline->setSampleCount(renderTarget()->sampleCount());
    // QPainter output is premultiplied.
    QRhiGraphicsPipeline::TargetBlend blend;
    blend.enable = true;
    blend.srcColor = QRhiGraphicsPipeline::One;
    blend.dstColor = QRhiGraphicsPipeline::OneMinusSrcAlpha;
    blend.srcAlpha = QRhiGraphicsPipeline::One;
    blend.dstAlpha = QRhiGraphicsPipeline::OneMinusSrcAlpha;
    m_overlayPipeline->setTargetBlends({blend});

    if (!m_pipeline->create() || !m_overlayPipeline->create())
    {
        releasePipeline();
        m_rhi = nullptr;
        emit renderFailed();
    }
}

void CanvasFrameView::updateOverlay(QRhiResourceUpdateBatch *batch)
{
    const QSize pixelSize = renderTarget()->pixelSize();
    const qreal ratio = devicePixelRatioF();
    QRegion dirty = m_canvas->takeOverlayDirtyRegion();
    if (m_overlayImage.size() != pixelSize
        || m_overlayImage.devicePixelRatio() != ratio)
    {
        m_overlayTexture.reset(
            m_rhi->newTexture(QRhiTexture::BGRA8, pixelSize));
        m_overlayTexture->create();
        rebuildOverlayShaderResourceBindings();
        m_overlayImage = QImage(pixelSize, QImage::Format_ARGB32_Premultiplied);
        m_overlayImage.setDevicePixelRatio(ratio);
        dirty = rect();
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

void CanvasFrameView::render(QRhiCommandBuffer *cb)
{
    if (!m_rhi || !m_pipeline)
    {
        return;
    }

    const CanvasWidget::DisplayedFrame frame =
        m_canvas->resolveDisplayedFrame();
    QRhiResourceUpdateBatch *batch = m_rhi->nextResourceUpdateBatch();

    bool frameValid = false;
    if (!frame.image.isNull())
    {
        QImage source = frame.image;
        if (source.format() != QImage::Format_ARGB32_Premultiplied
            && source.format() != QImage::Format_RGB32)
        {
            source =
                source.convertToFormat(QImage::Format_ARGB32_Premultiplied);
        }
        const bool recreate =
            !m_frameTexture || m_frameTexture->pixelSize() != source.size();
        if (recreate)
        {
            m_frameTexture.reset(
                m_rhi->newTexture(QRhiTexture::BGRA8, source.size()));
            m_frameTexture->create();
            rebuildShaderResourceBindings();
        }
        const QRect uploadBounds =
            (recreate ? source.rect() : frame.dirtyBounds)
                .intersected(source.rect());
        if (!uploadBounds.isEmpty())
        {
            // ARGB32 scanlines are BGRA bytes in memory, so the rows can feed
            // a BGRA8 texture without a per-pixel conversion. The description
            // references the image bytes in place; m_uploadSourceFrame keeps
            // them alive until the batch is committed in beginPass below.
            m_uploadSourceFrame = source;
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
    updateOverlay(batch);

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

    cb->beginPass(renderTarget(), Theme::canvasBackground(), {1.0f, 0}, batch);
    cb->setGraphicsPipeline(m_pipeline.get());
    const QSize outputSize = renderTarget()->pixelSize();
    cb->setViewport(QRhiViewport(
        0, 0, float(outputSize.width()), float(outputSize.height())));
    // Passed explicitly: recreating the frame texture replaces the bindings
    // object, while the pipeline still holds the one it was created with.
    cb->setShaderResources(m_bindings.get());
    const QRhiCommandBuffer::VertexInput vertexInput(m_vertexBuffer.get(), 0);
    cb->setVertexInput(0, 1, &vertexInput);
    cb->draw(4);
    cb->setGraphicsPipeline(m_overlayPipeline.get());
    cb->setViewport(QRhiViewport(
        0, 0, float(outputSize.width()), float(outputSize.height())));
    cb->setShaderResources(m_overlayBindings.get());
    const QRhiCommandBuffer::VertexInput overlayInput(
        m_overlayVertexBuffer.get(), 0);
    cb->setVertexInput(0, 1, &overlayInput);
    cb->draw(4);
    cb->endPass();
    // Dropped now that beginPass has committed the batch: holding this
    // reference across frames would force the canvas to deep-copy the
    // composed preview on its next in-place patch.
    m_uploadSourceFrame = {};
}

}
