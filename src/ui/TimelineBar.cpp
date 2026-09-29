// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/TimelineBar.hpp"

#include "document/DocumentController.hpp"
#include "document/DocumentLimits.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/FrameScrubber.hpp"
#include "ui/Icons.hpp"
#include "ui/WobblePlayButton.hpp"

#include <QApplication>
#include <QEvent>
#include <QHBoxLayout>
#include <QLabel>
#include <QSignalBlocker>
#include <QSpinBox>
#include <QToolButton>

#include <algorithm>
#include <cmath>

namespace ugurugu
{

namespace
{

bool isObjectOrDescendantOf(QObject *object, const QObject *ancestor)
{
    while (object)
    {
        if (object == ancestor)
        {
            return true;
        }
        object = object->parent();
    }
    return false;
}

}

TimelineBar::TimelineBar(
    DocumentController *controller, CanvasWidget *canvas, QWidget *parent)
    : QWidget(parent)
    , m_controller(controller)
    , m_canvas(canvas)
{
    setObjectName(QStringLiteral("TimelineBar"));
    buildLayout();
    connectControls();
    syncFromDocument();
    qApp->installEventFilter(this);
}

TimelineBar::~TimelineBar()
{
    if (qApp)
    {
        qApp->removeEventFilter(this);
    }
}

bool TimelineBar::eventFilter(QObject *watched, QEvent *event)
{
    const bool frameControlEvent =
        isObjectOrDescendantOf(watched, m_currentFrameSpin);
    if (frameControlEvent
        && (event->type() == QEvent::FocusIn
            || event->type() == QEvent::MouseButtonPress
            || event->type() == QEvent::Wheel
            || event->type() == QEvent::KeyPress))
    {
        m_canvas->setAnimating(false);
    }
    return QWidget::eventFilter(watched, event);
}

void TimelineBar::buildLayout()
{
    auto *layout = new QHBoxLayout(this);
    layout->setContentsMargins(12, 9, 14, 9);
    layout->setSpacing(10);

    m_playButton = new WobblePlayButton(this);
    m_playButton->setObjectName(QStringLiteral("timelinePlayButton"));
    m_playButton->setToolTip(tr("Play preview (P)"));
    m_playButton->setAccessibleName(tr("Play preview"));
    layout->addWidget(m_playButton);

    m_currentFrameSpin = new QSpinBox(this);
    m_currentFrameSpin->setObjectName(QStringLiteral("currentFrameSpin"));
    m_currentFrameSpin->setRange(1, DocumentLimits::maximumAnimationFrames);
    m_currentFrameSpin->setAccessibleName(tr("Current frame"));
    m_currentFrameSpin->setAlignment(Qt::AlignCenter);
    m_currentFrameSpin->setFixedWidth(46);
    layout->addWidget(m_currentFrameSpin);

    auto *frameSeparator = new QLabel(QStringLiteral("/"), this);
    layout->addWidget(frameSeparator);

    m_framesSpin = new QSpinBox(this);
    m_framesSpin->setObjectName(QStringLiteral("framesSpin"));
    m_framesSpin->setRange(DocumentLimits::minimumAnimationFrames,
        DocumentLimits::maximumAnimationFrames);
    m_framesSpin->setAccessibleName(tr("Animation frames"));
    m_framesSpin->setToolTip(tr("Animation frames"));
    m_framesSpin->setAlignment(Qt::AlignCenter);
    m_framesSpin->setFixedWidth(46);
    layout->addWidget(m_framesSpin);

    layout->addSpacing(4);

    m_scrubber = new FrameScrubber(m_controller, m_canvas, this);
    layout->addWidget(m_scrubber, 1);

    layout->addSpacing(12);

    auto *fpsLabel = new QLabel(tr("FPS"), this);
    fpsLabel->setProperty("fieldLabel", true);
    layout->addWidget(fpsLabel);

    m_fpsSpin = new QSpinBox(this);
    m_fpsSpin->setObjectName(QStringLiteral("fpsSpin"));
    m_fpsSpin->setRange(qRound(DocumentLimits::minimumFramesPerSecond),
        qRound(DocumentLimits::maximumFramesPerSecond));
    m_fpsSpin->setToolTip(tr("Playback speed (frames per second)"));
    m_fpsSpin->setAccessibleName(tr("Playback speed"));
    fpsLabel->setBuddy(m_fpsSpin);
    layout->addWidget(m_fpsSpin);

    m_collapseButton = new QToolButton(this);
    m_collapseButton->setObjectName(
        QStringLiteral("collapseAnimationBarButton"));
    m_collapseButton->setIcon(Icons::icon(IconGlyph::MoveDown));
    m_collapseButton->setText(tr("Hide"));
    m_collapseButton->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    m_collapseButton->setToolTip(tr("Hide animation bar (Ctrl+T)"));
    m_collapseButton->setAccessibleName(tr("Hide animation bar"));
    layout->addWidget(m_collapseButton);
}

void TimelineBar::connectControls()
{
    m_playButton->setChecked(m_canvas->isAnimating());
    connect(m_playButton,
        &QToolButton::toggled,
        m_canvas,
        &CanvasWidget::setAnimating);
    connect(m_canvas,
        &CanvasWidget::animatingChanged,
        m_playButton,
        &WobblePlayButton::setChecked);
    connect(m_collapseButton,
        &QToolButton::clicked,
        this,
        &TimelineBar::collapseRequested);

    connect(m_currentFrameSpin,
        &QSpinBox::valueChanged,
        this,
        [this](int value)
        {
            if (!m_syncing)
            {
                m_canvas->setAnimating(false);
                m_canvas->setCurrentFrame(value - 1);
            }
        });
    connect(m_canvas,
        &CanvasWidget::currentFrameChanged,
        this,
        [this](int frame)
        {
            if (m_canvas->isAnimating())
            {
                return;
            }
            const QSignalBlocker spinBlocker(m_currentFrameSpin);
            m_currentFrameSpin->setValue(frame + 1);
        });
    connect(m_canvas,
        &CanvasWidget::animatingChanged,
        this,
        [this](bool animating)
        {
            if (animating)
            {
                return;
            }
            const QSignalBlocker spinBlocker(m_currentFrameSpin);
            m_currentFrameSpin->setValue(m_canvas->currentFrame() + 1);
        });

    connect(m_framesSpin,
        &QSpinBox::valueChanged,
        this,
        [this](int value)
        {
            if (!m_syncing)
            {
                m_controller->setAnimationFrames(value);
            }
        });
    connect(m_fpsSpin,
        &QSpinBox::valueChanged,
        this,
        [this](int value)
        {
            if (!m_syncing)
            {
                m_controller->setFramesPerSecond(value);
            }
        });

    connect(m_controller,
        &DocumentController::documentChanged,
        this,
        &TimelineBar::syncFromDocument);
}

void TimelineBar::syncFromDocument()
{
    m_syncing = true;
    const Document &document = m_controller->document();
    const int frames = std::max(1, document.animationFrames);

    m_currentFrameSpin->setMaximum(frames);
    if (!m_canvas->isAnimating())
    {
        m_currentFrameSpin->setValue(m_canvas->currentFrame() + 1);
    }

    m_framesSpin->setValue(document.animationFrames);
    m_fpsSpin->setValue(qRound(document.framesPerSecond));
    m_syncing = false;
}

}
