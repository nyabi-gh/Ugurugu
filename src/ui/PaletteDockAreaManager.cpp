// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/PaletteDockAreaManager.hpp"

#include <QCoreApplication>
#include <QDockWidget>
#include <QEvent>
#include <QMainWindow>
#include <QSet>
#include <QSettings>
#include <QTabBar>
#include <QTimer>
#include <QToolButton>
#include <QVBoxLayout>
#include <QWidget>

#include <limits>

namespace ugurugu
{

namespace
{

constexpr int railWidth = 42;

bool isManagedArea(Qt::DockWidgetArea area)
{
    return area == Qt::LeftDockWidgetArea || area == Qt::RightDockWidgetArea;
}

QString collapsedSettingKey(Qt::DockWidgetArea area)
{
    return area == Qt::LeftDockWidgetArea
               ? QStringLiteral("dock/v4/leftAreaCollapsed")
               : QStringLiteral("dock/v4/rightAreaCollapsed");
}

QString railObjectName(Qt::DockWidgetArea area)
{
    return area == Qt::LeftDockWidgetArea
               ? QStringLiteral("LeftPaletteRailDock")
               : QStringLiteral("RightPaletteRailDock");
}

QString expandButtonObjectName(Qt::DockWidgetArea area)
{
    return area == Qt::LeftDockWidgetArea
               ? QStringLiteral("expandLeftPaletteAreaButton")
               : QStringLiteral("expandRightPaletteAreaButton");
}

QToolButton *collapseButton(const QDockWidget *dock)
{
    return dock->findChild<QToolButton *>(
        QStringLiteral("collapsePaletteButton"));
}

}

PaletteDockAreaManager::PaletteDockAreaManager(
    QMainWindow *window, int layoutVersion)
    : QObject(window)
    , m_window(window)
    , m_layoutVersion(layoutVersion)
{
    setObjectName(QStringLiteral("PaletteDockAreaManager"));
    m_left.rail = createRail(Qt::LeftDockWidgetArea);
    m_right.rail = createRail(Qt::RightDockWidgetArea);
    m_expandedLayoutState = m_window->saveState(m_layoutVersion);
}

void PaletteDockAreaManager::registerDock(QDockWidget *dock)
{
    if (!dock || m_docks.contains(dock))
    {
        return;
    }
    m_docks.append(dock);
    connect(dock,
        &QObject::destroyed,
        this,
        [this, dock]()
        {
            m_docks.removeAll(dock);
            m_left.docks.remove(dock);
            m_right.docks.remove(dock);
        });
    const auto updateControls = [this]()
    {
        requestAreaControlsUpdate();
    };
    connect(dock,
        &QDockWidget::dockLocationChanged,
        this,
        [updateControls](Qt::DockWidgetArea)
        {
            updateControls();
        });
    connect(dock,
        &QDockWidget::topLevelChanged,
        this,
        [updateControls](bool)
        {
            updateControls();
        });
    connect(dock,
        &QDockWidget::visibilityChanged,
        this,
        [updateControls](bool)
        {
            updateControls();
        });
    requestAreaControlsUpdate();
}

bool PaletteDockAreaManager::isCollapsed(Qt::DockWidgetArea area) const
{
    return isManagedArea(area) && state(area).collapsed;
}

bool PaletteDockAreaManager::isDockCollapsed(const QDockWidget *dock) const
{
    if (!dock)
    {
        return false;
    }
    auto *mutableDock = const_cast<QDockWidget *>(dock);
    return (m_left.collapsed && m_left.docks.contains(mutableDock))
           || (m_right.collapsed && m_right.docks.contains(mutableDock));
}

void PaletteDockAreaManager::setCollapsed(
    Qt::DockWidgetArea area, bool collapsed)
{
    if (!isManagedArea(area) || state(area).collapsed == collapsed)
    {
        return;
    }
    if (collapsed)
    {
        captureExpandedLayoutState();
        collapseArea(area, true);
    }
    else
    {
        restoreExpandedLayout(area, true);
    }
}

void PaletteDockAreaManager::restorePersistedState()
{
    captureExpandedLayoutState();
    const QSettings settings;
    for (Qt::DockWidgetArea area :
        {Qt::LeftDockWidgetArea, Qt::RightDockWidgetArea})
    {
        if (!settings.value(collapsedSettingKey(area), false).toBool())
        {
            continue;
        }
        if (m_window->isVisible())
        {
            scheduleCollapse(area);
            continue;
        }
        // Removing a dock widget between restoreState() and the first show
        // corrupts the dock layout in Qt 6.11 and the show then crashes in
        // QDockAreaLayoutItem::skip(). The docks are only hidden here so the
        // area does not flash open; the collapse itself waits for the first
        // show event.
        m_pendingCollapsedAreas.append(area);
        for (QDockWidget *dock : docksInArea(area))
        {
            dock->hide();
        }
        m_window->installEventFilter(this);
    }
}

bool PaletteDockAreaManager::eventFilter(QObject *object, QEvent *event)
{
    if (object == m_window && event->type() == QEvent::Show)
    {
        m_window->removeEventFilter(this);
        const QList<Qt::DockWidgetArea> pendingAreas = m_pendingCollapsedAreas;
        m_pendingCollapsedAreas.clear();
        for (const Qt::DockWidgetArea area : pendingAreas)
        {
            scheduleCollapse(area);
        }
    }
    return QObject::eventFilter(object, event);
}

void PaletteDockAreaManager::scheduleCollapse(Qt::DockWidgetArea area)
{
    QTimer::singleShot(0,
        this,
        [this, area]()
        {
            if (!isCollapsed(area))
            {
                collapseArea(area, false);
            }
        });
}

void PaletteDockAreaManager::resetCollapsedAreas()
{
    for (Qt::DockWidgetArea area :
        {Qt::LeftDockWidgetArea, Qt::RightDockWidgetArea})
    {
        if (state(area).collapsed)
        {
            restoreExpandedLayout(area, false);
        }
        QSettings().setValue(collapsedSettingKey(area), false);
    }
}

QByteArray PaletteDockAreaManager::layoutStateForPersistence()
{
    captureExpandedLayoutState();
    return m_expandedLayoutState;
}

void PaletteDockAreaManager::requestAreaControlsUpdate()
{
    QTimer::singleShot(0, this, &PaletteDockAreaManager::updateAreaControls);
}

PaletteDockAreaManager *PaletteDockAreaManager::find(const QDockWidget *dock)
{
    if (!dock)
    {
        return nullptr;
    }
    auto *window = qobject_cast<QMainWindow *>(dock->window());
    return window ? window->findChild<PaletteDockAreaManager *>(
                        QStringLiteral("PaletteDockAreaManager"),
                        Qt::FindDirectChildrenOnly)
                  : nullptr;
}

PaletteDockAreaManager::AreaState &PaletteDockAreaManager::state(
    Qt::DockWidgetArea area)
{
    return area == Qt::LeftDockWidgetArea ? m_left : m_right;
}

const PaletteDockAreaManager::AreaState &PaletteDockAreaManager::state(
    Qt::DockWidgetArea area) const
{
    return area == Qt::LeftDockWidgetArea ? m_left : m_right;
}

QList<QDockWidget *> PaletteDockAreaManager::docksInArea(
    Qt::DockWidgetArea area) const
{
    QList<QDockWidget *> result;
    for (QDockWidget *dock : m_docks)
    {
        if (!dock->isFloating() && m_window->dockWidgetArea(dock) == area)
        {
            result.append(dock);
        }
    }
    return result;
}

QList<QTabBar *> PaletteDockAreaManager::dockAreaTabBars() const
{
    QList<QTabBar *> result;
    for (QTabBar *tabBar : m_window->findChildren<QTabBar *>())
    {
        // Skips tab bars belonging to dialogs and to floating dock groups,
        // which are separate windows parented to the main window.
        if (tabBar->window() == m_window)
        {
            result.append(tabBar);
        }
    }
    return result;
}

void PaletteDockAreaManager::collapseArea(Qt::DockWidgetArea area, bool persist)
{
    AreaState &areaState = state(area);
    areaState.docks.clear();
    areaState.collapsed = true;
    for (QDockWidget *dock : docksInArea(area))
    {
        areaState.docks.insert(dock, {dock->toggleViewAction()->isEnabled()});
        dock->hide();
        m_window->removeDockWidget(dock);
        dock->toggleViewAction()->setEnabled(false);
    }
    if (persist)
    {
        QSettings().setValue(collapsedSettingKey(area), true);
    }
    showRail(area);
    requestAreaControlsUpdate();
}

void PaletteDockAreaManager::restoreExpandedLayout(
    Qt::DockWidgetArea expandedArea, bool persist)
{
    const bool keepLeftCollapsed =
        m_left.collapsed && expandedArea != Qt::LeftDockWidgetArea;
    const bool keepRightCollapsed =
        m_right.collapsed && expandedArea != Qt::RightDockWidgetArea;
    hideRail(Qt::LeftDockWidgetArea);
    hideRail(Qt::RightDockWidgetArea);
    restoreDockActions(m_left);
    restoreDockActions(m_right);
    m_left.docks.clear();
    m_right.docks.clear();
    m_left.collapsed = false;
    m_right.collapsed = false;
    if (!m_expandedLayoutState.isEmpty())
    {
        m_window->restoreState(m_expandedLayoutState, m_layoutVersion);
    }
    if (persist)
    {
        QSettings().setValue(collapsedSettingKey(expandedArea), false);
    }
    if (keepLeftCollapsed)
    {
        collapseArea(Qt::LeftDockWidgetArea, false);
    }
    if (keepRightCollapsed)
    {
        collapseArea(Qt::RightDockWidgetArea, false);
    }
    requestAreaControlsUpdate();
}

void PaletteDockAreaManager::restoreDockActions(AreaState &areaState)
{
    for (auto iterator = areaState.docks.cbegin();
        iterator != areaState.docks.cend();
        ++iterator)
    {
        iterator.key()->toggleViewAction()->setEnabled(
            iterator.value().toggleEnabled);
    }
}

void PaletteDockAreaManager::updateAreaControls()
{
    for (QDockWidget *dock : m_docks)
    {
        if (QToolButton *button = collapseButton(dock))
        {
            button->hide();
        }
    }

    QSet<QString> selectedDockTitles;
    for (QTabBar *tabBar : dockAreaTabBars())
    {
        if (!tabBar->property("paletteControlsConnected").toBool())
        {
            connect(tabBar,
                &QTabBar::currentChanged,
                this,
                [this](int)
                {
                    requestAreaControlsUpdate();
                });
            tabBar->setProperty("paletteControlsConnected", true);
        }
        if (tabBar->currentIndex() >= 0)
        {
            selectedDockTitles.insert(tabBar->tabText(tabBar->currentIndex()));
        }
    }

    for (Qt::DockWidgetArea area :
        {Qt::LeftDockWidgetArea, Qt::RightDockWidgetArea})
    {
        QDockWidget *topDock = nullptr;
        int top = std::numeric_limits<int>::max();
        for (QDockWidget *dock : docksInArea(area))
        {
            const bool tabified =
                !m_window->tabifiedDockWidgets(dock).isEmpty();
            if (dock->isHidden() || !dock->titleBarWidget()
                || (tabified
                    && !selectedDockTitles.contains(dock->windowTitle())))
            {
                continue;
            }
            const int dockTop = dock->mapTo(m_window, QPoint()).y();
            if (dockTop < top)
            {
                top = dockTop;
                topDock = dock;
            }
        }
        if (topDock)
        {
            if (QToolButton *button = collapseButton(topDock))
            {
                button->show();
            }
        }
    }
}

void PaletteDockAreaManager::showRail(Qt::DockWidgetArea area)
{
    AreaState &areaState = state(area);
    m_window->addDockWidget(area, areaState.rail);
    areaState.rail->show();
}

void PaletteDockAreaManager::hideRail(Qt::DockWidgetArea area)
{
    AreaState &areaState = state(area);
    areaState.rail->hide();
    m_window->removeDockWidget(areaState.rail);
}

void PaletteDockAreaManager::captureExpandedLayoutState()
{
    if (!m_left.collapsed && !m_right.collapsed)
    {
        m_expandedLayoutState = m_window->saveState(m_layoutVersion);
    }
}

QDockWidget *PaletteDockAreaManager::createRail(Qt::DockWidgetArea area)
{
    auto *rail = new QDockWidget(m_window);
    rail->setObjectName(railObjectName(area));
    rail->setAllowedAreas(area);
    rail->setFeatures(QDockWidget::NoDockWidgetFeatures);
    rail->setMinimumWidth(railWidth);
    rail->setMaximumWidth(railWidth);
    rail->hide();
    auto *titleBar = new QWidget(rail);
    titleBar->setFixedHeight(0);
    rail->setTitleBarWidget(titleBar);

    auto *body = new QWidget(rail);
    auto *layout = new QVBoxLayout(body);
    layout->setContentsMargins(5, 8, 5, 8);
    auto *expandButton = new QToolButton(body);
    expandButton->setObjectName(expandButtonObjectName(area));
    expandButton->setText(area == Qt::LeftDockWidgetArea ? QStringLiteral("›")
                                                         : QStringLiteral("‹"));
    expandButton->setFixedSize(30, 30);
    const QString label =
        QCoreApplication::translate("PaletteDockTitleBar", "Expand panel dock");
    expandButton->setToolTip(label);
    expandButton->setAccessibleName(label);
    layout->addWidget(expandButton, 0, Qt::AlignTop | Qt::AlignHCenter);
    layout->addStretch(1);
    rail->setWidget(body);
    connect(expandButton,
        &QToolButton::clicked,
        this,
        [this, area]()
        {
            setCollapsed(area, false);
        });
    return rail;
}

}
