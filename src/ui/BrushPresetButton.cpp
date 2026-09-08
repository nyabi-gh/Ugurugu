// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/BrushPresetButton.hpp"

#include "brush/BrushPreset.hpp"
#include "render/RenderEngine.hpp"
#include "ui/Theme.hpp"

#include <QEvent>
#include <QFontMetrics>
#include <QPainter>
#include <QPainterPath>

#include <algorithm>
#include <cmath>
#include <numbers>

namespace ugurugu
{

namespace
{

constexpr int cardWidth = 132;
constexpr int previewWidth = 116;
constexpr int previewHeight = 28;
constexpr int previewTop = 3;
constexpr int previewGap = 2;
constexpr int nameBandFloor = 16;
constexpr int cardBottomPadding = 3;
constexpr qreal previewWobbleAmount = 2.2;

QFont nameFont(const QFont &base)
{
    return Theme::scaledFont(base, Theme::TextRole::Label);
}

int nameBandHeight(const QFont &base)
{
    return std::max(nameBandFloor, QFontMetrics(nameFont(base)).height());
}

int cardHeight(const QFont &base)
{
    return previewTop + previewHeight + previewGap + nameBandHeight(base)
           + cardBottomPadding;
}

// The preview is rendered straight into the pixels the screen has, so the
// stroke geometry is built at that same scale rather than at a fixed factor
// that only lines up on a doubled display.
Stroke previewStroke(
    const BrushPreset &preset, qreal scale, bool tabletPressureEnabled)
{
    Stroke stroke;
    stroke.seed = qHash(preset.id);
    stroke.color = Theme::textPrimary();
    stroke.width = std::min(preset.defaultSize, 14.0) * scale;
    stroke.brush = preset.settings;

    constexpr int sampleCount = 26;
    stroke.points.reserve(sampleCount);
    for (int index = 0; index < sampleCount; ++index)
    {
        const qreal t = static_cast<qreal>(index) / (sampleCount - 1);
        StrokePoint point;
        point.position = QPointF((10.0 + t * (previewWidth - 20.0)) * scale,
            (previewHeight * 0.5 - std::sin(t * std::numbers::pi * 1.5) * 4.0)
                * scale);
        point.pressure = tabletPressureEnabled
                             ? 0.2 + 0.8 * std::sin(t * std::numbers::pi)
                             : 1.0;
        stroke.points.append(point);
    }
    return stroke;
}

}

BrushPresetButton::BrushPresetButton(const BrushPreset &preset, QWidget *parent)
    : QAbstractButton(parent)
    , m_preset(&preset)
{
    setCheckable(true);
    setCursor(Qt::PointingHandCursor);
    const QString displayName = BrushPresetCatalog::displayName(preset);
    setToolTip(displayName);
    setAccessibleName(displayName);
    setAttribute(Qt::WA_Hover);
    m_frames.resize(previewFrameCount);
}

QString BrushPresetButton::presetId() const
{
    return m_preset->id;
}

void BrushPresetButton::setPreviewFrame(int frame)
{
    const int normalized = frame % previewFrameCount;
    if (m_frame == normalized)
    {
        return;
    }
    m_frame = normalized;
    update();
}

void BrushPresetButton::setTabletPressureEnabled(bool enabled)
{
    if (m_tabletPressureEnabled == enabled)
    {
        return;
    }
    m_tabletPressureEnabled = enabled;
    m_frames.fill(QImage());
    update();
}

QSize BrushPresetButton::sizeHint() const
{
    return QSize(cardWidth, cardHeight(font()));
}

bool BrushPresetButton::event(QEvent *event)
{
    // Cached previews hold pixels for one screen scale. Moving the window to a
    // display with another one has to throw them away, or the cards keep
    // showing a resampled copy of the old rendering.
    if (event->type() == QEvent::DevicePixelRatioChange)
    {
        m_frames.fill(QImage());
        update();
    }
    return QAbstractButton::event(event);
}

void BrushPresetButton::paintEvent(QPaintEvent *event)
{
    Q_UNUSED(event);
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing);

    const QRectF frame = QRectF(rect()).adjusted(0.75, 0.75, -0.75, -0.75);
    QPainterPath card;
    card.addRoundedRect(frame, 8.0, 8.0);
    QColor background =
        isChecked() ? Theme::hoverBackground() : Theme::canvasBackground();
    if (underMouse() && !isChecked())
    {
        background = Theme::hoverBackground();
    }
    painter.fillPath(card, background);
    painter.setPen(
        isChecked() ? QPen(Theme::accent(), 1.5) : QPen(Theme::border(), 1.0));
    painter.drawPath(card);

    const int paintedPreviewWidth =
        std::min(previewWidth, std::max(1, width() - 16));
    painter.setRenderHint(
        QPainter::SmoothPixmapTransform, paintedPreviewWidth != previewWidth);
    painter.drawImage(QRectF((width() - paintedPreviewWidth) * 0.5,
                          previewTop,
                          paintedPreviewWidth,
                          previewHeight),
        frameImage(m_frame));

    const int nameHeight = nameBandHeight(font());
    painter.setFont(nameFont(font()));
    painter.setPen(isChecked() ? Theme::textPrimary() : Theme::textMuted());
    const QString name = painter.fontMetrics().elidedText(
        BrushPresetCatalog::displayName(*m_preset),
        Qt::ElideRight,
        width() - 12);
    painter.drawText(QRectF(6.0,
                         height() - cardBottomPadding - nameHeight,
                         width() - 12.0,
                         nameHeight),
        Qt::AlignHCenter | Qt::AlignVCenter,
        name);
}

const QImage &BrushPresetButton::frameImage(int frame)
{
    QImage &cached = m_frames[frame];
    if (!cached.isNull())
    {
        return cached;
    }

    const qreal scale = devicePixelRatioF();
    Document document;
    document.size =
        QSize(qRound(previewWidth * scale), qRound(previewHeight * scale));
    document.background = QColor(0, 0, 0, 0);
    document.animationFrames = previewFrameCount;
    document.wobbleAmount = previewWobbleAmount;

    Layer layer;
    layer.strokes.append(
        previewStroke(*m_preset, scale, m_tabletPressureEnabled));
    document.layers.append(layer);

    cached = RenderEngine::render(document, frame);
    cached.setDevicePixelRatio(scale);
    return cached;
}

}
