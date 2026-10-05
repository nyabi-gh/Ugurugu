// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QImage>
#include <QWindow>

#include <memory>
#include <rhi/qrhi.h>

namespace ugurugu
{

class CanvasWidget;
class CanvasWidgetTestAccess;

// GPU half of the canvas display: draws the CPU-composed frame image as a
// textured quad under the document transform, with the checker background
// evaluated in the fragment shader, and blends the QPainter overlay (shadow,
// border, outlines, cursor) over it from an image of its own. Frame and
// overlay pixels are re-uploaded only inside the regions the canvas reports
// as changed.
//
// It is a native child window with its own swap chain rather than a
// QRhiWidget. Qt composites a QRhiWidget into the top-level window, and while
// any other widget in that window has something to repaint, Qt repaints and
// re-uploads the whole area under the QRhiWidget along with it; a status bar
// label or an animated button then turned every canvas frame into a
// full-canvas raster pass. This window presents on its own and the widgets
// around it repaint on their own.
//
// It is transparent for mouse input, which the platform hands to the canvas
// widget underneath. Pen, touch and gesture input still reaches this window
// and is forwarded to the canvas widget.
class CanvasDisplayWindow final : public QWindow
{
    Q_OBJECT
    friend class CanvasWidgetTestAccess;

public:
    explicit CanvasDisplayWindow(CanvasWidget *canvas);
    ~CanvasDisplayWindow() override;

    // Renders a frame now and returns the presented pixels.
    QImage grabFramebuffer();

signals:
    void renderFailed();

protected:
    bool event(QEvent *event) override;
    void exposeEvent(QExposeEvent *event) override;

private:
    bool initialize();
    void releaseResources();
    void fail();
    void bindFrameResources();
    void bindOverlayResources();
    void updateOverlay(QRhiResourceUpdateBatch *batch, const QSize &pixelSize);
    bool renderFrame(QRhiReadbackResult *readback = nullptr);

    CanvasWidget *m_canvas;
    bool m_failed = false;
    std::unique_ptr<QRhi> m_rhi;
    std::unique_ptr<QRhiSwapChain> m_swapChain;
    std::unique_ptr<QRhiRenderPassDescriptor> m_renderPass;
    std::unique_ptr<QRhiBuffer> m_vertexBuffer;
    std::unique_ptr<QRhiBuffer> m_uniformBuffer;
    std::unique_ptr<QRhiSampler> m_sampler;
    std::unique_ptr<QRhiTexture> m_frameTexture;
    std::unique_ptr<QRhiShaderResourceBindings> m_bindings;
    std::unique_ptr<QRhiGraphicsPipeline> m_pipeline;
    std::unique_ptr<QRhiBuffer> m_overlayVertexBuffer;
    std::unique_ptr<QRhiTexture> m_overlayTexture;
    std::unique_ptr<QRhiShaderResourceBindings> m_overlayBindings;
    std::unique_ptr<QRhiGraphicsPipeline> m_overlayPipeline;
    // Uploads reference this image's bytes in place; it is only repainted by
    // the next frame, after the one that recorded the upload was submitted.
    QImage m_overlayImage;
};

}
