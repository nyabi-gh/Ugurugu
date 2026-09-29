// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "brush/BrushPreset.hpp"
#include "brush/EraserPreset.hpp"
#include "ui/AboutDialog.hpp"
#include "ui/BrushPopoverPanel.hpp"
#include "ui/BucketPopoverPanel.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/ColorDock.hpp"
#include "ui/ColorHistoryDock.hpp"
#include "ui/EraserPopoverPanel.hpp"
#include "ui/HelpDialog.hpp"
#include "ui/Icons.hpp"
#include "ui/LassoPopoverPanel.hpp"
#include "ui/LayerDock.hpp"
#include "ui/MainWindow.hpp"
#include "ui/PaletteDockAreaManager.hpp"
#include "ui/PopoverToolButton.hpp"
#include "ui/SelectionActionBar.hpp"
#include "ui/SettingsDialog.hpp"
#include "ui/ShortcutBinding.hpp"
#include "ui/SpinBoxCaretGuard.hpp"
#include "ui/TimelineBar.hpp"
#include "ui/ToolDock.hpp"
#include "ui/ToolPopover.hpp"
#include "ui/WandPopoverPanel.hpp"
#include "ui/WobbleDock.hpp"
#include "ui/WobblePopoverPanel.hpp"

#include <QActionGroup>
#include <QDoubleSpinBox>
#include <QLabel>
#include <QMenu>
#include <QMenuBar>
#include <QSignalBlocker>
#include <QSizePolicy>
#include <QSlider>
#include <QSpinBox>
#include <QStatusBar>
#include <QToolBar>
#include <QToolButton>

#include <algorithm>
#include <cmath>

namespace ugurugu
{

namespace
{

constexpr int minimumZoomPercent = 1;
constexpr int maximumZoomPercent = 1600;
constexpr int zoomSliderSteps = 1000;

int zoomPercentFromSlider(int value)
{
    const qreal progress = std::clamp(value, 0, zoomSliderSteps)
                           / static_cast<qreal>(zoomSliderSteps);
    const qreal minimum = std::log(minimumZoomPercent);
    const qreal maximum = std::log(maximumZoomPercent);
    return qRound(std::exp(minimum + (maximum - minimum) * progress));
}

int sliderFromZoomPercent(int percent)
{
    const qreal clamped =
        std::clamp(percent, minimumZoomPercent, maximumZoomPercent);
    const qreal minimum = std::log(minimumZoomPercent);
    const qreal maximum = std::log(maximumZoomPercent);
    return qRound(
        (std::log(clamped) - minimum) / (maximum - minimum) * zoomSliderSteps);
}
}

void MainWindow::createActions()
{
    const auto registerShortcut =
        [this](QAction *action,
            const QKeySequence &defaultShortcut,
            const QList<QKeySequence> &aliases = QList<QKeySequence>())
    {
        action->setProperty("shortcutLabel", action->text());
        ShortcutBinding::initialize(action, defaultShortcut, aliases);
        m_shortcutActions.append(action);
    };

    auto *newAction = new QAction(tr("&New"), this);
    newAction->setObjectName(QStringLiteral("newAction"));
    registerShortcut(newAction, QKeySequence(QKeySequence::New));
    connect(newAction, &QAction::triggered, this, &MainWindow::newDocument);

    auto *openAction = new QAction(tr("&Open…"), this);
    openAction->setObjectName(QStringLiteral("openAction"));
    registerShortcut(openAction, QKeySequence(QKeySequence::Open));
    connect(openAction, &QAction::triggered, this, &MainWindow::chooseOpenFile);

    auto *insertImageAction = new QAction(tr("Insert &image…"), this);
    insertImageAction->setObjectName(QStringLiteral("insertImageAction"));
    registerShortcut(insertImageAction, {});
    connect(insertImageAction,
        &QAction::triggered,
        this,
        &MainWindow::chooseInsertImage);

    m_saveAction = new QAction(tr("&Save"), this);
    m_saveAction->setObjectName(QStringLiteral("saveAction"));
    registerShortcut(m_saveAction, QKeySequence(QKeySequence::Save));
    connect(m_saveAction,
        &QAction::triggered,
        this,
        [this]()
        {
            save();
        });

    auto *saveAsAction = new QAction(tr("Save &As…"), this);
    saveAsAction->setObjectName(QStringLiteral("saveAsAction"));
    registerShortcut(saveAsAction, QKeySequence(QKeySequence::SaveAs));
    connect(saveAsAction,
        &QAction::triggered,
        this,
        [this]()
        {
            saveAs();
        });

    auto *exportGifAction = new QAction(tr("Export animated &GIF…"), this);
    exportGifAction->setObjectName(QStringLiteral("exportGifAction"));
    registerShortcut(exportGifAction, QKeySequence(QStringLiteral("Ctrl+E")));
    connect(exportGifAction, &QAction::triggered, this, &MainWindow::exportGif);

    auto *exportWebPAction = new QAction(tr("Export animated &WebP…"), this);
    exportWebPAction->setObjectName(QStringLiteral("exportWebPAction"));
    registerShortcut(exportWebPAction, {});
    connect(
        exportWebPAction, &QAction::triggered, this, &MainWindow::exportWebP);

    auto *exportPngAction =
        new QAction(tr("Export current frame as &image…"), this);
    exportPngAction->setObjectName(QStringLiteral("exportPngAction"));
    registerShortcut(exportPngAction, {});
    connect(
        exportPngAction, &QAction::triggered, this, &MainWindow::exportImage);

    auto *quitAction = new QAction(tr("&Quit"), this);
    quitAction->setObjectName(QStringLiteral("quitAction"));
    registerShortcut(quitAction, QKeySequence(QKeySequence::Quit));
    connect(quitAction, &QAction::triggered, this, &QWidget::close);

    auto *settingsAction = new QAction(tr("&Settings…"), this);
    settingsAction->setObjectName(QStringLiteral("settingsAction"));
    settingsAction->setIcon(Icons::icon(IconGlyph::Settings));
    settingsAction->setToolTip(tr("Settings"));
    registerShortcut(settingsAction, QKeySequence(QKeySequence::Preferences));
    settingsAction->setMenuRole(QAction::PreferencesRole);
    connect(settingsAction,
        &QAction::triggered,
        this,
        [this]()
        {
            SettingsDialog dialog(this, m_shortcutActions);
            connect(&dialog,
                &SettingsDialog::animateWhileDrawingChanged,
                m_canvas,
                &CanvasWidget::setAnimateWhileDrawing);
            connect(&dialog,
                &SettingsDialog::wobbleAnimationEnabledChanged,
                this,
                &MainWindow::applyWobbleAnimationEnabled);
            dialog.exec();
        });

    auto *importPresetAction = new QAction(tr("&Import WWP preset…"), this);
    importPresetAction->setObjectName(QStringLiteral("importWwpPresetAction"));
    registerShortcut(importPresetAction, {});
    connect(importPresetAction,
        &QAction::triggered,
        this,
        &MainWindow::importWwpPreset);

    auto *exportPresetAction = new QAction(tr("&Export WWP preset…"), this);
    exportPresetAction->setObjectName(QStringLiteral("exportWwpPresetAction"));
    registerShortcut(exportPresetAction, {});
    connect(exportPresetAction,
        &QAction::triggered,
        this,
        &MainWindow::exportWwpPreset);

    auto *checkForUpdatesAction = new QAction(tr("Check for &Updates…"), this);
    checkForUpdatesAction->setObjectName(
        QStringLiteral("checkForUpdatesAction"));

    auto *helpAction = new QAction(tr("Ugurugu &Help"), this);
    helpAction->setObjectName(QStringLiteral("helpAction"));
    registerShortcut(helpAction, QKeySequence(Qt::Key_F1));
    connect(helpAction, &QAction::triggered, this, &MainWindow::showHelp);

    auto *aboutAction = new QAction(tr("&About Ugurugu"), this);
    aboutAction->setObjectName(QStringLiteral("aboutAction"));
    // macOS moves this into the application menu. Naming the role leaves that
    // to the platform instead of to Qt's guess at the action's text.
    aboutAction->setMenuRole(QAction::AboutRole);
    registerShortcut(aboutAction, {});
    connect(aboutAction, &QAction::triggered, this, &MainWindow::showAbout);

    QAction *undoAction = new QAction(tr("&Undo"), this);
    undoAction->setObjectName(QStringLiteral("undoAction"));
    undoAction->setIcon(Icons::icon(IconGlyph::Undo));
    registerShortcut(undoAction, QKeySequence(QKeySequence::Undo));

    QAction *redoAction = new QAction(tr("&Redo"), this);
    redoAction->setObjectName(QStringLiteral("redoAction"));
    redoAction->setIcon(Icons::icon(IconGlyph::Redo));
    registerShortcut(redoAction, QKeySequence(QKeySequence::Redo));

    // A pending selection transform is undone before any history entry, so the
    // visible undo action reflects that first and the stack's own state only
    // when nothing is pending.
    const auto syncHistoryActions = [this, undoAction, redoAction]()
    {
        DocumentUndoStack *stack = m_controller.undoStack();
        const bool pending = m_canvas->hasPendingSelectionTransform();
        undoAction->setEnabled(pending || stack->canUndo());
        undoAction->setText(
            pending ? tr("Undo Selection Transform") : stack->undoText());
        redoAction->setEnabled(!pending && stack->canRedo());
        redoAction->setText(stack->redoText());
    };
    for (const auto signal : {&DocumentUndoStack::canUndoChanged,
             &DocumentUndoStack::canRedoChanged})
    {
        connect(m_controller.undoStack(), signal, this, syncHistoryActions);
    }
    for (const auto signal : {&DocumentUndoStack::undoTextChanged,
             &DocumentUndoStack::redoTextChanged})
    {
        connect(m_controller.undoStack(), signal, this, syncHistoryActions);
    }
    connect(m_canvas,
        &CanvasWidget::selectionTransformSessionChanged,
        this,
        syncHistoryActions);
    syncHistoryActions();
    connect(undoAction,
        &QAction::triggered,
        this,
        [this]()
        {
            if (m_canvas->hasPendingSelectionTransform())
            {
                m_canvas->cancelSelectionTransform();
                return;
            }
            m_controller.undoStack()->undo();
        });
    connect(redoAction,
        &QAction::triggered,
        this,
        [this]()
        {
            if (m_canvas->hasPendingSelectionTransform())
            {
                return;
            }
            m_controller.undoStack()->redo();
        });
    syncHistoryActions();

    auto *resizeCanvasAction = new QAction(tr("Change canvas size…"), this);
    resizeCanvasAction->setObjectName(QStringLiteral("resizeCanvasAction"));
    registerShortcut(resizeCanvasAction, {});
    connect(resizeCanvasAction,
        &QAction::triggered,
        this,
        &MainWindow::resizeCanvas);

    auto *resizeImageAction = new QAction(tr("Change image size…"), this);
    resizeImageAction->setObjectName(QStringLiteral("resizeImageAction"));
    registerShortcut(resizeImageAction, {});
    connect(
        resizeImageAction, &QAction::triggered, this, &MainWindow::resizeImage);

    auto *backgroundAction = new QAction(tr("Canvas background…"), this);
    backgroundAction->setObjectName(QStringLiteral("backgroundAction"));
    registerShortcut(backgroundAction, {});
    connect(backgroundAction,
        &QAction::triggered,
        this,
        &MainWindow::chooseBackgroundColor);

    m_moveSelectionAction = new QAction(tr("Move selection"), this);
    m_moveSelectionAction->setObjectName(QStringLiteral("moveSelectionAction"));
    m_moveSelectionAction->setCheckable(true);
    m_moveSelectionAction->setIcon(Icons::toggleIcon(IconGlyph::Move));
    m_moveSelectionAction->setToolTip(tr("Move selected content by dragging"));
    registerShortcut(m_moveSelectionAction, {});
    connect(m_moveSelectionAction,
        &QAction::toggled,
        m_canvas,
        &CanvasWidget::setSelectionMoveMode);
    connect(m_canvas,
        &CanvasWidget::selectionMoveModeChanged,
        m_moveSelectionAction,
        &QAction::setChecked);

    m_scaleSelectionAction = new QAction(tr("Scale selection…"), this);
    m_scaleSelectionAction->setObjectName(
        QStringLiteral("scaleSelectionAction"));
    m_scaleSelectionAction->setIcon(Icons::icon(IconGlyph::Scale));
    m_scaleSelectionAction->setToolTip(tr("Scale selected content"));
    m_scaleSelectionAction->setEnabled(false);
    registerShortcut(m_scaleSelectionAction, {});
    connect(m_scaleSelectionAction,
        &QAction::triggered,
        this,
        &MainWindow::scaleSelection);

    m_rotateSelectionAction = new QAction(tr("Rotate selection…"), this);
    m_rotateSelectionAction->setObjectName(
        QStringLiteral("rotateSelectionAction"));
    m_rotateSelectionAction->setIcon(Icons::icon(IconGlyph::Rotate));
    m_rotateSelectionAction->setToolTip(tr("Rotate selected content"));
    m_rotateSelectionAction->setEnabled(false);
    registerShortcut(m_rotateSelectionAction, {});
    connect(m_rotateSelectionAction,
        &QAction::triggered,
        this,
        &MainWindow::rotateSelection);

    m_selectAllAction = new QAction(tr("Select &all"), this);
    m_selectAllAction->setObjectName(QStringLiteral("selectAllAction"));
    m_selectAllAction->setToolTip(tr("Select the whole canvas"));
    registerShortcut(m_selectAllAction, QKeySequence(QKeySequence::SelectAll));
    connect(m_selectAllAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::selectAll);

    m_invertSelectionAction = new QAction(tr("&Invert selection"), this);
    m_invertSelectionAction->setObjectName(
        QStringLiteral("invertSelectionAction"));
    m_invertSelectionAction->setToolTip(tr("Invert the selected area"));
    m_invertSelectionAction->setEnabled(false);
    registerShortcut(
        m_invertSelectionAction, QKeySequence(QStringLiteral("Ctrl+Shift+I")));
    connect(m_invertSelectionAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::invertSelection);

    m_cutSelectionAction = new QAction(tr("Cu&t"), this);
    m_cutSelectionAction->setObjectName(QStringLiteral("cutSelectionAction"));
    m_cutSelectionAction->setToolTip(
        tr("Copy the selection to the clipboard and delete it"));
    m_cutSelectionAction->setEnabled(false);
    registerShortcut(m_cutSelectionAction, QKeySequence(QKeySequence::Cut));
    connect(m_cutSelectionAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::cutSelection);

    m_copySelectionAction = new QAction(tr("&Copy"), this);
    m_copySelectionAction->setObjectName(QStringLiteral("copySelectionAction"));
    m_copySelectionAction->setIcon(Icons::icon(IconGlyph::Copy));
    m_copySelectionAction->setToolTip(
        tr("Copy the selection to a new layer and the clipboard"));
    m_copySelectionAction->setEnabled(false);
    registerShortcut(m_copySelectionAction, QKeySequence(QKeySequence::Copy));
    connect(m_copySelectionAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::copySelection);

    m_pasteAction = new QAction(tr("&Paste"), this);
    m_pasteAction->setObjectName(QStringLiteral("pasteAction"));
    m_pasteAction->setToolTip(tr("Paste the clipboard as a new layer"));
    registerShortcut(m_pasteAction, QKeySequence(QKeySequence::Paste));
    connect(m_pasteAction,
        &QAction::triggered,
        this,
        &MainWindow::pasteFromClipboard);

    m_editStrokePropertiesAction =
        new QAction(tr("Edit selected stroke properties…"), this);
    m_editStrokePropertiesAction->setObjectName(
        QStringLiteral("editStrokePropertiesAction"));
    m_editStrokePropertiesAction->setIcon(Icons::icon(IconGlyph::Brush));
    m_editStrokePropertiesAction->setToolTip(
        tr("Change the color or width of selected strokes"));
    m_editStrokePropertiesAction->setEnabled(false);
    registerShortcut(m_editStrokePropertiesAction, {});
    connect(m_editStrokePropertiesAction,
        &QAction::triggered,
        this,
        &MainWindow::editSelectedStrokeProperties);

    m_flipSelectionHorizontalAction =
        new QAction(tr("Flip selection horizontally"), this);
    m_flipSelectionHorizontalAction->setObjectName(
        QStringLiteral("flipSelectionHorizontalAction"));
    m_flipSelectionHorizontalAction->setIcon(
        Icons::icon(IconGlyph::MirrorHorizontal));
    m_flipSelectionHorizontalAction->setToolTip(
        tr("Flip selected content horizontally"));
    registerShortcut(m_flipSelectionHorizontalAction, {});
    connect(m_flipSelectionHorizontalAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::flipSelectionHorizontally);

    m_flipSelectionVerticalAction =
        new QAction(tr("Flip selection vertically"), this);
    m_flipSelectionVerticalAction->setObjectName(
        QStringLiteral("flipSelectionVerticalAction"));
    m_flipSelectionVerticalAction->setIcon(
        Icons::icon(IconGlyph::MirrorVertical));
    m_flipSelectionVerticalAction->setToolTip(
        tr("Flip selected content vertically"));
    registerShortcut(m_flipSelectionVerticalAction, {});
    connect(m_flipSelectionVerticalAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::flipSelectionVertically);

    m_applySelectionTransformAction = new QAction(tr("Apply transform"), this);
    m_applySelectionTransformAction->setObjectName(
        QStringLiteral("applySelectionTransformAction"));
    m_applySelectionTransformAction->setIcon(Icons::icon(IconGlyph::Confirm));
    m_applySelectionTransformAction->setToolTip(
        tr("Apply selection transform (Enter)"));
    m_applySelectionTransformAction->setEnabled(false);
    registerShortcut(m_applySelectionTransformAction,
        QKeySequence(QStringLiteral("Return")));
    connect(m_applySelectionTransformAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::applySelectionTransform);

    m_cancelSelectionTransformAction =
        new QAction(tr("Cancel transform"), this);
    m_cancelSelectionTransformAction->setObjectName(
        QStringLiteral("cancelSelectionTransformAction"));
    m_cancelSelectionTransformAction->setIcon(Icons::icon(IconGlyph::Cancel));
    m_cancelSelectionTransformAction->setToolTip(
        tr("Cancel selection transform (Esc)"));
    m_cancelSelectionTransformAction->setEnabled(false);
    registerShortcut(m_cancelSelectionTransformAction, {});
    connect(m_cancelSelectionTransformAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::cancelSelectionTransform);

    auto *selectionSamplingGroup = new QActionGroup(this);
    selectionSamplingGroup->setExclusive(true);
    auto *smoothSelectionSamplingAction = new QAction(tr("Smooth"), this);
    smoothSelectionSamplingAction->setObjectName(
        QStringLiteral("smoothSelectionSamplingAction"));
    smoothSelectionSamplingAction->setCheckable(true);
    const QString smoothSamplingDescription =
        tr("Interpolate neighboring pixels during selection transforms");
    smoothSelectionSamplingAction->setToolTip(smoothSamplingDescription);
    smoothSelectionSamplingAction->setStatusTip(smoothSamplingDescription);
    smoothSelectionSamplingAction->setData(
        static_cast<int>(SamplingMode::Smooth));
    selectionSamplingGroup->addAction(smoothSelectionSamplingAction);
    auto *pixelBasedSelectionSamplingAction =
        new QAction(tr("Pixel-based"), this);
    pixelBasedSelectionSamplingAction->setObjectName(
        QStringLiteral("pixelBasedSelectionSamplingAction"));
    pixelBasedSelectionSamplingAction->setCheckable(true);
    const QString pixelBasedSamplingDescription =
        tr("Skip interpolation without removing existing transparency; edges "
           "may look stepped");
    pixelBasedSelectionSamplingAction->setToolTip(
        pixelBasedSamplingDescription);
    pixelBasedSelectionSamplingAction->setStatusTip(
        pixelBasedSamplingDescription);
    pixelBasedSelectionSamplingAction->setData(
        static_cast<int>(SamplingMode::Nearest));
    selectionSamplingGroup->addAction(pixelBasedSelectionSamplingAction);
    connect(selectionSamplingGroup,
        &QActionGroup::triggered,
        m_canvas,
        [this](QAction *action)
        {
            m_canvas->setSelectionTransformSampling(
                static_cast<SamplingMode>(action->data().toInt()));
        });
    const auto syncSelectionSamplingActions =
        [smoothSelectionSamplingAction, pixelBasedSelectionSamplingAction](
            SamplingMode sampling)
    {
        smoothSelectionSamplingAction->setChecked(
            sampling == SamplingMode::Smooth);
        pixelBasedSelectionSamplingAction->setChecked(
            sampling == SamplingMode::Nearest);
    };
    connect(m_canvas,
        &CanvasWidget::selectionTransformSamplingChanged,
        this,
        syncSelectionSamplingActions);
    syncSelectionSamplingActions(m_canvas->selectionTransformSampling());

    m_deleteSelectionAction = new QAction(tr("Delete selected content"), this);
    m_deleteSelectionAction->setObjectName(
        QStringLiteral("deleteSelectionAction"));
    m_deleteSelectionAction->setIcon(Icons::icon(IconGlyph::Delete));
    m_deleteSelectionAction->setToolTip(tr("Delete selected content"));
    registerShortcut(m_deleteSelectionAction, QKeySequence(Qt::Key_Delete));
    connect(m_deleteSelectionAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::deleteSelection);

    m_fillSelectionAction = new QAction(tr("Fill selection"), this);
    m_fillSelectionAction->setObjectName(QStringLiteral("fillSelectionAction"));
    m_fillSelectionAction->setIcon(Icons::icon(IconGlyph::Bucket));
    m_fillSelectionAction->setToolTip(
        tr("Fill the selected area with the brush color"));
    registerShortcut(
        m_fillSelectionAction, QKeySequence(Qt::AltModifier | Qt::Key_Delete));
    connect(m_fillSelectionAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::fillSelection);

    m_deselectSelectionAction = new QAction(tr("Deselect"), this);
    m_deselectSelectionAction->setObjectName(
        QStringLiteral("deselectSelectionAction"));
    m_deselectSelectionAction->setIcon(Icons::icon(IconGlyph::Deselect));
    m_deselectSelectionAction->setToolTip(tr("Deselect (Ctrl+D)"));
    registerShortcut(
        m_deselectSelectionAction, QKeySequence(QStringLiteral("Ctrl+D")));
    connect(m_deselectSelectionAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::deselectSelection);

    auto *escapeCanvasAction =
        new QAction(tr("Cancel current canvas action"), this);
    escapeCanvasAction->setObjectName(QStringLiteral("escapeCanvasAction"));
    escapeCanvasAction->setShortcutContext(Qt::WindowShortcut);
    registerShortcut(escapeCanvasAction, QKeySequence(Qt::Key_Escape));
    connect(escapeCanvasAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::handleEscape);

    const auto syncSelectionActions = [this](bool hasArea, bool hasContent)
    {
        m_moveSelectionAction->setEnabled(hasContent);
        m_scaleSelectionAction->setEnabled(hasContent);
        m_rotateSelectionAction->setEnabled(hasContent);
        m_cutSelectionAction->setEnabled(hasContent);
        m_copySelectionAction->setEnabled(hasContent);
        m_invertSelectionAction->setEnabled(hasArea);
        m_editStrokePropertiesAction->setEnabled(
            hasContent && m_canvas->hasEditableStrokeSelection()
            && !m_canvas->hasSelectionTransformSession());
        m_flipSelectionHorizontalAction->setEnabled(hasContent);
        m_flipSelectionVerticalAction->setEnabled(hasContent);
        m_deleteSelectionAction->setEnabled(hasContent);
        // Filling needs an area, not content: filling an empty selection is
        // the whole point of the command.
        m_fillSelectionAction->setEnabled(hasArea);
        m_deselectSelectionAction->setEnabled(hasArea);
        if (!hasContent)
        {
            m_moveSelectionAction->setChecked(false);
        }
    };
    connect(m_canvas,
        &CanvasWidget::selectionAvailabilityChanged,
        this,
        syncSelectionActions);
    syncSelectionActions(
        m_canvas->hasSelection(), m_canvas->hasTransformableSelection());
    const auto syncSelectionTransformSession = [this](bool active, bool dirty)
    {
        m_applySelectionTransformAction->setEnabled(active && dirty);
        m_cancelSelectionTransformAction->setEnabled(active);
        m_editStrokePropertiesAction->setEnabled(
            !active && m_canvas->hasEditableStrokeSelection());
    };
    connect(m_canvas,
        &CanvasWidget::selectionTransformSessionChanged,
        this,
        syncSelectionTransformSession);
    syncSelectionTransformSession(m_canvas->hasSelectionTransformSession(),
        m_canvas->hasPendingSelectionTransform());

    auto *selectionBar = new SelectionActionBar(m_canvas);
    selectionBar->addActionButton(m_moveSelectionAction);
    selectionBar->addActionButton(m_scaleSelectionAction);
    selectionBar->addActionButton(m_rotateSelectionAction);
    selectionBar->addSeparator();
    selectionBar->addActionButton(m_flipSelectionHorizontalAction);
    selectionBar->addActionButton(m_flipSelectionVerticalAction);
    selectionBar->addSeparator();
    selectionBar->addActionButton(m_applySelectionTransformAction);
    selectionBar->addActionButton(m_cancelSelectionTransformAction);
    selectionBar->addSeparator();
    selectionBar->addActionButton(m_copySelectionAction);
    selectionBar->addActionButton(m_editStrokePropertiesAction);
    selectionBar->addActionButton(m_fillSelectionAction);
    selectionBar->addActionButton(m_deleteSelectionAction);
    selectionBar->addSeparator();
    selectionBar->addActionButton(m_deselectSelectionAction);
    m_canvas->setSelectionActionBar(selectionBar);

    auto *clearLayerAction = new QAction(tr("Clear active layer"), this);
    clearLayerAction->setObjectName(QStringLiteral("clearLayerAction"));
    clearLayerAction->setEnabled(
        !m_controller.document().activeLayerId.isNull());
    registerShortcut(clearLayerAction, {});
    connect(clearLayerAction,
        &QAction::triggered,
        this,
        [this]()
        {
            m_controller.clearLayer(m_controller.document().activeLayerId);
        });
    connect(&m_controller,
        &DocumentController::activeLayerChanged,
        clearLayerAction,
        [clearLayerAction](const QUuid &id)
        {
            clearLayerAction->setEnabled(!id.isNull());
        });

    auto *zoomInAction = new QAction(tr("Zoom &in"), this);
    zoomInAction->setObjectName(QStringLiteral("zoomInAction"));
    registerShortcut(zoomInAction,
        QKeySequence(QKeySequence::ZoomIn),
        {QKeySequence(QStringLiteral("Ctrl+="))});
    connect(zoomInAction, &QAction::triggered, m_canvas, &CanvasWidget::zoomIn);

    auto *zoomOutAction = new QAction(tr("Zoom &out"), this);
    zoomOutAction->setObjectName(QStringLiteral("zoomOutAction"));
    registerShortcut(zoomOutAction, QKeySequence(QKeySequence::ZoomOut));
    connect(
        zoomOutAction, &QAction::triggered, m_canvas, &CanvasWidget::zoomOut);

    auto *actualSizeAction = new QAction(tr("Actual &pixels"), this);
    actualSizeAction->setObjectName(QStringLiteral("actualSizeAction"));
    actualSizeAction->setToolTip(tr("Show the canvas at 100%"));
    registerShortcut(actualSizeAction, QKeySequence(QStringLiteral("Ctrl+1")));
    connect(actualSizeAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::resetZoom);

    auto *fitAction = new QAction(tr("&Fit canvas"), this);
    fitAction->setObjectName(QStringLiteral("fitAction"));
    fitAction->setIcon(Icons::icon(IconGlyph::FitView));
    fitAction->setToolTip(tr("Fit the whole canvas in the window"));
    registerShortcut(fitAction, QKeySequence(QStringLiteral("Ctrl+0")));
    connect(
        fitAction, &QAction::triggered, m_canvas, &CanvasWidget::fitToWindow);

    auto *rotateCanvasLeftAction = new QAction(tr("Rotate canvas &left"), this);
    rotateCanvasLeftAction->setObjectName(
        QStringLiteral("rotateCanvasLeftAction"));
    rotateCanvasLeftAction->setIcon(Icons::icon(IconGlyph::Undo));
    rotateCanvasLeftAction->setToolTip(
        tr("Rotate the canvas view 5° counterclockwise"));
    registerShortcut(rotateCanvasLeftAction, QKeySequence(QStringLiteral("-")));
    connect(rotateCanvasLeftAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::rotateCanvasLeft);

    auto *rotateCanvasRightAction =
        new QAction(tr("Rotate canvas &right"), this);
    rotateCanvasRightAction->setObjectName(
        QStringLiteral("rotateCanvasRightAction"));
    rotateCanvasRightAction->setIcon(Icons::icon(IconGlyph::Redo));
    rotateCanvasRightAction->setToolTip(
        tr("Rotate the canvas view 5° clockwise"));
    registerShortcut(
        rotateCanvasRightAction, QKeySequence(QStringLiteral("^")));
    connect(rotateCanvasRightAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::rotateCanvasRight);

    auto *resetCanvasRotationAction =
        new QAction(tr("Rese&t canvas rotation"), this);
    resetCanvasRotationAction->setObjectName(
        QStringLiteral("resetCanvasRotationAction"));
    resetCanvasRotationAction->setIcon(Icons::icon(IconGlyph::Rotate));
    resetCanvasRotationAction->setToolTip(
        tr("Reset the canvas view to 0° without changing zoom or mirroring"));
    registerShortcut(resetCanvasRotationAction, {});
    connect(resetCanvasRotationAction,
        &QAction::triggered,
        m_canvas,
        &CanvasWidget::resetCanvasRotation);

    m_mirrorCanvasAction = new QAction(tr("Flip canvas horizontally"), this);
    m_mirrorCanvasAction->setObjectName(QStringLiteral("mirrorCanvasAction"));
    m_mirrorCanvasAction->setCheckable(true);
    m_mirrorCanvasAction->setIcon(
        Icons::toggleIcon(IconGlyph::MirrorHorizontal));
    m_mirrorCanvasAction->setToolTip(
        tr("Temporarily flip the canvas horizontally"));
    registerShortcut(m_mirrorCanvasAction, QKeySequence(QStringLiteral("M")));
    connect(m_mirrorCanvasAction,
        &QAction::toggled,
        m_canvas,
        &CanvasWidget::setCanvasMirrored);
    connect(m_canvas,
        &CanvasWidget::canvasMirroredChanged,
        m_mirrorCanvasAction,
        &QAction::setChecked);

    m_playAction = new QAction(tr("&Animate preview"), this);
    m_playAction->setCheckable(true);
    m_playAction->setChecked(true);
    m_playAction->setIcon(Icons::toggleIcon(IconGlyph::Play));
    m_playAction->setObjectName(QStringLiteral("playAction"));
    registerShortcut(m_playAction, QKeySequence(QStringLiteral("P")));
    connect(
        m_playAction, &QAction::toggled, m_canvas, &CanvasWidget::setAnimating);
    connect(m_canvas,
        &CanvasWidget::animatingChanged,
        m_playAction,
        &QAction::setChecked);

    applyWobbleAnimationEnabled(SettingsDialog::wobbleAnimationEnabled());

    m_brushAction = new QAction(tr("&Brush"), this);
    m_brushAction->setCheckable(true);
    m_brushAction->setIcon(Icons::toggleIcon(IconGlyph::Brush));
    m_brushAction->setObjectName(QStringLiteral("brushAction"));
    registerShortcut(m_brushAction, QKeySequence(QStringLiteral("B")));

    m_eraserAction = new QAction(tr("&Eraser"), this);
    m_eraserAction->setCheckable(true);
    m_eraserAction->setIcon(Icons::toggleIcon(IconGlyph::Eraser));
    m_eraserAction->setObjectName(QStringLiteral("eraserAction"));
    registerShortcut(m_eraserAction, QKeySequence(QStringLiteral("E")));

    m_lassoAction = new QAction(tr("&Area select"), this);
    m_lassoAction->setCheckable(true);
    m_lassoAction->setIcon(Icons::toggleIcon(IconGlyph::Lasso));
    m_lassoAction->setObjectName(QStringLiteral("lassoAction"));
    registerShortcut(m_lassoAction, QKeySequence(QStringLiteral("L")));

    m_wandAction = new QAction(tr("Auto se&lect"), this);
    m_wandAction->setCheckable(true);
    m_wandAction->setIcon(Icons::toggleIcon(IconGlyph::Wand));
    m_wandAction->setObjectName(QStringLiteral("wandAction"));
    registerShortcut(m_wandAction, QKeySequence(QStringLiteral("W")));

    m_bucketAction = new QAction(tr("Paint &bucket"), this);
    m_bucketAction->setCheckable(true);
    m_bucketAction->setIcon(Icons::toggleIcon(IconGlyph::Bucket));
    m_bucketAction->setObjectName(QStringLiteral("bucketAction"));
    registerShortcut(m_bucketAction, QKeySequence(QStringLiteral("G")));

    m_textAction = new QAction(tr("Te&xt"), this);
    m_textAction->setCheckable(true);
    m_textAction->setIcon(Icons::toggleIcon(IconGlyph::Text));
    m_textAction->setObjectName(QStringLiteral("textAction"));
    m_textAction->setToolTip(
        tr("Place wobbly text drawn with the brush color"));
    registerShortcut(m_textAction, QKeySequence(QStringLiteral("T")));

    m_eyedropperAction = new QAction(tr("E&yedropper"), this);
    m_eyedropperAction->setCheckable(true);
    m_eyedropperAction->setIcon(Icons::toggleIcon(IconGlyph::Eyedropper));
    m_eyedropperAction->setObjectName(QStringLiteral("eyedropperAction"));
    m_eyedropperAction->setToolTip(
        tr("Pick a color from the canvas. Alt+click does the same from any "
           "paint tool."));
    registerShortcut(m_eyedropperAction, QKeySequence(QStringLiteral("I")));

    // The animation bar is a fixture of the central layout rather than a dock,
    // so it needs its own toggle and its own persisted visibility.
    m_showTimelineAction = new QAction(tr("Show &animation bar"), this);
    m_showTimelineAction->setObjectName(QStringLiteral("showTimelineAction"));
    m_showTimelineAction->setCheckable(true);
    m_showTimelineAction->setChecked(timelineVisibleSetting());
    registerShortcut(
        m_showTimelineAction, QKeySequence(QStringLiteral("Ctrl+T")));
    connect(m_showTimelineAction,
        &QAction::toggled,
        this,
        &MainWindow::setTimelineVisible);
    connect(m_timeline,
        &TimelineBar::collapseRequested,
        this,
        [this]()
        {
            m_showTimelineAction->setChecked(false);
        });

    auto *resetPanelLayoutAction = new QAction(tr("Reset panel layout"), this);
    resetPanelLayoutAction->setObjectName(
        QStringLiteral("resetPanelLayoutAction"));
    connect(resetPanelLayoutAction,
        &QAction::triggered,
        this,
        [this]()
        {
            m_paletteDockAreaManager->resetCollapsedAreas();
            resetDockLayout();
        });

    auto *toolGroup = new QActionGroup(this);
    toolGroup->setExclusive(true);
    toolGroup->addAction(m_brushAction);
    toolGroup->addAction(m_eraserAction);
    toolGroup->addAction(m_lassoAction);
    toolGroup->addAction(m_wandAction);
    toolGroup->addAction(m_bucketAction);
    toolGroup->addAction(m_textAction);
    toolGroup->addAction(m_eyedropperAction);
    connect(m_brushAction,
        &QAction::triggered,
        this,
        [this]()
        {
            m_canvas->setTool(CanvasWidget::Tool::Brush);
        });
    connect(m_eraserAction,
        &QAction::triggered,
        this,
        [this]()
        {
            m_canvas->setTool(CanvasWidget::Tool::Eraser);
        });
    connect(m_lassoAction,
        &QAction::triggered,
        this,
        [this]()
        {
            m_canvas->setTool(CanvasWidget::Tool::Lasso);
        });
    connect(m_wandAction,
        &QAction::triggered,
        this,
        [this]()
        {
            m_canvas->setTool(CanvasWidget::Tool::Wand);
        });
    connect(m_bucketAction,
        &QAction::triggered,
        this,
        [this]()
        {
            m_canvas->setTool(CanvasWidget::Tool::Bucket);
        });
    connect(m_textAction,
        &QAction::triggered,
        this,
        [this]()
        {
            m_canvas->setTool(CanvasWidget::Tool::Text);
        });
    connect(m_eyedropperAction,
        &QAction::triggered,
        this,
        [this]()
        {
            m_canvas->setTool(CanvasWidget::Tool::Eyedropper);
        });
    const auto syncToolAction = [this](CanvasWidget::Tool tool)
    {
        switch (tool)
        {
        case CanvasWidget::Tool::Brush:
            m_brushAction->setChecked(true);
            break;
        case CanvasWidget::Tool::Eraser:
            m_eraserAction->setChecked(true);
            break;
        case CanvasWidget::Tool::Lasso:
            m_lassoAction->setChecked(true);
            break;
        case CanvasWidget::Tool::Wand:
            m_wandAction->setChecked(true);
            break;
        case CanvasWidget::Tool::Bucket:
            m_bucketAction->setChecked(true);
            break;
        case CanvasWidget::Tool::Text:
            m_textAction->setChecked(true);
            break;
        case CanvasWidget::Tool::Eyedropper:
            m_eyedropperAction->setChecked(true);
            break;
        }
    };
    syncToolAction(m_canvas->tool());
    connect(m_canvas, &CanvasWidget::toolChanged, this, syncToolAction);

    ShortcutBinding::resolveAliasConflicts(m_shortcutActions);

    addAction(newAction);
    addAction(openAction);
    addAction(m_saveAction);
    addAction(saveAsAction);
    addAction(exportGifAction);
    addAction(exportWebPAction);
    addAction(exportPngAction);
    addAction(quitAction);
    addAction(undoAction);
    addAction(redoAction);
    addAction(resizeImageAction);
    addAction(resizeCanvasAction);
    addAction(m_selectAllAction);
    addAction(m_invertSelectionAction);
    addAction(m_cutSelectionAction);
    addAction(m_copySelectionAction);
    addAction(m_pasteAction);
    addAction(m_moveSelectionAction);
    addAction(m_scaleSelectionAction);
    addAction(m_rotateSelectionAction);
    addAction(m_flipSelectionHorizontalAction);
    addAction(m_flipSelectionVerticalAction);
    addAction(m_applySelectionTransformAction);
    addAction(m_cancelSelectionTransformAction);
    addAction(m_editStrokePropertiesAction);
    addAction(m_fillSelectionAction);
    addAction(m_deleteSelectionAction);
    addAction(m_deselectSelectionAction);
    addAction(escapeCanvasAction);
    addAction(clearLayerAction);
    addAction(zoomInAction);
    addAction(zoomOutAction);
    addAction(actualSizeAction);
    addAction(fitAction);
    addAction(rotateCanvasLeftAction);
    addAction(rotateCanvasRightAction);
    addAction(resetCanvasRotationAction);
    addAction(m_mirrorCanvasAction);
    addAction(m_playAction);
    addAction(checkForUpdatesAction);
    addAction(helpAction);
    addAction(aboutAction);
    addAction(importPresetAction);
    addAction(exportPresetAction);
}

void MainWindow::createMenus()
{
    QMenu *fileMenu = menuBar()->addMenu(tr("&File"));
    fileMenu->addAction(findChild<QAction *>(QStringLiteral("newAction")));
    fileMenu->addAction(findChild<QAction *>(QStringLiteral("openAction")));
    fileMenu->addAction(
        findChild<QAction *>(QStringLiteral("insertImageAction")));
    fileMenu->addSeparator();
    fileMenu->addAction(m_saveAction);
    fileMenu->addAction(findChild<QAction *>(QStringLiteral("saveAsAction")));
    fileMenu->addSeparator();
    fileMenu->addAction(
        findChild<QAction *>(QStringLiteral("exportGifAction")));
    fileMenu->addAction(
        findChild<QAction *>(QStringLiteral("exportWebPAction")));
    fileMenu->addAction(
        findChild<QAction *>(QStringLiteral("exportPngAction")));
    fileMenu->addSeparator();
    fileMenu->addAction(findChild<QAction *>(QStringLiteral("quitAction")));

    QMenu *editMenu = menuBar()->addMenu(tr("&Edit"));
    editMenu->addAction(findChild<QAction *>(QStringLiteral("undoAction")));
    editMenu->addAction(findChild<QAction *>(QStringLiteral("redoAction")));
    editMenu->addSeparator();
    editMenu->addAction(m_cutSelectionAction);
    editMenu->addAction(m_copySelectionAction);
    editMenu->addAction(m_pasteAction);
    editMenu->addSeparator();
    editMenu->addAction(
        findChild<QAction *>(QStringLiteral("resizeImageAction")));
    editMenu->addAction(
        findChild<QAction *>(QStringLiteral("resizeCanvasAction")));
    editMenu->addAction(
        findChild<QAction *>(QStringLiteral("backgroundAction")));
    editMenu->addSeparator();
    QMenu *selectionMenu = editMenu->addMenu(tr("&Selection"));
    selectionMenu->addAction(m_selectAllAction);
    selectionMenu->addAction(m_invertSelectionAction);
    selectionMenu->addAction(m_deselectSelectionAction);
    selectionMenu->addSeparator();
    selectionMenu->addAction(m_moveSelectionAction);
    selectionMenu->addAction(m_scaleSelectionAction);
    selectionMenu->addAction(m_rotateSelectionAction);
    QMenu *selectionSamplingMenu =
        selectionMenu->addMenu(tr("Transform method"));
    selectionSamplingMenu->setObjectName(
        QStringLiteral("selectionTransformSamplingMenu"));
    selectionSamplingMenu->setToolTipsVisible(true);
    selectionSamplingMenu->addAction(
        findChild<QAction *>(QStringLiteral("smoothSelectionSamplingAction")));
    selectionSamplingMenu->addAction(findChild<QAction *>(
        QStringLiteral("pixelBasedSelectionSamplingAction")));
    selectionMenu->addSeparator();
    selectionMenu->addAction(m_flipSelectionHorizontalAction);
    selectionMenu->addAction(m_flipSelectionVerticalAction);
    selectionMenu->addSeparator();
    selectionMenu->addAction(m_applySelectionTransformAction);
    selectionMenu->addAction(m_cancelSelectionTransformAction);
    selectionMenu->addSeparator();
    selectionMenu->addAction(m_editStrokePropertiesAction);
    selectionMenu->addAction(m_fillSelectionAction);
    selectionMenu->addAction(m_deleteSelectionAction);
    editMenu->addSeparator();
    editMenu->addAction(
        findChild<QAction *>(QStringLiteral("clearLayerAction")));
    editMenu->addSeparator();
    editMenu->addAction(findChild<QAction *>(QStringLiteral("settingsAction")));

    QMenu *viewMenu = menuBar()->addMenu(tr("&View"));
    viewMenu->addAction(findChild<QAction *>(QStringLiteral("zoomInAction")));
    viewMenu->addAction(findChild<QAction *>(QStringLiteral("zoomOutAction")));
    viewMenu->addAction(
        findChild<QAction *>(QStringLiteral("actualSizeAction")));
    viewMenu->addAction(findChild<QAction *>(QStringLiteral("fitAction")));
    viewMenu->addSeparator();
    QMenu *rotationMenu = viewMenu->addMenu(tr("&Rotate canvas"));
    rotationMenu->addAction(
        findChild<QAction *>(QStringLiteral("rotateCanvasLeftAction")));
    rotationMenu->addAction(
        findChild<QAction *>(QStringLiteral("rotateCanvasRightAction")));
    rotationMenu->addAction(
        findChild<QAction *>(QStringLiteral("resetCanvasRotationAction")));
    viewMenu->addAction(m_mirrorCanvasAction);
    viewMenu->addSeparator();
    viewMenu->addAction(m_playAction);

    QMenu *toolMenu = menuBar()->addMenu(tr("&Tools"));
    toolMenu->addAction(m_brushAction);
    toolMenu->addAction(m_eraserAction);
    toolMenu->addAction(m_lassoAction);
    toolMenu->addAction(m_wandAction);
    toolMenu->addAction(m_bucketAction);
    toolMenu->addAction(m_textAction);
    toolMenu->addAction(m_eyedropperAction);
    toolMenu->addSeparator();
    toolMenu->addAction(
        findChild<QAction *>(QStringLiteral("importWwpPresetAction")));
    toolMenu->addAction(
        findChild<QAction *>(QStringLiteral("exportWwpPresetAction")));

    QMenu *windowMenu = menuBar()->addMenu(tr("&Window"));
    windowMenu->addAction(m_showTimelineAction);
    windowMenu->addSeparator();
    windowMenu->addAction(m_toolDock->toggleViewAction());
    windowMenu->addAction(m_colorDock->toggleViewAction());
    windowMenu->addAction(m_colorHistoryDock->toggleViewAction());
    windowMenu->addAction(m_wobbleDock->toggleViewAction());
    windowMenu->addAction(m_layerDock->toggleViewAction());
    windowMenu->addSeparator();
    windowMenu->addAction(
        findChild<QAction *>(QStringLiteral("resetPanelLayoutAction")));

    QMenu *helpMenu = menuBar()->addMenu(tr("&Help"));
    helpMenu->addAction(findChild<QAction *>(QStringLiteral("helpAction")));
    helpMenu->addSeparator();
    helpMenu->addAction(
        findChild<QAction *>(QStringLiteral("checkForUpdatesAction")));
    helpMenu->addSeparator();
    helpMenu->addAction(findChild<QAction *>(QStringLiteral("aboutAction")));
}

void MainWindow::showHelp()
{
    if (auto *existing = findChild<HelpDialog *>())
    {
        existing->show();
        existing->raise();
        existing->activateWindow();
        return;
    }
    auto *dialog = new HelpDialog(this);
    dialog->show();
}

void MainWindow::showAbout()
{
    if (auto *existing = findChild<AboutDialog *>())
    {
        existing->show();
        existing->raise();
        existing->activateWindow();
        return;
    }
    auto *dialog = new AboutDialog(this);
    dialog->show();
}

void MainWindow::createToolBars()
{
    auto *rail = new QToolBar(tr("Tools"), this);
    rail->setObjectName(QStringLiteral("ToolRail"));
    rail->setMovable(false);
    rail->setIconSize(QSize(24, 24));
    addToolBar(Qt::LeftToolBarArea, rail);

    const auto addRailButton =
        [rail](QAction *action, IconGlyph glyph, const QString &label)
    {
        auto *button = new PopoverToolButton(rail);
        button->setDefaultAction(action);
        // The button re-reads the action's iconText whenever the action
        // changes, and the default iconText keeps localized mnemonic
        // suffixes like "지우개(E)", widening the rail on the first click.
        action->setIconText(label);
        button->setText(label);
        button->setIconSize(rail->iconSize());
        button->setHoverGlyph(glyph);
        button->setToolButtonStyle(Qt::ToolButtonTextUnderIcon);
        button->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
        rail->addWidget(button);
        return button;
    };

    addRailButton(m_brushAction, IconGlyph::Brush, tr("Brush"));
    addRailButton(m_eraserAction, IconGlyph::Eraser, tr("Eraser"));
    addRailButton(m_lassoAction, IconGlyph::Lasso, tr("Area select"));
    addRailButton(m_wandAction, IconGlyph::Wand, tr("Auto select"));
    addRailButton(m_bucketAction, IconGlyph::Bucket, tr("Paint bucket"));
    addRailButton(m_textAction, IconGlyph::Text, tr("Text"));
    addRailButton(m_eyedropperAction, IconGlyph::Eyedropper, tr("Eyedropper"));

    connect(m_canvas,
        &CanvasWidget::brushPresetChanged,
        this,
        [this](const QString &)
        {
            if (m_canvas->tool() != CanvasWidget::Tool::Brush)
            {
                m_brushAction->trigger();
            }
        });
    connect(m_canvas,
        &CanvasWidget::eraserPresetChanged,
        this,
        [this](const QString &)
        {
            if (m_canvas->tool() != CanvasWidget::Tool::Eraser)
            {
                m_eraserAction->trigger();
            }
        });

    QToolBar *quick = addToolBar(tr("Quick access"));
    quick->setObjectName(QStringLiteral("PaintTools"));
    quick->setMovable(false);
    quick->setIconSize(QSize(22, 22));

    auto *panelsButton = new QToolButton(quick);
    panelsButton->setObjectName(QStringLiteral("panelsButton"));
    panelsButton->setIcon(Icons::icon(IconGlyph::Panels));
    panelsButton->setIconSize(quick->iconSize());
    panelsButton->setToolTip(tr("Open and close panels"));
    panelsButton->setAccessibleName(tr("Panels"));
    panelsButton->setPopupMode(QToolButton::InstantPopup);
    auto *panelsMenu = new QMenu(panelsButton);
    panelsMenu->addAction(m_showTimelineAction);
    panelsMenu->addSeparator();
    panelsMenu->addAction(m_toolDock->toggleViewAction());
    panelsMenu->addAction(m_colorDock->toggleViewAction());
    panelsMenu->addAction(m_colorHistoryDock->toggleViewAction());
    panelsMenu->addAction(m_wobbleDock->toggleViewAction());
    panelsMenu->addAction(m_layerDock->toggleViewAction());
    panelsMenu->addSeparator();
    panelsMenu->addAction(
        findChild<QAction *>(QStringLiteral("resetPanelLayoutAction")));
    panelsButton->setMenu(panelsMenu);
    quick->addWidget(panelsButton);
    quick->addSeparator();

    auto *quickSpacer = new QWidget(quick);
    quickSpacer->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Preferred);
    quick->addWidget(quickSpacer);

    quick->addAction(findChild<QAction *>(QStringLiteral("undoAction")));
    quick->addAction(findChild<QAction *>(QStringLiteral("redoAction")));
    quick->addSeparator();
    QAction *settingsAction =
        findChild<QAction *>(QStringLiteral("settingsAction"));
    quick->addAction(settingsAction);
    if (QWidget *settingsButton = quick->widgetForAction(settingsAction))
    {
        settingsButton->setObjectName(QStringLiteral("settingsButton"));
    }
}

void MainWindow::createStatusBar()
{
    m_recoveryFailureLabel = new QLabel(this);
    m_recoveryFailureLabel->setObjectName(
        QStringLiteral("recoveryFailureLabel"));
    m_recoveryFailureLabel->setVisible(false);
    statusBar()->addPermanentWidget(m_recoveryFailureLabel);

    m_pointerLabel = new QLabel(this);
    m_pointerLabel->setMinimumWidth(150);
    statusBar()->addPermanentWidget(m_pointerLabel);

    m_zoomSlider = new QSlider(Qt::Horizontal, this);
    m_zoomSlider->setObjectName(QStringLiteral("zoomSlider"));
    m_zoomSlider->setRange(0, zoomSliderSteps);
    m_zoomSlider->setFixedWidth(96);
    m_zoomSlider->setToolTip(tr("Canvas zoom"));
    m_zoomSlider->setAccessibleName(tr("Canvas zoom"));
    statusBar()->addPermanentWidget(m_zoomSlider);

    m_zoomSpin = new QSpinBox(this);
    m_zoomSpin->setObjectName(QStringLiteral("zoomPercentSpin"));
    m_zoomSpin->setRange(minimumZoomPercent, maximumZoomPercent);
    m_zoomSpin->setSuffix(tr("%"));
    installSuffixCaretGuard(m_zoomSpin);
    m_zoomSpin->setWrapping(false);
    m_zoomSpin->setFixedWidth(72);
    m_zoomSpin->setToolTip(tr("Canvas zoom percentage"));
    m_zoomSpin->setAccessibleName(tr("Canvas zoom percentage"));
    statusBar()->addPermanentWidget(m_zoomSpin);

    auto *rotateLeftButton = new QToolButton(this);
    rotateLeftButton->setObjectName(QStringLiteral("rotateCanvasLeftButton"));
    rotateLeftButton->setDefaultAction(
        findChild<QAction *>(QStringLiteral("rotateCanvasLeftAction")));
    rotateLeftButton->setIconSize(QSize(18, 18));
    statusBar()->addPermanentWidget(rotateLeftButton);

    auto *rotationSpin = new QDoubleSpinBox(this);
    rotationSpin->setObjectName(QStringLiteral("canvasRotationSpin"));
    rotationSpin->setRange(-180.0, 180.0);
    rotationSpin->setDecimals(1);
    rotationSpin->setSingleStep(5.0);
    rotationSpin->setSuffix(tr("°"));
    installSuffixCaretGuard(rotationSpin);
    rotationSpin->setWrapping(true);
    rotationSpin->setFixedWidth(76);
    rotationSpin->setToolTip(tr("Canvas rotation angle"));
    rotationSpin->setAccessibleName(tr("Canvas rotation angle"));
    statusBar()->addPermanentWidget(rotationSpin);

    auto *rotateRightButton = new QToolButton(this);
    rotateRightButton->setObjectName(QStringLiteral("rotateCanvasRightButton"));
    rotateRightButton->setDefaultAction(
        findChild<QAction *>(QStringLiteral("rotateCanvasRightAction")));
    rotateRightButton->setIconSize(QSize(18, 18));
    statusBar()->addPermanentWidget(rotateRightButton);

    auto *resetRotationButton = new QToolButton(this);
    resetRotationButton->setObjectName(
        QStringLiteral("resetCanvasRotationButton"));
    resetRotationButton->setDefaultAction(
        findChild<QAction *>(QStringLiteral("resetCanvasRotationAction")));
    resetRotationButton->setIconSize(QSize(18, 18));
    statusBar()->addPermanentWidget(resetRotationButton);

    auto *mirrorButton = new QToolButton(this);
    mirrorButton->setObjectName(QStringLiteral("mirrorCanvasButton"));
    mirrorButton->setDefaultAction(m_mirrorCanvasAction);
    mirrorButton->setIconSize(QSize(18, 18));
    mirrorButton->setText(tr("Mirror"));
    mirrorButton->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    statusBar()->addPermanentWidget(mirrorButton);

    auto *fitButton = new QToolButton(this);
    fitButton->setObjectName(QStringLiteral("fitCanvasButton"));
    fitButton->setDefaultAction(
        findChild<QAction *>(QStringLiteral("fitAction")));
    fitButton->setIconSize(QSize(18, 18));
    fitButton->setText(tr("Fit"));
    fitButton->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    statusBar()->addPermanentWidget(fitButton);

    connect(m_canvas,
        &CanvasWidget::pointerPositionChanged,
        this,
        [this](const QPointF &position, bool inside)
        {
            m_pointerLabel->setText(inside ? tr("x %1  y %2")
                                                 .arg(qRound(position.x()))
                                                 .arg(qRound(position.y()))
                                           : QString());
        });
    connect(m_zoomSlider,
        &QSlider::valueChanged,
        this,
        [this](int value)
        {
            m_canvas->setZoomPercent(zoomPercentFromSlider(value));
        });
    connect(m_zoomSpin,
        &QSpinBox::valueChanged,
        m_canvas,
        &CanvasWidget::setZoomPercent);
    connect(m_canvas,
        &CanvasWidget::zoomChanged,
        this,
        [this](int percent)
        {
            const QSignalBlocker sliderBlocker(m_zoomSlider);
            const QSignalBlocker spinBlocker(m_zoomSpin);
            m_zoomSlider->setValue(sliderFromZoomPercent(percent));
            m_zoomSpin->setValue(percent);
        });
    connect(rotationSpin,
        &QDoubleSpinBox::valueChanged,
        m_canvas,
        &CanvasWidget::setCanvasRotation);
    connect(m_canvas,
        &CanvasWidget::canvasRotationChanged,
        this,
        [rotationSpin](qreal degrees)
        {
            const QSignalBlocker blocker(rotationSpin);
            rotationSpin->setValue(degrees);
        });
    connect(m_canvas,
        &CanvasWidget::interactionMessage,
        this,
        [this](const QString &message)
        {
            statusBar()->showMessage(message, 4000);
        });
    const int initialZoom = qRound(m_canvas->zoom() * 100.0);
    m_zoomSlider->setValue(sliderFromZoomPercent(initialZoom));
    m_zoomSpin->setValue(initialZoom);
    statusBar()->showMessage(tr("Ready"), 2000);
}
}
