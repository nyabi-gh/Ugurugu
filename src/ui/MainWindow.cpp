// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/MainWindow.hpp"

#include "app/RecoveryStore.hpp"
#include "brush/BrushPreset.hpp"
#include "brush/EraserPreset.hpp"
#include "document/DocumentLimits.hpp"
#include "document/LayerHierarchy.hpp"
#include "io/AnimationExportPolicy.hpp"
#include "io/DocumentSerializer.hpp"
#include "io/RenderExportPolicy.hpp"
#include "io/SelectionClipboardCodec.hpp"
#include "io/WawaV10Importer.hpp"
#include "render/RenderEngine.hpp"
#include "ui/BrushPopoverPanel.hpp"
#include "ui/CanvasSizeDialog.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/ColorDock.hpp"
#include "ui/ColorHistoryDock.hpp"
#include "ui/EraserPopoverPanel.hpp"
#include "ui/Icons.hpp"
#include "ui/ImageSizeDialog.hpp"
#include "ui/LassoPopoverPanel.hpp"
#include "ui/LayerDock.hpp"
#include "ui/PaletteDockAreaManager.hpp"
#include "ui/PopoverToolButton.hpp"
#include "ui/SelectionActionBar.hpp"
#include "ui/SettingsDialog.hpp"
#include "ui/ShortcutBinding.hpp"
#include "ui/SpinBoxCaretGuard.hpp"
#include "ui/StrokePropertiesDialog.hpp"
#include "ui/TimelineBar.hpp"
#include "ui/ToolDock.hpp"
#include "ui/ToolPopover.hpp"
#include "ui/WandPopoverPanel.hpp"
#include "ui/WobbleDock.hpp"

#ifdef Q_OS_MACOS
#include "ui/MacWindowChrome.hpp"
#endif

#include <QAction>
#include <QActionGroup>
#include <QApplication>
#include <QClipboard>
#include <QCloseEvent>
#include <QColorDialog>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDir>
#include <QEvent>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QImageReader>
#include <QInputDialog>
#include <QKeyEvent>
#include <QLabel>
#include <QMenu>
#include <QMenuBar>
#include <QMessageBox>
#include <QMetaType>
#include <QMimeData>
#include <QProgressDialog>
#include <QPushButton>
#include <QSet>
#include <QSettings>
#include <QShortcut>
#include <QShowEvent>
#include <QSignalBlocker>
#include <QSlider>
#include <QSpinBox>
#include <QStatusBar>
#include <QStringList>
#include <QTabWidget>
#include <QToolBar>
#include <QToolButton>
#include <QVBoxLayout>
#include <QVariant>

#include <spdlog/spdlog.h>

#include <algorithm>
#include <cmath>
#include <limits>
#include <optional>
#include <utility>

namespace ugurugu
{

namespace
{

constexpr int dockLayoutVersion = 4;

QString projectExtension()
{
    return QStringLiteral("ugu");
}

// The same format under earlier names of the application, so these stay
// readable and only ever appear in open filters, never in a save path.
QStringList legacyProjectExtensions()
{
    return {QStringLiteral("wagle"), QStringLiteral("wobble")};
}

QString projectFilterPattern()
{
    QStringList patterns = {QStringLiteral("*.") + projectExtension()};
    for (const QString &extension : legacyProjectExtensions())
    {
        patterns.append(QStringLiteral("*.") + extension);
    }
    return patterns.join(QLatin1Char(' '));
}

QSize requestCanvasSize(QWidget *parent,
    const QSize &current,
    const QString &title,
    const QString &description = {})
{
    QDialog dialog(parent);
    dialog.setWindowTitle(title);

    auto *layout = new QFormLayout(&dialog);
    if (!description.isEmpty())
    {
        auto *label = new QLabel(description, &dialog);
        label->setWordWrap(true);
        layout->addRow(label);
    }
    auto *widthSpin = new QSpinBox(&dialog);
    auto *heightSpin = new QSpinBox(&dialog);
    widthSpin->setRange(64, DocumentLimits::maximumCanvasEdge);
    heightSpin->setRange(64, DocumentLimits::maximumCanvasEdge);
    widthSpin->setValue(current.width());
    heightSpin->setValue(current.height());
    widthSpin->setSuffix(QObject::tr(" px"));
    installSuffixCaretGuard(widthSpin);
    heightSpin->setSuffix(QObject::tr(" px"));
    installSuffixCaretGuard(heightSpin);
    layout->addRow(QObject::tr("Width"), widthSpin);
    layout->addRow(QObject::tr("Height"), heightSpin);

    auto *buttons = new QDialogButtonBox(
        QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog);
    QObject::connect(
        buttons, &QDialogButtonBox::accepted, &dialog, &QDialog::accept);
    QObject::connect(
        buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    layout->addRow(buttons);

    if (dialog.exec() != QDialog::Accepted)
    {
        return {};
    }
    return QSize(widthSpin->value(), heightSpin->value());
}

}

MainWindow::MainWindow(QWidget *parent)
    : QMainWindow(parent)
{
    setObjectName(QStringLiteral("MainWindow"));
    setAcceptDrops(false);
    setMinimumSize(900, 640);
    setDockOptions(QMainWindow::AnimatedDocks | QMainWindow::AllowNestedDocks
                   | QMainWindow::AllowTabbedDocks
                   | QMainWindow::GroupedDragging);
    setTabPosition(Qt::AllDockWidgetAreas, QTabWidget::North);

    auto *central = new QWidget(this);
    auto *centralLayout = new QVBoxLayout(central);
    centralLayout->setContentsMargins(0, 0, 0, 0);
    centralLayout->setSpacing(0);
    m_canvas = new CanvasWidget(&m_controller, central);
    restoreDrawingToolSettings();
    centralLayout->addWidget(m_canvas, 1);
    m_timeline = new TimelineBar(&m_controller, m_canvas, central);
    centralLayout->addWidget(m_timeline);
    setCentralWidget(central);

    m_toolDock = new ToolDock(m_canvas, this);
    m_colorDock = new ColorDock(m_canvas, this);
    m_colorHistoryDock = new ColorHistoryDock(m_canvas, this);
    m_wobbleDock = new WobbleDock(&m_controller, this);
    m_layerDock = new LayerDock(&m_controller, this);
    resetDockLayout();
    connect(m_layerDock,
        &LayerDock::groupSelectionChanged,
        m_canvas,
        &CanvasWidget::setGroupSelectionActive);

    createActions();
    createMenus();
    createToolBars();
    createStatusBar();
    connectDocument();
    m_drawingToolSettingsSaveTimer.setSingleShot(true);
    m_drawingToolSettingsSaveTimer.setInterval(200);
    connect(&m_drawingToolSettingsSaveTimer,
        &QTimer::timeout,
        this,
        &MainWindow::saveDrawingToolSettings);
    connectDrawingToolSettings();
    connect(&m_exportWorker,
        &ExportWorker::progress,
        this,
        [this](ExportWorker::Kind, int value, int maximum)
        {
            if (m_exportProgress)
            {
                m_exportProgress->setMaximum(maximum);
                m_exportProgress->setValue(value);
            }
        });
    connect(&m_exportWorker,
        &ExportWorker::finished,
        this,
        &MainWindow::handleExportFinished);
    m_autosaveTimer.setInterval(30000);
    connect(
        &m_autosaveTimer, &QTimer::timeout, this, &MainWindow::writeAutosave);
    m_autosaveTimer.start();
    m_recoveryOwnedBySession = !QFileInfo::exists(RecoveryStore::filePath());
    m_startupResolved = m_recoveryOwnedBySession;

    const QSettings settings;
    const bool geometryRestored = restoreGeometry(
        settings.value(QStringLiteral("window/geometry")).toByteArray());
    const bool stateRestored = restoreState(
        settings.value(QStringLiteral("window/state")).toByteArray(),
        dockLayoutVersion);
    if (!stateRestored)
    {
        resetDockLayout();
    }
    if (!geometryRestored)
    {
        resize(1280, 820);
    }

    m_paletteDockAreaManager =
        new PaletteDockAreaManager(this, dockLayoutVersion);
    for (QDockWidget *dock : {static_cast<QDockWidget *>(m_toolDock),
             static_cast<QDockWidget *>(m_colorDock),
             static_cast<QDockWidget *>(m_colorHistoryDock),
             static_cast<QDockWidget *>(m_wobbleDock),
             static_cast<QDockWidget *>(m_layerDock)})
    {
        m_paletteDockAreaManager->registerDock(dock);
    }
    m_paletteDockAreaManager->restorePersistedState();

    m_canvas->setAnimateWhileDrawing(SettingsDialog::animateWhileDrawing());
    setTimelineVisible(m_showTimelineAction->isChecked());
    updateWindowTitle();
    m_canvas->setFocus(Qt::OtherFocusReason);
    qApp->installEventFilter(this);

#ifdef Q_OS_MACOS
    applySeamlessTitleBar(this);
#endif
}

bool MainWindow::openFile(const QString &filePath)
{
    if (filePath.isEmpty())
    {
        return false;
    }
    if (!m_startupResolved)
    {
        return initializeSession(filePath)
               == StartupResult::OpenedRequestedFile;
    }
    if (rejectReservedRecoveryPath(filePath, tr("Open failed")))
    {
        return false;
    }
    if (!maybeSave())
    {
        return false;
    }

    const std::optional<Document> document = readProject(filePath);
    return document && activateProject(filePath, *document);
}

std::optional<Document> MainWindow::readProject(const QString &filePath)
{
    if (filePath.isEmpty())
    {
        return std::nullopt;
    }

    m_pendingWawaImportSummary.reset();
    m_suggestedSavePath.clear();
    QString error;
    if (QFileInfo(filePath).suffix().compare(
            QStringLiteral("wawa"), Qt::CaseInsensitive)
        == 0)
    {
        QFile file(filePath);
        if (!file.open(QIODevice::ReadOnly) || file.size() < 0
            || file.size() > DocumentLimits::maximumProjectBytes)
        {
            error = tr("The .wawa project could not be read or is too large.");
        }
        else
        {
            const QByteArray data =
                file.read(DocumentLimits::maximumProjectBytes + 1);
            if (data.size() > DocumentLimits::maximumProjectBytes
                || !file.atEnd())
            {
                error =
                    tr("The .wawa project could not be read or is too large.");
            }
            else if (const std::optional<WawaImportResult> imported =
                         WawaV10Importer::import(data, &error);
                imported)
            {
                m_pendingWawaImportSummary = imported->summary;
                const QFileInfo source(filePath);
                m_suggestedSavePath =
                    QDir(source.absolutePath())
                        .filePath(source.completeBaseName()
                                  + QStringLiteral(".") + projectExtension());
                return imported->document;
            }
        }
        spdlog::error("Failed to import project {}: {}",
            filePath.toUtf8().constData(),
            error.toUtf8().constData());
        QMessageBox::critical(this,
            tr("Import failed"),
            tr("Could not import the .wawa project.\n\n%1").arg(error));
        return std::nullopt;
    }

    std::optional<Document> document =
        DocumentSerializer::load(filePath, &error);
    if (!document)
    {
        spdlog::error("Failed to open project {}: {}",
            filePath.toUtf8().constData(),
            error.toUtf8().constData());
        QMessageBox::critical(this,
            tr("Open failed"),
            tr("Could not open the project.\n\n%1").arg(error));
        return std::nullopt;
    }
    return document;
}

bool MainWindow::activateProject(const QString &filePath, Document document)
{
    if (rejectReservedRecoveryPath(filePath, tr("Open failed")))
    {
        return false;
    }
    QString error;
    const std::optional<WawaImportSummary> importSummary =
        std::exchange(m_pendingWawaImportSummary, std::nullopt);
    const bool importedWawa = importSummary.has_value();
    const bool loaded =
        importedWawa
            ? m_controller.loadRecoveredDocument(std::move(document), &error)
            : m_controller.loadDocument(std::move(document), &error);
    if (!loaded)
    {
        if (importedWawa)
        {
            m_suggestedSavePath.clear();
        }
        spdlog::error("Failed to prepare project {}: {}",
            filePath.toUtf8().constData(),
            error.toUtf8().constData());
        QMessageBox::critical(this,
            tr("Open failed"),
            tr("The project could not be prepared.\n\n%1").arg(error));
        return false;
    }
    m_currentFilePath =
        importedWawa ? QString() : QFileInfo(filePath).absoluteFilePath();
    clearAutosave();
    m_canvas->fitToWindow();
    updateWindowTitle();
    warnLegacyLayerHierarchy();
    if (importedWawa)
    {
        const WawaImportSummary summary =
            importSummary.value_or(WawaImportSummary{});
        QString details =
            tr("Imported native .wawa version 10 as a new, unsaved "
               "Ugurugu document.\n\n"
               "Layers: %1\nBase images: %2\nPaint strokes: %3\n"
               "Eraser strokes: %4\nPolygon fills: %5")
                .arg(summary.layers)
                .arg(summary.baseImages)
                .arg(summary.paintStrokes)
                .arg(summary.eraserStrokes)
                .arg(summary.polygonFills);
        if (summary.skippedOperations > 0 || summary.clampedWidths > 0)
        {
            details += tr("\nSkipped operations: %1\nClamped widths: %2")
                           .arg(summary.skippedOperations)
                           .arg(summary.clampedWidths);
        }
        details += tr("\n\nWobble, airbrush, and polygon fills are "
                      "best-effort conversions and may look different from "
                      "WiggleWiggleTool.");
        QMessageBox::information(this, tr(".wawa import complete"), details);
        statusBar()->showMessage(tr("Imported %1 as a new document")
                                     .arg(QFileInfo(filePath).fileName()),
            5000);
        spdlog::info(
            "Imported .wawa project {}", filePath.toUtf8().constData());
        return true;
    }
    m_suggestedSavePath.clear();
    statusBar()->showMessage(tr("Opened %1").arg(m_currentFilePath), 4000);
    spdlog::info("Opened project {}", m_currentFilePath.toUtf8().constData());
    return true;
}

bool MainWindow::offerRecovery()
{
    return initializeSession() == StartupResult::Recovered;
}

MainWindow::StartupResult MainWindow::initializeSession(
    const QString &requestedFilePath)
{
    const QString recoveryPath = RecoveryStore::filePath();
    const bool hasRecovery = QFileInfo::exists(recoveryPath);
    const bool hasRequestedFile =
        !requestedFilePath.isEmpty()
        && !RecoveryStore::isRecoveryPath(requestedFilePath);

    if (!hasRecovery)
    {
        m_recoveryOwnedBySession = true;
        m_startupResolved = true;
        if (!hasRequestedFile)
        {
            return StartupResult::Ready;
        }
        std::optional<Document> requestedDocument =
            readProject(requestedFilePath);
        if (!requestedDocument)
        {
            return StartupResult::Failed;
        }
        return activateProject(requestedFilePath, std::move(*requestedDocument))
                   ? StartupResult::OpenedRequestedFile
                   : StartupResult::Failed;
    }

    QMessageBox dialog(QMessageBox::Question,
        tr("Recover unsaved work"),
        hasRequestedFile ? tr("Ugurugu found work from a previous session. "
                              "Choose what to do before opening %1.")
                               .arg(QFileInfo(requestedFilePath).fileName())
                         : tr("Ugurugu found work from a previous session. "
                              "Choose whether to recover or discard it."),
        QMessageBox::NoButton,
        this);
    QPushButton *recoverButton =
        dialog.addButton(tr("Recover"), QMessageBox::AcceptRole);
    recoverButton->setObjectName(QStringLiteral("startupRecoverButton"));
    QPushButton *preserveButton = nullptr;
    QPushButton *discardButton = nullptr;
    if (hasRequestedFile)
    {
        preserveButton = dialog.addButton(
            tr("Keep Recovery and Open File"), QMessageBox::ActionRole);
        preserveButton->setObjectName(
            QStringLiteral("startupPreserveRecoveryButton"));
        discardButton = dialog.addButton(
            tr("Discard Recovery and Open File"), QMessageBox::DestructiveRole);
    }
    else
    {
        discardButton =
            dialog.addButton(tr("Discard"), QMessageBox::DestructiveRole);
    }
    discardButton->setObjectName(
        QStringLiteral("startupDiscardRecoveryButton"));
    QPushButton *cancelButton = dialog.addButton(QMessageBox::Cancel);
    cancelButton->setObjectName(QStringLiteral("startupCancelButton"));
    dialog.setDefaultButton(recoverButton);
    dialog.exec();

    if (dialog.clickedButton() == cancelButton || !dialog.clickedButton())
    {
        return StartupResult::Canceled;
    }
    if (dialog.clickedButton() == recoverButton)
    {
        if (!recoverAutosave())
        {
            return StartupResult::Failed;
        }
        m_recoveryOwnedBySession = true;
        m_startupResolved = true;
        return StartupResult::Recovered;
    }
    if (dialog.clickedButton() == preserveButton)
    {
        std::optional<Document> requestedDocument =
            readProject(requestedFilePath);
        if (!requestedDocument)
        {
            return StartupResult::Failed;
        }
        const QString preservedPath = preserveAutosave();
        if (preservedPath.isEmpty())
        {
            return StartupResult::Failed;
        }
        m_recoveryOwnedBySession = true;
        m_startupResolved = true;
        if (!activateProject(requestedFilePath, std::move(*requestedDocument)))
        {
            return StartupResult::Failed;
        }
        statusBar()->showMessage(tr("Opened %1. Recovery preserved at %2")
                                     .arg(m_currentFilePath, preservedPath),
            8000);
        return StartupResult::OpenedRequestedFile;
    }
    std::optional<Document> requestedDocument;
    if (hasRequestedFile)
    {
        requestedDocument = readProject(requestedFilePath);
        if (!requestedDocument)
        {
            return StartupResult::Failed;
        }
    }
    if (!requestedDocument)
    {
        if (!discardAutosave())
        {
            return StartupResult::Failed;
        }
        m_startupResolved = true;
        return StartupResult::Ready;
    }
    if (!activateProject(requestedFilePath, std::move(*requestedDocument)))
    {
        return StartupResult::Failed;
    }
    m_startupResolved = true;
    if (!discardAutosave())
    {
        m_recoveryOwnedBySession = true;
        m_recoverySessionId = QUuid::createUuid();
        m_recoveryRevision = 0;
        clearRecoveryMetadata();
    }
    return StartupResult::OpenedRequestedFile;
}

bool MainWindow::recoverAutosave()
{
    const QString recoveryPath = RecoveryStore::filePath();

    QString error;
    const std::optional<RecoveryStore::Snapshot> recovered =
        RecoveryStore::load(&error);
    if (!recovered)
    {
        spdlog::error("Failed to load recovery file {}: {}",
            recoveryPath.toUtf8().constData(),
            error.toUtf8().constData());
        QString preservationError;
        QString quarantinedPath;
        m_recoveryWriter.runExclusiveFileOperation(
            [&preservationError, &quarantinedPath]()
            {
                quarantinedPath = RecoveryStore::quarantine(&preservationError);
                return !quarantinedPath.isEmpty();
            });
        const QString preservedPath =
            quarantinedPath.isEmpty() ? recoveryPath : quarantinedPath;
        if (!preservationError.isEmpty())
        {
            spdlog::warn("Could not quarantine recovery file {}: {}",
                recoveryPath.toUtf8().constData(),
                preservationError.toUtf8().constData());
        }
        if (!quarantinedPath.isEmpty())
        {
            m_autosavePending = false;
            clearRecoveryMetadata();
            m_recoveryOwnedBySession = true;
        }
        QMessageBox::warning(this,
            tr("Recovery failed"),
            tr("The recovery file could not be opened.\n\n%1"
               "\n\nThe recovery file was not deleted. "
               "You can find it at:\n%2")
                .arg(error, preservedPath));
        return false;
    }
    if (recovered->metadataStatus == RecoveryStore::MetadataStatus::Invalid)
    {
        spdlog::warn("Ignored invalid recovery metadata in {}",
            recoveryPath.toUtf8().constData());
    }

    QString transitionError;
    if (!m_controller.loadRecoveredDocument(
            recovered->document, &transitionError))
    {
        spdlog::error("Failed to prepare recovered document: {}",
            transitionError.toUtf8().constData());
        QMessageBox::warning(this,
            tr("Recovery failed"),
            tr("The recovered document could not be prepared.\n\n%1")
                .arg(transitionError));
        return false;
    }
    m_currentFilePath.clear();
    m_suggestedSavePath.clear();
    m_recoveryRevision = 0;
    clearRecoveryMetadata();
    m_canvas->fitToWindow();
    updateWindowTitle();
    warnLegacyLayerHierarchy();
    statusBar()->showMessage(tr("Recovered unsaved work."), 5000);
    return true;
}

void MainWindow::closeEvent(QCloseEvent *event)
{
    if (!maybeSave())
    {
        event->ignore();
        return;
    }
    QSettings settings;
    settings.setValue(QStringLiteral("window/geometry"), saveGeometry());
    settings.setValue(QStringLiteral("window/state"),
        m_paletteDockAreaManager->layoutStateForPersistence());
    m_toolDock->rememberWidth();
    saveDrawingToolSettings();
    clearAutosave();
    event->accept();
}

void MainWindow::showEvent(QShowEvent *event)
{
    QMainWindow::showEvent(event);
    m_paletteDockAreaManager->requestAreaControlsUpdate();
    if (!m_initialFitApplied)
    {
        m_initialFitApplied = true;
        QTimer::singleShot(0, m_canvas, &CanvasWidget::fitToWindow);
        showShortcutChangeNoticeOnce();
    }
}

void MainWindow::showShortcutChangeNoticeOnce()
{
    QSettings settings;
    const QString noticeKey = QStringLiteral("notices/ctrlDDeselects");
    if (settings.value(noticeKey, false).toBool())
    {
        return;
    }
    settings.setValue(noticeKey, true);
    // window/geometry is written on every close, so its presence separates
    // an upgrade from a fresh install. New users never used the old Ctrl+D
    // and should not see the notice.
    if (!settings.contains(QStringLiteral("window/geometry")))
    {
        return;
    }
    auto *dialog = new QMessageBox(QMessageBox::Information,
        tr("Shortcut change"),
        tr("Ctrl+D now deselects, matching the convention of other drawing "
           "tools. To duplicate content, copy it with Ctrl+C: the copy is "
           "placed on a new layer and can be dragged right away."),
        QMessageBox::Ok,
        this);
    dialog->setObjectName(QStringLiteral("shortcutChangeNotice"));
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->show();
}

bool MainWindow::eventFilter(QObject *watched, QEvent *event)
{
    if (watched == m_canvas
        && (event->type() == QEvent::KeyPress
            || event->type() == QEvent::KeyRelease))
    {
        const auto *keyEvent = static_cast<QKeyEvent *>(event);
        if (keyEvent->key() == Qt::Key_Space && !keyEvent->isAutoRepeat())
        {
            if (event->type() == QEvent::KeyRelease)
            {
                m_canvas->setPanModifierActive(false);
                return true;
            }
            m_canvas->setPanModifierActive(true);
            return true;
        }
    }

    if (event->type() == QEvent::ApplicationDeactivate
        || (event->type() == QEvent::WindowDeactivate && watched == this))
    {
        m_canvas->cancelActiveInteraction();
        saveDrawingToolSettings();
        writeAutosave();
    }
    else if (event->type() == QEvent::TabletLeaveProximity)
    {
        m_canvas->cancelActiveInteraction();
    }

    return QMainWindow::eventFilter(watched, event);
}

void MainWindow::connectDocument()
{
    connect(&m_controller,
        &DocumentController::documentChanged,
        this,
        [this]()
        {
            m_autosavePending = true;
            ++m_autosaveEditGeneration;
        });
    connect(&m_recoveryWriter,
        &RecoveryWriter::writeFinished,
        this,
        &MainWindow::handleAutosaveWritten);
    connect(&m_controller,
        &DocumentController::modifiedChanged,
        this,
        [this](bool modified)
        {
            refreshUnsavedState();
            updateWindowTitle();
            if (!modified && !m_canvas->hasPendingSelectionTransform())
            {
                clearAutosave();
            }
        });
    connect(m_canvas,
        &CanvasWidget::selectionTransformSessionChanged,
        this,
        [this](bool, bool dirty)
        {
            refreshUnsavedState();
            if (dirty || m_controller.isModified())
            {
                m_autosavePending = true;
                ++m_autosaveEditGeneration;
            }
            else
            {
                clearAutosave();
            }
        });
}

void MainWindow::updateWindowTitle()
{
    const QString name = m_currentFilePath.isEmpty()
                             ? tr("Untitled")
                             : QFileInfo(m_currentFilePath).fileName();
    setWindowTitle(tr("%1[*] — Ugurugu").arg(name));
    setWindowFilePath(m_currentFilePath);
}

void MainWindow::resetDockLayout()
{
    removeDockWidget(m_toolDock);
    removeDockWidget(m_colorDock);
    removeDockWidget(m_colorHistoryDock);
    removeDockWidget(m_wobbleDock);
    removeDockWidget(m_layerDock);

    m_toolDock->setFloating(false);
    m_colorDock->setFloating(false);
    m_colorHistoryDock->setFloating(false);
    m_wobbleDock->setFloating(false);
    m_layerDock->setFloating(false);

    addDockWidget(Qt::RightDockWidgetArea, m_toolDock);
    addDockWidget(Qt::RightDockWidgetArea, m_colorDock);
    splitDockWidget(m_toolDock, m_colorDock, Qt::Vertical);
    addDockWidget(Qt::RightDockWidgetArea, m_colorHistoryDock);
    splitDockWidget(m_colorDock, m_colorHistoryDock, Qt::Vertical);
    addDockWidget(Qt::RightDockWidgetArea, m_wobbleDock);
    tabifyDockWidget(m_toolDock, m_wobbleDock);
    addDockWidget(Qt::RightDockWidgetArea, m_layerDock);
    tabifyDockWidget(m_colorHistoryDock, m_layerDock);

    m_toolDock->show();
    m_colorDock->show();
    m_colorHistoryDock->show();
    m_wobbleDock->show();
    m_layerDock->show();
    m_toolDock->raise();
    m_colorHistoryDock->raise();
    resizeDocks({m_toolDock}, {m_toolDock->preferredWidth()}, Qt::Horizontal);
    resizeDocks(
        {m_toolDock, m_colorDock, m_colorHistoryDock}, {3, 3, 4}, Qt::Vertical);
}

bool MainWindow::hasUnsavedWork() const
{
    return m_controller.isModified()
           || m_canvas->hasPendingSelectionTransform();
}

void MainWindow::refreshUnsavedState()
{
    setWindowModified(hasUnsavedWork());
}

bool MainWindow::maybeSave()
{
    if (!hasUnsavedWork())
    {
        return true;
    }

    QDialog dialog(this);
    dialog.setWindowTitle(tr("Unsaved changes"));
    auto *layout = new QVBoxLayout(&dialog);
    layout->setContentsMargins(18, 16, 18, 14);
    layout->setSpacing(14);
    layout->addWidget(
        new QLabel(tr("The document has unsaved changes."), &dialog));

    auto *buttonLayout = new QHBoxLayout;
    buttonLayout->setSpacing(8);
    buttonLayout->addStretch(1);
    auto *saveButton = new QPushButton(tr("Save (S)"), &dialog);
    saveButton->setObjectName(QStringLiteral("unsavedChangesSaveButton"));
    saveButton->setDefault(true);
    buttonLayout->addWidget(saveButton);
    auto *discardButton = new QPushButton(tr("Don't Save (N)"), &dialog);
    discardButton->setObjectName(QStringLiteral("unsavedChangesDiscardButton"));
    buttonLayout->addWidget(discardButton);
    auto *cancelButton = new QPushButton(tr("Cancel (ESC)"), &dialog);
    cancelButton->setObjectName(QStringLiteral("unsavedChangesCancelButton"));
    buttonLayout->addWidget(cancelButton);
    layout->addLayout(buttonLayout);

    connect(saveButton,
        &QPushButton::clicked,
        &dialog,
        [&dialog]()
        {
            dialog.done(1);
        });
    connect(discardButton,
        &QPushButton::clicked,
        &dialog,
        [&dialog]()
        {
            dialog.done(2);
        });
    connect(cancelButton, &QPushButton::clicked, &dialog, &QDialog::reject);
    auto *saveShortcut =
        new QShortcut(QKeySequence(QStringLiteral("S")), &dialog);
    auto *discardShortcut =
        new QShortcut(QKeySequence(QStringLiteral("N")), &dialog);
    auto *cancelShortcut =
        new QShortcut(QKeySequence(QStringLiteral("Esc")), &dialog);
    saveShortcut->setContext(Qt::ApplicationShortcut);
    discardShortcut->setContext(Qt::ApplicationShortcut);
    cancelShortcut->setContext(Qt::ApplicationShortcut);
    connect(
        saveShortcut, &QShortcut::activated, saveButton, &QPushButton::click);
    connect(discardShortcut,
        &QShortcut::activated,
        discardButton,
        &QPushButton::click);
    connect(cancelShortcut,
        &QShortcut::activated,
        cancelButton,
        &QPushButton::click);

    const int choice = dialog.exec();
    if (choice == 1)
    {
        return save();
    }
    return choice == 2;
}

bool MainWindow::save()
{
    return m_currentFilePath.isEmpty() ? saveAs()
                                       : saveToFile(m_currentFilePath);
}

bool MainWindow::saveAs()
{
    const QString selected = QFileDialog::getSaveFileName(this,
        tr("Save project"),
        saveDialogStartPath(projectExtension()),
        tr("Ugurugu projects (*.%1)").arg(projectExtension()));
    if (selected.isEmpty())
    {
        return false;
    }
    return saveToFile(normalizedPath(selected, projectExtension()));
}

bool MainWindow::saveToFile(const QString &filePath)
{
    if (rejectReservedRecoveryPath(filePath, tr("Save failed")))
    {
        return false;
    }
    if (m_canvas->hasPendingSelectionTransform()
        && !m_canvas->applySelectionTransform())
    {
        spdlog::error("Aborted saving {}: the pending selection transform "
                      "could not be applied",
            filePath.toUtf8().constData());
        QMessageBox::critical(this,
            tr("Save failed"),
            tr("The pending selection transform could not be "
               "applied. Adjust or cancel the transform, then save "
               "again."));
        return false;
    }
    QString error;
    if (!m_controller.saveDocument(filePath, &error))
    {
        spdlog::error("Failed to save project {}: {}",
            filePath.toUtf8().constData(),
            error.toUtf8().constData());
        QMessageBox::critical(this,
            tr("Save failed"),
            tr("Could not save the project.\n\n%1").arg(error));
        return false;
    }
    m_currentFilePath = QFileInfo(filePath).absoluteFilePath();
    m_suggestedSavePath.clear();
    m_controller.markSaved();
    clearAutosave();
    updateWindowTitle();
    statusBar()->showMessage(tr("Saved %1").arg(m_currentFilePath), 4000);
    spdlog::info("Saved project {}", m_currentFilePath.toUtf8().constData());
    return true;
}

bool MainWindow::rejectReservedRecoveryPath(
    const QString &filePath, const QString &title)
{
    if (!RecoveryStore::isRecoveryPath(filePath))
    {
        return false;
    }
    QMessageBox::warning(this,
        title,
        tr("The recovery file location is reserved. Choose a different "
           "project path."));
    return true;
}

void MainWindow::newDocument()
{
    if (!maybeSave())
    {
        return;
    }
    const QSize size = requestCanvasSize(
        this, m_controller.document().size, tr("New document"));
    if (!size.isValid())
    {
        return;
    }
    QString error;
    if (!m_controller.newDocument(size, &error))
    {
        spdlog::error(
            "Failed to prepare a new document: {}", error.toUtf8().constData());
        QMessageBox::critical(this,
            tr("New document failed"),
            tr("The new document could not be prepared.\n\n%1").arg(error));
        return;
    }
    m_currentFilePath.clear();
    m_suggestedSavePath.clear();
    clearAutosave();
    m_canvas->fitToWindow();
    updateWindowTitle();
    spdlog::info("Created {}x{} document", size.width(), size.height());
}

void MainWindow::resizeCanvas()
{
    CanvasSizeDialog dialog(m_controller.document().size, this);
    if (dialog.exec() != QDialog::Accepted)
    {
        return;
    }
    const CanvasSizeDialog::Result requested = dialog.currentResult();
    if (requested.size == m_controller.document().size
        && requested.contentOffset.isNull())
    {
        return;
    }
    if (m_canvas->hasPendingSelectionTransform()
        && !m_canvas->applySelectionTransform())
    {
        QMessageBox::critical(this,
            tr("Canvas size"),
            tr("The pending selection transform could not be "
               "applied. Adjust or cancel the transform, then "
               "change the size again."));
        return;
    }
    m_canvas->setSelectionMoveMode(false);
    m_canvas->cancelActiveInteraction();
    const bool hadSelection = m_canvas->hasSelection();
    if (hadSelection)
    {
        m_controller.undoStack()->beginMacro(tr("Resize canvas"));
        m_canvas->deselectSelection();
    }
    const bool resized =
        m_controller.resizeCanvas(requested.size, requested.contentOffset);
    if (hadSelection)
    {
        m_controller.undoStack()->endMacro();
    }
    if (!resized)
    {
        QMessageBox::warning(this,
            tr("Canvas size"),
            tr("The canvas size could not be changed. "
               "Try a smaller size or offset."));
    }
}

void MainWindow::resizeImage()
{
    ImageSizeDialog dialog(m_controller.document().size, this);
    if (dialog.exec() != QDialog::Accepted)
    {
        return;
    }
    const QSize requested = dialog.imageSize();
    if (requested == m_controller.document().size)
    {
        return;
    }
    if (m_canvas->hasPendingSelectionTransform()
        && !m_canvas->applySelectionTransform())
    {
        QMessageBox::critical(this,
            tr("Image size"),
            tr("The pending selection transform could not be "
               "applied. Adjust or cancel the transform, then "
               "change the size again."));
        return;
    }
    m_canvas->setSelectionMoveMode(false);
    m_canvas->cancelActiveInteraction();
    const bool hadSelection = m_canvas->hasSelection();
    if (hadSelection)
    {
        m_controller.undoStack()->beginMacro(tr("Resize image"));
        m_canvas->deselectSelection();
    }
    const bool resized = m_controller.resizeImage(requested);
    if (hadSelection)
    {
        m_controller.undoStack()->endMacro();
    }
    if (!resized)
    {
        QMessageBox::warning(this,
            tr("Image size"),
            tr("The image size could not be changed. "
               "Try smaller dimensions."));
    }
}

void MainWindow::chooseBackgroundColor()
{
    // ShowAlphaChannel is what exposes a transparent background: alpha 0 is a
    // valid choice here, not an accident to be corrected.
    const QColor chosen =
        QColorDialog::getColor(m_controller.document().background,
            this,
            tr("Canvas background"),
            QColorDialog::ShowAlphaChannel);
    if (!chosen.isValid())
    {
        return;
    }
    m_controller.setBackground(chosen);
}

void MainWindow::pasteFromClipboard()
{
    const auto showNotice = [this](const QString &message)
    {
        statusBar()->showMessage(message, 4000);
    };
    const QMimeData *mimeData = QGuiApplication::clipboard()->mimeData();
    if (!mimeData)
    {
        showNotice(tr("There is nothing to paste."));
        return;
    }
    const QString selectionMimeType =
        SelectionClipboardCodec::availableMimeType(*mimeData);
    if (selectionMimeType.isEmpty())
    {
        if (mimeData->hasImage())
        {
            showNotice(
                tr("Pasting images from other apps is not supported yet."));
        }
        else
        {
            showNotice(tr("There is nothing to paste."));
        }
        return;
    }
    QString error;
    const std::optional<SelectionClipboardCodec::Pasted> pasted =
        SelectionClipboardCodec::decode(
            mimeData->data(selectionMimeType), &error);
    if (!pasted)
    {
        showNotice(error.isEmpty()
                       ? tr("The clipboard content could not be pasted.")
                       : error);
        return;
    }
    switch (m_controller.pasteLayer(
        pasted->layer, pasted->canvasSize, {}, {}, pasted->rasterAssets))
    {
    case DocumentController::PasteLayerResult::Pasted:
        showNotice(tr("Pasted as a new layer."));
        break;
    case DocumentController::PasteLayerResult::RejectedLayerLimit:
        showNotice(tr("The paste was rejected because the document "
                      "already has the maximum number of layers."));
        break;
    case DocumentController::PasteLayerResult::RejectedStrokeLimit:
        showNotice(tr("The paste was rejected because it would exceed "
                      "the stroke limit."));
        break;
    case DocumentController::PasteLayerResult::RejectedPointLimit:
        showNotice(tr("The paste was rejected because it would exceed "
                      "the point limit."));
        break;
    case DocumentController::PasteLayerResult::RejectedMaskLimit:
        showNotice(tr("The paste was rejected because it would exceed "
                      "the mask budget."));
        break;
    case DocumentController::PasteLayerResult::RejectedInvalidLayer:
    case DocumentController::PasteLayerResult::RejectedCommit:
        showNotice(tr("The clipboard content could not be pasted."));
        break;
    }
}

void MainWindow::scaleSelection()
{
    bool accepted = false;
    const double percent = QInputDialog::getDouble(this,
        tr("Scale selection"),
        tr("Scale (%)"),
        125.0,
        10.0,
        400.0,
        0,
        &accepted);
    if (accepted)
    {
        m_canvas->scaleSelection(percent / 100.0);
    }
}

void MainWindow::rotateSelection()
{
    bool accepted = false;
    const double degrees = QInputDialog::getDouble(this,
        tr("Rotate selection"),
        tr("Angle (degrees)"),
        90.0,
        -360.0,
        360.0,
        1,
        &accepted);
    if (accepted && !qFuzzyIsNull(degrees))
    {
        if (m_canvas->rotateSelection(degrees))
        {
            m_canvas->applySelectionTransform();
        }
    }
}

void MainWindow::editSelectedStrokeProperties()
{
    if (m_canvas->hasSelectionTransformSession())
    {
        return;
    }
    const QUuid layerId = m_canvas->selectionLayerId();
    const QVector<QUuid> strokeIds = m_canvas->selectedStrokeIds();
    const Layer *layer = m_controller.document().layer(layerId);
    if (!layer || strokeIds.isEmpty())
    {
        return;
    }

    const QSet<QUuid> selected(strokeIds.cbegin(), strokeIds.cend());
    StrokePropertiesDialog::Values values;
    bool colorMixed = false;
    bool widthMixed = false;
    for (const Stroke &stroke : layer->strokes)
    {
        if (!selected.contains(stroke.id))
        {
            continue;
        }
        if (stroke.mode == StrokeMode::Paint || stroke.mode == StrokeMode::Fill)
        {
            if (!values.colorSupported)
            {
                values.color = stroke.color;
            }
            else if (values.color && *values.color != stroke.color)
            {
                colorMixed = true;
            }
            values.colorSupported = true;
        }
        if (stroke.mode == StrokeMode::Paint
            || stroke.mode == StrokeMode::Erase)
        {
            if (!values.widthSupported)
            {
                values.width = stroke.width;
            }
            else if (values.width
                     && !qFuzzyCompare(*values.width, stroke.width))
            {
                widthMixed = true;
            }
            values.widthSupported = true;
        }
    }
    if (!values.colorSupported && !values.widthSupported)
    {
        return;
    }
    if (colorMixed)
    {
        values.color.reset();
    }
    if (widthMixed)
    {
        values.width.reset();
    }
    StrokePropertiesDialog dialog(values, this);
    if (dialog.exec() != QDialog::Accepted)
    {
        return;
    }
    m_controller.updateStrokeAttributes(
        layerId, strokeIds, dialog.color(), dialog.selectedWidth());
}

void MainWindow::writeAutosave()
{
    if (!m_startupResolved || !m_recoveryOwnedBySession || !hasUnsavedWork()
        || !m_autosavePending
        || (!m_currentFilePath.isEmpty()
            && RecoveryStore::isRecoveryPath(m_currentFilePath)))
    {
        return;
    }
    const quint64 lastRevision =
        std::max(m_recoveryRevision, m_submittedRecoveryRevision);
    if (lastRevision == std::numeric_limits<quint64>::max())
    {
        m_recoverySessionId = QUuid::createUuid();
        m_recoveryRevision = 0;
        m_submittedRecoveryRevision = 0;
    }
    const quint64 nextRevision =
        std::max(m_recoveryRevision, m_submittedRecoveryRevision) + 1;
    const RecoveryStore::Metadata metadata{m_recoverySessionId,
        m_currentFilePath,
        nextRevision,
        QDateTime::currentDateTimeUtc()};
    if (m_canvas->hasPendingSelectionTransform())
    {
        m_recoveryWriter.submitWrite(
            m_canvas->documentWithPendingSelectionTransform(), metadata);
    }
    else
    {
        QString error;
        std::optional<DocumentSerializer::PreparedDocument> snapshot =
            m_controller.serializationSnapshot(&error);
        if (!snapshot)
        {
            reportRecoveryFailure(error);
            return;
        }
        m_recoveryWriter.submitWrite(std::move(*snapshot), metadata);
    }
    m_submittedRecoveryRevision = nextRevision;
    m_submittedEditGeneration = m_autosaveEditGeneration;
}

void MainWindow::handleAutosaveWritten(
    bool success, quint64 revision, const QString &error)
{
    if (!success)
    {
        reportRecoveryFailure(error);
        return;
    }
    clearRecoveryFailure();
    m_recoveryRevision = std::max(m_recoveryRevision, revision);
    clearRecoveryMetadata();
    // Only the completion of the most recent submission may release the
    // pending state: an older write finishing after a newer submission does
    // not cover it, and if that newer write then fails the state must still
    // demand a retry.
    if (revision == m_submittedRecoveryRevision
        && m_autosaveEditGeneration == m_submittedEditGeneration)
    {
        m_autosavePending = false;
    }
}

// A recovery snapshot that never lands is the one failure the artist has no
// other way to notice: the document still looks saved, the timer still runs,
// and the work is unprotected until the next manual save. The log alone did
// not reach anyone. The status bar keeps saying so for as long as it is true,
// and the first failure of a streak also raises a modeless notice — modeless
// because this arrives on a timer, not on something the artist just did.
void MainWindow::reportRecoveryFailure(const QString &reason)
{
    spdlog::warn("Failed to write recovery file {}: {}",
        RecoveryStore::filePath().toUtf8().constData(),
        reason.toUtf8().constData());
    const QString summary =
        tr("Automatic recovery is not saving. Save your work yourself.");
    if (m_recoveryFailureLabel)
    {
        m_recoveryFailureLabel->setText(summary);
        m_recoveryFailureLabel->setToolTip(reason);
        m_recoveryFailureLabel->setVisible(true);
    }
    statusBar()->showMessage(summary, 8000);
    if (m_recoveryFailureNoticeShown)
    {
        return;
    }
    m_recoveryFailureNoticeShown = true;
    auto *dialog = new QMessageBox(QMessageBox::Warning,
        tr("Automatic recovery failed"),
        tr("Ugurugu could not write its recovery snapshot, so unsaved work is "
           "no longer protected against a crash. Save the project yourself.\n\n"
           "%1")
            .arg(reason),
        QMessageBox::Ok,
        this);
    dialog->setObjectName(QStringLiteral("recoveryFailureNotice"));
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->show();
}

void MainWindow::clearRecoveryFailure()
{
    m_recoveryFailureNoticeShown = false;
    if (m_recoveryFailureLabel)
    {
        m_recoveryFailureLabel->clear();
        m_recoveryFailureLabel->setToolTip(QString());
        m_recoveryFailureLabel->setVisible(false);
    }
}

bool MainWindow::discardAutosave()
{
    QString error;
    if (!m_recoveryWriter.runExclusiveFileOperation(
            [&error]()
            {
                return RecoveryStore::discard(&error);
            }))
    {
        spdlog::warn("{}", error.toUtf8().constData());
        QMessageBox::warning(this,
            tr("Recovery could not be discarded"),
            tr("The recovery file was not deleted.\n\n%1").arg(error));
        return false;
    }
    m_autosavePending = false;
    clearRecoveryMetadata();
    m_recoveryOwnedBySession = true;
    m_recoverySessionId = QUuid::createUuid();
    m_recoveryRevision = 0;
    m_submittedRecoveryRevision = 0;
    return true;
}

QString MainWindow::preserveAutosave()
{
    QString error;
    QString preservedPath;
    m_recoveryWriter.runExclusiveFileOperation(
        [&error, &preservedPath]()
        {
            preservedPath = RecoveryStore::preserve(&error);
            return !preservedPath.isEmpty();
        });
    if (preservedPath.isEmpty())
    {
        spdlog::warn("{}", error.toUtf8().constData());
        QMessageBox::warning(this,
            tr("Recovery could not be preserved"),
            tr("The recovery file was left unchanged.\n\n%1").arg(error));
        return {};
    }
    m_autosavePending = false;
    clearRecoveryMetadata();
    m_recoveryOwnedBySession = true;
    m_recoverySessionId = QUuid::createUuid();
    m_recoveryRevision = 0;
    m_submittedRecoveryRevision = 0;
    return preservedPath;
}

void MainWindow::clearRecoveryMetadata()
{
    QSettings settings;
    settings.remove(QStringLiteral("recovery/sourcePath"));
}

void MainWindow::warnLegacyLayerHierarchy()
{
    const LayerHierarchyAnalysis hierarchy =
        analyzeLayerHierarchy(m_controller.document());
    if (!hierarchy.isValid()
        || hierarchy.maximumDepth() <= DocumentLimits::maximumLayerDepth)
    {
        return;
    }

    QMessageBox dialog(QMessageBox::Warning,
        tr("Layer group nesting limit"),
        tr("Some layers in this project are nested %1 levels deep inside "
           "layer groups. The structure will be preserved, but edits cannot "
           "increase the document's maximum nesting depth. New documents "
           "allow up to %2 levels.")
            .arg(hierarchy.maximumDepth())
            .arg(DocumentLimits::maximumLayerDepth),
        QMessageBox::Ok,
        this);
    dialog.setObjectName(QStringLiteral("legacyLayerDepthWarning"));
    dialog.exec();
}

bool MainWindow::clearAutosave()
{
    if (!m_currentFilePath.isEmpty()
        && RecoveryStore::isRecoveryPath(m_currentFilePath))
    {
        spdlog::error("Refused to delete the active project at the recovery "
                      "path {}",
            m_currentFilePath.toUtf8().constData());
        return true;
    }
    return !m_recoveryOwnedBySession || discardAutosave();
}

void MainWindow::chooseOpenFile()
{
    const QString filePath = QFileDialog::getOpenFileName(this,
        tr("Open project"),
        QString(),
        tr("Supported projects (%1 *.wawa);;"
           "Ugurugu projects (%1);;"
           "WiggleWiggleTool projects (*.wawa);;All files (*)")
            .arg(projectFilterPattern()));
    if (!filePath.isEmpty())
    {
        openFile(filePath);
    }
}

void MainWindow::chooseInsertImage()
{
    const QString filePath = QFileDialog::getOpenFileName(this,
        tr("Insert image"),
        {},
        tr("Image files (*.png *.jpg *.jpeg *.webp *.bmp *.gif *.tif *.tiff);;"
           "All files (*)"));
    if (filePath.isEmpty())
    {
        return;
    }

    QImageReader reader(filePath);
    reader.setAutoTransform(true);
    const QImage image = reader.read();
    if (image.isNull())
    {
        QMessageBox::warning(this,
            tr("Could not insert image"),
            tr("The image could not be decoded.\n\n%1")
                .arg(reader.errorString()));
        return;
    }

    using Result = DocumentController::InsertImageResult;
    const Result result = m_controller.insertImage(image, filePath);
    if (result == Result::Inserted)
    {
        statusBar()->showMessage(
            tr("Inserted %1").arg(QFileInfo(filePath).fileName()), 4000);
        return;
    }

    QString reason;
    switch (result)
    {
    case Result::Inserted:
        return;
    case Result::RejectedInvalidImage:
        reason = tr("The image dimensions or pixel data are not supported.");
        break;
    case Result::RejectedLayerLimit:
        reason = tr("The document has reached its layer limit.");
        break;
    case Result::RejectedAssetLimit:
        reason = tr("The document has reached its image asset limit.");
        break;
    case Result::RejectedCommit:
        reason = tr("The image would exceed the project size limit.");
        break;
    }
    QMessageBox::warning(this, tr("Could not insert image"), reason);
}

void MainWindow::applyWobbleAnimationEnabled(bool enabled)
{
    m_canvas->setWobbleAnimationEnabled(enabled);
    m_timeline->setEnabled(enabled);
    m_playAction->setEnabled(enabled);
    if (auto *exportGifAction =
            findChild<QAction *>(QStringLiteral("exportGifAction")))
    {
        exportGifAction->setEnabled(enabled && !m_exportWorker.isBusy());
    }
    if (auto *exportImageAction =
            findChild<QAction *>(QStringLiteral("exportPngAction")))
    {
        const QString label = enabled ? tr("Export current frame as &image…")
                                      : tr("Export &image…");
        exportImageAction->setText(label);
        exportImageAction->setProperty("shortcutLabel", label);
        exportImageAction->setEnabled(!m_exportWorker.isBusy());
    }
}

QString MainWindow::normalizedPath(
    const QString &filePath, const QString &extension) const
{
    if (QFileInfo(filePath).suffix().compare(extension, Qt::CaseInsensitive)
        == 0)
    {
        return filePath;
    }
    return filePath + QStringLiteral(".") + extension;
}

QString MainWindow::saveDialogStartPath(const QString &extension) const
{
    if (!m_currentFilePath.isEmpty())
    {
        const QFileInfo currentFile(m_currentFilePath);
        // A project opened under a legacy extension falls through, so Save As
        // proposes the same name carrying the current one.
        if (currentFile.suffix().compare(extension, Qt::CaseInsensitive) == 0)
        {
            return currentFile.absoluteFilePath();
        }
        return QDir(currentFile.absolutePath())
            .filePath(currentFile.completeBaseName() + QStringLiteral(".")
                      + extension);
    }
    if (!m_suggestedSavePath.isEmpty()
        && extension.compare(projectExtension(), Qt::CaseInsensitive) == 0)
    {
        return m_suggestedSavePath;
    }
    return QDir(SettingsDialog::defaultSaveFolder())
        .filePath(QStringLiteral("Untitled.") + extension);
}

}
