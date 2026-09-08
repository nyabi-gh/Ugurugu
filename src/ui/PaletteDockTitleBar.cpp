// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/PaletteDockTitleBar.hpp"

#include "ui/PaletteDockAreaManager.hpp"

#include <QCoreApplication>
#include <QDockWidget>
#include <QHBoxLayout>
#include <QMainWindow>
#include <QMouseEvent>
#include <QPainter>
#include <QToolButton>
#include <QWidget>

namespace ugurugu
{

namespace
{

Qt::DockWidgetArea dockArea(const QDockWidget *dock)
{
    auto *window = dock ? qobject_cast<QMainWindow *>(dock->window()) : nullptr;
    return window ? window->dockWidgetArea(const_cast<QDockWidget *>(dock))
                  : Qt::NoDockWidgetArea;
}

void updateCollapseButton(QDockWidget *dock)
{
    QToolButton *button =
        dock->findChild<QToolButton *>(QStringLiteral("collapsePaletteButton"));
    if (!button)
    {
        return;
    }
    const Qt::DockWidgetArea area = dockArea(dock);
    const QString label =
        QCoreApplication::translate("PaletteDockTitleBar", "Collapse");
    button->setText(area == Qt::LeftDockWidgetArea
                        ? QStringLiteral("‹ %1").arg(label)
                        : QStringLiteral("%1 ›").arg(label));
    button->hide();
    if (PaletteDockAreaManager *manager = PaletteDockAreaManager::find(dock))
    {
        manager->requestAreaControlsUpdate();
    }
}

class CompactPaletteTitleBar final : public QWidget
{
public:
    explicit CompactPaletteTitleBar(QDockWidget *dock)
        : QWidget(dock)
    {
        setObjectName(QStringLiteral("dockedPaletteTitleBar"));
        setFixedHeight(22);
        setAccessibleName(QCoreApplication::translate(
            "PaletteDockTitleBar", "%1 panel drag handle")
                .arg(dock->windowTitle()));

        auto *layout = new QHBoxLayout(this);
        layout->setContentsMargins(22, 1, 2, 1);
        layout->addStretch(1);
        auto *collapseButton = new QToolButton(this);
        collapseButton->setObjectName(QStringLiteral("collapsePaletteButton"));
        collapseButton->setFixedSize(70, 20);
        const QString collapseLabel = QCoreApplication::translate(
            "PaletteDockTitleBar", "Collapse panel dock");
        collapseButton->setToolTip(collapseLabel);
        collapseButton->setAccessibleName(collapseLabel);
        layout->addWidget(collapseButton);
        auto *closeButton = new QToolButton(this);
        closeButton->setObjectName(QStringLiteral("closePaletteButton"));
        closeButton->setText(QStringLiteral("×"));
        closeButton->setFixedSize(20, 20);
        closeButton->setToolTip(
            QCoreApplication::translate("PaletteDockTitleBar", "Close panel"));
        closeButton->setAccessibleName(closeButton->toolTip());
        layout->addWidget(closeButton);
        connect(collapseButton,
            &QToolButton::clicked,
            dock,
            [dock]()
            {
                setPaletteDockCollapsed(dock, true);
            });
        connect(closeButton, &QToolButton::clicked, dock, &QDockWidget::close);
        updateCollapseButton(dock);
    }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing, true);
        painter.setPen(Qt::NoPen);
        painter.setBrush(QColor(135, 139, 146));
        for (int y : {8, 14})
        {
            for (int x : {8, 14})
            {
                painter.drawEllipse(QPoint(x, y), 1, 1);
            }
        }
    }

    void mousePressEvent(QMouseEvent *event) override
    {
        event->ignore();
    }

    void mouseMoveEvent(QMouseEvent *event) override
    {
        event->ignore();
    }

    void mouseReleaseEvent(QMouseEvent *event) override
    {
        event->ignore();
    }

    void mouseDoubleClickEvent(QMouseEvent *event) override
    {
        event->ignore();
    }
};

}

void installCompactPaletteTitleBar(QDockWidget *dock)
{
    auto *dockedTitleBar = new CompactPaletteTitleBar(dock);
    dock->setTitleBarWidget(dockedTitleBar);

    QObject::connect(dock,
        &QDockWidget::topLevelChanged,
        dock,
        [dock, dockedTitleBar](bool floating)
        {
            dock->setTitleBarWidget(floating ? nullptr : dockedTitleBar);
            if (!floating)
            {
                updateCollapseButton(dock);
            }
        });
    QObject::connect(dock,
        &QDockWidget::dockLocationChanged,
        dock,
        [dock](Qt::DockWidgetArea)
        {
            updateCollapseButton(dock);
        });
}

bool isPaletteDockCollapsed(const QDockWidget *dock)
{
    PaletteDockAreaManager *manager = PaletteDockAreaManager::find(dock);
    return manager && manager->isDockCollapsed(dock);
}

void setPaletteDockCollapsed(QDockWidget *dock, bool collapsed)
{
    PaletteDockAreaManager *manager = PaletteDockAreaManager::find(dock);
    if (manager)
    {
        manager->setCollapsed(dockArea(dock), collapsed);
    }
}

}
