// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/WobblePreview.hpp"

#include "document/DocumentController.hpp"
#include "ui/Theme.hpp"

#include <QEvent>
#include <QPainter>
#include <QPainterPath>

#include <algorithm>
#include <cmath>
#include <numbers>

namespace ugurugu
{

WobblePreview::WobblePreview(DocumentController *controller, QWidget *parent)
    : QWidget(parent)
    , m_controller(controller)
{
    setFixedSize(sizeHint());
    setAccessibleName(tr("Wobble preview"));
    setToolTip(tr("Live preview of the wobble strength"));

    m_timer.setInterval(120);
    connect(&m_timer,
        &QTimer::timeout,
        this,
        [this]()
        {
            m_phase += 1.0;
            if (currentAmount() > 0.0)
            {
                update();
            }
        });
    m_settleTimer.setSingleShot(true);
    m_settleTimer.setInterval(1000);
    connect(&m_settleTimer,
        &QTimer::timeout,
        this,
        [this]()
        {
            syncAnimationState();
        });
    m_lastAmount = currentAmount();
    connect(m_controller,
        &DocumentController::documentChanged,
        this,
        [this]()
        {
            const qreal amount = currentAmount();
            if (!qFuzzyCompare(amount, m_lastAmount))
            {
                m_lastAmount = amount;
                m_settleTimer.start();
                syncAnimationState();
            }
            update();
        });
}

QSize WobblePreview::sizeHint() const
{
    return QSize(58, 24);
}

bool WobblePreview::isAnimationActive() const
{
    return m_timer.isActive();
}

void WobblePreview::setScopeLayer(const QUuid &layerId)
{
    if (m_scopeLayer == layerId)
    {
        return;
    }
    m_scopeLayer = layerId;
    m_lastAmount = currentAmount();
    update();
}

qreal WobblePreview::currentAmount() const
{
    const Document &document = m_controller->document();
    const Layer *layer = document.layer(m_scopeLayer);
    return layer ? effectiveWobbleAmount(document, *layer)
                 : document.wobbleAmount;
}

void WobblePreview::paintEvent(QPaintEvent *)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);

    const qreal amount = currentAmount();
    const qreal amplitude = std::min(6.5, amount * 0.55);
    const qreal centerY = height() / 2.0;

    QPainterPath line;
    bool started = false;
    for (int sampleX = 5; sampleX <= width() - 5; sampleX += 2)
    {
        const qreal x = sampleX;
        const qreal y =
            centerY
            + amplitude
                  * (0.62 * std::sin(x * 0.16 + m_phase)
                      + 0.38
                            * std::sin(x * 0.37 + m_phase * 1.4
                                       + std::numbers::pi_v<qreal> / 3.0));
        if (!started)
        {
            line.moveTo(x, y);
            started = true;
        }
        else
        {
            line.lineTo(x, y);
        }
    }

    painter.setPen(
        QPen(Theme::accent(), 2.0, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    painter.drawPath(line);
}

void WobblePreview::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    m_shown = true;
    syncAnimationState();
}

void WobblePreview::hideEvent(QHideEvent *event)
{
    QWidget::hideEvent(event);
    m_shown = false;
    syncAnimationState();
}

void WobblePreview::changeEvent(QEvent *event)
{
    QWidget::changeEvent(event);
    if (event->type() == QEvent::EnabledChange)
    {
        syncAnimationState();
    }
}

void WobblePreview::syncAnimationState()
{
    if (m_shown && isEnabled() && m_settleTimer.isActive())
    {
        m_timer.start();
    }
    else
    {
        m_timer.stop();
    }
}

}
