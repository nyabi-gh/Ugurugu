// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QImage>
#include <QRhiWidget>

#include <memory>
#include <rhi/qrhi.h>

namespace ugurugu
{

class CanvasWidget;

// GPU half of the canvas display: draws the CPU-composed frame image as a
// textured quad under the document transform, with the checker background
// evaluated in the fragment shader. Pan, zoom and playback blits therefore
// cost a texture draw instead of a full-viewport CPU resample; frame pixels
// are re-uploaded only inside the region CanvasWidget::resolveDisplayedFrame
// reports as changed. Overlays are drawn by CanvasOverlayView on top.
class CanvasFrameView final : public QRhiWidget
{
    Q_OBJECT

public:
    explicit CanvasFrameView(CanvasWidget *canvas);
    ~CanvasFrameView() override;

protected:
    void initialize(QRhiCommandBuffer *cb) override;
    void render(QRhiCommandBuffer *cb) override;

private:
    void releasePipeline();
    void rebuildShaderResourceBindings();

    CanvasWidget *m_canvas;
    QRhi *m_rhi = nullptr;
    std::unique_ptr<QRhiBuffer> m_vertexBuffer;
    std::unique_ptr<QRhiBuffer> m_uniformBuffer;
    std::unique_ptr<QRhiSampler> m_sampler;
    std::unique_ptr<QRhiTexture> m_frameTexture;
    std::unique_ptr<QRhiShaderResourceBindings> m_bindings;
    std::unique_ptr<QRhiGraphicsPipeline> m_pipeline;
    // Upload descriptions reference these bytes without copying; the frame
    // that recorded the upload has committed it by the time the next render
    // replaces the reference.
    QImage m_uploadSourceFrame;
};

}
