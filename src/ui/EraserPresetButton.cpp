// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/EraserPresetButton.hpp"

#include "brush/EraserPreset.hpp"
#include "render/RenderEngine.hpp"
#include "ui/Theme.hpp"

#include <QEvent>
#include <QFontMetrics>
#include <QPainter>
#include <QPainterPath>

#include <algorithm>
#include <utility>

namespace ugurugu
{

namespace
{

constexpr int cardWidth = 86;
constexpr int previewWidth = 70;
constexpr int previewHeight = 36;
constexpr int previewTop = 5;
constexpr int previewGap = 3;
constexpr int nameBandFloor = 16;
constexpr int cardBottomPadding = 4;

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

// Built at the screen's own scale, so the rendered preview lands on whole
// device pixels instead of being resampled from a fixed doubled rendering.
Stroke previewBackground(qreal scale)
{
    Stroke stroke;
    stroke.color = Theme::textPrimary();
    stroke.width = 20.0 * scale;
    stroke.brush.sizeDynamics = 0.0;
    stroke.brush.antialiasing = true;
    stroke.points = {
        {QPointF(4.0, 18.0) * scale, 1.0}, {QPointF(66.0, 18.0) * scale, 1.0}};
    return stroke;
}

Stroke previewErase(const EraserPreset &preset, qreal scale)
{
    Stroke stroke;
    stroke.seed = qHash(preset.id);
    stroke.mode = StrokeMode::Erase;
    stroke.width = 16.0 * scale;
    stroke.brush = preset.settings;
    constexpr int sampleCount = 18;
    stroke.points.reserve(sampleCount);
    for (int index = 0; index < sampleCount; ++index)
    {
        const qreal progress = static_cast<qreal>(index) / (sampleCount - 1);
        stroke.points.append(
            {QPointF(26.0 + progress * 18.0, 4.0 + progress * 28.0) * scale,
                1.0});
    }
    return stroke;
}

}

EraserPresetButton::EraserPresetButton(
    const EraserPreset &preset, QWidget *parent)
    : QAbstractButton(parent)
    , m_preset(&preset)
{
    setCheckable(true);
    setCursor(Qt::PointingHandCursor);
    const QString displayName = EraserPresetCatalog::displayName(preset);
    setToolTip(displayName);
    setAccessibleName(displayName);
    setAttribute(Qt::WA_Hover);
}

QString EraserPresetButton::presetId() const
{
    return m_preset->id;
}

QSize EraserPresetButton::sizeHint() const
{
    return QSize(cardWidth, cardHeight(font()));
}

bool EraserPresetButton::event(QEvent *event)
{
    // The cached preview holds pixels for one screen scale, so a move to a
    // display with another one has to discard it.
    if (event->type() == QEvent::DevicePixelRatioChange)
    {
        m_preview = QImage();
        update();
    }
    return QAbstractButton::event(event);
}

void EraserPresetButton::paintEvent(QPaintEvent *event)
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

    painter.drawImage(
        QPointF((width() - previewWidth) * 0.5, previewTop), previewImage());

    const int nameHeight = nameBandHeight(font());
    painter.setFont(nameFont(font()));
    painter.setPen(isChecked() ? Theme::textPrimary() : Theme::textMuted());
    const QString name = painter.fontMetrics().elidedText(
        EraserPresetCatalog::displayName(*m_preset),
        Qt::ElideRight,
        width() - 10);
    painter.drawText(QRectF(5.0,
                         height() - cardBottomPadding - nameHeight,
                         width() - 10.0,
                         nameHeight),
        Qt::AlignHCenter | Qt::AlignVCenter,
        name);
}

const QImage &EraserPresetButton::previewImage()
{
    if (!m_preview.isNull())
    {
        return m_preview;
    }
    const qreal scale = devicePixelRatioF();
    Document document;
    document.size =
        QSize(qRound(previewWidth * scale), qRound(previewHeight * scale));
    document.background = Qt::transparent;
    document.wobbleAmount = 0.0;
    Layer layer;
    layer.strokes = {previewBackground(scale), previewErase(*m_preset, scale)};
    document.layers.append(std::move(layer));
    m_preview = RenderEngine::render(document, 0);
    m_preview.setDevicePixelRatio(scale);
    return m_preview;
}

}
