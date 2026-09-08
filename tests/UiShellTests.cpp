// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/UiTestHelpers.hpp"
#include "support/UiTestSuites.hpp"
#include "ui/PaletteDockTitleBar.hpp"
#include "ui/PopoverToolButton.hpp"

#include <QFutureWatcher>
#include <QImageReader>
#include <QScopeGuard>
#include <QScrollArea>
#include <QSemaphore>
#include <QStyleOptionViewItem>
#include <QTabBar>
#include <QThreadPool>
#include <QToolBar>
#include <QTranslator>
#include <QtConcurrentRun>

#include <atomic>
#include <new>

namespace ugurugu
{

namespace
{

// Every palette dock declares this as the width it can be narrowed to, so no
// panel's contents may report a larger minimum.
constexpr int declaredPaletteDockWidth = 150;

// QMainWindowLayout pools the tab bar of a dissolved dock group: it stays a
// visible child of the window, parked outside it, so only the rendered region
// tells whether a tab is actually left over.
bool hasOnScreenTabBar(const QMainWindow &window)
{
    return std::ranges::any_of(window.findChildren<QTabBar *>(),
        [](const QTabBar *tabBar)
        {
            return !tabBar->visibleRegion().isEmpty();
        });
}

bool visibleChildrenFit(const QWidget *container)
{
    return std::ranges::all_of(container->findChildren<QWidget *>(
                                   QString(), Qt::FindDirectChildrenOnly),
        [container](const QWidget *child)
        {
            return !child->isVisible()
                   || container->contentsRect().contains(child->geometry());
        });
}

QStringList dockTabOrder(const QMainWindow &window, const QDockWidget *dock)
{
    for (const QTabBar *tabBar : window.findChildren<QTabBar *>())
    {
        QStringList titles;
        for (int index = 0; index < tabBar->count(); ++index)
        {
            titles.append(tabBar->tabText(index));
        }
        if (titles.contains(dock->windowTitle()))
        {
            return titles;
        }
    }
    return {};
}

}

class UiShellTests final : public QObject
{
    Q_OBJECT

private slots:
    void init()
    {
        QSettings settings;
        settings.remove(QStringLiteral("drawingTools"));
        settings.remove(QStringLiteral("brush/recentColors"));
        settings.remove(QStringLiteral("brush/colorHistory"));
        settings.remove(QStringLiteral("canvas/strokeStabilization"));
        settings.remove(QStringLiteral("dock"));
        settings.remove(QStringLiteral("window"));
        settings.sync();
    }

    void cleanup()
    {
        QSettings settings;
        settings.remove(QStringLiteral("drawingTools"));
        settings.remove(QStringLiteral("brush/recentColors"));
        settings.remove(QStringLiteral("brush/colorHistory"));
        settings.remove(QStringLiteral("canvas/strokeStabilization"));
        settings.remove(QStringLiteral("dock"));
        settings.remove(QStringLiteral("window"));
        settings.sync();
    }

    void launchesAndEdits()
    {
        MainWindow window;
        window.resize(1100, 720);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        LayerDock *layerDock = window.findChild<LayerDock *>();
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QAction *insertImageAction =
            window.findChild<QAction *>(QStringLiteral("insertImageAction"));
        QVERIFY(canvas);
        QVERIFY(layerDock);
        QCOMPARE(window.tabPosition(Qt::LeftDockWidgetArea), QTabWidget::North);
        QCOMPARE(
            window.tabPosition(Qt::RightDockWidgetArea), QTabWidget::North);
        QVERIFY(undoAction);
        QVERIFY(insertImageAction);
        QCOMPARE(insertImageAction->text(), QStringLiteral("Insert &image…"));
        QVERIFY(!undoAction->isEnabled());
        const QList<BrushPresetButton *> presetButtons =
            window.findChildren<BrushPresetButton *>();
        QSpinBox *brushSizeSpin =
            window.findChild<QSpinBox *>(QStringLiteral("brushSizeSpin"));
        QVERIFY(brushSizeSpin);
        QCOMPARE(presetButtons.size(), BrushPresetCatalog::builtIns().size());
        BrushPresetButton *softAirbrushButton = nullptr;
        for (BrushPresetButton *button : presetButtons)
        {
            if (button->presetId() == QStringLiteral("soft-airbrush"))
            {
                softAirbrushButton = button;
            }
        }
        QVERIFY(softAirbrushButton);
        softAirbrushButton->click();
        QCOMPARE(canvas->brushPresetId(), QStringLiteral("soft-airbrush"));
        QVERIFY(softAirbrushButton->isChecked());
        QCOMPARE(brushSizeSpin->value(),
            qRound(BrushPresetCatalog::find(QStringLiteral("soft-airbrush"))
                    ->defaultSize));

        QSpinBox *currentFrameSpin =
            window.findChild<QSpinBox *>(QStringLiteral("currentFrameSpin"));
        QVERIFY(currentFrameSpin);
        QCOMPARE(currentFrameSpin->maximum(), 30);
        currentFrameSpin->setFocus(Qt::OtherFocusReason);
        QTest::mouseClick(currentFrameSpin,
            Qt::LeftButton,
            Qt::NoModifier,
            currentFrameSpin->rect().center());
        QTRY_VERIFY(!canvas->isAnimating());
        const int targetFrameValue = currentFrameSpin->value() == 1 ? 2 : 1;
        currentFrameSpin->setValue(targetFrameValue);
        QCOMPARE(canvas->currentFrame(), targetFrameValue - 1);

        const QPoint start = canvas->rect().center() - QPoint(50, 20);
        const QPoint end = canvas->rect().center() + QPoint(50, 20);
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, start);
        QTest::mouseMove(canvas, end, 10);
        QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, end);
        QTRY_VERIFY(undoAction->isEnabled());

        const QString screenshotPath =
            qEnvironmentVariable("UGURUGU_TEST_SCREENSHOT");
        if (!screenshotPath.isEmpty())
        {
            QVERIFY(window.grab().save(screenshotPath, "PNG"));
            QVERIFY(QFileInfo(screenshotPath).size() > 0);
        }

        const QString brushPanelScreenshotPath =
            qEnvironmentVariable("UGURUGU_BRUSH_PANEL_SCREENSHOT");
        if (!brushPanelScreenshotPath.isEmpty())
        {
            BrushPopoverPanel *brushPanel =
                window.findChild<BrushPopoverPanel *>();
            QVERIFY(brushPanel);
            QWidget *popover = brushPanel->window();
            popover->adjustSize();
            QVERIFY(popover->grab().save(brushPanelScreenshotPath, "PNG"));
            QVERIFY(QFileInfo(brushPanelScreenshotPath).size() > 0);
        }

        undoAction->trigger();
        QTRY_VERIFY(!undoAction->isEnabled());

        QListWidget *layerList = layerDock->findChild<QListWidget *>();
        QVERIFY(layerList);
        QCOMPARE(layerList->count(), 1);

        QToolButton *addButton = layerDock->findChild<QToolButton *>(
            QStringLiteral("layerAddButton"));
        QVERIFY(addButton);
        QTest::mouseClick(addButton, Qt::LeftButton);
        QTRY_COMPARE(layerList->count(), 2);
        QTRY_VERIFY(undoAction->isEnabled());

        undoAction->trigger();
        QTRY_COMPARE(layerList->count(), 1);

        QAction *settingsAction =
            window.findChild<QAction *>(QStringLiteral("settingsAction"));
        QAction *checkForUpdatesAction = window.findChild<QAction *>(
            QStringLiteral("checkForUpdatesAction"));
        QToolButton *settingsButton =
            window.findChild<QToolButton *>(QStringLiteral("settingsButton"));
        QVERIFY(settingsAction);
        QVERIFY(checkForUpdatesAction);
        QVERIFY(settingsButton);
        QCOMPARE(settingsButton->defaultAction(), settingsAction);
        QVERIFY(window.windowTitle().contains(QStringLiteral("Ugurugu")));
    }

    void exposesBrushPresetNameToAccessibility()
    {
        const BrushPreset &preset = BrushPresetCatalog::defaultPreset();
        BrushPresetButton button(preset);
        QCOMPARE(
            button.accessibleName(), BrushPresetCatalog::displayName(preset));
    }

    void exposesEraserPresetNameToAccessibility()
    {
        const EraserPreset &preset = EraserPresetCatalog::defaultPreset();
        EraserPresetButton button(preset);
        QCOMPARE(
            button.accessibleName(), EraserPresetCatalog::displayName(preset));
    }

    void switchesEraserPresetFromCards()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        EraserPopoverPanel panel(&canvas);

        const QList<EraserPresetButton *> buttons =
            panel.findChildren<EraserPresetButton *>();
        QCOMPARE(buttons.size(), EraserPresetCatalog::builtIns().size());
        EraserPresetButton *softButton = nullptr;
        for (EraserPresetButton *button : buttons)
        {
            if (button->presetId() == QStringLiteral("soft-eraser"))
            {
                softButton = button;
                break;
            }
        }
        QVERIFY(softButton);
        softButton->click();
        QCOMPARE(canvas.eraserPresetId(), QStringLiteral("soft-eraser"));
        QVERIFY(softButton->isChecked());
        QCOMPARE(canvas.eraserWidth(),
            EraserPresetCatalog::find(QStringLiteral("soft-eraser"))
                ->defaultSize);
    }

    void switchesWandReferenceFromCards()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        WandPopoverPanel panel(&canvas);

        WandReferenceButton *activeButton =
            panel.findChild<WandReferenceButton *>(
                QStringLiteral("wandReferenceActiveButton"));
        WandReferenceButton *markedButton =
            panel.findChild<WandReferenceButton *>(
                QStringLiteral("wandReferenceMarkedButton"));
        WandReferenceButton *visibleButton =
            panel.findChild<WandReferenceButton *>(
                QStringLiteral("wandReferenceVisibleButton"));
        QVERIFY(activeButton);
        QVERIFY(markedButton);
        QVERIFY(visibleButton);
        QCOMPARE(panel.findChildren<WandReferenceButton *>().size(), 3);
        QVERIFY(activeButton->isChecked());

        markedButton->click();
        QCOMPARE(canvas.wandReference(),
            CanvasWidget::WandReference::ReferenceLayers);
        QVERIFY(markedButton->isChecked());

        canvas.setWandReference(CanvasWidget::WandReference::AllVisibleLayers);
        QVERIFY(visibleButton->isChecked());
    }

    void switchesSelectionShapeFromCards()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        LassoPopoverPanel panel(&canvas);

        SelectionShapeButton *freehandButton =
            panel.findChild<SelectionShapeButton *>(
                QStringLiteral("selectionShapeFreehandButton"));
        SelectionShapeButton *rectangleButton =
            panel.findChild<SelectionShapeButton *>(
                QStringLiteral("selectionShapeRectangleButton"));
        SelectionShapeButton *ellipseButton =
            panel.findChild<SelectionShapeButton *>(
                QStringLiteral("selectionShapeEllipseButton"));
        QVERIFY(freehandButton);
        QVERIFY(rectangleButton);
        QVERIFY(ellipseButton);
        QCOMPARE(panel.findChildren<SelectionShapeButton *>().size(), 3);
        QVERIFY(freehandButton->isChecked());

        rectangleButton->click();
        QCOMPARE(
            canvas.selectionShape(), CanvasWidget::SelectionShape::Rectangle);
        QVERIFY(rectangleButton->isChecked());

        canvas.setSelectionShape(CanvasWidget::SelectionShape::Ellipse);
        QVERIFY(ellipseButton->isChecked());
    }

    void tabsToFrameScrubberAndExposesCurrentFrame()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        FrameScrubber scrubber(&controller, &canvas);

        QCOMPARE(scrubber.focusPolicy(), Qt::StrongFocus);
        QVERIFY(scrubber.focusPolicy() & Qt::TabFocus);
        QCOMPARE(
            scrubber.accessibleDescription(), QStringLiteral("Frame 1 of 30"));
        QTest::keyClick(&scrubber, Qt::Key_Right);
        QCOMPARE(canvas.currentFrame(), 1);
        QCOMPARE(
            scrubber.accessibleDescription(), QStringLiteral("Frame 2 of 30"));
    }

    void togglesLayerVisibilityFromKeyboard()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        const QUuid layerId = controller.document().activeLayerId;
        LayerDock dock(&controller);
        QListWidget *list = dock.findChild<QListWidget *>();
        QVERIFY(list);
        QCOMPARE(list->count(), 1);
        QCOMPARE(list->item(0)->data(Qt::AccessibleDescriptionRole).toString(),
            QStringLiteral("Layer is visible"));

        QTest::keyClick(list, Qt::Key_Space);
        QTRY_VERIFY(!controller.document().layer(layerId)->visible);
        QCOMPARE(list->item(0)->data(Qt::AccessibleDescriptionRole).toString(),
            QStringLiteral("Layer is hidden"));

        QTest::keyClick(list, Qt::Key_Space);
        QTRY_VERIFY(controller.document().layer(layerId)->visible);
    }

    void updatesLayerItemsIncrementallyAndRendersThumbnailsOffThread()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(4096, 4096)));
        const QUuid layerId = controller.document().activeLayerId;
        LayerDock dock(&controller);
        QListWidget *list = dock.findChild<QListWidget *>();
        QVERIFY(list);
        QCOMPARE(list->count(), 1);
        QListWidgetItem *originalItem = list->item(0);

        Stroke stroke;
        stroke.width = 20.0;
        stroke.points = {
            {QPointF(100.0, 100.0), 1.0}, {QPointF(1000.0, 1000.0), 1.0}};
        QCOMPARE(controller.addStroke(layerId, stroke),
            DocumentController::AddStrokeResult::Added);
        QCOMPARE(list->item(0), originalItem);
        QTRY_VERIFY_WITH_TIMEOUT(!list->item(0)
                                     ->data(LayerItemRoles::Thumbnail)
                                     .value<QPixmap>()
                                     .isNull(),
            5000);

        QCOMPARE(controller.renameLayer(layerId, QStringLiteral("Retained")),
            DocumentController::RenameLayerResult::Renamed);
        QCOMPARE(list->item(0), originalItem);
        QCOMPARE(list->item(0)->text(), QStringLiteral("Retained"));

        controller.addLayer();
        QListWidgetItem *retainedItem = nullptr;
        for (int row = 0; row < list->count(); ++row)
        {
            if (list->item(row)->data(LayerItemRoles::LayerId).toUuid()
                == layerId)
            {
                retainedItem = list->item(row);
                break;
            }
        }
        QCOMPARE(retainedItem, originalItem);
    }

    void animatesWobblePreviewOnlyAroundChanges()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        WobblePopoverPanel panel(&controller);
        WobblePreview *preview = panel.findChild<WobblePreview *>();
        QVERIFY(preview);
        QShowEvent showEvent;
        QApplication::sendEvent(preview, &showEvent);
        QVERIFY(!preview->isAnimationActive());

        controller.setWobbleAmount(controller.document().wobbleAmount + 1.0);
        QVERIFY(preview->isAnimationActive());

        panel.setEnabled(false);
        QVERIFY(!preview->isAnimationActive());
        panel.setEnabled(true);
        QVERIFY(preview->isAnimationActive());

        QTRY_VERIFY_WITH_TIMEOUT(!preview->isAnimationActive(), 3000);

        QHideEvent hideEvent;
        QApplication::sendEvent(preview, &hideEvent);
        QVERIFY(!preview->isAnimationActive());
    }

    void editsMotionSettingsFromPopover()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        WobblePopoverPanel panel(&controller);
        auto *style =
            panel.findChild<QComboBox *>(QStringLiteral("motionStyleCombo"));
        auto *poseCount =
            panel.findChild<QSpinBox *>(QStringLiteral("motionPoseCountSpin"));
        auto *detail =
            panel.findChild<QSpinBox *>(QStringLiteral("motionDetailSpin"));
        auto *linked =
            panel.findChild<QSpinBox *>(QStringLiteral("motionLinkedSpin"));
        auto *randomness =
            panel.findChild<QSpinBox *>(QStringLiteral("motionRandomnessSpin"));
        auto *broken =
            panel.findChild<QCheckBox *>(QStringLiteral("brokenLineCheckBox"));
        auto *breakAmount =
            panel.findChild<QSpinBox *>(QStringLiteral("breakAmountSpin"));
        auto *breakRange =
            panel.findChild<QDoubleSpinBox *>(QStringLiteral("breakRangeSpin"));
        QVERIFY(style);
        QVERIFY(poseCount);
        QVERIFY(detail);
        QVERIFY(linked);
        QVERIFY(randomness);
        QVERIFY(broken);
        QVERIFY(breakAmount);
        QVERIFY(breakRange);
        QVERIFY(!poseCount->isEnabled());

        style->setCurrentIndex(
            style->findData(static_cast<int>(MotionStyle::Smooth)));
        poseCount->setValue(5);
        detail->setValue(18);
        linked->setValue(45);
        randomness->setValue(70);
        broken->setChecked(true);
        breakAmount->setValue(60);
        breakRange->setValue(48.0);

        QCOMPARE(controller.document().motion.style, MotionStyle::Smooth);
        QCOMPARE(controller.document().motion.poseCount, 5);
        QCOMPARE(controller.document().motion.detail, 18);
        QCOMPARE(controller.document().motion.linked, 0.45);
        QCOMPARE(controller.document().motion.randomness, 0.7);
        QVERIFY(controller.document().motion.brokenLine);
        QCOMPARE(controller.document().motion.breakAmount, 0.6);
        QCOMPARE(controller.document().motion.breakRange, 48.0);
        QVERIFY(poseCount->isEnabled());
        QVERIFY(breakAmount->isEnabled());
        QVERIFY(breakRange->isEnabled());
    }

    void recordsUsedColorsWithoutRecordingPickerChanges()
    {
        const QString key = QStringLiteral("brush/colorHistory");
        QSettings().remove(key);
        ColorHistoryGrid grid;
        const QColor first(20, 40, 60, 80);
        const QColor second(30, 50, 70, 90);
        const QColor finalColor(40, 60, 80, 100);

        grid.setActiveColor(first);
        grid.setActiveColor(second);
        QVERIFY(!QSettings().contains(key));
        grid.recordColor(finalColor);
        // Checked before the event loop runs: the write is debounced, so it
        // cannot have happened yet. Waiting a fraction of the debounce instead
        // raced with it on a loaded machine.
        QVERIFY(!QSettings().contains(key));
        QTRY_COMPARE(QSettings().value(key).toStringList().value(0),
            finalColor.name(QColor::HexArgb));
    }

    void roundTripsWwpPresetAndAppliesMotionAsOneUndoEntry()
    {
        QSettings settings;
        settings.setValue(QStringLiteral("drawingTools/activeTool"),
            QStringLiteral("bucket"));
        settings.setValue(QStringLiteral("drawingTools/brush/color"),
            QStringLiteral("#80406080"));
        settings.setValue(QStringLiteral("drawingTools/fill/tolerance"), 72);

        Document document = Document::createDefault(QSize(80, 60));
        document.animationFrames = 20;
        document.wobbleAmount = 4.25;
        document.motion.style = MotionStyle::Smooth;
        document.motion.poseCount = 9;
        document.motion.detail = 18;
        document.motion.linked = 0.4;
        document.motion.randomness = 0.3;
        document.motion.brokenLine = true;
        document.motion.breakAmount = 0.45;
        document.motion.breakRange = 36.0;

        const WwpPreset captured = WwpPresetCodec::capture(settings, document);
        const QByteArray encoded = WwpPresetCodec::encode(captured);
        QVERIFY(!encoded.isEmpty());
        QString error;
        const std::optional<WwpPreset> decoded =
            WwpPresetCodec::decode(encoded, &error);
        QVERIFY2(decoded.has_value(), qPrintable(error));
        QCOMPARE(decoded->wobbleAmount, captured.wobbleAmount);
        QCOMPARE(decoded->motion, captured.motion);
        QCOMPARE(decoded->drawingTools, captured.drawingTools);

        settings.remove(QStringLiteral("drawingTools"));
        WwpPresetCodec::applyDrawingTools(*decoded, settings);
        QCOMPARE(settings.value(QStringLiteral("drawingTools/activeTool"))
                     .toString(),
            QStringLiteral("bucket"));
        QCOMPARE(settings.value(QStringLiteral("drawingTools/fill/tolerance"))
                     .toInt(),
            72);

        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(80, 60)));
        controller.setAnimationFrames(20);
        controller.undoStack()->clear();
        QVERIFY(controller.applyMotionPreset(
            decoded->wobbleAmount, decoded->motion));
        QCOMPARE(controller.undoStack()->count(), 1);
        QCOMPARE(controller.document().wobbleAmount, 4.25);
        QCOMPARE(controller.document().motion, decoded->motion);
        controller.undoStack()->undo();
        QVERIFY(controller.document().motion != decoded->motion);

        QVERIFY(!WwpPresetCodec::decode(
            QByteArrayLiteral("WIGGLEWIGGLETOOL_PRESET=1\nSize=12"), &error));
        QVERIFY(error.contains(QStringLiteral("WiggleWiggleTool")));
    }

    void changesLayerBlendModeFromDock()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        LayerDock dock(&controller);
        QComboBox *blendModeCombo =
            dock.findChild<QComboBox *>(QStringLiteral("layerBlendModeCombo"));
        QVERIFY(blendModeCombo);
        QCOMPARE(blendModeCombo->count(), 4);
        QCOMPARE(blendModeCombo->currentData().toInt(),
            static_cast<int>(LayerBlendMode::Normal));

        blendModeCombo->setCurrentIndex(blendModeCombo->findData(
            static_cast<int>(LayerBlendMode::Overlay)));
        QCOMPARE(controller.document().layers.first().blendMode,
            LayerBlendMode::Overlay);
        QCOMPARE(controller.undoStack()->count(), 1);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.first().blendMode,
            LayerBlendMode::Normal);
        QCOMPARE(blendModeCombo->currentData().toInt(),
            static_cast<int>(LayerBlendMode::Normal));
        controller.undoStack()->redo();
        QCOMPARE(blendModeCombo->currentData().toInt(),
            static_cast<int>(LayerBlendMode::Overlay));
    }

    void restoresRejectedLayerRenameInDock()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        const QUuid layerId = controller.document().activeLayerId;
        const QString originalName = controller.document().layer(layerId)->name;
        LayerDock dock(&controller);
        QListWidget *list = dock.findChild<QListWidget *>();
        QVERIFY(list);
        QCOMPARE(list->count(), 1);

        list->item(0)->setText(QString(
            DocumentLimits::maximumLayerNameLength + 1, QLatin1Char('x')));

        QCOMPARE(controller.document().layer(layerId)->name, originalName);
        QCOMPARE(list->item(0)->text(), originalName);
        QCOMPARE(controller.undoStack()->count(), 0);
    }

    void limitsLayerNameEditorLength()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        LayerDock dock(&controller);
        QListWidget *list = dock.findChild<QListWidget *>();
        QVERIFY(list);
        QCOMPARE(list->count(), 1);

        list->editItem(list->item(0));
        QLineEdit *editor = list->findChild<QLineEdit *>();
        QVERIFY(editor);
        QCOMPARE(editor->maxLength(), DocumentLimits::maximumLayerNameLength);
    }

    void reportsStrokeCommitFailureFromCanvas()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        const QUuid layerId = controller.document().activeLayerId;
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        QSignalSpy messages(&canvas, &CanvasWidget::interactionMessage);
        const QPoint center = canvas.rect().center();

        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, center);
        controller.removeLayer(layerId);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(20, 0));

        QCOMPARE(messages.size(), 1);
        QCOMPARE(messages.first().first().toString(),
            QStringLiteral("The stroke could not be added because its layer is "
                           "no longer available."));
        QVERIFY(controller.document().layers.isEmpty());
    }

    void managesLayerGroupsAndClippingFromDock()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        const QUuid layerId = controller.document().activeLayerId;
        LayerDock dock(&controller);
        QToolButton *groupButton = dock.findChild<QToolButton *>(
            QStringLiteral("layerAddGroupButton"));
        QComboBox *parentCombo = dock.findChild<QComboBox *>(
            QStringLiteral("layerParentGroupCombo"));
        QCheckBox *clipCheck =
            dock.findChild<QCheckBox *>(QStringLiteral("layerClipCheck"));
        QCheckBox *referenceCheck =
            dock.findChild<QCheckBox *>(QStringLiteral("layerReferenceCheck"));
        QListWidget *list = dock.findChild<QListWidget *>();
        QVERIFY(groupButton);
        QVERIFY(parentCombo);
        QVERIFY(clipCheck);
        QVERIFY(referenceCheck);
        QVERIFY(list);

        groupButton->click();
        QTRY_COMPARE(list->count(), 2);
        const Layer *layer = controller.document().layer(layerId);
        QVERIFY(layer);
        QVERIFY(!layer->parentGroupId.isNull());
        QCOMPARE(parentCombo->currentData().toUuid(), layer->parentGroupId);

        clipCheck->click();
        QVERIFY(controller.document().layer(layerId)->clipToLayerBelow);
        controller.undoStack()->undo();
        QVERIFY(!controller.document().layer(layerId)->clipToLayerBelow);

        referenceCheck->click();
        QVERIFY(controller.document().layer(layerId)->reference);
        controller.undoStack()->undo();
        QVERIFY(!controller.document().layer(layerId)->reference);
    }

    void filtersLayerHierarchyActionsAtDepthLimit()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        LayerDock dock(&controller);
        QToolButton *groupButton = dock.findChild<QToolButton *>(
            QStringLiteral("layerAddGroupButton"));
        QComboBox *parentCombo = dock.findChild<QComboBox *>(
            QStringLiteral("layerParentGroupCombo"));
        QListWidget *list = dock.findChild<QListWidget *>();
        QVERIFY(groupButton);
        QVERIFY(parentCombo);
        QVERIFY(list);

        for (int depth = 0; depth < DocumentLimits::maximumLayerDepth; ++depth)
        {
            QVERIFY(groupButton->isEnabled());
            groupButton->click();
        }
        QVERIFY(!groupButton->isEnabled());

        QUuid hierarchyRootId;
        for (const Layer &layer : controller.document().layers)
        {
            if (layer.kind == LayerKind::Group && layer.parentGroupId.isNull())
            {
                hierarchyRootId = layer.id;
                break;
            }
        }
        QVERIFY(!hierarchyRootId.isNull());

        controller.addLayerGroup();
        QUuid emptyRootId;
        for (const Layer &layer : controller.document().layers)
        {
            if (layer.kind == LayerKind::Group && layer.parentGroupId.isNull()
                && layer.id != hierarchyRootId)
            {
                emptyRootId = layer.id;
                break;
            }
        }
        QVERIFY(!emptyRootId.isNull());

        QListWidgetItem *hierarchyRootItem = nullptr;
        for (int row = 0; row < list->count(); ++row)
        {
            QListWidgetItem *item = list->item(row);
            if (item->data(LayerItemRoles::LayerId).toUuid() == hierarchyRootId)
            {
                hierarchyRootItem = item;
                break;
            }
        }
        QVERIFY(hierarchyRootItem);
        list->setCurrentItem(hierarchyRootItem);

        QVERIFY(!groupButton->isEnabled());
        QCOMPARE(parentCombo->findData(QVariant::fromValue(emptyRootId)), -1);
        QVERIFY(parentCombo->findData(QVariant::fromValue(QUuid())) >= 0);
    }

    void movesLayerAcrossGroupsByDrag()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(96, 96)));
        const QUuid firstPaintId = controller.document().activeLayerId;
        controller.addLayerGroup(firstPaintId);
        const QUuid groupId =
            controller.document().layer(firstPaintId)->parentGroupId;
        controller.addLayer();
        const QUuid secondPaintId = controller.document().activeLayerId;
        controller.addLayer();
        const QUuid rootPaintId = controller.document().activeLayerId;
        controller.setLayerParentGroup(rootPaintId, {});

        LayerDock dock(&controller);
        auto *list = dock.findChild<LayerListWidget *>();
        QVERIFY(list);
        const auto rowOf = [list](const QUuid &id)
        {
            for (int row = 0; row < list->count(); ++row)
            {
                if (list->item(row)->data(LayerItemRoles::LayerId).toUuid()
                    == id)
                {
                    return row;
                }
            }
            return -1;
        };
        QCOMPARE(rowOf(groupId), 0);
        QCOMPARE(rowOf(secondPaintId), 1);
        QCOMPARE(rowOf(firstPaintId), 2);
        QCOMPARE(rowOf(rootPaintId), 3);

        emit list->dropRequested(rowOf(firstPaintId),
            rowOf(rootPaintId),
            LayerListWidget::DropPlacement::AboveTarget);
        QVERIFY(
            controller.document().layer(firstPaintId)->parentGroupId.isNull());
        QCOMPARE(rowOf(groupId), 0);
        QCOMPARE(rowOf(secondPaintId), 1);
        QCOMPARE(rowOf(firstPaintId), 2);
        QCOMPARE(rowOf(rootPaintId), 3);

        controller.undoStack()->undo();
        QCOMPARE(
            controller.document().layer(firstPaintId)->parentGroupId, groupId);
        QCOMPARE(rowOf(firstPaintId), 2);
        controller.undoStack()->redo();
        QVERIFY(
            controller.document().layer(firstPaintId)->parentGroupId.isNull());
        controller.undoStack()->undo();

        emit list->dropRequested(rowOf(firstPaintId),
            rowOf(secondPaintId),
            LayerListWidget::DropPlacement::AboveTarget);
        QCOMPARE(
            controller.document().layer(firstPaintId)->parentGroupId, groupId);
        QCOMPARE(rowOf(firstPaintId), 1);
        QCOMPARE(rowOf(secondPaintId), 2);

        emit list->dropRequested(rowOf(rootPaintId),
            rowOf(groupId),
            LayerListWidget::DropPlacement::BelowTarget);
        QCOMPARE(
            controller.document().layer(rootPaintId)->parentGroupId, groupId);
        QCOMPARE(rowOf(rootPaintId), 1);

        emit list->dropRequested(
            rowOf(rootPaintId), -1, LayerListWidget::DropPlacement::OnViewport);
        QVERIFY(
            controller.document().layer(rootPaintId)->parentGroupId.isNull());
        QCOMPARE(rowOf(rootPaintId), list->count() - 1);
    }

    void blocksDrawingWhileGroupSelected()
    {
        MainWindow window;
        window.resize(1100, 720);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        LayerDock *layerDock = window.findChild<LayerDock *>();
        QVERIFY(canvas);
        QVERIFY(layerDock);
        auto *list = layerDock->findChild<LayerListWidget *>();
        QVERIFY(list);

        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        const QUuid paintId = controller.document().activeLayerId;
        controller.addLayerGroup(paintId);
        const QUuid groupId =
            controller.document().layer(paintId)->parentGroupId;

        const auto selectRowOf = [list](const QUuid &id)
        {
            for (int row = 0; row < list->count(); ++row)
            {
                QListWidgetItem *item = list->item(row);
                if (item->data(LayerItemRoles::LayerId).toUuid() == id)
                {
                    list->setCurrentItem(item);
                    return true;
                }
            }
            return false;
        };
        const auto strokeCount = [&controller, &paintId]()
        {
            const Layer *layer = controller.document().layer(paintId);
            return layer ? layer->strokes.size() : qsizetype(-1);
        };
        const auto drag = [canvas]()
        {
            const QPoint start = canvas->rect().center() - QPoint(40, 10);
            const QPoint end = canvas->rect().center() + QPoint(40, 10);
            QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, start);
            QTest::mouseMove(canvas, end, 10);
            QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, end);
        };

        QVERIFY(selectRowOf(groupId));
        QCOMPARE(canvas->cursor().shape(), Qt::ForbiddenCursor);
        QSignalSpy messages(canvas, &CanvasWidget::interactionMessage);
        drag();
        QCOMPARE(strokeCount(), 0);
        QVERIFY(!messages.isEmpty());

        canvas->setTool(CanvasWidget::Tool::Bucket);
        QTest::mouseClick(
            canvas, Qt::LeftButton, Qt::NoModifier, canvas->rect().center());
        QCOMPARE(strokeCount(), 0);
        canvas->setTool(CanvasWidget::Tool::Brush);

        QVERIFY(selectRowOf(paintId));
        QVERIFY(canvas->cursor().shape() != Qt::ForbiddenCursor);
        drag();
        QCOMPARE(strokeCount(), 1);
    }

    void rejectsInvalidLayerDrops()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(96, 96)));
        const QUuid paintId = controller.document().activeLayerId;
        for (int depth = 0; depth < DocumentLimits::maximumLayerDepth; ++depth)
        {
            controller.addLayerGroup(paintId);
        }
        controller.addLayerGroup(QUuid());
        const QUuid emptyGroupId = controller.document().layers.last().id;
        QUuid outerGroupId;
        for (const Layer &layer : controller.document().layers)
        {
            if (layer.kind == LayerKind::Group && layer.parentGroupId.isNull()
                && layer.id != emptyGroupId)
            {
                outerGroupId = layer.id;
                break;
            }
        }
        QVERIFY(!outerGroupId.isNull());
        QUuid secondGroupId;
        for (const Layer &layer : controller.document().layers)
        {
            if (layer.parentGroupId == outerGroupId)
            {
                secondGroupId = layer.id;
                break;
            }
        }
        QVERIFY(!secondGroupId.isNull());

        LayerDock dock(&controller);
        auto *list = dock.findChild<LayerListWidget *>();
        QVERIFY(list);
        const auto rowOf = [list](const QUuid &id)
        {
            for (int row = 0; row < list->count(); ++row)
            {
                if (list->item(row)->data(LayerItemRoles::LayerId).toUuid()
                    == id)
                {
                    return row;
                }
            }
            return -1;
        };
        const int historyCount = controller.undoStack()->count();

        emit list->dropRequested(rowOf(outerGroupId),
            rowOf(emptyGroupId),
            LayerListWidget::DropPlacement::BelowTarget);
        QVERIFY(
            controller.document().layer(outerGroupId)->parentGroupId.isNull());

        emit list->dropRequested(rowOf(outerGroupId),
            rowOf(secondGroupId),
            LayerListWidget::DropPlacement::BelowTarget);
        QVERIFY(
            controller.document().layer(outerGroupId)->parentGroupId.isNull());
        QCOMPARE(controller.document().layer(secondGroupId)->parentGroupId,
            outerGroupId);
        QCOMPARE(controller.undoStack()->count(), historyCount);
    }

    void warnsWhenOpeningLegacyLayerHierarchy()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());
        const QString projectPath =
            directory.filePath(QStringLiteral("legacy-depth.ugu"));
        QString error;
        QVERIFY2(DocumentSerializer::save(projectPath,
                     nestedLayerDocument(DocumentLimits::maximumLayerDepth + 1),
                     &error),
            qPrintable(error));

        MainWindow window;
        bool warningAccepted = false;
        QTimer::singleShot(0,
            &window,
            [&warningAccepted]()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (!dialog
                    || dialog->objectName()
                           != QStringLiteral("legacyLayerDepthWarning"))
                {
                    return;
                }
                warningAccepted = true;
                dialog->accept();
            });

        QVERIFY(window.openFile(projectPath));
        QVERIFY(warningAccepted);
        QCOMPARE(
            window.windowFilePath(), QFileInfo(projectPath).absoluteFilePath());
    }

    void warnsWhenRecoveringLegacyLayerHierarchy()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());
        QString error;
        QVERIFY2(DocumentSerializer::save(recoveryPath,
                     nestedLayerDocument(DocumentLimits::maximumLayerDepth + 1),
                     &error),
            qPrintable(error));

        bool recoverClicked = false;
        bool warningAccepted = false;
        MainWindow window;
        scheduleDialogButtonClickAndAcceptNext(&window,
            QStringLiteral("startupRecoverButton"),
            &recoverClicked,
            &warningAccepted);

        QCOMPARE(
            window.initializeSession(), MainWindow::StartupResult::Recovered);
        QVERIFY(recoverClicked);
        QVERIFY(warningAccepted);
        QVERIFY(window.windowFilePath().isEmpty());
    }

    void explainsStaticExportHierarchyMemoryLimit()
    {
        MainWindow window;
        QVERIFY(MainWindowTestAccess::loadDocument(window,
            nestedLayerDocument(
                DocumentLimits::maximumLayerDepth, QSize(4096, 4096))));
        bool warningAccepted = false;
        QTimer::singleShot(0,
            &window,
            [&warningAccepted]()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (!dialog
                    || dialog->objectName()
                           != QStringLiteral("staticExportMemoryWarning"))
                {
                    return;
                }
                warningAccepted = true;
                dialog->accept();
            });

        MainWindowTestAccess::exportImage(window);

        QVERIFY(warningAccepted);
    }

    void exportsScaledGifAtRequestedSize()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        Document document = Document::createDefault(QSize(64, 48));
        document.animationFrames = 2;
        Stroke stroke;
        stroke.width = 12.0;
        stroke.points = {{QPointF(8.0, 8.0), 1.0}, {QPointF(56.0, 40.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        ExportWorker worker;
        QSignalSpy finished(&worker, &ExportWorker::finished);
        const QString path = directory.filePath(QStringLiteral("scaled.gif"));
        QVERIFY(worker.startGif(document, path, {QSize(32, 24), true}));
        QTRY_COMPARE_WITH_TIMEOUT(finished.size(), 1, 10000);
        QVERIFY2(finished.at(0).at(1).toBool(),
            qPrintable(finished.at(0).at(4).toString()));

        QImageReader reader(path, QByteArrayLiteral("gif"));
        QCOMPARE(reader.size(), QSize(32, 24));
    }

    void exportsScaledWebPAtRequestedSize()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        Document document = Document::createDefault(QSize(64, 48));
        document.animationFrames = 2;

        ExportWorker worker;
        QSignalSpy finished(&worker, &ExportWorker::finished);
        const QString path = directory.filePath(QStringLiteral("scaled.webp"));
        QVERIFY(worker.startWebP(document, path, {QSize(32, 24), true}));
        QTRY_COMPARE_WITH_TIMEOUT(finished.size(), 1, 10000);
        QVERIFY2(finished.at(0).at(1).toBool(),
            qPrintable(finished.at(0).at(4).toString()));
        QFile file(path);
        QVERIFY(file.open(QIODevice::ReadOnly));
        const QByteArray bytes = file.readAll();
        QVERIFY(bytes.startsWith("RIFF"));
        QCOMPARE(bytes.mid(8, 4), QByteArrayLiteral("WEBP"));
    }

    void flattensGifTransparencyWhenNotPreserved()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        Document document = Document::createDefault(QSize(32, 24));
        document.background = Qt::transparent;
        document.animationFrames = 2;

        ExportWorker worker;
        QSignalSpy finished(&worker, &ExportWorker::finished);
        const QString path = directory.filePath(QStringLiteral("opaque.gif"));
        QVERIFY(worker.startGif(document, path, {QSize(), false}));
        QTRY_COMPARE_WITH_TIMEOUT(finished.size(), 1, 10000);
        QVERIFY2(finished.at(0).at(1).toBool(),
            qPrintable(finished.at(0).at(4).toString()));

        QImageReader reader(path, QByteArrayLiteral("gif"));
        const QImage decoded = reader.read();
        QVERIFY2(!decoded.isNull(), qPrintable(reader.errorString()));
        QCOMPARE(decoded.pixelColor(0, 0).alpha(), 255);
    }

    void exportsSnapshotsOffThreadAndCancelsWithoutPartialFiles()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        Document document = Document::createDefault(QSize(512, 512));
        document.background = Qt::transparent;
        Stroke stroke;
        stroke.width = 16.0;
        stroke.points = {
            {QPointF(32.0, 32.0), 1.0}, {QPointF(480.0, 480.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        ExportWorker worker;
        QSignalSpy finished(&worker, &ExportWorker::finished);
        bool eventLoopAdvanced = false;
        QTimer::singleShot(0,
            &worker,
            [&eventLoopAdvanced]()
            {
                eventLoopAdvanced = true;
            });
        const QString imagePath =
            directory.filePath(QStringLiteral("snapshot.png"));
        QVERIFY(worker.startImage(document, 0, imagePath, false));
        QVERIFY(worker.isBusy());
        QTRY_VERIFY(eventLoopAdvanced);
        QTRY_COMPARE_WITH_TIMEOUT(finished.size(), 1, 5000);
        QVERIFY(QFileInfo::exists(imagePath));
        QVERIFY(!QImage(imagePath).isNull());

        document.animationFrames = 60;
        const QString gifPath =
            directory.filePath(QStringLiteral("canceled.gif"));
        QVERIFY(worker.startGif(document, gifPath, {}));
        worker.cancel();
        QTRY_COMPARE_WITH_TIMEOUT(finished.size(), 2, 5000);
        const QList<QVariant> &canceledResult = finished.at(1);
        QVERIFY(!canceledResult.at(1).toBool());
        QVERIFY(canceledResult.at(2).toBool());
        QVERIFY(!QFileInfo::exists(gifPath));
    }

    void exportReportsFailureWhenTheWriteThrows()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const Document document = Document::createDefault(QSize(32, 24));

        ExportWorker worker;
        QSignalSpy finished(&worker, &ExportWorker::finished);
        const QString path = directory.filePath(QStringLiteral("thrown.png"));
        worker.throwFromNextExportForTesting();
        QVERIFY(worker.startImage(document, 0, path, false));
        QTRY_COMPARE_WITH_TIMEOUT(finished.size(), 1, 10000);
        QVERIFY2(!finished.at(0).at(1).toBool(),
            "a throwing export reported success");
        QVERIFY(!finished.at(0).at(2).toBool());
        QVERIFY(!finished.at(0).at(4).toString().isEmpty());
        QVERIFY(!QFileInfo::exists(path));

        // A worker left busy would hang the destructor's wait, so the next
        // export has to be accepted and to run.
        QVERIFY(worker.waitForIdle(5000));
        QVERIFY(!worker.isBusy());
        QVERIFY(worker.startImage(document, 0, path, false));
        QTRY_COMPARE_WITH_TIMEOUT(finished.size(), 2, 10000);
        QVERIFY2(finished.at(1).at(1).toBool(),
            qPrintable(finished.at(1).at(4).toString()));
    }

    void watchedFutureResultTreatsAThrownTaskAsNoResult()
    {
        QFutureWatcher<QImage> watcher;
        QSignalSpy finished(&watcher, &QFutureWatcher<QImage>::finished);
        watcher.setFuture(QtConcurrent::run(
            []() -> QImage
            {
                throw std::bad_alloc();
            }));
        QTRY_COMPARE_WITH_TIMEOUT(finished.size(), 1, 10000);
        QVERIFY2(watchedFutureResult(watcher).isNull(),
            "a pool exception escaped into the completion handler");
    }

    void joinsDetachedBackgroundWorkBeforeShutdown()
    {
        // Layer thumbnails and selection visibility run detached on the global
        // pool and outlive the widgets that started them. The caches they read
        // through are function-local statics, so anything still running when
        // those are destroyed reads freed memory.
        QSemaphore started;
        std::atomic_bool finished{false};
        QThreadPool::globalInstance()->start(
            [&started, &finished]()
            {
                started.release();
                QThread::msleep(50);
                finished.store(true, std::memory_order_relaxed);
            });
        started.acquire();
        joinDetachedBackgroundWork();
        QVERIFY2(finished.load(std::memory_order_relaxed),
            "shutdown returned with pool work still running");
    }

    void editsStrokePropertyDialogValues()
    {
        StrokePropertiesDialog::Values values;
        values.colorSupported = true;
        values.widthSupported = true;
        values.color = QColor(10, 20, 30);
        values.width = 8.0;
        StrokePropertiesDialog dialog(values);

        QDoubleSpinBox *width = dialog.findChild<QDoubleSpinBox *>(
            QStringLiteral("strokeWidthSpin"));
        QVERIFY(width);
        QVERIFY(!dialog.findChild<QDoubleSpinBox *>(
            QStringLiteral("strokeRoughnessSpin")));
        width->setValue(18.0);
        QVERIFY(dialog.selectedWidth().has_value());
        QVERIFY(dialog.color().has_value());
        QCOMPARE(*dialog.selectedWidth(), 18.0);
        QCOMPARE(*dialog.color(), *values.color);
    }

    void editsAndRestoresShortcuts()
    {
        const QString brushKey = QStringLiteral("shortcuts/brushAction");
        const QString eraserKey = QStringLiteral("shortcuts/eraserAction");
        const QString folderKey = QStringLiteral("files/defaultSaveFolder");
        const QString languageKey = QStringLiteral("appearance/language");
        SettingValueGuard brushGuard(brushKey);
        SettingValueGuard eraserGuard(eraserKey);
        SettingValueGuard folderGuard(folderKey);
        SettingValueGuard languageGuard(languageKey);
        QTemporaryDir saveFolder;
        QVERIFY(saveFolder.isValid());
        QSettings settings;
        settings.remove(brushKey);
        settings.remove(eraserKey);
        settings.setValue(folderKey, saveFolder.path());
        settings.remove(languageKey);
        settings.sync();

        QAction brushAction(QStringLiteral("Brush"));
        brushAction.setObjectName(QStringLiteral("brushAction"));
        ShortcutBinding::initialize(
            &brushAction, QKeySequence(QStringLiteral("B")));

        QAction eraserAction(QStringLiteral("Eraser"));
        eraserAction.setObjectName(QStringLiteral("eraserAction"));
        ShortcutBinding::initialize(
            &eraserAction, QKeySequence(QStringLiteral("E")));

        SettingsDialog dialog(nullptr, {&brushAction, &eraserAction});
        QLineEdit *folderEdit = dialog.findChild<QLineEdit *>(
            QStringLiteral("defaultSaveFolderEdit"));
        QKeySequenceEdit *brushEditor = dialog.findChild<QKeySequenceEdit *>(
            QStringLiteral("brushActionShortcutEdit"));
        QComboBox *languageCombo =
            dialog.findChild<QComboBox *>(QStringLiteral("languageCombo"));
        QVERIFY(folderEdit);
        QVERIFY(brushEditor);
        QVERIFY(languageCombo);
        QCOMPARE(folderEdit->text(), saveFolder.path());
        QCOMPARE(SettingsDialog::defaultSaveFolder(), saveFolder.path());
        QCOMPARE(SettingsDialog::uiLanguage(), QStringLiteral("system"));

        languageCombo->setCurrentIndex(
            languageCombo->findData(QStringLiteral("ko")));
        QCOMPARE(SettingsDialog::uiLanguage(), QStringLiteral("ko"));

        brushEditor->setKeySequence(QKeySequence(QStringLiteral("V")));
        QTRY_COMPARE(brushAction.shortcut(), QKeySequence(QStringLiteral("V")));
        QCOMPARE(settings.value(brushKey).toString(), QStringLiteral("V"));

        brushEditor->setKeySequence(QKeySequence(QStringLiteral("E")));
        QTRY_COMPARE(
            brushEditor->keySequence(), QKeySequence(QStringLiteral("V")));
        QCOMPARE(brushAction.shortcut(), QKeySequence(QStringLiteral("V")));

        QDialogButtonBox *buttons = dialog.findChild<QDialogButtonBox *>();
        QVERIFY(buttons);
        QPushButton *restoreButton =
            buttons->button(QDialogButtonBox::RestoreDefaults);
        QVERIFY(restoreButton);
        QTest::mouseClick(restoreButton, Qt::LeftButton);
        QCOMPARE(brushAction.shortcut(), QKeySequence(QStringLiteral("B")));
        QVERIFY(!settings.contains(brushKey));
        QVERIFY(!settings.contains(folderKey));
        QVERIFY(!settings.contains(languageKey));
        QCOMPARE(folderEdit->text(), SettingsDialog::defaultSaveFolder());
        QCOMPARE(SettingsDialog::uiLanguage(), QStringLiteral("system"));
    }

    void preservesShortcutAliasesAcrossEditingAndRestore()
    {
        const QString zoomKey = QStringLiteral("shortcuts/zoomAction");
        const QString otherKey = QStringLiteral("shortcuts/otherAction");
        SettingValueGuard zoomGuard(zoomKey);
        SettingValueGuard otherGuard(otherKey);
        QSettings settings;
        settings.remove(zoomKey);
        settings.remove(otherKey);

        QAction zoomAction(QStringLiteral("Zoom in"));
        zoomAction.setObjectName(QStringLiteral("zoomAction"));
        const QKeySequence zoomDefault(QStringLiteral("Ctrl++"));
        const QKeySequence zoomAlias(QStringLiteral("Ctrl+="));
        ShortcutBinding::initialize(&zoomAction, zoomDefault, {zoomAlias});

        QAction otherAction(QStringLiteral("Other"));
        otherAction.setObjectName(QStringLiteral("otherAction"));
        const QKeySequence otherDefault(QStringLiteral("O"));
        ShortcutBinding::initialize(&otherAction, otherDefault);

        SettingsDialog dialog(nullptr, {&zoomAction, &otherAction});
        QKeySequenceEdit *zoomEditor = dialog.findChild<QKeySequenceEdit *>(
            QStringLiteral("zoomActionShortcutEdit"));
        QKeySequenceEdit *otherEditor = dialog.findChild<QKeySequenceEdit *>(
            QStringLiteral("otherActionShortcutEdit"));
        QVERIFY(zoomEditor);
        QVERIFY(otherEditor);
        QCOMPARE(zoomAction.shortcuts(),
            QList<QKeySequence>({zoomDefault, zoomAlias}));

        otherEditor->setKeySequence(zoomAlias);
        QTRY_COMPARE(otherEditor->keySequence(), otherDefault);
        QCOMPARE(otherAction.shortcuts(), QList<QKeySequence>({otherDefault}));

        const QKeySequence customPrimary(QStringLiteral("Ctrl+I"));
        zoomEditor->setKeySequence(customPrimary);
        QTRY_COMPARE(ShortcutBinding::primary(&zoomAction), customPrimary);
        QCOMPARE(zoomAction.shortcuts(),
            QList<QKeySequence>({customPrimary, zoomAlias}));
        QCOMPARE(settings.value(zoomKey).toString(),
            customPrimary.toString(QKeySequence::PortableText));

        QDialogButtonBox *buttons = dialog.findChild<QDialogButtonBox *>();
        QVERIFY(buttons);
        QPushButton *restoreButton =
            buttons->button(QDialogButtonBox::RestoreDefaults);
        QVERIFY(restoreButton);
        QTest::mouseClick(restoreButton, Qt::LeftButton);
        QCOMPARE(ShortcutBinding::primary(&zoomAction), zoomDefault);
        QCOMPARE(zoomAction.shortcuts(),
            QList<QKeySequence>({zoomDefault, zoomAlias}));
        QVERIFY(!settings.contains(zoomKey));
    }

    void preservesPersistedPrimaryShortcutsOverNewAliases()
    {
        const QString zoomKey = QStringLiteral("shortcuts/zoomInAction");
        const QString otherKey = QStringLiteral("shortcuts/actualSizeAction");
        SettingValueGuard zoomGuard(zoomKey);
        SettingValueGuard otherGuard(otherKey);
        const QKeySequence zoomAlias(QStringLiteral("Ctrl+="));
        const QKeySequence zoomPrimary(QStringLiteral("Ctrl+I"));
        QSettings settings;
        settings.setValue(
            zoomKey, zoomPrimary.toString(QKeySequence::PortableText));
        settings.setValue(
            otherKey, zoomAlias.toString(QKeySequence::PortableText));
        settings.sync();

        MainWindow window;
        QAction *zoomAction =
            window.findChild<QAction *>(QStringLiteral("zoomInAction"));
        QAction *otherAction =
            window.findChild<QAction *>(QStringLiteral("actualSizeAction"));
        QVERIFY(zoomAction);
        QVERIFY(otherAction);
        QCOMPARE(ShortcutBinding::primary(zoomAction), zoomPrimary);
        QCOMPARE(ShortcutBinding::primary(otherAction), zoomAlias);
        QCOMPARE(zoomAction->shortcuts(), QList<QKeySequence>({zoomPrimary}));
        QCOMPARE(otherAction->shortcuts(), QList<QKeySequence>({zoomAlias}));

        SettingsDialog dialog(nullptr, {zoomAction, otherAction});

        QKeySequenceEdit *otherEditor = dialog.findChild<QKeySequenceEdit *>(
            QStringLiteral("actualSizeActionShortcutEdit"));
        QVERIFY(otherEditor);
        const QKeySequence replacement(QStringLiteral("Ctrl+K"));
        otherEditor->setKeySequence(replacement);
        QTRY_COMPARE(ShortcutBinding::primary(otherAction), replacement);
        QCOMPARE(zoomAction->shortcuts(),
            QList<QKeySequence>({zoomPrimary, zoomAlias}));
    }

    void showsApplicationVersionInAboutTab()
    {
        const ApplicationVersionGuard versionGuard;
        QApplication::setApplicationVersion(QStringLiteral("9.8.7-test"));

        SettingsDialog dialog;
        QTabWidget *tabs = dialog.findChild<QTabWidget *>();
        QLabel *versionLabel = dialog.findChild<QLabel *>(
            QStringLiteral("applicationVersionLabel"));
        QLabel *developmentCreditLabel = dialog.findChild<QLabel *>(
            QStringLiteral("developmentCreditLabel"));
        QLabel *iconCreditLabel =
            dialog.findChild<QLabel *>(QStringLiteral("iconCreditLabel"));
        QWidget *aboutTab =
            dialog.findChild<QWidget *>(QStringLiteral("aboutTab"));
        QVERIFY(tabs);
        QVERIFY(versionLabel);
        QVERIFY(developmentCreditLabel);
        QVERIFY(iconCreditLabel);
        QVERIFY(aboutTab);
        QCOMPARE(versionLabel->text(), QStringLiteral("Version 9.8.7-test"));
        QCOMPARE(developmentCreditLabel->text(),
            QStringLiteral("Development support by seuppi"));
        QCOMPARE(iconCreditLabel->text(),
            QStringLiteral("App icon artwork by seuppi"));
        QCOMPARE(
            tabs->tabText(tabs->indexOf(aboutTab)), QStringLiteral("About"));
    }

    void appliesThemeColorAndShowsInAppHelp()
    {
        const QString accentKey = QStringLiteral("appearance/accentColor");
        SettingValueGuard accentGuard(accentKey);
        Theme::setAccent(*qApp, Theme::defaultAccent());
        const QColor custom(82, 116, 236);
        Theme::setAccent(*qApp, custom);
        QCOMPARE(Theme::accent(), custom);
        QCOMPARE(QSettings().value(accentKey).toString(),
            custom.name(QColor::HexRgb));

        SettingsDialog settingsDialog;
        QPushButton *themeButton = settingsDialog.findChild<QPushButton *>(
            QStringLiteral("themeColorButton"));
        QVERIFY(themeButton);
        QCOMPARE(themeButton->text(), custom.name(QColor::HexRgb));

        MainWindow window;
        QAction *webPAction =
            window.findChild<QAction *>(QStringLiteral("exportWebPAction"));
        QAction *helpAction =
            window.findChild<QAction *>(QStringLiteral("helpAction"));
        QVERIFY(webPAction);
        QVERIFY(helpAction);
        QCOMPARE(helpAction->shortcut(), QKeySequence(Qt::Key_F1));
        helpAction->trigger();
        HelpDialog *help = window.findChild<HelpDialog *>();
        QTRY_VERIFY(help);
        QTextBrowser *browser =
            help->findChild<QTextBrowser *>(QStringLiteral("helpBrowser"));
        QVERIFY(browser);
        QVERIFY(browser->toPlainText().contains(QStringLiteral("WebP")));
        help->close();
        Theme::setAccent(*qApp, Theme::defaultAccent());
    }

    // Small labels used to be pinned to pixel counts, which ignored the
    // Windows text size and macOS display settings the system font carries.
    // They now hang off that font, and these are the properties that keep
    // them readable while they do.
    void derivesSmallTextFromTheSystemFont()
    {
        const QFont original = QApplication::font();
        auto restore = qScopeGuard(
            [&original]()
            {
                QApplication::setFont(original);
            });

        QFont base = original;
        base.setPointSizeF(9.0);
        const qreal title =
            Theme::scaledFont(base, Theme::TextRole::Title).pointSizeF();
        const qreal label =
            Theme::scaledFont(base, Theme::TextRole::Label).pointSizeF();
        const qreal caption =
            Theme::scaledFont(base, Theme::TextRole::Caption).pointSizeF();
        QVERIFY(title >= label);
        QVERIFY(label >= caption);
        QCOMPARE(title, base.pointSizeF());

        // Weight and family have to survive the step, or the bundled font
        // stops being the one that draws these labels.
        QFont styled = base;
        styled.setFamilies({QStringLiteral("Courier New")});
        styled.setWeight(QFont::DemiBold);
        const QFont stepped =
            Theme::scaledFont(styled, Theme::TextRole::Caption);
        QCOMPARE(stepped.families(), styled.families());
        QCOMPARE(stepped.weight(), QFont::DemiBold);

        QApplication::setFont(base);
        const int captionAtDefault =
            Theme::fontPixelSize(Theme::TextRole::Caption);
        QVERIFY(captionAtDefault >= Theme::minimumTextPixelSize());

        // A system font the user has enlarged has to carry the small steps up
        // with it. That is the whole point of deriving them.
        QFont enlarged = base;
        enlarged.setPointSizeF(base.pointSizeF() * 2.0);
        QApplication::setFont(enlarged);
        QVERIFY(
            Theme::fontPixelSize(Theme::TextRole::Caption) > captionAtDefault);

        // A very small one must not drag them under the size at which Hangul
        // and kana lose the strokes that tell them apart.
        QFont tiny = base;
        tiny.setPointSizeF(4.0);
        QApplication::setFont(tiny);
        QCOMPARE(Theme::fontPixelSize(Theme::TextRole::Caption),
            Theme::minimumTextPixelSize());
        QCOMPARE(Theme::fontPixelSize(Theme::TextRole::Label),
            Theme::minimumTextPixelSize());
    }

    // Cards and rows used to reserve a fixed number of pixels for text that
    // was itself fixed. Now that the text follows the system font, the space
    // it is given has to follow it too.
    void growsTextLayoutsWithTheSystemFont()
    {
        BrushPresetButton brush(BrushPresetCatalog::defaultPreset());
        EraserPresetButton eraser(EraserPresetCatalog::defaultPreset());
        LayerItemDelegate delegate;
        QStyleOptionViewItem option;
        option.rect = QRect(0, 0, 200, 56);

        QFont small = brush.font();
        small.setPointSizeF(4.0);
        brush.setFont(small);
        eraser.setFont(small);
        option.font = small;
        // Floors keep the compact brush card readable and preserve the
        // original eraser and layer layouts.
        QVERIFY(brush.sizeHint().height() >= 52);
        QVERIFY(brush.sizeHint().height() < 64);
        QVERIFY(eraser.sizeHint().height() >= 64);
        QVERIFY(delegate.sizeHint(option, QModelIndex()).height() >= 56);

        QFont large = brush.font();
        large.setPointSizeF(28.0);
        brush.setFont(large);
        eraser.setFont(large);
        option.font = large;
        QVERIFY(brush.sizeHint().height() > 52);
        QVERIFY(eraser.sizeHint().height() > 64);
        QVERIFY(delegate.sizeHint(option, QModelIndex()).height() > 56);
    }

    void keepsBrushPresetsInsideANarrowToolDock()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        QMainWindow window;
        auto *toolDock = new ToolDock(&canvas, &window);
        window.addDockWidget(Qt::LeftDockWidgetArea, toolDock);
        window.setCentralWidget(new QWidget(&window));
        toolDock->setMaximumWidth(declaredPaletteDockWidth);

        QScrollArea *scroll = toolDock->findChild<QScrollArea *>(
            QStringLiteral("toolSettingsScrollArea"));
        QVERIFY(scroll);
        scroll->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOn);

        window.resize(600, 360);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));
        window.resizeDocks(
            {toolDock}, {declaredPaletteDockWidth}, Qt::Horizontal);
        QCoreApplication::processEvents();

        QWidget *presetGrid = nullptr;
        for (QWidget *candidate : toolDock->findChildren<QWidget *>(
                 QStringLiteral("brushPresetGrid")))
        {
            if (candidate->isVisible())
            {
                presetGrid = candidate;
                break;
            }
        }
        QVERIFY(presetGrid);

        QWidget *viewport = scroll->viewport();
        const QRect gridRect(
            presetGrid->mapTo(viewport, QPoint()), presetGrid->size());
        QVERIFY2(viewport->contentsRect().left() <= gridRect.left()
                     && gridRect.right() <= viewport->contentsRect().right(),
            qPrintable(QStringLiteral("brush preset grid [%1, %2], viewport "
                                      "[%3, %4]")
                    .arg(gridRect.left())
                    .arg(gridRect.right())
                    .arg(viewport->contentsRect().left())
                    .arg(viewport->contentsRect().right())));
    }

    void keepsSelectionSamplingInsideANarrowToolDock()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.setTool(CanvasWidget::Tool::Lasso);
        QMainWindow window;
        auto *toolDock = new ToolDock(&canvas, &window);
        window.addDockWidget(Qt::LeftDockWidgetArea, toolDock);
        window.setCentralWidget(new QWidget(&window));
        toolDock->setMaximumWidth(declaredPaletteDockWidth);

        QScrollArea *scroll = toolDock->findChild<QScrollArea *>(
            QStringLiteral("toolSettingsScrollArea"));
        QComboBox *sampling = toolDock->findChild<QComboBox *>(
            QStringLiteral("selectionTransformSamplingCombo"));
        QVERIFY(scroll);
        QVERIFY(sampling);
        scroll->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOn);
        QFont large = sampling->font();
        large.setPointSizeF(28.0);
        sampling->setFont(large);

        window.resize(600, 360);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));
        window.resizeDocks(
            {toolDock}, {declaredPaletteDockWidth}, Qt::Horizontal);
        QCoreApplication::processEvents();

        QWidget *viewport = scroll->viewport();
        const QRect samplingRect(
            sampling->mapTo(viewport, QPoint()), sampling->size());
        QVERIFY2(
            viewport->contentsRect().left() <= samplingRect.left()
                && samplingRect.right() <= viewport->contentsRect().right(),
            qPrintable(QStringLiteral("selection sampling [%1, %2], viewport "
                                      "[%3, %4]")
                    .arg(samplingRect.left())
                    .arg(samplingRect.right())
                    .arg(viewport->contentsRect().left())
                    .arg(viewport->contentsRect().right())));
    }

    // The About dialog is where the program states its copyright, the license
    // it is offered under and the absence of a warranty, so each of those has
    // to survive edits to the notice.
    void showsLicenseNoticeInAboutDialog()
    {
        MainWindow window;
        QAction *aboutAction =
            window.findChild<QAction *>(QStringLiteral("aboutAction"));
        QVERIFY(aboutAction);
        QCOMPARE(aboutAction->menuRole(), QAction::AboutRole);
        aboutAction->trigger();

        AboutDialog *about = window.findChild<AboutDialog *>();
        QTRY_VERIFY(about);
        QTextBrowser *browser =
            about->findChild<QTextBrowser *>(QStringLiteral("aboutBrowser"));
        QVERIFY(browser);
        const QString notice = browser->toPlainText();
        QVERIFY(!QApplication::applicationVersion().isEmpty());
        QVERIFY(notice.contains(QApplication::applicationVersion()));
        QVERIFY(notice.contains(QStringLiteral("Copyright")));
        QVERIFY(notice.contains(QStringLiteral("GNU General Public License")));
        QVERIFY(notice.contains(QStringLiteral("any later version")));
        QVERIFY(notice.contains(QStringLiteral("WITHOUT ANY WARRANTY")));

        // Reopening has to reuse the one dialog rather than stack copies of
        // the notice on top of each other.
        aboutAction->trigger();
        QCOMPARE(window.findChildren<AboutDialog *>().size(), 1);
        about->close();
    }

    void migratesGlobalStrokeStabilizationToDrawingTools()
    {
        const QString legacyKey = QStringLiteral("canvas/strokeStabilization");
        QSettings settings;
        settings.setValue(legacyKey, 0.65);
        settings.sync();

        MainWindow window;
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);
        for (const BrushPreset &preset : BrushPresetCatalog::builtIns())
        {
            QCOMPARE(canvas->brushPresetStabilization(preset.id), 0.65);
            QVERIFY(settings.contains(
                QStringLiteral("drawingTools/brush/presetStabilizations/%1")
                    .arg(preset.id)));
        }
        for (const EraserPreset &preset : EraserPresetCatalog::builtIns())
        {
            QCOMPARE(canvas->eraserPresetStabilization(preset.id), 0.65);
            QVERIFY(settings.contains(
                QStringLiteral("drawingTools/eraser/presetStabilizations/%1")
                    .arg(preset.id)));
        }
        QVERIFY(!settings.contains(
            QStringLiteral("drawingTools/eraser/stabilization")));
        QVERIFY(!settings.contains(legacyKey));
    }

    void configuresCanvasSizeDialog()
    {
        CanvasSizeDialog dialog(QSize(640, 480));
        QCheckBox *relativeCheck = dialog.findChild<QCheckBox *>(
            QStringLiteral("canvasRelativeSizeCheck"));
        QSpinBox *widthSpin =
            dialog.findChild<QSpinBox *>(QStringLiteral("canvasWidthSpin"));
        QSpinBox *heightSpin =
            dialog.findChild<QSpinBox *>(QStringLiteral("canvasHeightSpin"));
        QSpinBox *offsetXSpin =
            dialog.findChild<QSpinBox *>(QStringLiteral("canvasOffsetXSpin"));
        QSpinBox *offsetYSpin =
            dialog.findChild<QSpinBox *>(QStringLiteral("canvasOffsetYSpin"));
        QToolButton *topLeft = dialog.findChild<QToolButton *>(
            QStringLiteral("canvasAnchorTopLeft"));
        QToolButton *center = dialog.findChild<QToolButton *>(
            QStringLiteral("canvasAnchorCenter"));
        QToolButton *bottomRight = dialog.findChild<QToolButton *>(
            QStringLiteral("canvasAnchorBottomRight"));
        QVERIFY(relativeCheck);
        QVERIFY(widthSpin);
        QVERIFY(heightSpin);
        QVERIFY(offsetXSpin);
        QVERIFY(offsetYSpin);
        QVERIFY(topLeft);
        QVERIFY(center);
        QVERIFY(bottomRight);

        QCOMPARE(dialog.canvasSize(), QSize(640, 480));
        QCOMPARE(dialog.contentOffset(), QPoint());
        QVERIFY(center->isChecked());

        widthSpin->setValue(800);
        heightSpin->setValue(600);
        QCOMPARE(dialog.canvasSize(), QSize(800, 600));
        QCOMPARE(dialog.contentOffset(), QPoint(80, 60));

        topLeft->click();
        QCOMPARE(dialog.contentOffset(), QPoint());
        bottomRight->click();
        QCOMPARE(dialog.contentOffset(), QPoint(160, 120));

        relativeCheck->setChecked(true);
        QCOMPARE(dialog.canvasSize(), QSize(800, 600));
        QCOMPARE(widthSpin->value(), 160);
        QCOMPARE(heightSpin->value(), 120);
        widthSpin->setValue(-100);
        heightSpin->setValue(20);
        QCOMPARE(dialog.canvasSize(), QSize(540, 500));
        QCOMPARE(dialog.contentOffset(), QPoint(-100, 20));

        center->click();
        QCOMPARE(dialog.contentOffset(), QPoint(-50, 10));
        offsetXSpin->setValue(37);
        offsetYSpin->setValue(-12);
        QCOMPARE(dialog.contentOffset(), QPoint(37, -12));
        QVERIFY(!center->isChecked());

        relativeCheck->setChecked(false);
        QCOMPARE(widthSpin->value(), 540);
        QCOMPARE(heightSpin->value(), 500);
        const CanvasSizeDialog::Result result = dialog.currentResult();
        QCOMPARE(result.size, QSize(540, 500));
        QCOMPARE(result.contentOffset, QPoint(37, -12));
    }

    void configuresImageSizeDialog()
    {
        ImageSizeDialog dialog(QSize(640, 480));
        QSpinBox *widthSpin =
            dialog.findChild<QSpinBox *>(QStringLiteral("imageWidthSpin"));
        QSpinBox *heightSpin =
            dialog.findChild<QSpinBox *>(QStringLiteral("imageHeightSpin"));
        QDoubleSpinBox *percentageSpin = dialog.findChild<QDoubleSpinBox *>(
            QStringLiteral("imageScalePercentSpin"));
        QCheckBox *keepAspectCheck = dialog.findChild<QCheckBox *>(
            QStringLiteral("imageKeepAspectCheck"));
        QLabel *warningLabel = dialog.findChild<QLabel *>(
            QStringLiteral("imageDistortionWarningLabel"));
        QVERIFY(widthSpin);
        QVERIFY(heightSpin);
        QVERIFY(percentageSpin);
        QVERIFY(keepAspectCheck);
        QVERIFY(warningLabel);
        QVERIFY(keepAspectCheck->isChecked());

        widthSpin->setValue(1280);
        QCOMPARE(dialog.imageSize(), QSize(1280, 960));
        QVERIFY(qAbs(dialog.horizontalScale() - 2.0) < 0.0001);
        QVERIFY(qAbs(dialog.verticalScale() - 2.0) < 0.0001);

        percentageSpin->setValue(150.0);
        QCOMPARE(dialog.imageSize(), QSize(960, 720));
        keepAspectCheck->setChecked(false);
        widthSpin->setValue(800);
        heightSpin->setValue(900);
        const ImageSizeDialog::Result distorted = dialog.currentResult();
        QCOMPARE(distorted.size, QSize(800, 900));
        QVERIFY(qAbs(distorted.horizontalScale - 1.25) < 0.0001);
        QVERIFY(qAbs(distorted.verticalScale - 1.875) < 0.0001);
        QVERIFY(warningLabel->text().contains(QStringLiteral("distorted")));

        keepAspectCheck->setChecked(true);
        QCOMPARE(dialog.imageSize(), QSize(800, 600));
        percentageSpin->setValue(200.0);
        QCOMPARE(dialog.imageSize(), QSize(1280, 960));
        const ImageSizeDialog::Result uniform = dialog.currentResult();
        QVERIFY(qAbs(uniform.horizontalScale - 2.0) < 0.0001);
        QVERIFY(qAbs(uniform.verticalScale - 2.0) < 0.0001);
    }

    void unlocksImageSizeDialogForExtremeAspectRatios()
    {
        QCOMPARE(LayerThumbnailRenderer::renderSize(QSize(1, 4096), 2.0),
            QSize(1, 64));
        QCOMPARE(LayerThumbnailRenderer::renderSize(QSize(4096, 1), 2.0),
            QSize(96, 1));
        // A thumbnail is rendered for the pixels the screen actually has, so
        // a fractional scale asks for a fractional multiple of the band and
        // an extreme aspect ratio still keeps its one pixel side.
        QCOMPARE(LayerThumbnailRenderer::renderSize(QSize(1, 4096), 1.5),
            QSize(1, 48));
        QCOMPARE(LayerThumbnailRenderer::renderSize(QSize(4096, 1), 1.5),
            QSize(72, 1));
        QCOMPARE(LayerThumbnailRenderer::renderSize(QSize(400, 400), 1.0),
            QSize(32, 32));
        QCOMPARE(
            LayerThumbnailRenderer::renderSize(QSize(400, 400), 0.0), QSize());
        QCOMPARE(PreviewRenderPolicy::renderSize(QSize(4096, 4096), 16.0),
            QSize(4096, 4096));

        ImageSizeDialog dialog(QSize(4096, 1));
        QSpinBox *widthSpin =
            dialog.findChild<QSpinBox *>(QStringLiteral("imageWidthSpin"));
        QSpinBox *heightSpin =
            dialog.findChild<QSpinBox *>(QStringLiteral("imageHeightSpin"));
        QCheckBox *keepAspectCheck = dialog.findChild<QCheckBox *>(
            QStringLiteral("imageKeepAspectCheck"));
        QDialogButtonBox *buttons = dialog.findChild<QDialogButtonBox *>();
        QVERIFY(widthSpin);
        QVERIFY(heightSpin);
        QVERIFY(keepAspectCheck);
        QVERIFY(buttons);
        QVERIFY(!keepAspectCheck->isChecked());
        QVERIFY(!keepAspectCheck->isEnabled());
        QCOMPARE(widthSpin->minimum(), 1);
        QCOMPARE(widthSpin->maximum(), 4096);
        QCOMPARE(heightSpin->minimum(), 1);
        QCOMPARE(heightSpin->maximum(), 4096);

        widthSpin->setValue(2048);
        QCOMPARE(dialog.imageSize(), QSize(2048, 1));
        QVERIFY(buttons->button(QDialogButtonBox::Ok)->isEnabled());

        ImageSizeDialog tallDialog(QSize(1, 4096));
        QCheckBox *tallKeepAspect = tallDialog.findChild<QCheckBox *>(
            QStringLiteral("imageKeepAspectCheck"));
        QVERIFY(tallKeepAspect);
        QVERIFY(!tallKeepAspect->isChecked());
        QVERIFY(!tallKeepAspect->isEnabled());

        ImageSizeDialog narrowDialog(QSize(100, 1));
        QCheckBox *narrowKeepAspect = narrowDialog.findChild<QCheckBox *>(
            QStringLiteral("imageKeepAspectCheck"));
        QSpinBox *narrowWidthSpin = narrowDialog.findChild<QSpinBox *>(
            QStringLiteral("imageWidthSpin"));
        QVERIFY(narrowKeepAspect);
        QVERIFY(narrowWidthSpin);
        QVERIFY(narrowKeepAspect->isChecked());
        QVERIFY(narrowKeepAspect->isEnabled());
        narrowWidthSpin->setValue(400);
        QCOMPARE(narrowDialog.imageSize(), QSize(400, 4));
    }

    void handlesUnsavedChangesDialogShortcuts_data()
    {
        QTest::addColumn<int>("key");
        QTest::addColumn<bool>("closesWindow");
        QTest::addColumn<bool>("savesDocument");

        QTest::newRow("save") << int(Qt::Key_S) << true << true;
        QTest::newRow("discard") << int(Qt::Key_N) << true << false;
        QTest::newRow("cancel") << int(Qt::Key_Escape) << false << false;
    }

    void handlesUnsavedChangesDialogShortcuts()
    {
        QFETCH(int, key);
        QFETCH(bool, closesWindow);
        QFETCH(bool, savesDocument);

        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        EnvironmentVariableGuard recoveryGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());
        const QString filePath =
            directory.filePath(QStringLiteral("shortcuts.ugu"));
        QString error;
        QVERIFY2(
            DocumentSerializer::save(
                filePath, Document::createDefault(QSize(100, 100)), &error),
            qPrintable(error));

        MainWindow window;
        window.resize(1000, 680);
        QVERIFY(window.openFile(filePath));
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        QToolButton *addLayerButton =
            window.findChild<QToolButton *>(QStringLiteral("layerAddButton"));
        QVERIFY(addLayerButton);
        addLayerButton->click();
        QTRY_VERIFY(window.isWindowModified());

        bool dialogInspected = false;
        QString saveText;
        QString discardText;
        QString cancelText;
        QTimer::singleShot(0,
            &window,
            [&]()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (!dialog)
                {
                    return;
                }
                QPushButton *saveButton = dialog->findChild<QPushButton *>(
                    QStringLiteral("unsavedChangesSaveButton"));
                QPushButton *discardButton = dialog->findChild<QPushButton *>(
                    QStringLiteral("unsavedChangesDiscardButton"));
                QPushButton *cancelButton = dialog->findChild<QPushButton *>(
                    QStringLiteral("unsavedChangesCancelButton"));
                if (!saveButton || !discardButton || !cancelButton)
                {
                    QTest::keyClick(dialog, Qt::Key_Escape);
                    return;
                }
                saveText = saveButton->text();
                discardText = discardButton->text();
                cancelText = cancelButton->text();
                dialogInspected = true;
                QWidget *keyTarget = QApplication::focusWidget();
                QTest::keyClick(
                    keyTarget ? keyTarget : dialog, static_cast<Qt::Key>(key));
            });
        QTimer::singleShot(1000,
            &window,
            []()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (dialog)
                {
                    dialog->reject();
                }
            });

        QCOMPARE(window.close(), closesWindow);
        QVERIFY(dialogInspected);
        QCOMPARE(saveText, QStringLiteral("Save (S)"));
        QCOMPARE(discardText, QStringLiteral("Don't Save (N)"));
        QCOMPARE(cancelText, QStringLiteral("Cancel (ESC)"));

        const std::optional<Document> savedDocument =
            DocumentSerializer::load(filePath, &error);
        QVERIFY2(savedDocument.has_value(), qPrintable(error));
        QCOMPARE(savedDocument->layers.size(), savesDocument ? 2 : 1);
        if (!closesWindow)
        {
            QVERIFY(window.isVisible());
            QVERIFY(window.isWindowModified());
        }
    }

    void keepsToolRailAndDockWidthsStableAcrossToolSwitches()
    {
        // The report came from the Korean UI, whose rail labels lay out
        // differently from the English source strings, so the test runs
        // with the Korean translation installed.
        // The multi-config Windows build nests the test binary one level
        // below where qt_add_translation writes the .qm files.
        QTranslator korean;
        const QString applicationDir = QCoreApplication::applicationDirPath();
        bool koreanLoaded = false;
        for (const QString &directory : {applicationDir,
                 applicationDir + QStringLiteral("/.."),
                 applicationDir + QStringLiteral("/../..")})
        {
            if (korean.load(QStringLiteral("ugurugu_ko"), directory))
            {
                koreanLoaded = true;
                break;
            }
        }
        QVERIFY(koreanLoaded);
        QCoreApplication::installTranslator(&korean);
        const auto removeTranslator = qScopeGuard(
            [&korean]()
            {
                QCoreApplication::removeTranslator(&korean);
            });

        MainWindow window;
        window.resize(1280, 820);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));
        QCoreApplication::processEvents();

        QToolBar *rail =
            window.findChild<QToolBar *>(QStringLiteral("ToolRail"));
        ToolDock *toolDock = window.findChild<ToolDock *>();
        QVERIFY(rail);
        QVERIFY(toolDock);
        const int railWidth = rail->width();
        const int dockWidth = toolDock->width();

        const QList<PopoverToolButton *> buttons =
            rail->findChildren<PopoverToolButton *>();
        QCOMPARE(buttons.size(), 7);
        for (PopoverToolButton *button : buttons)
        {
            // Hover first like a real pointer does, so the wobble icon
            // animation and the action's checked toggle both run.
            QTest::mouseMove(button, button->rect().center());
            QTest::qWait(60);
            QTest::mouseClick(button, Qt::LeftButton);
            QTest::qWait(500);
            QCoreApplication::processEvents();
            if (rail->width() != railWidth || toolDock->width() != dockWidth)
            {
                qWarning() << "after" << button->text() << "rail"
                           << rail->width() << "dock" << toolDock->width()
                           << "expected" << railWidth << dockWidth;
            }
            QCOMPARE(rail->width(), railWidth);
            QCOMPARE(toolDock->width(), dockWidth);
        }
        // Toggling re-syncs each button's text from its action, which would
        // resurrect the menu mnemonics ("Te&xt", Korean "텍스트(&X)") that the
        // rail labels deliberately omit — and mnemonic suffixes widen the
        // rail on the first click in locales that render them.
        const QList<PopoverToolButton *> railButtons =
            rail->findChildren<PopoverToolButton *>();
        QCOMPARE(railButtons.size(), 7);
        for (const PopoverToolButton *button : railButtons)
        {
            QVERIFY(!button->text().contains(QLatin1Char('&')));
            QVERIFY(!button->text().contains(QLatin1Char('(')));
        }
    }

    void exposesIndependentPaletteDocksAndRestoresLayerFollowing()
    {
        MainWindow window;
        window.resize(1100, 720);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        ToolDock *toolDock = window.findChild<ToolDock *>();
        ColorDock *colorDock = window.findChild<ColorDock *>();
        ColorHistoryDock *colorHistoryDock =
            window.findChild<ColorHistoryDock *>();
        WobbleDock *wobbleDock = window.findChild<WobbleDock *>();
        LayerDock *layerDock = window.findChild<LayerDock *>();
        QVERIFY(toolDock);
        QVERIFY(colorDock);
        QVERIFY(colorHistoryDock);
        QVERIFY(wobbleDock);
        QVERIFY(layerDock);
        QVERIFY(!toolDock->findChild<ColorWheel *>());
        QVERIFY(!toolDock->findChild<WobblePopoverPanel *>());
        QVERIFY(colorDock->findChild<ColorWheel *>());
        ColorPairSwatch *colorSwatch =
            colorDock->findChild<ColorPairSwatch *>();
        QVERIFY(colorSwatch);
        QCOMPARE(colorSwatch->currentColor(),
            window.findChild<CanvasWidget *>()->brushColor());
        ColorHistoryGrid *colorHistory =
            colorHistoryDock->findChild<ColorHistoryGrid *>();
        QVERIFY(colorHistory);
        QCOMPARE(colorHistory->findChildren<QToolButton *>().size(), 256);
        ColorHistoryGrid responsiveHistory;
        responsiveHistory.show();
        QVERIFY(QTest::qWaitForWindowExposed(&responsiveHistory));
        responsiveHistory.resize(190, 1000);
        QCoreApplication::processEvents();
        const int narrowHistoryHeight = responsiveHistory.minimumHeight();
        responsiveHistory.resize(340, 1000);
        QCoreApplication::processEvents();
        QVERIFY(responsiveHistory.minimumHeight() < narrowHistoryHeight);
        QCOMPARE(
            colorDock->findChild<ColorWheel *>()->heightForWidth(180), 180);
        QVERIFY(wobbleDock->findChild<WobblePopoverPanel *>());
        QToolBar *toolRail =
            window.findChild<QToolBar *>(QStringLiteral("ToolRail"));
        QVERIFY(toolRail);
        const QList<PopoverToolButton *> railButtons =
            toolRail->findChildren<PopoverToolButton *>();
        QCOMPARE(railButtons.size(), 7);
        for (const PopoverToolButton *button : railButtons)
        {
            QVERIFY(!button->text().contains(QLatin1Char('(')));
            QVERIFY(std::abs(button->geometry().center().x()
                             - toolRail->contentsRect().center().x())
                    <= 1);
        }
        const QList<QDockWidget *> paletteDocks{
            toolDock, colorDock, colorHistoryDock, wobbleDock, layerDock};
        for (QDockWidget *dock : paletteDocks)
        {
            QVERIFY(dock->titleBarWidget());
            QVERIFY(dock->titleBarWidget()->height() > 0);
            QVERIFY(dock->titleBarWidget()->height() <= 24);
        }
        QTRY_COMPARE(
            std::ranges::count_if(window.findChildren<QToolButton *>(
                                      QStringLiteral("collapsePaletteButton")),
                [](const QToolButton *button)
                {
                    return button->isVisible();
                }),
            1);

        for (const QDockWidget *dock : paletteDocks)
        {
            QVERIFY2(dock->minimumWidth() <= declaredPaletteDockWidth,
                qPrintable(QStringLiteral("%1 contents demand %2")
                        .arg(dock->objectName())
                        .arg(dock->minimumWidth())));
        }

        // How narrow a dock can actually get depends on the platform's frame
        // and font metrics, so squeeze it and check the panels reflow at
        // whatever width it settles on rather than at a fixed pixel count.
        const int wideToolDockWidth = toolDock->width();
        window.resizeDocks(
            paletteDocks, {150, 150, 150, 150, 150}, Qt::Horizontal);
        QTRY_VERIFY(toolDock->width() < wideToolDockWidth);
        const auto verifyResponsiveGrid = [&window](const QString &name)
        {
            QWidget *grid = window.findChild<QWidget *>(name);
            QVERIFY(grid);
            QCoreApplication::processEvents();
            QVERIFY2(visibleChildrenFit(grid), qPrintable(name));
        };
        verifyResponsiveGrid(QStringLiteral("brushCategoryGrid"));
        verifyResponsiveGrid(QStringLiteral("brushPresetGrid"));
        verifyResponsiveGrid(QStringLiteral("colorShapeGrid"));
        verifyResponsiveGrid(QStringLiteral("currentColorGrid"));
        wobbleDock->raise();
        QCoreApplication::processEvents();
        verifyResponsiveGrid(QStringLiteral("wobbleAmountGrid"));
        layerDock->raise();
        QCoreApplication::processEvents();
        verifyResponsiveGrid(QStringLiteral("layerButtonGrid"));
        toolDock->raise();
        colorHistoryDock->raise();

        window.addDockWidget(Qt::LeftDockWidgetArea, colorHistoryDock);
        QCOMPARE(
            window.dockWidgetArea(colorHistoryDock), Qt::LeftDockWidgetArea);
        QVERIFY(colorHistoryDock->titleBarWidget());
        QToolButton *collapsePaletteButton =
            colorHistoryDock->findChild<QToolButton *>(
                QStringLiteral("collapsePaletteButton"));
        QVERIFY(collapsePaletteButton);
        collapsePaletteButton->click();
        QTRY_VERIFY(isPaletteDockCollapsed(colorHistoryDock));
        QVERIFY(colorHistoryDock->isHidden());
        QDockWidget *leftPaletteRail = window.findChild<QDockWidget *>(
            QStringLiteral("LeftPaletteRailDock"));
        QToolButton *expandLeftPaletteAreaButton =
            window.findChild<QToolButton *>(
                QStringLiteral("expandLeftPaletteAreaButton"));
        QVERIFY(leftPaletteRail);
        QVERIFY(expandLeftPaletteAreaButton);
        QTRY_VERIFY(leftPaletteRail->isVisible());
        QVERIFY(leftPaletteRail->width() < 100);
        expandLeftPaletteAreaButton->click();
        QTRY_VERIFY(!isPaletteDockCollapsed(colorHistoryDock));
        QVERIFY(!colorHistoryDock->isHidden());
        QVERIFY(leftPaletteRail->isHidden());
        window.addDockWidget(Qt::RightDockWidgetArea, colorHistoryDock);
        window.tabifyDockWidget(colorHistoryDock, layerDock);
        colorHistoryDock->raise();
        QCOMPARE(
            window.dockWidgetArea(colorHistoryDock), Qt::RightDockWidgetArea);

        const int expandedCanvasWidth = window.centralWidget()->width();
        QToolButton *collapseRightPaletteButton =
            toolDock->findChild<QToolButton *>(
                QStringLiteral("collapsePaletteButton"));
        QVERIFY(collapseRightPaletteButton);
        collapseRightPaletteButton->click();
        for (QDockWidget *dock : paletteDocks)
        {
            QTRY_VERIFY(isPaletteDockCollapsed(dock));
            QVERIFY(dock->isHidden());
            QVERIFY(!dock->toggleViewAction()->isEnabled());
            QCOMPARE(window.dockWidgetArea(dock), Qt::NoDockWidgetArea);
        }
        QDockWidget *rightPaletteRail = window.findChild<QDockWidget *>(
            QStringLiteral("RightPaletteRailDock"));
        QToolButton *expandRightPaletteAreaButton =
            window.findChild<QToolButton *>(
                QStringLiteral("expandRightPaletteAreaButton"));
        QVERIFY(rightPaletteRail);
        QVERIFY(expandRightPaletteAreaButton);
        QCOMPARE(window
                     .findChildren<QToolButton *>(
                         QStringLiteral("expandRightPaletteAreaButton"))
                     .size(),
            1);
        QVERIFY(rightPaletteRail->isVisible());
        QVERIFY(rightPaletteRail->width() < 100);
        QTRY_VERIFY(window.centralWidget()->width() > expandedCanvasWidth);
        QTRY_VERIFY(!hasOnScreenTabBar(window));
        expandRightPaletteAreaButton->click();
        for (QDockWidget *dock : paletteDocks)
        {
            QTRY_VERIFY(!isPaletteDockCollapsed(dock));
            QVERIFY(!dock->isHidden());
            QVERIFY(dock->toggleViewAction()->isEnabled());
        }
        QVERIFY(rightPaletteRail->isHidden());
        QVERIFY(window.tabifiedDockWidgets(toolDock).contains(wobbleDock));
        QVERIFY(
            window.tabifiedDockWidgets(colorHistoryDock).contains(layerDock));

        colorHistoryDock->setFloating(true);
        QTRY_VERIFY(colorHistoryDock->isFloating());
        colorHistoryDock->setFloating(false);
        QTRY_VERIFY(!colorHistoryDock->isFloating());
        QVERIFY(colorHistoryDock->titleBarWidget());
        colorHistoryDock->setFloating(true);
        QTRY_VERIFY(colorHistoryDock->isFloating());
        colorHistoryDock->setFloating(false);
        QTRY_VERIFY(!colorHistoryDock->isFloating());
        QVERIFY(colorHistoryDock->titleBarWidget());

        QToolButton *panelsButton =
            window.findChild<QToolButton *>(QStringLiteral("panelsButton"));
        QAction *resetPanelLayoutAction = window.findChild<QAction *>(
            QStringLiteral("resetPanelLayoutAction"));
        QVERIFY(panelsButton);
        QVERIFY(panelsButton->menu());
        QVERIFY(resetPanelLayoutAction);

        toolDock->hide();
        colorDock->hide();
        colorHistoryDock->hide();
        wobbleDock->hide();
        layerDock->hide();
        resetPanelLayoutAction->trigger();
        QVERIFY(toolDock->toggleViewAction()->isChecked());
        QVERIFY(colorDock->toggleViewAction()->isChecked());
        QVERIFY(colorHistoryDock->toggleViewAction()->isChecked());
        QVERIFY(wobbleDock->toggleViewAction()->isChecked());
        QVERIFY(layerDock->toggleViewAction()->isChecked());
        QVERIFY(window.tabifiedDockWidgets(toolDock).contains(wobbleDock));
        QVERIFY(
            window.tabifiedDockWidgets(colorHistoryDock).contains(layerDock));

        TimelineBar *timeline = window.findChild<TimelineBar *>();
        QToolButton *collapseAnimationBarButton =
            window.findChild<QToolButton *>(
                QStringLiteral("collapseAnimationBarButton"));
        QAction *showTimelineAction =
            window.findChild<QAction *>(QStringLiteral("showTimelineAction"));
        QVERIFY(timeline);
        QVERIFY(collapseAnimationBarButton);
        QVERIFY(showTimelineAction);
        collapseAnimationBarButton->click();
        QVERIFY(timeline->isHidden());
        QVERIFY(!showTimelineAction->isChecked());
        showTimelineAction->trigger();
        QVERIFY(!timeline->isHidden());

        colorDock->hide();
        QVERIFY(colorDock->isHidden());
        QVERIFY(!colorDock->toggleViewAction()->isChecked());
        colorDock->toggleViewAction()->trigger();
        QVERIFY(!colorDock->isHidden());
        QVERIFY(colorDock->toggleViewAction()->isChecked());

        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        const QUuid layerId = controller.document().activeLayerId;
        controller.setLayerWobbleOverride(
            layerId, 4.0, controller.document().motion);
        auto *scope = wobbleDock->findChild<QComboBox *>(
            QStringLiteral("wobbleScopeCombo"));
        auto *follow = wobbleDock->findChild<QPushButton *>(
            QStringLiteral("followDocumentWobbleButton"));
        QVERIFY(scope);
        QVERIFY(follow);
        scope->setCurrentIndex(1);
        QVERIFY(follow->isEnabled());
        follow->click();
        QVERIFY(!controller.document().layer(layerId)->wobbleAmount);
        QVERIFY(!controller.document().layer(layerId)->motion);
    }

    // The layer panel is the only palette panel that shows text the user
    // supplies, so what it displays must not decide how narrow the palette
    // area can become. Windows measured even the shipped labels wider than
    // macOS did and the panel pinned the whole area open there; an inflated
    // font reproduces that platform difference on any host.
    void keepsLayerPanelMinimumWidthIndependentOfItsText()
    {
        const ApplicationFontGuard inflatedText(2);

        MainWindow window;
        window.resize(1100, 720);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        auto *layerDock = window.findChild<LayerDock *>();
        QVERIFY(layerDock);
        QCOMPARE(layerDock->minimumWidth(), declaredPaletteDockWidth);

        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        const QUuid paintId = controller.document().activeLayerId;
        controller.addLayerGroup(paintId);
        const QUuid groupId =
            controller.document().layer(paintId)->parentGroupId;
        QVERIFY(!groupId.isNull());
        controller.renameLayer(groupId,
            QString(DocumentLimits::maximumLayerNameLength, QLatin1Char('W')));
        QCoreApplication::processEvents();
        QCOMPARE(layerDock->minimumWidth(), declaredPaletteDockWidth);
    }

    // Windows measured the shipped labels wide enough that panels whose
    // minimum followed their own text pinned the whole palette area open,
    // while macOS stayed under the declared width and never noticed. An
    // inflated font reproduces that platform difference on any host, so no
    // palette panel may report a minimum that moves with its text at all.
    void keepsPaletteDockMinimumWidthIndependentOfFontMetrics()
    {
        const ApplicationFontGuard inflatedText(4);

        MainWindow window;
        window.resize(1100, 720);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));
        QCoreApplication::processEvents();

        ToolDock *toolDock = window.findChild<ToolDock *>();
        ColorDock *colorDock = window.findChild<ColorDock *>();
        ColorHistoryDock *colorHistoryDock =
            window.findChild<ColorHistoryDock *>();
        WobbleDock *wobbleDock = window.findChild<WobbleDock *>();
        LayerDock *layerDock = window.findChild<LayerDock *>();
        QVERIFY(toolDock);
        QVERIFY(colorDock);
        QVERIFY(colorHistoryDock);
        QVERIFY(wobbleDock);
        QVERIFY(layerDock);

        const QList<QDockWidget *> paletteDocks{
            toolDock, colorDock, colorHistoryDock, wobbleDock, layerDock};
        for (const QDockWidget *dock : paletteDocks)
        {
            QVERIFY2(dock->minimumWidth() <= declaredPaletteDockWidth,
                qPrintable(QStringLiteral("%1 contents demand %2 at %3pt")
                        .arg(dock->objectName())
                        .arg(dock->minimumWidth())
                        .arg(QApplication::font().pointSize())));
        }
    }

    void showsLayerCompositingAndTogglesWobblePerLayer()
    {
        MainWindow window;
        window.resize(1100, 820);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        LayerDock *layerDock = window.findChild<LayerDock *>();
        QVERIFY(layerDock);
        auto *layerList = layerDock->findChild<LayerListWidget *>();
        QVERIFY(layerList);
        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        const QUuid layerId = controller.document().activeLayerId;

        controller.setLayerOpacity(layerId, 0.8);
        controller.setLayerBlendMode(layerId, LayerBlendMode::Multiply);
        QCoreApplication::processEvents();

        const auto rowFor = [layerList](const QUuid &id) -> QListWidgetItem *
        {
            for (int row = 0; row < layerList->count(); ++row)
            {
                QListWidgetItem *item = layerList->item(row);
                if (item->data(LayerItemRoles::LayerId).toUuid() == id)
                {
                    return item;
                }
            }
            return nullptr;
        };

        QListWidgetItem *item = rowFor(layerId);
        QVERIFY(item);
        QCOMPARE(item->data(LayerItemRoles::OpacityPercent).toInt(), 80);
        QCOMPARE(item->data(LayerItemRoles::BlendModeName).toString(),
            LayerDock::tr("Multiply"));
        QVERIFY(item->data(LayerItemRoles::WobbleToggleable).toBool());
        QVERIFY(!item->data(LayerItemRoles::WobbleStopped).toBool());

        auto *delegate =
            qobject_cast<LayerItemDelegate *>(layerList->itemDelegate());
        QVERIFY(delegate);
        const QModelIndex index = layerList->indexFromItem(item);

        emit delegate->wobbleToggled(index);
        QCoreApplication::processEvents();
        const Layer *stopped = controller.document().layer(layerId);
        QVERIFY(stopped->wobbleAmount);
        QCOMPARE(*stopped->wobbleAmount, 0.0);
        // The override is only meaningful with both halves present.
        QVERIFY(stopped->motion);
        QVERIFY(rowFor(layerId)->data(LayerItemRoles::WobbleStopped).toBool());

        emit delegate->wobbleToggled(layerList->indexFromItem(rowFor(layerId)));
        QCoreApplication::processEvents();
        const Layer *following = controller.document().layer(layerId);
        QVERIFY(!following->wobbleAmount);
        QVERIFY(!following->motion);
        QVERIFY(!rowFor(layerId)->data(LayerItemRoles::WobbleStopped).toBool());

        controller.addLayerGroup(layerId);
        QCoreApplication::processEvents();
        for (const Layer &layer : controller.document().layers)
        {
            if (layer.kind != LayerKind::Group)
            {
                continue;
            }
            QListWidgetItem *groupItem = rowFor(layer.id);
            QVERIFY(groupItem);
            QVERIFY(
                !groupItem->data(LayerItemRoles::WobbleToggleable).toBool());
        }
    }

    void ignoresRetiredSimpleModeSetting()
    {
        QSettings().setValue(QStringLiteral("window/simpleMode"), true);
        MainWindow window;
        window.resize(1100, 720);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        ToolDock *toolDock = window.findChild<ToolDock *>();
        ColorDock *colorDock = window.findChild<ColorDock *>();
        ColorHistoryDock *colorHistoryDock =
            window.findChild<ColorHistoryDock *>();
        WobbleDock *wobbleDock = window.findChild<WobbleDock *>();
        LayerDock *layerDock = window.findChild<LayerDock *>();
        TimelineBar *timeline = window.findChild<TimelineBar *>();
        QVERIFY(toolDock);
        QVERIFY(colorDock);
        QVERIFY(colorHistoryDock);
        QVERIFY(wobbleDock);
        QVERIFY(layerDock);
        QVERIFY(timeline);
        QVERIFY(
            !window.findChild<QAction *>(QStringLiteral("simpleModeAction")));
        QVERIFY(!toolDock->isHidden());
        QVERIFY(!wobbleDock->isHidden());
        QVERIFY(!colorHistoryDock->isHidden());
        QVERIFY(!colorDock->isHidden());
        QVERIFY(!layerDock->isHidden());
        QVERIFY(!timeline->isHidden());
        QVERIFY(toolDock->toggleViewAction()->isEnabled());
    }

    void alignsBrushPresetCardsAcrossCategories()
    {
        MainWindow window;
        window.resize(1100, 1300);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        ToolDock *toolDock = window.findChild<ToolDock *>();
        ColorDock *colorDock = window.findChild<ColorDock *>();
        ColorHistoryDock *colorHistoryDock =
            window.findChild<ColorHistoryDock *>();
        BrushPopoverPanel *panel = window.findChild<BrushPopoverPanel *>();
        QWidget *categoryGrid =
            window.findChild<QWidget *>(QStringLiteral("brushCategoryGrid"));
        QVERIFY(toolDock);
        QVERIFY(colorDock);
        QVERIFY(colorHistoryDock);
        QVERIFY(panel);
        QVERIFY(categoryGrid);

        // The panel needs more height than its presets fill for the categories
        // to drift apart.
        window.resizeDocks(
            {toolDock, colorDock, colorHistoryDock}, {20, 1, 1}, Qt::Vertical);
        QCoreApplication::processEvents();

        const QList<QToolButton *> tabs =
            categoryGrid->findChildren<QToolButton *>(
                QString(), Qt::FindDirectChildrenOnly);
        QVERIFY(tabs.size() >= 2);

        QSet<int> gaps;
        QSet<int> cardHeights;
        for (QToolButton *tab : tabs)
        {
            tab->click();
            QCoreApplication::processEvents();
            int cardTop = -1;
            for (BrushPresetButton *card :
                window.findChildren<BrushPresetButton *>())
            {
                if (!card->isVisible())
                {
                    continue;
                }
                cardHeights.insert(card->height());
                const int top = card->mapTo(panel, QPoint()).y();
                if (cardTop < 0 || top < cardTop)
                {
                    cardTop = top;
                }
            }
            QVERIFY2(cardTop >= 0, qPrintable(tab->text()));
            gaps.insert(cardTop - categoryGrid->geometry().bottom());
        }

        // A category with fewer presets used to push its cards down by the
        // height of the rows it did not need.
        QCOMPARE(gaps.size(), 1);
        QCOMPARE(cardHeights.size(), 1);
    }

    void restoresPaletteTabsAcrossRestart()
    {
        int savedDockWidth = 0;
        int savedHistoryHeight = 0;
        QStringList savedToolTabOrder;
        QStringList savedHistoryTabOrder;
        {
            MainWindow window;
            window.resize(1100, 720);
            window.show();
            QVERIFY(QTest::qWaitForWindowExposed(&window));
            ColorDock *colorDock = window.findChild<ColorDock *>();
            ToolDock *toolDock = window.findChild<ToolDock *>();
            ColorHistoryDock *colorHistoryDock =
                window.findChild<ColorHistoryDock *>();
            WobbleDock *wobbleDock = window.findChild<WobbleDock *>();
            LayerDock *layerDock = window.findChild<LayerDock *>();
            QVERIFY(colorDock);
            QVERIFY(toolDock);
            QVERIFY(colorHistoryDock);
            QVERIFY(wobbleDock);
            QVERIFY(layerDock);
            window.addDockWidget(Qt::LeftDockWidgetArea, colorDock);
            window.resizeDocks({toolDock}, {246}, Qt::Horizontal);
            window.resizeDocks({colorHistoryDock}, {260}, Qt::Vertical);
            QCoreApplication::processEvents();
            savedDockWidth = toolDock->width();
            savedHistoryHeight = colorHistoryDock->height();
            savedToolTabOrder = dockTabOrder(window, toolDock);
            savedHistoryTabOrder = dockTabOrder(window, colorHistoryDock);
            QVERIFY(savedToolTabOrder.size() >= 2);
            QVERIFY(savedHistoryTabOrder.size() >= 2);
            QVERIFY(window.tabifiedDockWidgets(toolDock).contains(wobbleDock));
            wobbleDock->hide();
            QCoreApplication::processEvents();
            QToolButton *collapseRightPaletteButton =
                toolDock->findChild<QToolButton *>(
                    QStringLiteral("collapsePaletteButton"));
            QVERIFY(collapseRightPaletteButton);
            collapseRightPaletteButton->click();
            QTRY_VERIFY(isPaletteDockCollapsed(toolDock));
            QVERIFY(window.close());
        }

        MainWindow restored;
        ToolDock *toolDock = restored.findChild<ToolDock *>();
        ColorDock *colorDock = restored.findChild<ColorDock *>();
        ColorHistoryDock *colorHistoryDock =
            restored.findChild<ColorHistoryDock *>();
        WobbleDock *wobbleDock = restored.findChild<WobbleDock *>();
        LayerDock *layerDock = restored.findChild<LayerDock *>();
        QVERIFY(toolDock);
        QVERIFY(colorDock);
        QVERIFY(colorHistoryDock);
        QVERIFY(wobbleDock);
        QVERIFY(layerDock);
        restored.resize(1100, 720);
        restored.show();
        QVERIFY(QTest::qWaitForWindowExposed(&restored));

        QCOMPARE(restored.dockWidgetArea(colorDock), Qt::LeftDockWidgetArea);
        QVERIFY(isPaletteDockCollapsed(toolDock));
        QVERIFY(toolDock->isHidden());
        QDockWidget *rightPaletteRail = restored.findChild<QDockWidget *>(
            QStringLiteral("RightPaletteRailDock"));
        QToolButton *expandRightPaletteAreaButton =
            restored.findChild<QToolButton *>(
                QStringLiteral("expandRightPaletteAreaButton"));
        QVERIFY(rightPaletteRail);
        QVERIFY(expandRightPaletteAreaButton);
        QVERIFY(rightPaletteRail->isVisible());
        expandRightPaletteAreaButton->click();
        QTRY_VERIFY(!isPaletteDockCollapsed(toolDock));
        QCOMPARE(restored.dockWidgetArea(colorDock), Qt::LeftDockWidgetArea);
        QTRY_VERIFY(std::abs(toolDock->width() - savedDockWidth) <= 4);
        QTRY_VERIFY(
            std::abs(colorHistoryDock->height() - savedHistoryHeight) <= 4);
        QVERIFY(
            restored.tabifiedDockWidgets(colorHistoryDock).contains(layerDock));
        QCOMPARE(
            dockTabOrder(restored, colorHistoryDock), savedHistoryTabOrder);
        QVERIFY(wobbleDock->isHidden());
        QVERIFY(!wobbleDock->toggleViewAction()->isChecked());

        // A closed dock keeps its place in the tab group it was closed from,
        // but QMainWindow only reports tabified docks that are on screen.
        wobbleDock->toggleViewAction()->trigger();
        QTRY_VERIFY(!wobbleDock->isHidden());
        QTRY_VERIFY(
            restored.tabifiedDockWidgets(toolDock).contains(wobbleDock));
        QCOMPARE(dockTabOrder(restored, toolDock), savedToolTabOrder);
    }

    // Removing a dock widget between restoreState() and the first show
    // crashes inside Qt 6.11's dock layout, so a persisted collapse may only
    // hide the docks before the window is shown and must complete afterwards.
    void collapsesARestoredCollapsedAreaOnlyAfterFirstShow()
    {
        {
            MainWindow window;
            window.resize(1100, 720);
            window.show();
            QVERIFY(QTest::qWaitForWindowExposed(&window));
            ToolDock *toolDock = window.findChild<ToolDock *>();
            QVERIFY(toolDock);
            QToolButton *collapseRightPaletteButton =
                toolDock->findChild<QToolButton *>(
                    QStringLiteral("collapsePaletteButton"));
            QVERIFY(collapseRightPaletteButton);
            collapseRightPaletteButton->click();
            QTRY_VERIFY(isPaletteDockCollapsed(toolDock));
            QVERIFY(window.close());
        }

        MainWindow restored;
        ToolDock *toolDock = restored.findChild<ToolDock *>();
        QVERIFY(toolDock);
        QCoreApplication::processEvents();
        QCOMPARE(restored.dockWidgetArea(toolDock), Qt::RightDockWidgetArea);
        QVERIFY(toolDock->isHidden());
        QVERIFY(!isPaletteDockCollapsed(toolDock));

        restored.resize(1100, 720);
        restored.show();
        QVERIFY(QTest::qWaitForWindowExposed(&restored));
        QTRY_VERIFY(isPaletteDockCollapsed(toolDock));
        QCOMPARE(restored.dockWidgetArea(toolDock), Qt::NoDockWidgetArea);
        QDockWidget *rightPaletteRail = restored.findChild<QDockWidget *>(
            QStringLiteral("RightPaletteRailDock"));
        QVERIFY(rightPaletteRail);
        QTRY_VERIFY(rightPaletteRail->isVisible());

        QToolButton *expandRightPaletteAreaButton =
            restored.findChild<QToolButton *>(
                QStringLiteral("expandRightPaletteAreaButton"));
        QVERIFY(expandRightPaletteAreaButton);
        expandRightPaletteAreaButton->click();
        QTRY_VERIFY(!isPaletteDockCollapsed(toolDock));
        QVERIFY(!toolDock->isHidden());
        QCOMPARE(restored.dockWidgetArea(toolDock), Qt::RightDockWidgetArea);
    }
};

int runUiShellTests(int argc, char **argv)
{
    UiShellTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "UiShellTests.moc"
