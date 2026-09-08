// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "app/RecoveryWriter.hpp"
#include "document/DocumentController.hpp"
#include "io/ExportWorker.hpp"
#include "io/WawaV10Importer.hpp"

#include <QList>
#include <QMainWindow>
#include <QTimer>
#include <QUuid>

#include <optional>

class QAction;
class QCloseEvent;
class QEvent;
class QLabel;
class QProgressDialog;
class QSlider;
class QSpinBox;

namespace ugurugu
{

class CanvasWidget;
class ColorDock;
class ColorHistoryDock;
class LayerDock;
class ToolDock;
class MainWindowTestAccess;
class PaletteDockAreaManager;
class TimelineBar;
class WobbleDock;

class MainWindow final : public QMainWindow
{
    Q_OBJECT

public:
    enum class StartupResult
    {
        Ready,
        Recovered,
        OpenedRequestedFile,
        Canceled,
        Failed
    };

    explicit MainWindow(QWidget *parent = nullptr);
    bool openFile(const QString &filePath);
    bool offerRecovery();
    StartupResult initializeSession(const QString &requestedFilePath = {});

protected:
    void closeEvent(QCloseEvent *event) override;
    void showEvent(QShowEvent *event) override;
    bool eventFilter(QObject *watched, QEvent *event) override;

private:
    void createActions();
    void createMenus();
    void createToolBars();
    void createStatusBar();
    void connectDocument();
    void restoreDrawingToolSettings();
    void connectDrawingToolSettings();
    void scheduleDrawingToolSettingsSave();
    void saveDrawingToolSettings();
    void updateWindowTitle();
    void resetDockLayout();
    bool hasUnsavedWork() const;
    void refreshUnsavedState();
    bool maybeSave();
    bool save();
    bool saveAs();
    bool saveToFile(const QString &filePath);
    bool rejectReservedRecoveryPath(
        const QString &filePath, const QString &title);
    std::optional<Document> readProject(const QString &filePath);
    bool activateProject(const QString &filePath, Document document);
    bool recoverAutosave();
    bool discardAutosave();
    QString preserveAutosave();
    void clearRecoveryMetadata();
    void handleAutosaveWritten(
        bool success, quint64 revision, const QString &error);
    void reportRecoveryFailure(const QString &reason);
    void clearRecoveryFailure();
    void warnLegacyLayerHierarchy();
    void showShortcutChangeNoticeOnce();
    void writeAutosave();
    bool clearAutosave();
    void newDocument();
    void resizeCanvas();
    void resizeImage();
    void chooseBackgroundColor();
    void pasteFromClipboard();
    void scaleSelection();
    void rotateSelection();
    void editSelectedStrokeProperties();
    void chooseOpenFile();
    void chooseInsertImage();
    void importWwpPreset();
    void exportWwpPreset();
    void showHelp();
    void showAbout();
    void exportGif();
    void exportWebP();
    void exportAnimation(ExportWorker::Kind kind);
    void exportImage();
    void beginExportProgress(
        ExportWorker::Kind kind, const QString &filePath, int maximum);
    void handleExportFinished(ExportWorker::Kind kind,
        bool success,
        bool canceled,
        const QString &filePath,
        const QString &error);
    void updateExportActions();
    void applyWobbleAnimationEnabled(bool enabled);
    void setTimelineVisible(bool visible);
    static bool timelineVisibleSetting();
    QString normalizedPath(
        const QString &filePath, const QString &extension) const;
    QString saveDialogStartPath(const QString &extension) const;

    // Outlives every other member but not the child widgets: Qt deletes those
    // in ~QObject, after all members are gone, and each of them holds the
    // controller by raw pointer. Teardown is safe only because no event loop
    // runs through it - nothing between ~MainWindow and ~QObject may deliver a
    // timer, a paint or a queued completion, since the widgets those reach
    // still dereference the controller.
    DocumentController m_controller;
    CanvasWidget *m_canvas = nullptr;
    TimelineBar *m_timeline = nullptr;
    LayerDock *m_layerDock = nullptr;
    ToolDock *m_toolDock = nullptr;
    ColorDock *m_colorDock = nullptr;
    ColorHistoryDock *m_colorHistoryDock = nullptr;
    WobbleDock *m_wobbleDock = nullptr;
    PaletteDockAreaManager *m_paletteDockAreaManager = nullptr;
    QString m_currentFilePath;
    QString m_suggestedSavePath;
    std::optional<WawaImportSummary> m_pendingWawaImportSummary;
    QAction *m_saveAction = nullptr;
    QAction *m_playAction = nullptr;
    QAction *m_brushAction = nullptr;
    QAction *m_eraserAction = nullptr;
    QAction *m_lassoAction = nullptr;
    QAction *m_wandAction = nullptr;
    QAction *m_bucketAction = nullptr;
    QAction *m_textAction = nullptr;
    QAction *m_eyedropperAction = nullptr;
    QAction *m_showTimelineAction = nullptr;
    QAction *m_fillSelectionAction = nullptr;
    QAction *m_scaleSelectionAction = nullptr;
    QAction *m_rotateSelectionAction = nullptr;
    QAction *m_cutSelectionAction = nullptr;
    QAction *m_copySelectionAction = nullptr;
    QAction *m_pasteAction = nullptr;
    QAction *m_selectAllAction = nullptr;
    QAction *m_invertSelectionAction = nullptr;
    QAction *m_editStrokePropertiesAction = nullptr;
    QAction *m_moveSelectionAction = nullptr;
    QAction *m_flipSelectionHorizontalAction = nullptr;
    QAction *m_flipSelectionVerticalAction = nullptr;
    QAction *m_applySelectionTransformAction = nullptr;
    QAction *m_cancelSelectionTransformAction = nullptr;
    QAction *m_deleteSelectionAction = nullptr;
    QAction *m_deselectSelectionAction = nullptr;
    QAction *m_mirrorCanvasAction = nullptr;
    QList<QAction *> m_shortcutActions;
    QLabel *m_pointerLabel = nullptr;
    QLabel *m_recoveryFailureLabel = nullptr;
    QSlider *m_zoomSlider = nullptr;
    QSpinBox *m_zoomSpin = nullptr;
    QTimer m_autosaveTimer;
    QTimer m_drawingToolSettingsSaveTimer;
    bool m_autosavePending = false;
    bool m_initialFitApplied = false;
    bool m_startupResolved = false;
    bool m_recoveryOwnedBySession = false;
    bool m_recoveryFailureNoticeShown = false;
    QUuid m_recoverySessionId = QUuid::createUuid();
    quint64 m_recoveryRevision = 0;
    quint64 m_submittedRecoveryRevision = 0;
    quint64 m_autosaveEditGeneration = 0;
    quint64 m_submittedEditGeneration = 0;
    RecoveryWriter m_recoveryWriter;
    ExportWorker m_exportWorker;
    QProgressDialog *m_exportProgress = nullptr;

    friend class MainWindowTestAccess;
};

}
