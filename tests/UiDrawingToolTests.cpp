// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/DocumentControllerTestAccess.hpp"
#include "support/UiTestHelpers.hpp"
#include "support/UiTestSuites.hpp"
#include "ui/TabletPressureRow.hpp"

#include <QMenu>

namespace ugurugu
{

class UiDrawingToolTests final : public QObject
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
        settings.sync();
    }

    void cleanup()
    {
        QSettings settings;
        settings.remove(QStringLiteral("drawingTools"));
        settings.remove(QStringLiteral("brush/recentColors"));
        settings.remove(QStringLiteral("brush/colorHistory"));
        settings.remove(QStringLiteral("canvas/strokeStabilization"));
        settings.sync();
    }

    void picksBrushColorWithAltClick()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        canvas.setBrushColor(Qt::red);
        const QPoint center(200, 200);
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::AltModifier, center);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::AltModifier, center);

        QCOMPARE(canvas.brushColor(), QColor(Qt::white));
        const Layer &layer = controller.document().layers.first();
        QVERIFY(layer.strokes.isEmpty());
    }

    void preservesAlphaWhenPickingBrushColor()
    {
        Document document = Document::createDefault(QSize(100, 100));
        const QColor translucent(255, 0, 0, 128);
        document.background = translucent;
        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);

        canvas.setBrushColor(Qt::black);
        const QPoint center = canvas.rect().center();
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::AltModifier, center);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::AltModifier, center);

        QCOMPARE(canvas.brushColor(), translucent);
    }

    void usesPresetWobbleScaleForNewStrokes()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        const QPoint center(200, 200);
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, center);
        QTest::mouseMove(&canvas, center + QPoint(40, 0), 5);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(40, 0));

        const Layer &layer = controller.document().layers.first();
        QCOMPARE(layer.strokes.size(), 1);
        QCOMPARE(layer.strokes.first().brush.wobbleScale,
            BrushPresetCatalog::defaultPreset().settings.wobbleScale);
    }

    void keepsLineWobbleAfterSwitchingFromWobbleSpray()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        const auto drawStroke = [&canvas](int verticalOffset)
        {
            const QPoint start =
                canvas.rect().center() + QPoint(-40, verticalOffset);
            const QPoint end =
                canvas.rect().center() + QPoint(40, verticalOffset);
            QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, start);
            QTest::mouseMove(&canvas, end, 5);
            QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, end);
        };

        canvas.setBrushPreset(QStringLiteral("wobble-spray"));
        drawStroke(-30);
        QVERIFY(controller.document()
                .layers.first()
                .strokes.constLast()
                .brush.animatedJitter);

        int verticalOffset = -20;
        for (const BrushPreset &preset : BrushPresetCatalog::builtIns())
        {
            if (preset.settings.engine != BrushEngine::Line)
            {
                continue;
            }
            canvas.setBrushPreset(preset.id);
            drawStroke(verticalOffset);
            verticalOffset += 5;

            const Stroke &stroke =
                controller.document().layers.first().strokes.constLast();
            QCOMPARE(stroke.brush.engine, BrushEngine::Line);
            QCOMPARE(stroke.brush.wobbleScale, preset.settings.wobbleScale);

            Document isolated = Document::createDefault(QSize(100, 100));
            isolated.animationFrames = 10;
            isolated.wobbleAmount = 4.0;
            isolated.layers.first().strokes = {stroke};
            QVERIFY2(RenderEngine::render(isolated, 0)
                         != RenderEngine::render(isolated, 1),
                qPrintable(preset.id));
        }
    }

    void keepsBrushAndEraserSizesIndependent()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        BrushSizeRow brushSizeRow(
            &canvas, BrushSizeRow::Target::Brush, QStringLiteral("testBrush"));
        BrushSizeRow eraserSizeRow(&canvas,
            BrushSizeRow::Target::Eraser,
            QStringLiteral("testEraser"));
        QSpinBox *brushSizeSpin =
            brushSizeRow.findChild<QSpinBox *>(QStringLiteral("testBrushSpin"));
        QSpinBox *eraserSizeSpin = eraserSizeRow.findChild<QSpinBox *>(
            QStringLiteral("testEraserSpin"));
        QVERIFY(brushSizeSpin);
        QVERIFY(eraserSizeSpin);

        brushSizeSpin->setValue(17);
        QCOMPARE(canvas.brushWidth(), 17.0);
        QCOMPARE(eraserSizeSpin->value(), qRound(canvas.eraserWidth()));

        eraserSizeSpin->setValue(41);
        QCOMPARE(canvas.eraserWidth(), 41.0);
        QCOMPARE(brushSizeSpin->value(), 17);

        canvas.setBrushWidth(23.0);
        QCOMPARE(brushSizeSpin->value(), 23);
        QCOMPARE(eraserSizeSpin->value(), 41);
        canvas.setEraserWidth(57.0);
        QCOMPARE(eraserSizeSpin->value(), 57);
        QCOMPARE(brushSizeSpin->value(), 23);

        const QString originalPreset = canvas.brushPresetId();
        canvas.setBrushPreset(QStringLiteral("soft-airbrush"));
        QCOMPARE(canvas.eraserWidth(), 57.0);
        canvas.setBrushPreset(originalPreset);
        QCOMPARE(canvas.brushWidth(), 23.0);
        QCOMPARE(canvas.eraserWidth(), 57.0);

        const QPoint center = canvas.rect().center();
        canvas.setTool(CanvasWidget::Tool::Brush);
        QTest::mouseClick(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(40, 0));
        canvas.setTool(CanvasWidget::Tool::Eraser);
        QTest::mouseClick(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(40, 0));

        canvas.setTool(CanvasWidget::Tool::Lasso);
        QPointingDevice eraserStylus(QStringLiteral("Test eraser stylus"),
            2,
            QInputDevice::DeviceType::Stylus,
            QPointingDevice::PointerType::Eraser,
            QInputDevice::Capability::Position
                | QInputDevice::Capability::Pressure,
            1,
            1);
        const QPointF tabletPosition = QPointF(center) + QPointF(0.0, 40.0);
        const QPointF globalTabletPosition =
            canvas.mapToGlobal(tabletPosition.toPoint());
        QTabletEvent tabletHover(QEvent::TabletMove,
            &eraserStylus,
            tabletPosition,
            globalTabletPosition,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            Qt::NoModifier,
            Qt::NoButton,
            Qt::NoButton);
        QApplication::sendEvent(&canvas, &tabletHover);
        QCOMPARE(canvas.cursor().shape(), Qt::BlankCursor);
        QTest::mouseMove(&canvas, center + QPoint(10, 40));
        QCOMPARE(canvas.cursor().shape(), Qt::CrossCursor);
        QTabletEvent tabletPress(QEvent::TabletPress,
            &eraserStylus,
            tabletPosition,
            globalTabletPosition,
            0.8,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            Qt::NoModifier,
            Qt::LeftButton,
            Qt::LeftButton);
        QApplication::sendEvent(&canvas, &tabletPress);
        QTabletEvent tabletRelease(QEvent::TabletRelease,
            &eraserStylus,
            tabletPosition,
            globalTabletPosition,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            Qt::NoModifier,
            Qt::LeftButton,
            Qt::NoButton);
        QApplication::sendEvent(&canvas, &tabletRelease);

        const QVector<Stroke> &strokes =
            controller.document().layers.first().strokes;
        QCOMPARE(strokes.size(), 3);
        QVERIFY(strokes.at(0).mode == StrokeMode::Paint);
        QCOMPARE(strokes.at(0).width, 23.0);
        QVERIFY(strokes.at(1).mode == StrokeMode::Erase);
        QCOMPARE(strokes.at(1).width, 57.0);
        QVERIFY(strokes.at(2).mode == StrokeMode::Erase);
        QCOMPARE(strokes.at(2).width, 57.0);
    }

    void isolatesEraserBrushEngineFromActiveBrush()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        canvas.setBrushPreset(QStringLiteral("soft-airbrush"));
        canvas.setBrushAntialiasing(true);
        canvas.setTool(CanvasWidget::Tool::Eraser);
        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(30, 0));
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(30, 0));

        const Stroke &eraser =
            controller.document().layers.first().strokes.constLast();
        QCOMPARE(eraser.mode, StrokeMode::Erase);
        QCOMPARE(eraser.brush, EraserPresetCatalog::defaultPreset().settings);

        for (const QString &presetId :
            {QStringLiteral("soft-eraser"), QStringLiteral("kneaded-eraser")})
        {
            canvas.setEraserPreset(presetId);
            QTest::mouseClick(&canvas,
                Qt::LeftButton,
                Qt::NoModifier,
                center + QPoint(10, 20));
            const Stroke &presetStroke =
                controller.document().layers.first().strokes.constLast();
            QCOMPARE(presetStroke.mode, StrokeMode::Erase);
            QCOMPARE(presetStroke.brush,
                EraserPresetCatalog::find(presetId)->settings);
        }
    }

    void keepsEraserPresetSettingsIndependent()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);

        canvas.setEraserWidth(11.0);
        canvas.setEraserStabilization(0.15);
        canvas.setEraserPreset(QStringLiteral("soft-eraser"));
        canvas.setEraserWidth(62.0);
        canvas.setEraserStabilization(0.45);
        canvas.setEraserPreset(QStringLiteral("kneaded-eraser"));
        canvas.setEraserWidth(39.0);
        canvas.setEraserStabilization(0.75);

        QCOMPARE(canvas.eraserWidth(), 39.0);
        QCOMPARE(canvas.eraserStabilization(), 0.75);
        canvas.setEraserPreset(QStringLiteral("hard-eraser"));
        QCOMPARE(canvas.eraserWidth(), 11.0);
        QCOMPARE(canvas.eraserStabilization(), 0.15);
        canvas.setEraserPreset(QStringLiteral("soft-eraser"));
        QCOMPARE(canvas.eraserWidth(), 62.0);
        QCOMPARE(canvas.eraserStabilization(), 0.45);
    }

    void persistsDrawingToolSettings()
    {
        const QColor rememberedColor(18, 52, 86, 120);
        {
            MainWindow window;
            CanvasWidget *canvas = window.findChild<CanvasWidget *>();
            QVERIFY(canvas);

            canvas->setBrushPreset(QStringLiteral("monoline"));
            canvas->setBrushWidth(23.0);
            canvas->setBrushStabilization(0.23);
            canvas->setBrushPreset(QStringLiteral("soft-airbrush"));
            canvas->setBrushWidth(47.0);
            canvas->setBrushStabilization(0.64);
            canvas->setEraserPreset(QStringLiteral("hard-eraser"));
            canvas->setEraserWidth(57.0);
            canvas->setEraserStabilization(0.51);
            canvas->setEraserPreset(QStringLiteral("soft-eraser"));
            canvas->setEraserWidth(73.0);
            canvas->setEraserStabilization(0.26);
            canvas->setEraserPreset(QStringLiteral("kneaded-eraser"));
            canvas->setEraserWidth(49.0);
            canvas->setEraserStabilization(0.12);
            canvas->setBrushAntialiasing(true);
            canvas->setTabletPressureEnabled(false);
            canvas->setBrushColor(rememberedColor);
            canvas->setWandReference(
                CanvasWidget::WandReference::AllVisibleLayers);
            canvas->setSelectionShape(CanvasWidget::SelectionShape::Ellipse);
            canvas->setSelectionTransformSampling(SamplingMode::Nearest);
            canvas->setTool(CanvasWidget::Tool::Eraser);

            QVERIFY(window.close());
            QCOMPARE(QSettings()
                         .value(QStringLiteral("drawingTools/brush/presetId"))
                         .toString(),
                QStringLiteral("soft-airbrush"));
            QVERIFY(!QSettings()
                    .value(
                        QStringLiteral("drawingTools/tablet/pressureEnabled"),
                        true)
                    .toBool());
            QCOMPARE(QSettings()
                         .value(QStringLiteral(
                             "drawingTools/selection/transformSampling"))
                         .toString(),
                QStringLiteral("nearest"));
        }

        QSettings persistedSettings;
        persistedSettings.sync();
        QCOMPARE(persistedSettings.status(), QSettings::NoError);
        MainWindow restoredWindow;
        CanvasWidget *restored = restoredWindow.findChild<CanvasWidget *>();
        QVERIFY(restored);
        QCOMPARE(restored->brushPresetId(), QStringLiteral("soft-airbrush"));
        QCOMPARE(restored->eraserPresetId(), QStringLiteral("kneaded-eraser"));
        QCOMPARE(restored->brushWidth(), 47.0);
        QCOMPARE(restored->brushStabilization(), 0.64);
        QCOMPARE(restored->eraserWidth(), 49.0);
        QCOMPARE(restored->eraserStabilization(), 0.12);
        QVERIFY(restored->brushAntialiasing());
        QVERIFY(!restored->tabletPressureEnabled());
        QCOMPARE(restored->brushColor(), rememberedColor);
        QCOMPARE(restored->tool(), CanvasWidget::Tool::Eraser);
        QCOMPARE(restored->wandReference(),
            CanvasWidget::WandReference::AllVisibleLayers);
        QCOMPARE(
            restored->selectionShape(), CanvasWidget::SelectionShape::Ellipse);
        QCOMPARE(restored->selectionTransformSampling(), SamplingMode::Nearest);

        QAction *eraserAction =
            restoredWindow.findChild<QAction *>(QStringLiteral("eraserAction"));
        QSpinBox *brushSizeSpin = restoredWindow.findChild<QSpinBox *>(
            QStringLiteral("brushSizeSpin"));
        QSpinBox *eraserSizeSpin = restoredWindow.findChild<QSpinBox *>(
            QStringLiteral("eraserSizeSpin"));
        QSpinBox *brushStabilizationSpin = restoredWindow.findChild<QSpinBox *>(
            QStringLiteral("brushStabilizationSpin"));
        QSpinBox *eraserStabilizationSpin =
            restoredWindow.findChild<QSpinBox *>(
                QStringLiteral("eraserStabilizationSpin"));
        WandReferenceButton *wandReferenceButton =
            restoredWindow.findChild<WandReferenceButton *>(
                QStringLiteral("wandReferenceVisibleButton"));
        SelectionShapeButton *selectionShapeButton =
            restoredWindow.findChild<SelectionShapeButton *>(
                QStringLiteral("selectionShapeEllipseButton"));
        QCheckBox *antialiasingToggle = restoredWindow.findChild<QCheckBox *>(
            QStringLiteral("brushAntialiasingToggle"));
        QCheckBox *brushPressureToggle = restoredWindow.findChild<QCheckBox *>(
            QStringLiteral("brushTabletPressureToggle"));
        QCheckBox *eraserPressureToggle = restoredWindow.findChild<QCheckBox *>(
            QStringLiteral("eraserTabletPressureToggle"));
        QComboBox *transformSamplingCombo =
            restoredWindow.findChild<QComboBox *>(
                QStringLiteral("selectionTransformSamplingCombo"));
        QAction *smoothSamplingAction = restoredWindow.findChild<QAction *>(
            QStringLiteral("smoothSelectionSamplingAction"));
        QAction *pixelSamplingAction = restoredWindow.findChild<QAction *>(
            QStringLiteral("pixelBasedSelectionSamplingAction"));
        QMenu *transformSamplingMenu = restoredWindow.findChild<QMenu *>(
            QStringLiteral("selectionTransformSamplingMenu"));
        QVERIFY(eraserAction);
        QVERIFY(brushSizeSpin);
        QVERIFY(eraserSizeSpin);
        QVERIFY(brushStabilizationSpin);
        QVERIFY(eraserStabilizationSpin);
        QVERIFY(wandReferenceButton);
        QVERIFY(selectionShapeButton);
        QVERIFY(!restoredWindow.findChild<QSpinBox *>(
            QStringLiteral("brushRoughnessSpin")));
        QVERIFY(antialiasingToggle);
        QVERIFY(brushPressureToggle);
        QVERIFY(eraserPressureToggle);
        QVERIFY(transformSamplingCombo);
        QVERIFY(smoothSamplingAction);
        QVERIFY(pixelSamplingAction);
        QVERIFY(transformSamplingMenu);
        QVERIFY(transformSamplingMenu->toolTipsVisible());
        QVERIFY(eraserAction->isChecked());
        QCOMPARE(brushSizeSpin->value(), 47);
        QCOMPARE(eraserSizeSpin->value(), 49);
        QCOMPARE(brushStabilizationSpin->value(), 64);
        QCOMPARE(eraserStabilizationSpin->value(), 12);
        QCOMPARE(wandReferenceButton->reference(),
            CanvasWidget::WandReference::AllVisibleLayers);
        QVERIFY(wandReferenceButton->isChecked());
        QCOMPARE(selectionShapeButton->shape(),
            CanvasWidget::SelectionShape::Ellipse);
        QVERIFY(selectionShapeButton->isChecked());
        QVERIFY(antialiasingToggle->isChecked());
        QVERIFY(!brushPressureToggle->isChecked());
        QVERIFY(!eraserPressureToggle->isChecked());
        QCOMPARE(transformSamplingCombo->currentData().toInt(),
            static_cast<int>(SamplingMode::Nearest));
        QVERIFY(!smoothSamplingAction->isChecked());
        QVERIFY(pixelSamplingAction->isChecked());

        smoothSamplingAction->trigger();
        QCOMPARE(restored->selectionTransformSampling(), SamplingMode::Smooth);
        QCOMPARE(transformSamplingCombo->currentData().toInt(),
            static_cast<int>(SamplingMode::Smooth));
        QVERIFY(smoothSamplingAction->isChecked());
        QVERIFY(!pixelSamplingAction->isChecked());

        transformSamplingCombo->setCurrentIndex(
            transformSamplingCombo->findData(
                static_cast<int>(SamplingMode::Nearest)));
        QCOMPARE(restored->selectionTransformSampling(), SamplingMode::Nearest);
        QVERIFY(!smoothSamplingAction->isChecked());
        QVERIFY(pixelSamplingAction->isChecked());

        brushPressureToggle->click();
        QVERIFY(restored->tabletPressureEnabled());
        QVERIFY(brushPressureToggle->isChecked());
        QVERIFY(eraserPressureToggle->isChecked());

        restored->setBrushPreset(QStringLiteral("monoline"));
        QCOMPARE(restored->brushWidth(), 23.0);
        QCOMPARE(restored->brushStabilization(), 0.23);
        QCOMPARE(brushSizeSpin->value(), 23);
        QCOMPARE(brushStabilizationSpin->value(), 23);
        restored->setBrushPreset(QStringLiteral("soft-airbrush"));
        QCOMPARE(restored->brushWidth(), 47.0);
        QCOMPARE(restored->brushStabilization(), 0.64);
        QCOMPARE(brushSizeSpin->value(), 47);
        QCOMPARE(brushStabilizationSpin->value(), 64);

        restored->setEraserPreset(QStringLiteral("hard-eraser"));
        QCOMPARE(restored->eraserWidth(), 57.0);
        QCOMPARE(restored->eraserStabilization(), 0.51);
        QCOMPARE(eraserSizeSpin->value(), 57);
        QCOMPARE(eraserStabilizationSpin->value(), 51);
        restored->setEraserPreset(QStringLiteral("soft-eraser"));
        QCOMPARE(restored->eraserWidth(), 73.0);
        QCOMPARE(restored->eraserStabilization(), 0.26);
        QCOMPARE(eraserSizeSpin->value(), 73);
        QCOMPARE(eraserStabilizationSpin->value(), 26);
        restored->setEraserPreset(QStringLiteral("kneaded-eraser"));

        BrushPresetButton *selectedPresetButton = nullptr;
        for (BrushPresetButton *button :
            restoredWindow.findChildren<BrushPresetButton *>())
        {
            if (button->presetId() == QStringLiteral("soft-airbrush"))
            {
                selectedPresetButton = button;
                break;
            }
        }
        QVERIFY(selectedPresetButton);
        QVERIFY(selectedPresetButton->isChecked());

        EraserPresetButton *selectedEraserPresetButton = nullptr;
        for (EraserPresetButton *button :
            restoredWindow.findChildren<EraserPresetButton *>())
        {
            if (button->presetId() == QStringLiteral("kneaded-eraser"))
            {
                selectedEraserPresetButton = button;
                break;
            }
        }
        QVERIFY(selectedEraserPresetButton);
        QVERIFY(selectedEraserPresetButton->isChecked());
    }

    void migratesActiveColorFromRecentColors()
    {
        const QColor recentColor(42, 91, 137, 173);
        QSettings settings;
        settings.setValue(QStringLiteral("brush/recentColors"),
            QStringList{QStringLiteral("not-a-color"),
                recentColor.name(QColor::HexArgb)});
        settings.sync();

        MainWindow window;
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);
        QCOMPARE(canvas->brushColor(), recentColor);
        QVERIFY(window.close());
        settings.sync();
        QCOMPARE(settings.status(), QSettings::NoError);
        QCOMPARE(settings.value(QStringLiteral("drawingTools/brush/color"))
                     .toString(),
            recentColor.name(QColor::HexArgb));
    }

    void sanitizesInvalidDrawingToolSettings()
    {
        QSettings settings;
        settings.setValue(QStringLiteral("drawingTools/activeTool"),
            QStringLiteral("transform"));
        settings.setValue(QStringLiteral("drawingTools/brush/presetId"),
            QStringLiteral("missing-brush"));
        settings.setValue(QStringLiteral("drawingTools/eraser/presetId"),
            QStringLiteral("missing-eraser"));
        settings.setValue(QStringLiteral("drawingTools/brush/color"),
            QStringLiteral("not-a-color"));
        settings.setValue(QStringLiteral("drawingTools/brush/roughness"),
            std::numeric_limits<double>::infinity());
        settings.setValue(QStringLiteral("drawingTools/brush/antialiasing"),
            QStringLiteral("sometimes"));
        settings.setValue(QStringLiteral("drawingTools/tablet/pressureEnabled"),
            QStringLiteral("sometimes"));
        settings.setValue(
            QStringLiteral("drawingTools/brush/presetWidths/ink-pen"), 9999.0);
        settings.setValue(
            QStringLiteral("drawingTools/brush/presetWidths/g-pen"),
            std::numeric_limits<double>::quiet_NaN());
        settings.setValue(QStringLiteral("drawingTools/eraser/width"), -100.0);
        settings.setValue(
            QStringLiteral("drawingTools/brush/presetStabilizations/ink-pen"),
            9999.0);
        settings.setValue(QStringLiteral("drawingTools/eraser/stabilization"),
            std::numeric_limits<double>::quiet_NaN());
        settings.setValue(QStringLiteral("drawingTools/wand/reference"),
            QStringLiteral("selection-set"));
        settings.setValue(QStringLiteral("drawingTools/selection/shape"),
            QStringLiteral("polygon"));
        settings.setValue(
            QStringLiteral("drawingTools/selection/transformSampling"),
            QStringLiteral("sharpen"));
        settings.sync();

        MainWindow window;
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);
        QCOMPARE(canvas->tool(), CanvasWidget::Tool::Brush);
        QCOMPARE(
            canvas->brushPresetId(), BrushPresetCatalog::defaultPreset().id);
        QCOMPARE(
            canvas->eraserPresetId(), EraserPresetCatalog::defaultPreset().id);
        QCOMPARE(canvas->brushWidth(), DocumentLimits::maximumStrokeWidth);
        QCOMPARE(canvas->eraserWidth(), 1.0);
        QCOMPARE(canvas->brushStabilization(), 1.0);
        QCOMPARE(canvas->eraserStabilization(), 0.0);
        QVERIFY(!canvas->brushAntialiasing());
        QVERIFY(canvas->tabletPressureEnabled());
        QCOMPARE(canvas->brushColor(), QColor(Qt::black));
        QCOMPARE(
            canvas->wandReference(), CanvasWidget::WandReference::ActiveLayer);
        QCOMPARE(
            canvas->selectionShape(), CanvasWidget::SelectionShape::Freehand);
        QCOMPARE(canvas->selectionTransformSampling(), SamplingMode::Smooth);

        canvas->setBrushPreset(QStringLiteral("g-pen"));
        QCOMPARE(canvas->brushWidth(),
            BrushPresetCatalog::find(QStringLiteral("g-pen"))->defaultSize);

        const qreal brushWidth = canvas->brushWidth();
        const qreal eraserWidth = canvas->eraserWidth();
        const qreal brushStabilization = canvas->brushStabilization();
        const qreal eraserStabilization = canvas->eraserStabilization();
        canvas->setBrushWidth(std::numeric_limits<qreal>::quiet_NaN());
        canvas->setEraserWidth(std::numeric_limits<qreal>::infinity());
        canvas->setBrushStabilization(std::numeric_limits<qreal>::infinity());
        canvas->setEraserStabilization(std::numeric_limits<qreal>::quiet_NaN());
        QCOMPARE(canvas->brushWidth(), brushWidth);
        QCOMPARE(canvas->eraserWidth(), eraserWidth);
        QCOMPARE(canvas->brushStabilization(), brushStabilization);
        QCOMPARE(canvas->eraserStabilization(), eraserStabilization);
    }

    void deletesTheLastLayerAndAddsOneBack()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QListWidget *layerList = window.findChild<QListWidget *>();
        QToolButton *addButton =
            window.findChild<QToolButton *>(QStringLiteral("layerAddButton"));
        QToolButton *deleteButton = window.findChild<QToolButton *>(
            QStringLiteral("layerDeleteButton"));
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QAction *redoAction =
            window.findChild<QAction *>(QStringLiteral("redoAction"));
        QAction *clearLayerAction =
            window.findChild<QAction *>(QStringLiteral("clearLayerAction"));
        QVERIFY(canvas);
        QVERIFY(layerList);
        QVERIFY(addButton);
        QVERIFY(deleteButton);
        QVERIFY(undoAction);
        QVERIFY(redoAction);
        QVERIFY(clearLayerAction);
        QCOMPARE(layerList->count(), 1);
        QVERIFY(deleteButton->isEnabled());
        QVERIFY(clearLayerAction->isEnabled());

        QTest::mouseClick(deleteButton, Qt::LeftButton);
        QTRY_COMPARE(layerList->count(), 0);
        QVERIFY(!deleteButton->isEnabled());
        QVERIFY(addButton->isEnabled());
        QVERIFY(!clearLayerAction->isEnabled());
        QVERIFY(window.isWindowModified());

        QSignalSpy interactionMessages(
            canvas, &CanvasWidget::interactionMessage);
        QTest::mouseClick(
            canvas, Qt::LeftButton, Qt::NoModifier, canvas->rect().center());
        QCOMPARE(interactionMessages.size(), 1);
        QCOMPARE(interactionMessages.first().first().toString(),
            QStringLiteral("Add a layer before using this tool."));

        undoAction->trigger();
        QTRY_COMPARE(layerList->count(), 1);
        QVERIFY(deleteButton->isEnabled());
        QVERIFY(clearLayerAction->isEnabled());

        redoAction->trigger();
        QTRY_COMPARE(layerList->count(), 0);
        QVERIFY(addButton->isEnabled());
        QVERIFY(!clearLayerAction->isEnabled());

        QTest::mouseClick(addButton, Qt::LeftButton);
        QTRY_COMPARE(layerList->count(), 1);
        QVERIFY(deleteButton->isEnabled());
        QVERIFY(clearLayerAction->isEnabled());

        undoAction->trigger();
        QTRY_COMPARE(layerList->count(), 0);
        QVERIFY(addButton->isEnabled());
        QVERIFY(!clearLayerAction->isEnabled());
    }

    void mapsDrawingInputThroughTheMirroredCanvas()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.setCanvasMirrored(true);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const QPoint leftOfCenter(100, 200);
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, leftOfCenter);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, leftOfCenter);

        const Layer &layer = controller.document().layers.first();
        QCOMPARE(layer.strokes.size(), 1);
        QVERIFY(layer.strokes.first().points.first().position.x() > 70.0);
    }

    void mapsTabletInputThroughTheRotatedMirroredCanvas()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.setCanvasRotation(37.0);
        canvas.setCanvasMirrored(true);
        QVERIFY(canvas.tabletPressureEnabled());
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        QPointingDevice stylus(QStringLiteral("Rotated canvas stylus"),
            4,
            QInputDevice::DeviceType::Stylus,
            QPointingDevice::PointerType::Pen,
            QInputDevice::Capability::Position
                | QInputDevice::Capability::Pressure,
            1,
            1);
        const QPointF documentStart(18.0, 24.0);
        const QPointF documentMiddle(52.0, 45.0);
        const QPointF documentEnd(82.0, 74.0);
        const QPointF widgetStart =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentStart);
        const QPointF widgetMiddle =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentMiddle);
        const QPointF widgetEnd =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentEnd);
        const QPointF globalStart = canvas.mapToGlobal(widgetStart.toPoint());
        const QPointF globalMiddle = canvas.mapToGlobal(widgetMiddle.toPoint());
        const QPointF globalEnd = canvas.mapToGlobal(widgetEnd.toPoint());

        QTabletEvent tabletPress(QEvent::TabletPress,
            &stylus,
            widgetStart,
            globalStart,
            0.25,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            Qt::NoModifier,
            Qt::LeftButton,
            Qt::LeftButton);
        tabletPress.setTimestamp(100);
        QApplication::sendEvent(&canvas, &tabletPress);
        QTabletEvent tabletMove(QEvent::TabletMove,
            &stylus,
            widgetMiddle,
            globalMiddle,
            0.72,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            Qt::NoModifier,
            Qt::NoButton,
            Qt::LeftButton);
        tabletMove.setTimestamp(108);
        QApplication::sendEvent(&canvas, &tabletMove);
        QTabletEvent tabletRelease(QEvent::TabletRelease,
            &stylus,
            widgetEnd,
            globalEnd,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            Qt::NoModifier,
            Qt::LeftButton,
            Qt::NoButton);
        tabletRelease.setTimestamp(116);
        QApplication::sendEvent(&canvas, &tabletRelease);

        const QVector<Stroke> &strokes =
            controller.document().layers.first().strokes;
        QCOMPARE(strokes.size(), 1);
        const QVector<StrokePoint> &points = strokes.first().points;
        QVERIFY(points.size() >= 3);
        QVERIFY(qAbs(points.first().position.x() - documentStart.x()) < 0.0001);
        QVERIFY(qAbs(points.first().position.y() - documentStart.y()) < 0.0001);
        QCOMPARE(points.first().pressure, 0.25);
        QVERIFY(qAbs(points.at(1).position.x() - documentMiddle.x()) < 0.0001);
        QVERIFY(qAbs(points.at(1).position.y() - documentMiddle.y()) < 0.0001);
        QCOMPARE(points.at(1).pressure, 0.72);
        QVERIFY(qAbs(points.last().position.x() - documentEnd.x()) < 0.0001);
        QVERIFY(qAbs(points.last().position.y() - documentEnd.y()) < 0.0001);
        QCOMPARE(points.last().pressure, 0.72);
    }

    // The row used to give its label an Ignored horizontal size policy, so the
    // stretch beside it took every pixel and the label collapsed to nothing:
    // the panel showed a checkbox with no word next to it.
    void showsTheTabletPressureLabelBesideItsToggle()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(64, 64)));
        CanvasWidget canvas(&controller);

        // Measured on the row itself, at a width that comfortably fits the
        // label: the panel it normally sits in is as wide as the tool dock
        // happens to be, and a label squeezed by a narrow dock is allowed to
        // fall short of its hint. Collapsing to nothing is not, which is what
        // the Ignored size policy used to do here.
        TabletPressureRow row(&canvas, QStringLiteral("benchTabletPressure"));
        row.resize(360, 40);
        row.show();
        QVERIFY(QTest::qWaitForWindowExposed(&row));

        QLabel *label =
            row.findChild<QLabel *>(QStringLiteral("benchTabletPressureLabel"));
        QVERIFY(label);
        QVERIFY(!label->text().isEmpty());
        QVERIFY(label->sizeHint().width() > 0);
        QVERIFY(label->width() >= label->sizeHint().width());

        MainWindow window;
        window.resize(1400, 900);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));
        CanvasWidget *windowCanvas = window.findChild<CanvasWidget *>();
        QVERIFY(windowCanvas);

        const std::pair<CanvasTool, QString> rows[] = {
            {CanvasTool::Brush, QStringLiteral("brushTabletPressure")},
            {CanvasTool::Eraser, QStringLiteral("eraserTabletPressure")},
        };
        for (const auto &[tool, prefix] : rows)
        {
            // The panels live in a stack; only the current page gets laid out,
            // so the row has to be on screen before its width means anything.
            windowCanvas->setTool(tool);
            QCoreApplication::processEvents();
            QLabel *panelLabel =
                window.findChild<QLabel *>(prefix + QStringLiteral("Label"));
            QCheckBox *toggle = window.findChild<QCheckBox *>(
                prefix + QStringLiteral("Toggle"));
            QVERIFY(panelLabel);
            QVERIFY(toggle);
            QVERIFY(!panelLabel->text().isEmpty());
            QVERIFY(panelLabel->width() > 0);
        }
    }

    // The canvas shadow is fourteen antialiased passes over the canvas
    // outline. Stroked on every repaint it cost four times an ordinary one as
    // soon as the canvas was turned off the axes, so a rotated canvas dragged
    // every repaint down, playback included. It is baked once per outline now.
    void bakesTheCanvasShadowOncePerOutline()
    {
        Document document = Document::createDefault(QSize(320, 240));
        DocumentController controller;
        QVERIFY(controller.loadDocument(document));

        CanvasWidget canvas(&controller);
        canvas.resize(500, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        canvas.repaint();
        QCoreApplication::processEvents();

        const qint64 baked = CanvasWidgetTestAccess::shadowCacheKey(canvas);
        QVERIFY(baked != 0);

        canvas.repaint();
        QCoreApplication::processEvents();
        QCOMPARE(CanvasWidgetTestAccess::shadowCacheKey(canvas), baked);

        canvas.rotateCanvasRight();
        canvas.repaint();
        QCoreApplication::processEvents();
        const qint64 rotated = CanvasWidgetTestAccess::shadowCacheKey(canvas);
        QVERIFY(rotated != 0);
        QVERIFY(rotated != baked);

        canvas.repaint();
        QCoreApplication::processEvents();
        QCOMPARE(CanvasWidgetTestAccess::shadowCacheKey(canvas), rotated);
    }

    void disablesTabletPressureForBrushAndEraserStrokes()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        QPointingDevice stylus(QStringLiteral("Pressure toggle stylus"),
            5,
            QInputDevice::DeviceType::Stylus,
            QPointingDevice::PointerType::Pen,
            QInputDevice::Capability::Position
                | QInputDevice::Capability::Pressure,
            1,
            1);
        const auto sendTabletEvent = [&canvas, &stylus](QEvent::Type type,
                                         const QPointF &position,
                                         qreal pressure,
                                         Qt::MouseButton button,
                                         Qt::MouseButtons buttons,
                                         quint64 timestamp)
        {
            QTabletEvent event(type,
                &stylus,
                position,
                canvas.mapToGlobal(position.toPoint()),
                pressure,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                Qt::NoModifier,
                button,
                buttons);
            event.setTimestamp(timestamp);
            QApplication::sendEvent(&canvas, &event);
        };
        const auto drawTabletStroke = [&sendTabletEvent](const QPointF &start,
                                          const QPointF &end,
                                          quint64 timestamp)
        {
            sendTabletEvent(QEvent::TabletPress,
                start,
                0.2,
                Qt::LeftButton,
                Qt::LeftButton,
                timestamp);
            sendTabletEvent(QEvent::TabletMove,
                end,
                0.7,
                Qt::NoButton,
                Qt::LeftButton,
                timestamp + 8);
            sendTabletEvent(QEvent::TabletRelease,
                end,
                0.0,
                Qt::LeftButton,
                Qt::NoButton,
                timestamp + 16);
        };

        canvas.setTabletPressureEnabled(false);
        const QPointF brushStart = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(20.0, 30.0));
        const QPointF brushEnd = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(80.0, 30.0));
        sendTabletEvent(QEvent::TabletPress,
            brushStart,
            0.2,
            Qt::LeftButton,
            Qt::LeftButton,
            100);
        canvas.setTabletPressureEnabled(true);
        sendTabletEvent(QEvent::TabletMove,
            brushEnd,
            0.7,
            Qt::NoButton,
            Qt::LeftButton,
            108);
        sendTabletEvent(QEvent::TabletRelease,
            brushEnd,
            0.0,
            Qt::LeftButton,
            Qt::NoButton,
            116);

        const QVector<Stroke> &brushStrokes =
            controller.document().layers.first().strokes;
        QCOMPARE(brushStrokes.size(), 1);
        QCOMPARE(brushStrokes.first().mode, StrokeMode::Paint);
        for (const StrokePoint &point : brushStrokes.first().points)
        {
            QCOMPARE(point.pressure, 1.0);
        }

        canvas.setTabletPressureEnabled(false);
        canvas.setTool(CanvasWidget::Tool::Eraser);
        const QPointF eraserStart = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(20.0, 70.0));
        const QPointF eraserEnd = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(80.0, 70.0));
        drawTabletStroke(eraserStart, eraserEnd, 200);

        const QVector<Stroke> &allStrokes =
            controller.document().layers.first().strokes;
        QCOMPARE(allStrokes.size(), 2);
        QCOMPARE(allStrokes.last().mode, StrokeMode::Erase);
        for (const StrokePoint &point : allStrokes.last().points)
        {
            QCOMPARE(point.pressure, 1.0);
        }
    }

    void clipsDrawingToolsToPersistentLasso()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const QPoint center = canvas.rect().center();
        canvas.setTool(CanvasWidget::Tool::Lasso);
        const QPoint topLeft = center - QPoint(50, 50);
        const QPoint topRight = center + QPoint(50, -50);
        const QPoint bottomRight = center + QPoint(50, 50);
        const QPoint bottomLeft = center + QPoint(-50, 50);
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(&canvas, topRight, 5);
        QTest::mouseMove(&canvas, bottomRight, 5);
        QTest::mouseMove(&canvas, bottomLeft, 5);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QVERIFY(canvas.hasSelection());

        canvas.setTool(CanvasWidget::Tool::Brush);
        QVERIFY(canvas.hasSelection());
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(120, 0));
        QTest::mouseMove(&canvas, center + QPoint(120, 0), 5);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(120, 0));
        QCOMPARE(controller.document().layers.first().strokes.size(), 1);
        QVERIFY(!controller.document()
                .layers.first()
                .strokes.first()
                .clipMask.isNull());
        QImage rendered = RenderEngine::render(controller.document(), 0);
        QCOMPARE(rendered.pixelColor(20, 50), QColor(Qt::white));
        QCOMPARE(rendered.pixelColor(50, 50), QColor(Qt::black));

        canvas.setBrushColor(QColor(220, 30, 40));
        canvas.setTool(CanvasWidget::Tool::Bucket);
        QVERIFY(canvas.hasSelection());
        QTest::mouseClick(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(0, 25));
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);
        QVERIFY(!controller.document()
                .layers.first()
                .strokes.last()
                .clipMask.isNull());
        rendered = RenderEngine::render(controller.document(), 0);
        QCOMPARE(rendered.pixelColor(50, 42), canvas.brushColor());
        QCOMPARE(rendered.pixelColor(20, 42), QColor(Qt::white));
    }

    void aHiddenGroupRefusesStrokesOnTheLayersInside()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.layers.clear();
        Layer group;
        group.kind = LayerKind::Group;
        group.name = QStringLiteral("Group");
        group.visible = false;
        group.initialCanvasSize = document.size;
        Layer child;
        child.name = QStringLiteral("Child");
        child.parentGroupId = group.id;
        child.initialCanvasSize = document.size;
        const QUuid childId = child.id;
        document.layers.append(std::move(group));
        document.layers.append(std::move(child));
        document.activeLayerId = childId;

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        // The child is visible; the group holding it is not. The guard used to
        // read the layer's own flag only, so the stroke was committed where
        // nobody could see it and nothing said why.
        QSignalSpy messages(&canvas, &CanvasWidget::interactionMessage);
        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(40, 0));
        QTest::mouseMove(&canvas, center + QPoint(40, 0), 5);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(40, 0));

        const Layer *target = controller.document().layer(childId);
        QVERIFY(target);
        QVERIFY(target->strokes.isEmpty());
        QCOMPARE(messages.size(), 1);
    }

    void deactivationCancelsMouseAndTabletInput()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *windowCanvas = window.findChild<CanvasWidget *>();
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QVERIFY(windowCanvas);
        QVERIFY(undoAction);

        const QPoint windowCenter = windowCanvas->rect().center();
        QTest::mousePress(windowCanvas,
            Qt::LeftButton,
            Qt::NoModifier,
            windowCenter - QPoint(30, 0));
        QTest::mouseMove(windowCanvas, windowCenter + QPoint(30, 0), 5);
        QEvent deactivate(QEvent::ApplicationDeactivate);
        QApplication::sendEvent(qApp, &deactivate);
        QTest::mouseRelease(windowCanvas,
            Qt::LeftButton,
            Qt::NoModifier,
            windowCenter + QPoint(30, 0));
        QVERIFY(!undoAction->isEnabled());
        QVERIFY(!window.isWindowModified());

        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        QPointingDevice stylus(QStringLiteral("Test stylus"),
            1,
            QInputDevice::DeviceType::Stylus,
            QPointingDevice::PointerType::Pen,
            QInputDevice::Capability::Position
                | QInputDevice::Capability::Pressure,
            1,
            1);
        const QPointF center = canvas.rect().center();
        const QPointF globalCenter = canvas.mapToGlobal(center.toPoint());
        QTabletEvent tabletPress(QEvent::TabletPress,
            &stylus,
            center,
            globalCenter,
            0.7,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            Qt::NoModifier,
            Qt::LeftButton,
            Qt::LeftButton);
        QApplication::sendEvent(&canvas, &tabletPress);

        QFocusEvent focusOut(QEvent::FocusOut, Qt::ActiveWindowFocusReason);
        QApplication::sendEvent(&canvas, &focusOut);

        QTabletEvent tabletRelease(QEvent::TabletRelease,
            &stylus,
            center,
            globalCenter,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            Qt::NoModifier,
            Qt::LeftButton,
            Qt::NoButton);
        QApplication::sendEvent(&canvas, &tabletRelease);
        QVERIFY(controller.document().layers.first().strokes.isEmpty());

        QTest::mouseClick(
            &canvas, Qt::LeftButton, Qt::NoModifier, center.toPoint());
        QCOMPARE(controller.document().layers.first().strokes.size(), 1);

        canvas.setPanModifierActive(true);
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center.toPoint());
        QCOMPARE(canvas.cursor().shape(), Qt::ClosedHandCursor);
        QEvent ungrabMouse(QEvent::UngrabMouse);
        QApplication::sendEvent(&canvas, &ungrabMouse);
        QCOMPARE(canvas.cursor().shape(), Qt::BlankCursor);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center.toPoint());
        QCOMPARE(controller.document().layers.first().strokes.size(), 1);
    }

    void undoDuringActiveStrokeCancelsStrokeAndPreservesRedoTail()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QAction *redoAction =
            window.findChild<QAction *>(QStringLiteral("redoAction"));
        QVERIFY(canvas);
        QVERIFY(undoAction);
        QVERIFY(redoAction);

        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        DocumentUndoStack *history = controller.undoStack();
        const QSize documentSize = controller.document().size;
        QVERIFY(!documentSize.isEmpty());

        const auto activeLayerStrokeCount = [&controller]() -> qsizetype
        {
            const Document &document = controller.document();
            const Layer *layer = document.layer(document.activeLayerId);
            return layer ? layer->strokes.size() : -1;
        };
        const auto widgetPoint = [canvas](qreal x, qreal y)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                *canvas, QPointF(x, y))
                .toPoint();
        };
        const auto drawStroke = [canvas, &widgetPoint](qreal y)
        {
            const QPoint start = widgetPoint(20.0, y);
            const QPoint end = widgetPoint(80.0, y);
            QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, start);
            QTest::mouseMove(canvas, end, 5);
            QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, end);
        };

        const qreal firstY = documentSize.height() * 0.25;
        const qreal secondY = documentSize.height() * 0.5;
        const qreal thirdY = documentSize.height() * 0.75;
        drawStroke(firstY);
        drawStroke(secondY);
        QCOMPARE(activeLayerStrokeCount(), qsizetype(2));
        QCOMPARE(history->count(), 2);
        QCOMPARE(history->index(), 2);
        QVERIFY(history->canUndo());
        QVERIFY(!history->canRedo());
        QVERIFY(undoAction->isEnabled());
        QVERIFY(!redoAction->isEnabled());

        const QPoint thirdStart = widgetPoint(20.0, thirdY);
        const QPoint thirdEnd = widgetPoint(80.0, thirdY);
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, thirdStart);
        QTest::mouseMove(canvas, thirdEnd, 5);
        QVERIFY(CanvasWidgetTestAccess::drawing(*canvas));

        QVERIFY(undoAction->isEnabled());
        undoAction->trigger();

        QVERIFY(history->canUndo());
        QVERIFY(history->canRedo());
        QVERIFY(redoAction->isEnabled());
        const qsizetype strokesAfterUndo = activeLayerStrokeCount();
        const int historyCountAfterUndo = history->count();
        const int historyIndexAfterUndo = history->index();
        const quint64 revisionAfterUndo =
            DocumentControllerTestAccess::contentRevision(controller);
        const QByteArray documentAfterUndo =
            DocumentSerializer::toJson(controller.document());
        QCOMPARE(strokesAfterUndo, qsizetype(1));
        QCOMPARE(historyCountAfterUndo, 2);
        QCOMPARE(historyIndexAfterUndo, 1);

        QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, thirdEnd);

        QVERIFY(!CanvasWidgetTestAccess::drawing(*canvas));
        QVERIFY2(activeLayerStrokeCount() == strokesAfterUndo,
            qPrintable(
                QStringLiteral("release committed the canceled stroke: "
                               "strokes=%1 (expected %2), historyCount=%3, "
                               "historyIndex=%4, canUndo=%5, canRedo=%6")
                    .arg(activeLayerStrokeCount())
                    .arg(strokesAfterUndo)
                    .arg(history->count())
                    .arg(history->index())
                    .arg(history->canUndo())
                    .arg(history->canRedo())));
        QVERIFY2(history->canRedo(),
            qPrintable(QStringLiteral("the redo tail was discarded: "
                                      "historyCount=%1, historyIndex=%2")
                    .arg(history->count())
                    .arg(history->index())));
        QVERIFY(history->canUndo());
        QVERIFY(undoAction->isEnabled());
        QVERIFY(redoAction->isEnabled());
        QCOMPARE(history->count(), historyCountAfterUndo);
        QCOMPARE(history->index(), historyIndexAfterUndo);
        QCOMPARE(DocumentControllerTestAccess::contentRevision(controller),
            revisionAfterUndo);
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            documentAfterUndo);

        redoAction->trigger();
        QCOMPARE(activeLayerStrokeCount(), qsizetype(2));
        QVERIFY(!history->canRedo());
        QCOMPARE(history->index(), 2);
    }

    void undoDuringActiveLassoCancelsSelectionAndPreservesRedoTail()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QAction *redoAction =
            window.findChild<QAction *>(QStringLiteral("redoAction"));
        QVERIFY(canvas);
        QVERIFY(undoAction);
        QVERIFY(redoAction);

        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        DocumentUndoStack *history = controller.undoStack();
        const QSize documentSize = controller.document().size;
        QVERIFY(!documentSize.isEmpty());

        const auto activeLayerStrokeCount = [&controller]() -> qsizetype
        {
            const Document &document = controller.document();
            const Layer *layer = document.layer(document.activeLayerId);
            return layer ? layer->strokes.size() : -1;
        };
        const auto widgetPoint = [canvas](qreal x, qreal y)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                *canvas, QPointF(x, y))
                .toPoint();
        };

        for (const qreal y :
            {documentSize.height() * 0.25, documentSize.height() * 0.5})
        {
            const QPoint start = widgetPoint(20.0, y);
            const QPoint end = widgetPoint(80.0, y);
            QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, start);
            QTest::mouseMove(canvas, end, 5);
            QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, end);
        }
        QCOMPARE(activeLayerStrokeCount(), qsizetype(2));
        QCOMPARE(history->count(), 2);
        QCOMPARE(history->index(), 2);
        QVERIFY(!history->canRedo());

        canvas->setTool(CanvasWidget::Tool::Lasso);
        QVERIFY(!canvas->hasSelection());
        const QPoint topLeft = widgetPoint(20.0, 20.0);
        const QPoint topRight = widgetPoint(80.0, 20.0);
        const QPoint bottomRight = widgetPoint(80.0, 80.0);
        const QPoint bottomLeft = widgetPoint(20.0, 80.0);
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(canvas, topRight, 5);
        QTest::mouseMove(canvas, bottomRight, 5);
        QTest::mouseMove(canvas, bottomLeft, 5);
        QVERIFY(CanvasWidgetTestAccess::areaSelectionActive(*canvas));

        QVERIFY(undoAction->isEnabled());
        undoAction->trigger();

        QVERIFY(history->canRedo());
        const qsizetype strokesAfterUndo = activeLayerStrokeCount();
        const int historyCountAfterUndo = history->count();
        const int historyIndexAfterUndo = history->index();
        const QByteArray documentAfterUndo =
            DocumentSerializer::toJson(controller.document());
        QCOMPARE(strokesAfterUndo, qsizetype(1));
        QCOMPARE(historyIndexAfterUndo, 1);

        QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);

        QVERIFY2(!canvas->hasSelection(),
            qPrintable(QStringLiteral("release finished the canceled lasso: "
                                      "historyCount=%1, historyIndex=%2, "
                                      "canRedo=%3")
                    .arg(history->count())
                    .arg(history->index())
                    .arg(history->canRedo())));
        QVERIFY(history->canRedo());
        QVERIFY(redoAction->isEnabled());
        QCOMPARE(history->count(), historyCountAfterUndo);
        QCOMPARE(history->index(), historyIndexAfterUndo);
        QCOMPARE(activeLayerStrokeCount(), strokesAfterUndo);
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            documentAfterUndo);

        redoAction->trigger();
        QCOMPARE(activeLayerStrokeCount(), qsizetype(2));
        QVERIFY(!history->canRedo());
    }

    void documentReplacementDuringActiveStrokeCancelsStroke()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(200, 200)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        // Same layer UUID as the live document, the way reopening the same
        // file does; a stroke surviving the boundary would commit cleanly.
        const Document snapshot = controller.document();

        const auto activeLayerStrokeCount = [&controller]() -> qsizetype
        {
            const Document &document = controller.document();
            const Layer *layer = document.layer(document.activeLayerId);
            return layer ? layer->strokes.size() : -1;
        };
        const auto widgetPoint = [&canvas](qreal x, qreal y)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                canvas, QPointF(x, y))
                .toPoint();
        };

        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, widgetPoint(40.0, 60.0));
        QTest::mouseMove(&canvas, widgetPoint(160.0, 60.0), 5);
        QVERIFY(CanvasWidgetTestAccess::drawing(canvas));

        QVERIFY(controller.loadDocument(snapshot));
        QVERIFY(!CanvasWidgetTestAccess::drawing(canvas));

        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, widgetPoint(160.0, 60.0));
        QVERIFY2(activeLayerStrokeCount() == qsizetype(0),
            qPrintable(QStringLiteral(
                "release committed the stroke into the replaced document: "
                "strokes=%1, canUndo=%2")
                    .arg(activeLayerStrokeCount())
                    .arg(controller.undoStack()->canUndo())));
        QVERIFY(!controller.undoStack()->canUndo());
    }

    void documentReplacementDuringActiveLassoCancelsLasso()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(200, 200)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.setTool(CanvasWidget::Tool::Lasso);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const Document snapshot = controller.document();
        const auto widgetPoint = [&canvas](qreal x, qreal y)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                canvas, QPointF(x, y))
                .toPoint();
        };

        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, widgetPoint(40.0, 40.0));
        QTest::mouseMove(&canvas, widgetPoint(160.0, 40.0), 5);
        QTest::mouseMove(&canvas, widgetPoint(160.0, 160.0), 5);
        QVERIFY(CanvasWidgetTestAccess::areaSelectionActive(canvas));

        QVERIFY(controller.loadDocument(snapshot));
        QVERIFY(!CanvasWidgetTestAccess::areaSelectionActive(canvas));

        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, widgetPoint(160.0, 160.0));
        QVERIFY(!canvas.hasSelection());
    }

    void deactivationRestoresLassoAndCancelsSelectionMove()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 8.0;
        stroke.points = {
            {QPointF(20.0, 50.0), 1.0}, {QPointF(80.0, 50.0), 1.0}};
        document.layers.first().strokes = {stroke};

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.setTool(CanvasWidget::Tool::Lasso);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        const QPoint center = canvas.rect().center();
        const QPoint topLeft = center - QPoint(120, 80);
        const QPoint topRight = center + QPoint(120, -80);
        const QPoint bottomRight = center + QPoint(120, 80);
        const QPoint bottomLeft = center + QPoint(-120, 80);
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(&canvas, topRight, 5);
        QTest::mouseMove(&canvas, bottomRight, 5);
        QTest::mouseMove(&canvas, bottomLeft, 5);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QVERIFY(canvas.hasSelection());
        QTRY_VERIFY(canvas.hasTransformableSelection());

        const QByteArray documentBeforeMove =
            DocumentSerializer::toJson(controller.document());
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, center);
        QTest::mouseMove(&canvas, center + QPoint(50, 20), 5);
        QFocusEvent moveFocusOut(QEvent::FocusOut, Qt::ActiveWindowFocusReason);
        QApplication::sendEvent(&canvas, &moveFocusOut);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(50, 20));
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            documentBeforeMove);
        QVERIFY(canvas.hasSelection());
        QTRY_VERIFY(canvas.hasTransformableSelection());

        const QPoint outsideSelection = center + QPoint(140, 140);
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, outsideSelection);
        QTest::mouseMove(&canvas, outsideSelection - QPoint(30, 0), 5);
        QFocusEvent lassoFocusOut(
            QEvent::FocusOut, Qt::ActiveWindowFocusReason);
        QApplication::sendEvent(&canvas, &lassoFocusOut);
        QTest::mouseRelease(&canvas,
            Qt::LeftButton,
            Qt::NoModifier,
            outsideSelection - QPoint(30, 0));
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            documentBeforeMove);
        QVERIFY(canvas.hasSelection());
        QTRY_VERIFY(canvas.hasTransformableSelection());
    }

    void spacePanIsLimitedToCanvas()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QListWidget *layerList = window.findChild<QListWidget *>();
        QToolButton *addButton =
            window.findChild<QToolButton *>(QStringLiteral("layerAddButton"));
        QVERIFY(canvas);
        QVERIFY(layerList);
        QVERIFY(addButton);

        canvas->setFocus(Qt::OtherFocusReason);
        QTest::keyPress(canvas, Qt::Key_Space);
        QCOMPARE(canvas->cursor().shape(), Qt::OpenHandCursor);
        QTest::keyRelease(canvas, Qt::Key_Space);
        QCOMPARE(canvas->cursor().shape(), Qt::BlankCursor);

        const int layerCount = layerList->count();
        addButton->setFocus(Qt::OtherFocusReason);
        QTest::keyClick(addButton, Qt::Key_Space);
        QTRY_COMPARE(layerList->count(), layerCount + 1);
        QCOMPARE(canvas->cursor().shape(), Qt::BlankCursor);

        layerList->setCurrentRow(0);
        layerList->clearSelection();
        QVERIFY(layerList->selectedItems().isEmpty());
        layerList->setFocus(Qt::OtherFocusReason);
        QTest::keyClick(layerList, Qt::Key_Space);
        QCOMPARE(layerList->selectedItems().size(), 1);
        QCOMPARE(canvas->cursor().shape(), Qt::BlankCursor);

        QLineEdit editor(&window);
        editor.show();
        editor.setFocus(Qt::OtherFocusReason);
        QTest::keyClicks(&editor, QStringLiteral("Layer name"));
        QCOMPARE(editor.text(), QStringLiteral("Layer name"));
        QCOMPARE(canvas->cursor().shape(), Qt::BlankCursor);
    }

    void keepsTheCaretInFrontOfASpinBoxSuffix()
    {
        QSpinBox spin;
        spin.setRange(1, 100000);
        spin.setValue(640);
        spin.setSuffix(QStringLiteral(" px"));
        installSuffixCaretGuard(&spin);
        spin.show();
        QVERIFY(QTest::qWaitForWindowExposed(&spin));
        QLineEdit *edit = spin.findChild<QLineEdit *>();
        QVERIFY(edit);
        QCOMPARE(spin.text(), QStringLiteral("640 px"));

        // Where a click on the empty space right of the suffix lands. Typing
        // there used to be dropped by the validator without a trace.
        edit->setCursorPosition(static_cast<int>(spin.text().size()));
        QCOMPARE(edit->cursorPosition(), 3);
        QTest::keyClick(edit, Qt::Key_8);
        QCOMPARE(spin.text(), QStringLiteral("6408 px"));
        QCOMPARE(spin.value(), 6408);

        // And where a click on the suffix itself lands: Qt pushes it to the
        // end, which is the same dead spot.
        edit->setCursorPosition(6);
        QCOMPARE(edit->cursorPosition(), 4);
        QTest::keyClick(edit, Qt::Key_9);
        QCOMPARE(spin.value(), 64089);

        // A selection still owns the caret, so select-all and type keeps
        // replacing the whole number.
        edit->selectAll();
        QVERIFY(edit->hasSelectedText());
        QCOMPARE(edit->cursorPosition(), spin.text().size());
    }

    void takesTypingAfterClickingPastTheSizeSuffix()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.setAnimating(false);
        canvas.setBrushWidth(12.0);

        BrushSizeRow row(
            &canvas, BrushSizeRow::Target::Brush, QStringLiteral("testBrush"));
        row.resize(240, 40);
        row.show();
        QVERIFY(QTest::qWaitForWindowExposed(&row));
        QSpinBox *spin =
            row.findChild<QSpinBox *>(QStringLiteral("testBrushSpin"));
        QVERIFY(spin);
        QCOMPARE(spin->text(), QStringLiteral("12 px"));
        QLineEdit *edit = spin->findChild<QLineEdit *>();
        QVERIFY(edit);

        edit->setFocus(Qt::OtherFocusReason);
        QTest::mouseClick(edit,
            Qt::LeftButton,
            Qt::NoModifier,
            QPoint(edit->width() - 2, edit->height() / 2));
        QCOMPARE(edit->cursorPosition(), 2);
        QTest::keyClick(edit, Qt::Key_5);
        QCOMPARE(spin->value(), 125);
        QCOMPARE(canvas.brushWidth(), 125.0);
    }
};

int runUiDrawingToolTests(int argc, char **argv)
{
    UiDrawingToolTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "UiDrawingToolTests.moc"
