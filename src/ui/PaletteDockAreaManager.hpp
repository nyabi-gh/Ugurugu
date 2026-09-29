// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QByteArray>
#include <QHash>
#include <QList>
#include <QObject>

class QDockWidget;
class QMainWindow;
class QTabBar;

namespace ugurugu
{

// Owns only whether the left and right dock areas are collapsed, plus the
// layout snapshot taken before a collapse. Dock areas, tab groups, tab order,
// dock sizes and per-dock visibility remain owned by QMainWindow's
// saveState/restoreState, including for docks the user has closed.
class PaletteDockAreaManager final : public QObject
{
    Q_OBJECT

public:
    explicit PaletteDockAreaManager(QMainWindow *window, int layoutVersion);

    bool eventFilter(QObject *object, QEvent *event) override;

    void registerDock(QDockWidget *dock);
    bool isCollapsed(Qt::DockWidgetArea area) const;
    bool isDockCollapsed(const QDockWidget *dock) const;
    void setCollapsed(Qt::DockWidgetArea area, bool collapsed);
    void restorePersistedState();
    void resetCollapsedAreas();
    QByteArray layoutStateForPersistence();
    void requestAreaControlsUpdate();

    static PaletteDockAreaManager *find(const QDockWidget *dock);

private:
    struct DockState final
    {
        bool toggleEnabled = true;
    };

    struct AreaState final
    {
        QDockWidget *rail = nullptr;
        QHash<QDockWidget *, DockState> docks;
        bool collapsed = false;
    };

    AreaState &state(Qt::DockWidgetArea area);
    const AreaState &state(Qt::DockWidgetArea area) const;
    QList<QDockWidget *> docksInArea(Qt::DockWidgetArea area) const;
    QList<QTabBar *> dockAreaTabBars() const;
    void scheduleCollapse(Qt::DockWidgetArea area);
    void collapseArea(Qt::DockWidgetArea area, bool persist);
    void restoreExpandedLayout(Qt::DockWidgetArea expandedArea, bool persist);
    void restoreDockActions(AreaState &areaState);
    void updateAreaControls();
    void showRail(Qt::DockWidgetArea area);
    void hideRail(Qt::DockWidgetArea area);
    void captureExpandedLayoutState();
    QDockWidget *createRail(Qt::DockWidgetArea area);

    QMainWindow *m_window;
    int m_layoutVersion;
    QByteArray m_expandedLayoutState;
    QList<QDockWidget *> m_docks;
    QList<Qt::DockWidgetArea> m_pendingCollapsedAreas;
    AreaState m_left;
    AreaState m_right;
};

}
