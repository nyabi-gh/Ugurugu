// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/SelectionClipboardCodec.hpp"
#include "support/UiTestHelpers.hpp"
#include "support/UiTestSuites.hpp"

#include <QClipboard>
#include <QMessageBox>
#include <QMimeData>
#include <QtTest/qtesttouch.h>

#include <atomic>
#include <cmath>
#include <memory>
#include <numbers>

namespace ugurugu
{

namespace
{

bool selectionMaskContainsWidgetPoint(
    const CanvasWidget &canvas, const QPoint &widgetPoint)
{
    const QImage mask = CanvasWidgetTestAccess::selectionMask(canvas);
    if (mask.isNull())
    {
        return false;
    }
    const QPointF documentPosition =
        CanvasWidgetTestAccess::mapToDocument(canvas, widgetPoint);
    const int x = static_cast<int>(documentPosition.x());
    const int y = static_cast<int>(documentPosition.y());
    if (x < 0 || y < 0 || x >= mask.width() || y >= mask.height())
    {
        return false;
    }
    return mask.constScanLine(y)[x] >= 128;
}

void dragFreehandQuad(CanvasWidget *canvas,
    const QPoint &topLeft,
    const QPoint &bottomRight,
    Qt::KeyboardModifiers modifiers)
{
    const QPoint topRight(bottomRight.x(), topLeft.y());
    const QPoint bottomLeft(topLeft.x(), bottomRight.y());
    QTest::mousePress(canvas, Qt::LeftButton, modifiers, topLeft);
    QTest::mouseMove(canvas, topRight, 5);
    QTest::mouseMove(canvas, bottomRight, 5);
    QTest::mouseMove(canvas, bottomLeft, 5);
    QTest::mouseRelease(canvas, Qt::LeftButton, modifiers, topLeft);
}

Document outlinedCircleDocument()
{
    Document document = Document::createDefault(QSize(128, 128));
    document.wobbleAmount = 0.0;
    Stroke outline;
    outline.color = Qt::black;
    outline.width = 6.0;
    outline.brush.antialiasing = false;
    for (int sample = 0; sample <= 64; ++sample)
    {
        const qreal angle = 2.0 * std::numbers::pi_v<qreal> * sample / 64.0;
        outline.points.append({QPointF(64.0 + std::cos(angle) * 36.0,
                                   64.0 + std::sin(angle) * 36.0),
            1.0});
    }
    document.layers.first().strokes.append(outline);
    return document;
}

}

class UiSelectionTests final : public QObject
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

    void selectionParticipatesInUndoHistory()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QAction *redoAction =
            window.findChild<QAction *>(QStringLiteral("redoAction"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(undoAction);
        QVERIFY(redoAction);
        QVERIFY(!window.isWindowModified());

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        const QPoint topLeft = center - QPoint(80, 60);
        const QPoint topRight = center + QPoint(80, -60);
        const QPoint bottomRight = center + QPoint(80, 60);
        const QPoint bottomLeft = center + QPoint(-80, 60);
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(canvas, topRight, 5);
        QTest::mouseMove(canvas, bottomRight, 5);
        QTest::mouseMove(canvas, bottomLeft, 5);
        QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);

        QTRY_VERIFY(canvas->hasSelection());
        QVERIFY(undoAction->isEnabled());
        QVERIFY(!window.isWindowModified());

        const QString screenshotPath =
            qEnvironmentVariable("UGURUGU_SELECTION_SCREENSHOT");
        if (!screenshotPath.isEmpty())
        {
            QVERIFY(window.grab().save(screenshotPath, "PNG"));
            QVERIFY(QFileInfo(screenshotPath).size() > 0);
        }

        undoAction->trigger();
        QTRY_VERIFY(!canvas->hasSelection());
        QVERIFY(redoAction->isEnabled());
        QVERIFY(!window.isWindowModified());

        redoAction->trigger();
        QTRY_VERIFY(canvas->hasSelection());
        QVERIFY(!window.isWindowModified());
    }

    void fillsAnOutlinedAreaInsideALassoWithoutLosingTheBorder()
    {
        DocumentController controller;
        QVERIFY(controller.loadDocument(outlinedCircleDocument()));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                canvas, documentPoint)
                .toPoint();
        };
        canvas.setTool(CanvasWidget::Tool::Lasso);
        canvas.setSelectionShape(CanvasWidget::SelectionShape::Rectangle);
        QTest::mousePress(&canvas,
            Qt::LeftButton,
            Qt::NoModifier,
            widgetPoint(QPointF(20.0, 20.0)));
        QTest::mouseRelease(&canvas,
            Qt::LeftButton,
            Qt::NoModifier,
            widgetPoint(QPointF(108.0, 108.0)));
        QTRY_VERIFY(canvas.hasSelection());

        canvas.setBrushColor(QColor(220, 40, 70));
        canvas.setTool(CanvasWidget::Tool::Bucket);
        QTest::mouseClick(&canvas,
            Qt::LeftButton,
            Qt::NoModifier,
            widgetPoint(QPointF(64.0, 64.0)));
        QTRY_COMPARE(controller.document().layers.first().strokes.size(), 2);

        const QImage rendered = RenderEngine::render(controller.document(), 0);
        const QColor center = rendered.pixelColor(64, 64);
        const QColor border = rendered.pixelColor(100, 64);
        const QColor outside = rendered.pixelColor(112, 64);
        QVERIFY(center.red() > 180 && center.green() < 80);
        QVERIFY(border.red() < 40 && border.green() < 40 && border.blue() < 40);
        QVERIFY(outside.red() > 240 && outside.green() > 240
                && outside.blue() > 240);
    }

    void fillsTheSelectionWithTheBrushColor()
    {
        DocumentController controller;
        QVERIFY(controller.loadDocument(outlinedCircleDocument()));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                canvas, documentPoint)
                .toPoint();
        };
        canvas.setTool(CanvasWidget::Tool::Lasso);
        canvas.setSelectionShape(CanvasWidget::SelectionShape::Rectangle);
        QTest::mousePress(&canvas,
            Qt::LeftButton,
            Qt::NoModifier,
            widgetPoint(QPointF(36.0, 36.0)));
        QTest::mouseRelease(&canvas,
            Qt::LeftButton,
            Qt::NoModifier,
            widgetPoint(QPointF(92.0, 92.0)));
        QTRY_VERIFY(canvas.hasSelection());

        canvas.setBrushColor(QColor(40, 100, 220));
        QVERIFY(canvas.fillSelection());
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);
        const QImage rendered = RenderEngine::render(controller.document(), 0);
        const QColor center = rendered.pixelColor(64, 64);
        const QColor border = rendered.pixelColor(100, 64);
        QVERIFY(center.blue() > 180 && center.red() < 80);
        QVERIFY(border.red() < 40 && border.green() < 40 && border.blue() < 40);
    }

    void copyPasteCreatesNewLayerWithoutTouchingTheActiveLayer()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *cutAction =
            window.findChild<QAction *>(QStringLiteral("cutSelectionAction"));
        QAction *copyAction =
            window.findChild<QAction *>(QStringLiteral("copySelectionAction"));
        QAction *pasteAction =
            window.findChild<QAction *>(QStringLiteral("pasteAction"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(cutAction);
        QVERIFY(copyAction);
        QVERIFY(pasteAction);
        QCOMPARE(cutAction->shortcut(), QKeySequence(QKeySequence::Cut));
        QCOMPARE(copyAction->shortcut(), QKeySequence(QKeySequence::Copy));
        QCOMPARE(pasteAction->shortcut(), QKeySequence(QKeySequence::Paste));
        QVERIFY(!cutAction->isEnabled());
        QVERIFY(!copyAction->isEnabled());
        QVERIFY(pasteAction->isEnabled());

        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        const int initialLayerCount =
            static_cast<int>(controller.document().layers.size());
        const QUuid originalLayerId = controller.document().activeLayerId;

        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(120, 0));
        QTest::mouseMove(canvas, center + QPoint(120, 0), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(120, 0));
        QTRY_COMPARE(
            controller.document().layer(originalLayerId)->strokes.size(), 1);

        lassoAction->trigger();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(60, 40));
        QTest::mouseMove(canvas, center + QPoint(60, -40), 5);
        QTest::mouseMove(canvas, center + QPoint(60, 40), 5);
        QTest::mouseMove(canvas, center + QPoint(-60, 40), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(60, 40));
        QTRY_VERIFY(canvas->hasTransformableSelection());
        QTRY_VERIFY(copyAction->isEnabled());
        QVERIFY(cutAction->isEnabled());

        const QByteArray originalStrokes = DocumentSerializer::toJson(
            [&]
            {
                Document single = controller.document();
                single.layers = {*single.layer(originalLayerId)};
                single.activeLayerId = originalLayerId;
                return single;
            }());
        copyAction->trigger();
        const QMimeData *mimeData = QGuiApplication::clipboard()->mimeData();
        QVERIFY(mimeData);
        QVERIFY(mimeData->hasFormat(SelectionClipboardCodec::mimeType()));
        QVERIFY(mimeData->hasImage());

        // Copy itself places the shifted copy on a new layer, keeps the
        // selection following it there, and arms move mode.
        QTRY_COMPARE(
            controller.document().layers.size(), initialLayerCount + 1);
        const QUuid copyLayerId = controller.document().activeLayerId;
        QVERIFY(copyLayerId != originalLayerId);
        QVERIFY(canvas->hasSelection());
        QCOMPARE(canvas->selectionLayerId(), copyLayerId);
        QTRY_VERIFY(canvas->selectionMoveMode());
        QCOMPARE(DocumentSerializer::toJson(
                     [&]
                     {
                         Document single = controller.document();
                         single.layers = {*single.layer(originalLayerId)};
                         single.activeLayerId = originalLayerId;
                         return single;
                     }()),
            originalStrokes);

        pasteAction->trigger();
        QTRY_COMPARE(
            controller.document().layers.size(), initialLayerCount + 2);
        QVERIFY(controller.document().activeLayerId != copyLayerId);
    }

    void showsShortcutChangeNoticeOnceForUpgradedProfiles()
    {
        const QString noticeKey = QStringLiteral("notices/ctrlDDeselects");
        const QString geometryKey = QStringLiteral("window/geometry");
        {
            QSettings settings;
            settings.remove(noticeKey);
            settings.remove(geometryKey);
            settings.sync();
        }
        {
            MainWindow window;
            window.show();
            QVERIFY(QTest::qWaitForWindowExposed(&window));
            QTest::qWait(50);
            QVERIFY(!window.findChild<QMessageBox *>(
                QStringLiteral("shortcutChangeNotice")));
            QSettings settings;
            QVERIFY(settings.value(noticeKey).toBool());
        }
        {
            QSettings settings;
            settings.remove(noticeKey);
            settings.setValue(geometryKey, QByteArray("upgraded"));
            settings.sync();
        }
        {
            MainWindow window;
            window.show();
            QVERIFY(QTest::qWaitForWindowExposed(&window));
            QTRY_VERIFY(window.findChild<QMessageBox *>(
                QStringLiteral("shortcutChangeNotice")));
            QSettings settings;
            QVERIFY(settings.value(noticeKey).toBool());
        }
        {
            MainWindow window;
            window.show();
            QVERIFY(QTest::qWaitForWindowExposed(&window));
            QTest::qWait(50);
            QVERIFY(!window.findChild<QMessageBox *>(
                QStringLiteral("shortcutChangeNotice")));
        }
    }

    void ctrlDDeselectsAndDuplicateHasNoDefaultShortcut()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *deselectAction = window.findChild<QAction *>(
            QStringLiteral("deselectSelectionAction"));
        QAction *selectAllAction =
            window.findChild<QAction *>(QStringLiteral("selectAllAction"));
        QAction *invertAction = window.findChild<QAction *>(
            QStringLiteral("invertSelectionAction"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(deselectAction);
        QVERIFY(selectAllAction);
        QVERIFY(invertAction);
        QCOMPARE(
            deselectAction->shortcut(), QKeySequence(QStringLiteral("Ctrl+D")));
        QVERIFY(!window.findChild<QAction *>(
            QStringLiteral("duplicateSelectionAction")));
        QCOMPARE(
            selectAllAction->shortcut(), QKeySequence(QKeySequence::SelectAll));
        QCOMPARE(invertAction->shortcut(),
            QKeySequence(QStringLiteral("Ctrl+Shift+I")));

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        dragFreehandQuad(canvas,
            center - QPoint(60, 40),
            center + QPoint(60, 40),
            Qt::NoModifier);
        QTRY_VERIFY(canvas->hasSelection());

        QTest::keyClick(canvas, Qt::Key_D, Qt::ControlModifier);
        QTRY_VERIFY(!canvas->hasSelection());
    }

    void selectAllAndInvertFollowConventions()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *selectAllAction =
            window.findChild<QAction *>(QStringLiteral("selectAllAction"));
        QAction *invertAction = window.findChild<QAction *>(
            QStringLiteral("invertSelectionAction"));
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(selectAllAction);
        QVERIFY(invertAction);
        QVERIFY(undoAction);
        QVERIFY(!invertAction->isEnabled());

        selectAllAction->trigger();
        QTRY_VERIFY(canvas->hasSelection());
        const QImage fullMask = CanvasWidgetTestAccess::selectionMask(*canvas);
        QVERIFY(!fullMask.isNull());
        QVERIFY(fullMask.constScanLine(0)[0] >= 128);
        QVERIFY(
            fullMask.constScanLine(fullMask.height() - 1)[fullMask.width() - 1]
            >= 128);
        QTRY_VERIFY(invertAction->isEnabled());

        invertAction->trigger();
        QTRY_VERIFY(!canvas->hasSelection());

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        dragFreehandQuad(canvas,
            center - QPoint(60, 40),
            center + QPoint(60, 40),
            Qt::NoModifier);
        QTRY_VERIFY(canvas->hasSelection());
        QVERIFY(selectionMaskContainsWidgetPoint(*canvas, center));

        invertAction->trigger();
        QTRY_VERIFY(canvas->hasSelection());
        QVERIFY(!selectionMaskContainsWidgetPoint(*canvas, center));
        const QImage invertedMask =
            CanvasWidgetTestAccess::selectionMask(*canvas);
        QVERIFY(invertedMask.constScanLine(0)[0] >= 128);

        undoAction->trigger();
        QTRY_VERIFY(selectionMaskContainsWidgetPoint(*canvas, center));
    }

    void shiftAddsAndAltSubtractsFromTheSelection()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(undoAction);

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        const QPoint leftCenter = center - QPoint(105, 0);
        const QPoint rightCenter = center + QPoint(105, 0);

        dragFreehandQuad(canvas,
            center - QPoint(150, 50),
            center + QPoint(-60, 50),
            Qt::NoModifier);
        QTRY_VERIFY(canvas->hasSelection());
        QVERIFY(selectionMaskContainsWidgetPoint(*canvas, leftCenter));
        QVERIFY(!selectionMaskContainsWidgetPoint(*canvas, rightCenter));

        dragFreehandQuad(canvas,
            center + QPoint(60, -50),
            center + QPoint(150, 50),
            Qt::ShiftModifier);
        QTRY_VERIFY(selectionMaskContainsWidgetPoint(*canvas, rightCenter));
        QVERIFY(selectionMaskContainsWidgetPoint(*canvas, leftCenter));

        dragFreehandQuad(canvas,
            center - QPoint(160, 60),
            center + QPoint(-50, 60),
            Qt::AltModifier);
        QTRY_VERIFY(!selectionMaskContainsWidgetPoint(*canvas, leftCenter));
        QVERIFY(selectionMaskContainsWidgetPoint(*canvas, rightCenter));
        QVERIFY(canvas->hasSelection());

        undoAction->trigger();
        QTRY_VERIFY(selectionMaskContainsWidgetPoint(*canvas, leftCenter));
        QVERIFY(selectionMaskContainsWidgetPoint(*canvas, rightCenter));
        undoAction->trigger();
        QTRY_VERIFY(!selectionMaskContainsWidgetPoint(*canvas, rightCenter));
        QVERIFY(selectionMaskContainsWidgetPoint(*canvas, leftCenter));
    }

    void cutRemovesSelectionAndUndoRestoresIt()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *cutAction =
            window.findChild<QAction *>(QStringLiteral("cutSelectionAction"));
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(cutAction);
        QVERIFY(undoAction);

        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        const QUuid layerId = controller.document().activeLayerId;

        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(120, 0));
        QTest::mouseMove(canvas, center + QPoint(120, 0), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(120, 0));
        QTRY_COMPARE(controller.document().layer(layerId)->strokes.size(), 1);

        lassoAction->trigger();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(60, 40));
        QTest::mouseMove(canvas, center + QPoint(60, -40), 5);
        QTest::mouseMove(canvas, center + QPoint(60, 40), 5);
        QTest::mouseMove(canvas, center + QPoint(-60, 40), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(60, 40));
        QTRY_VERIFY(canvas->hasTransformableSelection());
        QTRY_VERIFY(cutAction->isEnabled());

        const QByteArray beforeCut =
            DocumentSerializer::toJson(controller.document());
        const int layerCountBeforeCut =
            static_cast<int>(controller.document().layers.size());
        cutAction->trigger();
        QTRY_VERIFY(!canvas->hasSelection());
        QCOMPARE(controller.document().layers.size(), layerCountBeforeCut);
        const Layer *layer = controller.document().layer(layerId);
        QVERIFY(layer);
        QCOMPARE(layer->strokes.size(), 2);
        QCOMPARE(layer->strokes.last().mode, StrokeMode::PixelSelection);
        const QMimeData *mimeData = QGuiApplication::clipboard()->mimeData();
        QVERIFY(mimeData);
        QVERIFY(mimeData->hasFormat(SelectionClipboardCodec::mimeType()));

        undoAction->trigger();
        QCOMPARE(DocumentSerializer::toJson(controller.document()), beforeCut);
        QTRY_VERIFY(canvas->hasSelection());
    }

    void selectsAStrokeCrossingTheLasso()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(undoAction);

        const QPoint center = canvas->rect().center();
        const QPoint strokeStart = center - QPoint(150, 0);
        const QPoint strokeEnd = center + QPoint(150, 0);
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, strokeStart);
        QTest::mouseMove(canvas, strokeEnd, 5);
        QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, strokeEnd);
        QTRY_VERIFY(window.isWindowModified());

        lassoAction->trigger();
        const QPoint topLeft = center - QPoint(30, 30);
        const QPoint topRight = center + QPoint(30, -30);
        const QPoint bottomRight = center + QPoint(30, 30);
        const QPoint bottomLeft = center + QPoint(-30, 30);
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(canvas, topRight, 5);
        QTest::mouseMove(canvas, bottomRight, 5);
        QTest::mouseMove(canvas, bottomLeft, 5);
        QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTRY_VERIFY(canvas->hasTransformableSelection());
        QTest::keyClick(canvas, Qt::Key_Delete);

        undoAction->trigger();
        undoAction->trigger();
        QVERIFY(window.isWindowModified());
        undoAction->trigger();
        QVERIFY(!window.isWindowModified());
    }

    void selectsWithRectangleAndEllipseShapes()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;

        Stroke centerStroke;
        centerStroke.width = 2.0;
        centerStroke.points = {
            {QPointF(45.0, 50.0), 1.0}, {QPointF(55.0, 50.0), 1.0}};
        centerStroke.brush.antialiasing = false;
        const QUuid centerId = centerStroke.id;

        Stroke cornerStroke;
        cornerStroke.width = 2.0;
        cornerStroke.points = {
            {QPointF(22.0, 22.0), 1.0}, {QPointF(24.0, 22.0), 1.0}};
        cornerStroke.brush.antialiasing = false;
        const QUuid cornerId = cornerStroke.id;

        document.layers.first().strokes = {centerStroke, cornerStroke};
        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.setZoomPercent(100);
        canvas.setTool(CanvasWidget::Tool::Lasso);

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            return (QPointF(canvas.rect().center()) + documentPoint
                    - QPointF(50.0, 50.0))
                .toPoint();
        };
        const QPoint topLeft = widgetPoint(QPointF(20.0, 20.0));
        const QPoint bottomRight = widgetPoint(QPointF(80.0, 80.0));

        canvas.setSelectionShape(CanvasWidget::SelectionShape::Rectangle);
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, bottomRight);
        QTRY_VERIFY(canvas.hasTransformableSelection());
        const QVector<QUuid> rectangleIds = canvas.selectedStrokeIds();
        QVERIFY(rectangleIds.contains(centerId));
        QVERIFY(rectangleIds.contains(cornerId));

        canvas.deselectSelection();
        canvas.setSelectionShape(CanvasWidget::SelectionShape::Ellipse);
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, bottomRight);
        QTRY_VERIFY(canvas.hasTransformableSelection());
        const QVector<QUuid> ellipseIds = canvas.selectedStrokeIds();
        QVERIFY(ellipseIds.contains(centerId));
        QVERIFY(!ellipseIds.contains(cornerId));
    }

    void selectsRectangleWithTabletPen()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.setZoomPercent(100);
        canvas.setTool(CanvasWidget::Tool::Lasso);
        canvas.setSelectionShape(CanvasWidget::SelectionShape::Rectangle);

        QPointingDevice stylus(QStringLiteral("Selection stylus"),
            2,
            QInputDevice::DeviceType::Stylus,
            QPointingDevice::PointerType::Pen,
            QInputDevice::Capability::Position
                | QInputDevice::Capability::Pressure,
            1,
            1);
        const QPointF start = canvas.rect().center() - QPoint(30, 25);
        const QPointF end = canvas.rect().center() + QPoint(30, 25);
        const QPointF globalStart = canvas.mapToGlobal(start.toPoint());
        const QPointF globalEnd = canvas.mapToGlobal(end.toPoint());
        QTabletEvent tabletPress(QEvent::TabletPress,
            &stylus,
            start,
            globalStart,
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
        QTabletEvent tabletRelease(QEvent::TabletRelease,
            &stylus,
            end,
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
        QApplication::sendEvent(&canvas, &tabletRelease);

        QTRY_VERIFY(canvas.hasSelection());
        QVERIFY(controller.document().layers.first().strokes.isEmpty());
    }

    void preservesTabletPressureAtStrokeEndpoint()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.setZoomPercent(100);
        canvas.setBrushWidth(20.0);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        QPointingDevice stylus(QStringLiteral("Pressure stylus"),
            3,
            QInputDevice::DeviceType::Stylus,
            QPointingDevice::PointerType::Pen,
            QInputDevice::Capability::Position
                | QInputDevice::Capability::Pressure,
            1,
            1);
        const QPointF start = canvas.rect().center() - QPoint(20, 0);
        const QPointF end = canvas.rect().center() + QPoint(20, 0);
        const QPointF globalStart = canvas.mapToGlobal(start.toPoint());
        const QPointF globalEnd = canvas.mapToGlobal(end.toPoint());
        QTabletEvent tabletPress(QEvent::TabletPress,
            &stylus,
            start,
            globalStart,
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
        QTabletEvent tabletMove(QEvent::TabletMove,
            &stylus,
            end,
            globalEnd,
            0.8,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            Qt::NoModifier,
            Qt::NoButton,
            Qt::LeftButton);
        QApplication::sendEvent(&canvas, &tabletMove);
        QTabletEvent tabletRelease(QEvent::TabletRelease,
            &stylus,
            end,
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
        QApplication::sendEvent(&canvas, &tabletRelease);

        const QVector<Stroke> &strokes =
            controller.document().layers.first().strokes;
        QCOMPARE(strokes.size(), 1);
        QCOMPARE(strokes.first().points.last().pressure, 0.8);
        const QImage rendered = RenderEngine::render(controller.document(), 0);
        QVERIFY(rendered.pixelColor(70, 56).alpha() > 0);
    }

    void keepsSelectionAcrossToolsAndTransformsIt()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *brushAction =
            window.findChild<QAction *>(QStringLiteral("brushAction"));
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *bucketAction =
            window.findChild<QAction *>(QStringLiteral("bucketAction"));
        QAction *scaleAction =
            window.findChild<QAction *>(QStringLiteral("scaleSelectionAction"));
        QAction *rotateAction = window.findChild<QAction *>(
            QStringLiteral("rotateSelectionAction"));
        QAction *copyAction =
            window.findChild<QAction *>(QStringLiteral("copySelectionAction"));
        QAction *moveAction =
            window.findChild<QAction *>(QStringLiteral("moveSelectionAction"));
        QAction *applyTransformAction = window.findChild<QAction *>(
            QStringLiteral("applySelectionTransformAction"));
        QAction *cancelTransformAction = window.findChild<QAction *>(
            QStringLiteral("cancelSelectionTransformAction"));
        SelectionActionBar *actionBar =
            window.findChild<SelectionActionBar *>();
        QToolButton *moveButton = window.findChild<QToolButton *>(
            QStringLiteral("moveSelectionButton"));
        QToolButton *applyTransformButton = window.findChild<QToolButton *>(
            QStringLiteral("applySelectionTransformButton"));
        QToolButton *cancelTransformButton = window.findChild<QToolButton *>(
            QStringLiteral("cancelSelectionTransformButton"));
        QVERIFY(canvas);
        QVERIFY(brushAction);
        QVERIFY(lassoAction);
        QVERIFY(bucketAction);
        QVERIFY(scaleAction);
        QVERIFY(rotateAction);
        QVERIFY(copyAction);
        QVERIFY(moveAction);
        QVERIFY(applyTransformAction);
        QVERIFY(cancelTransformAction);
        QVERIFY(actionBar);
        QVERIFY(moveButton);
        QVERIFY(applyTransformButton);
        QVERIFY(cancelTransformButton);
        QVERIFY(!scaleAction->isEnabled());
        QVERIFY(!rotateAction->isEnabled());
        QVERIFY(!copyAction->isEnabled());
        QVERIFY(!moveAction->isEnabled());
        QVERIFY(!applyTransformAction->isEnabled());
        QVERIFY(!cancelTransformAction->isEnabled());
        QVERIFY(!actionBar->isVisible());

        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(60, 0));
        QTest::mouseMove(canvas, center + QPoint(60, 0), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(60, 0));

        lassoAction->trigger();
        const QPoint topLeft = center - QPoint(90, 50);
        const QPoint topRight = center + QPoint(90, -50);
        const QPoint bottomRight = center + QPoint(90, 50);
        const QPoint bottomLeft = center + QPoint(-90, 50);
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(canvas, topRight, 5);
        QTest::mouseMove(canvas, bottomRight, 5);
        QTest::mouseMove(canvas, bottomLeft, 5);
        QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);

        QTRY_VERIFY(canvas->hasSelection());
        QTRY_VERIFY(canvas->hasTransformableSelection());
        QTRY_VERIFY(scaleAction->isEnabled());
        QTRY_VERIFY(rotateAction->isEnabled());
        QTRY_VERIFY(copyAction->isEnabled());
        QTRY_VERIFY(moveAction->isEnabled());
        QTRY_VERIFY(actionBar->isVisible());

        bucketAction->trigger();
        QVERIFY(canvas->hasSelection());
        brushAction->trigger();
        QVERIFY(canvas->hasSelection());
        QVERIFY(canvas->scaleSelection(0.75));
        QVERIFY(canvas->hasPendingSelectionTransform());
        QVERIFY(applyTransformAction->isEnabled());
        QVERIFY(cancelTransformAction->isEnabled());
        QVERIFY(canvas->rotateSelection(90.0));
        QVERIFY(canvas->hasPendingSelectionTransform());
        QTest::mouseClick(applyTransformButton, Qt::LeftButton);
        QTRY_VERIFY(!canvas->hasSelectionTransformSession());
        QVERIFY(!applyTransformAction->isEnabled());
        QVERIFY(!cancelTransformAction->isEnabled());
        QTRY_VERIFY(copyAction->isEnabled());
        copyAction->trigger();
        QTRY_VERIFY(canvas->hasTransformableSelection());

        QTRY_VERIFY(canvas->selectionMoveMode());
        moveButton->setFocus();
        QVERIFY(moveButton->hasFocus());
        QTest::keyClick(moveButton, Qt::Key_Escape);
        QTRY_VERIFY(!canvas->selectionMoveMode());
        QVERIFY(canvas->hasSelection());
        QTest::keyClick(moveButton, Qt::Key_Escape);
        QTRY_VERIFY(!canvas->hasSelection());
    }

    void routesTransformApplyAndCancelThroughMainWindowShortcuts()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString filePath =
            directory.filePath(QStringLiteral("transform-shortcuts.ugu"));
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke source;
        source.width = 10.0;
        source.points = {
            {QPointF(30.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);
        QString error;
        QVERIFY2(DocumentSerializer::save(filePath, document, &error),
            qPrintable(error));

        MainWindow window;
        window.resize(1000, 680);
        QVERIFY(window.openFile(filePath));
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *applyAction = window.findChild<QAction *>(
            QStringLiteral("applySelectionTransformAction"));
        QAction *cancelAction = window.findChild<QAction *>(
            QStringLiteral("cancelSelectionTransformAction"));
        QAction *escapeAction =
            window.findChild<QAction *>(QStringLiteral("escapeCanvasAction"));
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QToolButton *applyButton = window.findChild<QToolButton *>(
            QStringLiteral("applySelectionTransformButton"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(applyAction);
        QVERIFY(cancelAction);
        QVERIFY(escapeAction);
        QVERIFY(undoAction);
        QVERIFY(applyButton);
        QCOMPARE(
            applyAction->shortcut(), QKeySequence(QStringLiteral("Return")));
        QCOMPARE(escapeAction->shortcut(), QKeySequence(Qt::Key_Escape));

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        const QPoint topLeft = center - QPoint(100, 70);
        const QPoint topRight = center + QPoint(100, -70);
        const QPoint bottomRight = center + QPoint(100, 70);
        const QPoint bottomLeft = center + QPoint(-100, 70);
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(canvas, topRight, 5);
        QTest::mouseMove(canvas, bottomRight, 5);
        QTest::mouseMove(canvas, bottomLeft, 5);
        QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTRY_VERIFY(canvas->hasTransformableSelection());
        QVERIFY(!window.isWindowModified());
        const QString undoTextBeforeTransform = undoAction->text();

        QVERIFY(canvas->rotateSelection(15.0));
        QVERIFY(applyAction->isEnabled());
        QVERIFY(cancelAction->isEnabled());
        QVERIFY(canvas->hasPendingSelectionTransform());
        QSignalSpy escapeTriggered(escapeAction, &QAction::triggered);
        applyButton->setFocus();
        QVERIFY(applyButton->hasFocus());
        QTest::keyClick(applyButton, Qt::Key_Escape);
        QTRY_COMPARE(escapeTriggered.size(), 1);
        QVERIFY(!canvas->hasSelectionTransformSession());
        QVERIFY(canvas->hasTransformableSelection());
        QVERIFY(!window.isWindowModified());
        QCOMPARE(undoAction->text(), undoTextBeforeTransform);

        QVERIFY(canvas->scaleSelection(0.8));
        QVERIFY(canvas->hasPendingSelectionTransform());
        QSignalSpy applyTriggered(applyAction, &QAction::triggered);
        canvas->setFocus();
        QTest::keyClick(canvas, Qt::Key_Return);
        QTRY_COMPARE(applyTriggered.size(), 1);
        QTRY_VERIFY(!canvas->hasSelectionTransformSession());
        QTRY_VERIFY(window.isWindowModified());
        QVERIFY(undoAction->text() != undoTextBeforeTransform);

        undoAction->trigger();
        QTRY_VERIFY(!window.isWindowModified());
        QTRY_VERIFY(canvas->hasTransformableSelection());
        QCOMPARE(undoAction->text(), undoTextBeforeTransform);
    }

    void escapeDeselectsAfterConfirmedRotation()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString filePath =
            directory.filePath(QStringLiteral("rotate-then-deselect.ugu"));
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke source;
        source.width = 10.0;
        source.points = {
            {QPointF(25.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);
        QString error;
        QVERIFY2(DocumentSerializer::save(filePath, document, &error),
            qPrintable(error));

        MainWindow window;
        window.resize(1000, 680);
        QVERIFY(window.openFile(filePath));
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *rotateAction = window.findChild<QAction *>(
            QStringLiteral("rotateSelectionAction"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(rotateAction);

        canvas->setSelectionShape(CanvasWidget::SelectionShape::Rectangle);
        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(120, 80));
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(120, 80));
        QTRY_VERIFY(canvas->hasTransformableSelection());

        bool rotationAccepted = false;
        QTimer::singleShot(0,
            &window,
            [&rotationAccepted]()
            {
                QInputDialog *dialog = qobject_cast<QInputDialog *>(
                    QApplication::activeModalWidget());
                if (!dialog)
                {
                    return;
                }
                dialog->setDoubleValue(90.0);
                rotationAccepted = true;
                dialog->accept();
            });
        QTimer::singleShot(1000,
            &window,
            []()
            {
                if (QWidget *dialog = QApplication::activeModalWidget())
                {
                    dialog->close();
                }
            });
        rotateAction->trigger();
        QVERIFY(rotationAccepted);
        QVERIFY(!canvas->hasSelectionTransformSession());
        QTRY_VERIFY(canvas->hasTransformableSelection());
        const QByteArray rotatedDocument = DocumentSerializer::toJson(
            canvas->documentWithPendingSelectionTransform());
        QVERIFY(rotatedDocument != DocumentSerializer::toJson(document));

        canvas->setFocus();
        QTest::keyClick(canvas, Qt::Key_Escape);
        QTRY_VERIFY(!canvas->hasSelection());
        QCOMPARE(DocumentSerializer::toJson(
                     canvas->documentWithPendingSelectionTransform()),
            rotatedDocument);
    }

    void marksPendingTransformUnsavedAndPromptsBeforeClose()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        EnvironmentVariableGuard recoveryGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());
        const QString filePath =
            directory.filePath(QStringLiteral("pending-close.ugu"));
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke source;
        source.width = 10.0;
        source.points = {
            {QPointF(30.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);
        QString error;
        QVERIFY2(DocumentSerializer::save(filePath, document, &error),
            qPrintable(error));

        MainWindow window;
        window.resize(1000, 680);
        QVERIFY(window.openFile(filePath));
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTest::mouseMove(canvas, center + QPoint(100, -70), 5);
        QTest::mouseMove(canvas, center + QPoint(100, 70), 5);
        QTest::mouseMove(canvas, center + QPoint(-100, 70), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTRY_VERIFY(canvas->hasTransformableSelection());
        QVERIFY(!window.isWindowModified());

        QVERIFY(canvas->rotateSelection(15.0));
        QVERIFY(canvas->hasPendingSelectionTransform());
        QTRY_VERIFY(window.isWindowModified());

        bool promptShown = false;
        QTimer::singleShot(0,
            &window,
            [&promptShown]()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (!dialog)
                {
                    return;
                }
                QPushButton *cancelButton = dialog->findChild<QPushButton *>(
                    QStringLiteral("unsavedChangesCancelButton"));
                if (!cancelButton)
                {
                    QTest::keyClick(dialog, Qt::Key_Escape);
                    return;
                }
                promptShown = true;
                cancelButton->click();
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
        QVERIFY(!window.close());
        QVERIFY(promptShown);
        QVERIFY(canvas->hasPendingSelectionTransform());
        QVERIFY(window.isWindowModified());

        canvas->cancelSelectionTransform();
        QTRY_VERIFY(!window.isWindowModified());
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
        QVERIFY(window.close());
    }

    void savesPendingTransformAtomicallyAndBecomesClean()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        EnvironmentVariableGuard recoveryGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());
        const QString filePath =
            directory.filePath(QStringLiteral("pending-save.ugu"));
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke source;
        source.color = QColor(35, 95, 225);
        source.width = 10.0;
        source.points = {
            {QPointF(30.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);
        QString error;
        QVERIFY2(DocumentSerializer::save(filePath, document, &error),
            qPrintable(error));

        MainWindow window;
        window.resize(1000, 680);
        QVERIFY(window.openFile(filePath));
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *saveAction =
            window.findChild<QAction *>(QStringLiteral("saveAction"));
        DocumentUndoStack *undoStack =
            MainWindowTestAccess::controller(window).undoStack();
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(saveAction);
        QVERIFY(undoStack);

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTest::mouseMove(canvas, center + QPoint(100, -70), 5);
        QTest::mouseMove(canvas, center + QPoint(100, 70), 5);
        QTest::mouseMove(canvas, center + QPoint(-100, 70), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTRY_VERIFY(canvas->hasTransformableSelection());

        QVERIFY(canvas->scaleSelection(0.8));
        QVERIFY(canvas->rotateSelection(20.0));
        QVERIFY(canvas->hasPendingSelectionTransform());
        const QImage preview = RenderEngine::render(
            canvas->documentWithPendingSelectionTransform(), 0);
        const QString stackTextBeforeSave = undoStack->undoText();

        saveAction->trigger();
        QTRY_VERIFY(!window.isWindowModified());
        QVERIFY(!canvas->hasPendingSelectionTransform());
        QVERIFY(!canvas->hasSelectionTransformSession());
        QVERIFY(undoStack->canUndo());
        QVERIFY(undoStack->undoText() != stackTextBeforeSave);

        const std::optional<Document> saved =
            DocumentSerializer::load(filePath, &error);
        QVERIFY2(saved.has_value(), qPrintable(error));
        QCOMPARE(saved->layers.first().strokes.size(), 2);
        const Stroke &committed = saved->layers.first().strokes.last();
        QCOMPARE(committed.mode, StrokeMode::PixelSelection);
        QVERIFY(committed.pixelSelectionOp.has_value());
        QCOMPARE(RenderEngine::render(*saved, 0), preview);
        QCOMPARE(RenderEngine::render(
                     canvas->documentWithPendingSelectionTransform(), 0),
            preview);
    }

    void abortsSaveWhenPendingTransformCannotBeApplied()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        EnvironmentVariableGuard recoveryGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());
        const QString filePath =
            directory.filePath(QStringLiteral("pending-abort.ugu"));
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke source;
        source.width = 10.0;
        source.points = {
            {QPointF(30.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);
        QString error;
        QVERIFY2(DocumentSerializer::save(filePath, document, &error),
            qPrintable(error));
        QFile savedFile(filePath);
        QVERIFY(savedFile.open(QIODevice::ReadOnly));
        const QByteArray savedBytes = savedFile.readAll();
        savedFile.close();

        MainWindow window;
        window.resize(1000, 680);
        QVERIFY(window.openFile(filePath));
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *saveAction =
            window.findChild<QAction *>(QStringLiteral("saveAction"));
        DocumentUndoStack *undoStack =
            MainWindowTestAccess::controller(window).undoStack();
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(saveAction);
        QVERIFY(undoStack);
        canvas->setZoomPercent(100);

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(30, 30));
        QTest::mouseMove(canvas, center + QPoint(30, -30), 5);
        QTest::mouseMove(canvas, center + QPoint(30, 30), 5);
        QTest::mouseMove(canvas, center + QPoint(-30, 30), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(30, 30));
        QTRY_VERIFY(canvas->hasTransformableSelection());

        canvas->setSelectionMoveMode(true);
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, center);
        QTest::mouseMove(canvas, center + QPoint(120, 0), 5);
        QTest::mouseMove(canvas, center + QPoint(250, 0), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(250, 0));
        QVERIFY(canvas->hasPendingSelectionTransform());
        const QTransform rejected = canvas->pendingSelectionTransform();
        QVERIFY(rejected.dx() > 89.0);
        const QString stackTextBeforeSave = undoStack->undoText();
        const bool stackEnabledBeforeSave = undoStack->canUndo();

        bool failureShown = false;
        QTimer::singleShot(0,
            &window,
            [&failureShown]()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (!dialog)
                {
                    return;
                }
                failureShown = true;
                QTest::keyClick(dialog, Qt::Key_Escape);
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
        saveAction->trigger();

        QVERIFY(failureShown);
        QVERIFY(canvas->hasPendingSelectionTransform());
        QCOMPARE(canvas->pendingSelectionTransform(), rejected);
        QVERIFY(window.isWindowModified());
        QCOMPARE(undoStack->undoText(), stackTextBeforeSave);
        QCOMPARE(undoStack->canUndo(), stackEnabledBeforeSave);
        QFile unchangedFile(filePath);
        QVERIFY(unchangedFile.open(QIODevice::ReadOnly));
        QCOMPARE(unchangedFile.readAll(), savedBytes);
    }

    void autosavesPendingTransformSnapshotWithoutTouchingHistory()
    {
        const QString recoveryKey = QStringLiteral("recovery/sourcePath");
        SettingValueGuard recoveryValueGuard(recoveryKey);
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());
        const QString filePath =
            directory.filePath(QStringLiteral("pending-autosave.ugu"));
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke source;
        source.width = 10.0;
        source.points = {
            {QPointF(30.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);
        QString error;
        QVERIFY2(DocumentSerializer::save(filePath, document, &error),
            qPrintable(error));

        MainWindow window;
        window.resize(1000, 680);
        QVERIFY(window.openFile(filePath));
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        DocumentUndoStack *undoStack =
            MainWindowTestAccess::controller(window).undoStack();
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(undoStack);

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTest::mouseMove(canvas, center + QPoint(100, -70), 5);
        QTest::mouseMove(canvas, center + QPoint(100, 70), 5);
        QTest::mouseMove(canvas, center + QPoint(-100, 70), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTRY_VERIFY(canvas->hasTransformableSelection());

        QVERIFY(canvas->rotateSelection(25.0));
        QVERIFY(canvas->hasPendingSelectionTransform());
        const QString stackTextBefore = undoStack->undoText();
        const QImage preview = RenderEngine::render(
            canvas->documentWithPendingSelectionTransform(), 0);

        QEvent deactivate(QEvent::ApplicationDeactivate);
        QApplication::sendEvent(qApp, &deactivate);
        QVERIFY(MainWindowTestAccess::flushAutosave(window));
        QVERIFY(QFileInfo::exists(recoveryPath));
        QVERIFY(canvas->hasPendingSelectionTransform());
        QCOMPARE(undoStack->undoText(), stackTextBefore);

        const std::optional<RecoveryStore::Snapshot> recovered =
            RecoveryStore::load(&error);
        QVERIFY2(recovered.has_value(), qPrintable(error));
        QCOMPARE(
            recovered->metadataStatus, RecoveryStore::MetadataStatus::Valid);
        QVERIFY(recovered->metadata.has_value());
        QCOMPARE(recovered->metadata->sourcePath,
            QFileInfo(filePath).absoluteFilePath());
        QCOMPARE(recovered->metadata->revision, quint64(1));
        QCOMPARE(recovered->document.layers.first().strokes.size(), 2);
        QCOMPARE(recovered->document.layers.first().strokes.last().mode,
            StrokeMode::PixelSelection);
        QCOMPARE(RenderEngine::render(recovered->document, 0), preview);

        canvas->cancelSelectionTransform();
        QTRY_VERIFY(!QFileInfo::exists(recoveryPath));
        QVERIFY(!window.isWindowModified());
    }

    void routesUndoToPendingSessionBeforeHistory()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        EnvironmentVariableGuard recoveryGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());
        const QString filePath =
            directory.filePath(QStringLiteral("undo-routing.ugu"));
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke source;
        source.width = 10.0;
        source.points = {
            {QPointF(30.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);
        QString error;
        QVERIFY2(DocumentSerializer::save(filePath, document, &error),
            qPrintable(error));

        MainWindow window;
        window.resize(1000, 680);
        QVERIFY(window.openFile(filePath));
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *saveAction =
            window.findChild<QAction *>(QStringLiteral("saveAction"));
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QAction *redoAction =
            window.findChild<QAction *>(QStringLiteral("redoAction"));
        DocumentUndoStack *undoStack =
            MainWindowTestAccess::controller(window).undoStack();
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(saveAction);
        QVERIFY(undoAction);
        QVERIFY(redoAction);
        QVERIFY(undoStack);

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTest::mouseMove(canvas, center + QPoint(100, -70), 5);
        QTest::mouseMove(canvas, center + QPoint(100, 70), 5);
        QTest::mouseMove(canvas, center + QPoint(-100, 70), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTRY_VERIFY(canvas->hasTransformableSelection());

        QVERIFY(canvas->rotateSelection(12.0));
        QVERIFY(canvas->applySelectionTransform());
        saveAction->trigger();
        QTRY_VERIFY(!window.isWindowModified());
        QTRY_VERIFY(canvas->hasTransformableSelection());
        const QString stackUndoTextClean = undoStack->undoText();
        const bool stackUndoEnabledClean = undoStack->canUndo();
        const bool stackRedoEnabledClean = undoStack->canRedo();
        const QByteArray cleanDocument = DocumentSerializer::toJson(
            canvas->documentWithPendingSelectionTransform());

        canvas->setSelectionMoveMode(true);
        QVERIFY(canvas->selectionMoveMode());
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, center);
        QTest::mouseMove(canvas, center + QPoint(40, 0), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(40, 0));
        QVERIFY(canvas->hasPendingSelectionTransform());
        QVERIFY(undoAction->isEnabled());
        QVERIFY(!redoAction->isEnabled());
        QVERIFY(undoAction->text() != undoStack->undoText());
        QTRY_VERIFY(window.isWindowModified());

        undoAction->trigger();
        QVERIFY(!canvas->hasPendingSelectionTransform());
        QVERIFY(canvas->hasTransformableSelection());
        QVERIFY(!canvas->selectionMoveMode());
        QCOMPARE(undoStack->undoText(), stackUndoTextClean);
        QCOMPARE(undoStack->canUndo(), stackUndoEnabledClean);
        QCOMPARE(undoStack->canRedo(), stackRedoEnabledClean);
        QCOMPARE(DocumentSerializer::toJson(
                     canvas->documentWithPendingSelectionTransform()),
            cleanDocument);
        QTRY_VERIFY(!window.isWindowModified());
        QCOMPARE(undoAction->text(), undoStack->undoText());

        undoAction->trigger();
        QTRY_VERIFY(window.isWindowModified());
        QVERIFY(undoStack->canRedo());
        QVERIFY(redoAction->isEnabled());
        QTRY_VERIFY(canvas->hasTransformableSelection());

        QVERIFY(canvas->rotateSelection(8.0));
        QVERIFY(canvas->hasPendingSelectionTransform());
        QVERIFY(!redoAction->isEnabled());
        redoAction->trigger();
        QVERIFY(canvas->hasPendingSelectionTransform());
        QVERIFY(undoStack->canRedo());

        undoAction->trigger();
        QVERIFY(!canvas->hasPendingSelectionTransform());
        QVERIFY(undoStack->canRedo());
        QVERIFY(redoAction->isEnabled());
    }

    void enablesPendingUndoTextAndSingleUndoAfterApply()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        EnvironmentVariableGuard recoveryGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());
        const QString filePath =
            directory.filePath(QStringLiteral("pending-undo-text.ugu"));
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke source;
        source.width = 10.0;
        source.points = {
            {QPointF(30.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);
        QString error;
        QVERIFY2(DocumentSerializer::save(filePath, document, &error),
            qPrintable(error));

        MainWindow window;
        window.resize(1000, 680);
        QVERIFY(window.openFile(filePath));
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        DocumentUndoStack *undoStack =
            MainWindowTestAccess::controller(window).undoStack();
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(undoAction);
        QVERIFY(undoStack);
        QVERIFY(!undoAction->isEnabled());

        const QImage originalFrame = RenderEngine::render(
            canvas->documentWithPendingSelectionTransform(), 0);

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTest::mouseMove(canvas, center + QPoint(100, -70), 5);
        QTest::mouseMove(canvas, center + QPoint(100, 70), 5);
        QTest::mouseMove(canvas, center + QPoint(-100, 70), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTRY_VERIFY(canvas->hasTransformableSelection());
        QVERIFY(!window.isWindowModified());

        QVERIFY(canvas->rotateSelection(30.0));
        QVERIFY(canvas->hasPendingSelectionTransform());
        QVERIFY(undoAction->isEnabled());
        QVERIFY(undoAction->text() != undoStack->undoText());

        undoAction->trigger();
        QVERIFY(!canvas->hasPendingSelectionTransform());
        QVERIFY(canvas->hasTransformableSelection());
        QTRY_VERIFY(!window.isWindowModified());
        QCOMPARE(undoAction->text(), undoStack->undoText());
        QCOMPARE(RenderEngine::render(
                     canvas->documentWithPendingSelectionTransform(), 0),
            originalFrame);

        QVERIFY(canvas->rotateSelection(30.0));
        QVERIFY(canvas->applySelectionTransform());
        QTRY_VERIFY(window.isWindowModified());
        QVERIFY(RenderEngine::render(
                    canvas->documentWithPendingSelectionTransform(), 0)
                != originalFrame);

        undoAction->trigger();
        QTRY_VERIFY(!window.isWindowModified());
        QCOMPARE(RenderEngine::render(
                     canvas->documentWithPendingSelectionTransform(), 0),
            originalFrame);
    }

    void commitsPendingTransformBeforeResizingCanvas()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        EnvironmentVariableGuard recoveryGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());
        const QString filePath =
            directory.filePath(QStringLiteral("pending-resize.ugu"));
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke source;
        source.width = 10.0;
        source.points = {
            {QPointF(30.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);
        QString error;
        QVERIFY2(DocumentSerializer::save(filePath, document, &error),
            qPrintable(error));

        MainWindow window;
        window.resize(1000, 680);
        QVERIFY(window.openFile(filePath));
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *resizeCanvasAction =
            window.findChild<QAction *>(QStringLiteral("resizeCanvasAction"));
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(resizeCanvasAction);
        QVERIFY(undoAction);

        const QImage originalFrame = RenderEngine::render(
            canvas->documentWithPendingSelectionTransform(), 0);

        lassoAction->trigger();
        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTest::mouseMove(canvas, center + QPoint(100, -70), 5);
        QTest::mouseMove(canvas, center + QPoint(100, 70), 5);
        QTest::mouseMove(canvas, center + QPoint(-100, 70), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 70));
        QTRY_VERIFY(canvas->hasTransformableSelection());

        QVERIFY(canvas->rotateSelection(20.0));
        QVERIFY(canvas->hasPendingSelectionTransform());
        const QImage pendingFrame = RenderEngine::render(
            canvas->documentWithPendingSelectionTransform(), 0);

        bool dialogHandled = false;
        QTimer::singleShot(0,
            &window,
            [&]()
            {
                CanvasSizeDialog *dialog =
                    window.findChild<CanvasSizeDialog *>();
                if (!dialog)
                {
                    return;
                }
                QSpinBox *width = dialog->findChild<QSpinBox *>(
                    QStringLiteral("canvasWidthSpin"));
                if (!width)
                {
                    return;
                }
                width->setValue(width->value() + 20);
                dialogHandled = true;
                dialog->accept();
            });
        resizeCanvasAction->trigger();
        QVERIFY(dialogHandled);
        QVERIFY(!canvas->hasPendingSelectionTransform());
        QVERIFY(!canvas->hasSelectionTransformSession());
        QCOMPARE(canvas->documentWithPendingSelectionTransform().size,
            QSize(120, 100));
        const QImage resizedFrame = RenderEngine::render(
            canvas->documentWithPendingSelectionTransform(), 0);
        QCOMPARE(resizedFrame.copy(QRect(10, 0, 100, 100)), pendingFrame);

        undoAction->trigger();
        QCOMPARE(canvas->documentWithPendingSelectionTransform().size,
            QSize(100, 100));
        QCOMPARE(RenderEngine::render(
                     canvas->documentWithPendingSelectionTransform(), 0),
            pendingFrame);

        undoAction->trigger();
        QCOMPARE(RenderEngine::render(
                     canvas->documentWithPendingSelectionTransform(), 0),
            originalFrame);
        QTRY_VERIFY(!window.isWindowModified());
    }

    void keepsSelectionActionBarReachableInNarrowWindows()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke source;
        source.width = 10.0;
        source.points = {
            {QPointF(30.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(300, 400);
        canvas.setAnimating(false);

        auto *bar = new SelectionActionBar(&canvas);
        const QStringList actionNames = {
            QStringLiteral("moveSelectionAction"),
            QStringLiteral("scaleSelectionAction"),
            QStringLiteral("rotateSelectionAction"),
            QStringLiteral("flipSelectionHorizontalAction"),
            QStringLiteral("flipSelectionVerticalAction"),
            QStringLiteral("applySelectionTransformAction"),
            QStringLiteral("cancelSelectionTransformAction"),
            QStringLiteral("copySelectionAction"),
            QStringLiteral("deleteSelectionAction"),
            QStringLiteral("deselectSelectionAction"),
        };
        for (int index = 0; index < actionNames.size(); ++index)
        {
            auto *action = new QAction(actionNames[index], &canvas);
            action->setObjectName(actionNames[index]);
            bar->addActionButton(action);
            if (index == 2 || index == 4 || index == 6 || index == 8)
            {
                bar->addSeparator();
            }
        }
        canvas.setSelectionActionBar(bar);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        canvas.setTool(CanvasWidget::Tool::Lasso);
        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(60, 60));
        QTest::mouseMove(&canvas, center + QPoint(60, -60), 5);
        QTest::mouseMove(&canvas, center + QPoint(60, 60), 5);
        QTest::mouseMove(&canvas, center + QPoint(-60, 60), 5);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(60, 60));
        QTRY_VERIFY(canvas.hasTransformableSelection());
        QTRY_VERIFY(bar->isVisible());

        QVERIFY(bar->width() > canvas.width());
        QCOMPARE(bar->x(), (canvas.width() - bar->width()) / 2);

        QToolButton *applyButton = bar->findChild<QToolButton *>(
            QStringLiteral("applySelectionTransformButton"));
        QToolButton *cancelButton = bar->findChild<QToolButton *>(
            QStringLiteral("cancelSelectionTransformButton"));
        QVERIFY(applyButton);
        QVERIFY(cancelButton);
        QVERIFY(canvas.rect().contains(
            applyButton->mapTo(&canvas, applyButton->rect().center())));
        QVERIFY(canvas.rect().contains(
            cancelButton->mapTo(&canvas, cancelButton->rect().center())));
    }

    void activatesSelectionActionBarButtonsWithTouch()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);

        QAction action(QStringLiteral("Touch action"), &canvas);
        action.setObjectName(QStringLiteral("touchSelectionAction"));
        QSignalSpy triggers(&action, &QAction::triggered);
        auto *bar = new SelectionActionBar(&canvas);
        QToolButton *button = bar->addActionButton(&action);
        canvas.setSelectionActionBar(bar);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.selectAll();
        QTRY_VERIFY(bar->isVisible());
        QVERIFY(button);

        std::unique_ptr<QPointingDevice> touchScreen(
            QTest::createTouchDevice());
        QVERIFY(touchScreen);
        const QPoint buttonCenter =
            button->mapTo(&canvas, button->rect().center());
        auto touch =
            QTest::touchEvent(canvas.windowHandle(), touchScreen.get(), false);
        touch.press(0, buttonCenter, canvas.windowHandle());
        QVERIFY(touch.commit());
        touch.release(0, buttonCenter, canvas.windowHandle());
        QVERIFY(touch.commit());

        QTRY_COMPARE(triggers.size(), 1);
        QVERIFY(controller.document().layers.first().strokes.isEmpty());
    }

    void restoresSelectionTogetherWithDeletedContent()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.color = QColor(45, 105, 225);
        stroke.width = 8.0;
        stroke.points = {
            {QPointF(10.0, 50.0), 1.0}, {QPointF(90.0, 50.0), 1.0}};
        stroke.brush.antialiasing = false;
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        canvas.setTool(CanvasWidget::Tool::Lasso);
        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(50, 45));
        QTest::mouseMove(&canvas, center + QPoint(50, -45), 1);
        QTest::mouseMove(&canvas, center + QPoint(50, 45), 1);
        QTest::mouseMove(&canvas, center + QPoint(-50, 45), 1);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(50, 45));
        QTRY_VERIFY(canvas.hasTransformableSelection());

        const QImage before = RenderEngine::render(controller.document(), 0);
        const int selectionHistoryIndex = controller.undoStack()->index();
        QVERIFY(canvas.deleteSelection());
        QVERIFY(!canvas.hasSelection());
        const QImage after = RenderEngine::render(controller.document(), 0);
        QVERIFY(after != before);
        QCOMPARE(controller.undoStack()->index(), selectionHistoryIndex + 1);

        controller.undoStack()->undo();
        QTRY_VERIFY(canvas.hasTransformableSelection());
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);

        controller.undoStack()->redo();
        QVERIFY(!canvas.hasSelection());
        QCOMPARE(RenderEngine::render(controller.document(), 0), after);
    }

    void floatingSelectionTransformCommitsOnceAndCancelsLosslessly()
    {
        Document document = Document::createDefault(QSize(120, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke source;
        source.color = QColor(35, 95, 225);
        source.width = 12.0;
        source.points = {
            {QPointF(30.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(480, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            const QPointF center(canvas.rect().center());
            return (
                center + (documentPoint - QPointF(60.0, 50.0)) * canvas.zoom())
                .toPoint();
        };
        canvas.setTool(CanvasWidget::Tool::Lasso);
        const QPoint topLeft = widgetPoint(QPointF(20.0, 35.0));
        const QPoint topRight = widgetPoint(QPointF(80.0, 35.0));
        const QPoint bottomRight = widgetPoint(QPointF(80.0, 65.0));
        const QPoint bottomLeft = widgetPoint(QPointF(20.0, 65.0));
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(&canvas, topRight, 5);
        QTest::mouseMove(&canvas, bottomRight, 5);
        QTest::mouseMove(&canvas, bottomLeft, 5);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTRY_VERIFY(canvas.hasTransformableSelection());

        const QByteArray originalDocument =
            DocumentSerializer::toJson(controller.document());
        const QImage originalFrame =
            RenderEngine::render(controller.document(), 0);
        const int originalUndoCount = controller.undoStack()->count();
        const int originalUndoIndex = controller.undoStack()->index();

        QVERIFY(canvas.scaleSelection(1.2));
        const QTransform afterScale = canvas.pendingSelectionTransform();
        QVERIFY(canvas.rotateSelection(18.0));
        QVERIFY(canvas.pendingSelectionTransform() != afterScale);
        QVERIFY(canvas.flipSelectionHorizontally());
        QVERIFY(canvas.hasPendingSelectionTransform());
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            originalDocument);
        QCOMPARE(controller.document().layers.first().strokes.size(), 1);
        QCOMPARE(controller.undoStack()->count(), originalUndoCount);
        QCOMPARE(controller.undoStack()->index(), originalUndoIndex);

        canvas.handleEscape();
        QVERIFY(!canvas.hasSelectionTransformSession());
        QVERIFY(canvas.hasTransformableSelection());
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            originalDocument);
        QCOMPARE(controller.undoStack()->count(), originalUndoCount);
        QCOMPARE(controller.undoStack()->index(), originalUndoIndex);

        QVERIFY(canvas.scaleSelection(1.2));
        QVERIFY(canvas.rotateSelection(18.0));
        QVERIFY(canvas.flipSelectionHorizontally());
        const QTransform accumulated = canvas.pendingSelectionTransform();
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            originalDocument);
        QVERIFY(canvas.applySelectionTransform());
        QVERIFY(!canvas.hasSelectionTransformSession());
        QCOMPARE(controller.undoStack()->count(), originalUndoCount + 1);
        QCOMPARE(controller.undoStack()->index(), originalUndoIndex + 1);
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);
        const Stroke &operation =
            controller.document().layers.first().strokes.last();
        QCOMPARE(operation.mode, StrokeMode::PixelSelection);
        QVERIFY(operation.pixelSelectionOp.has_value());
        QCOMPARE(operation.pixelSelectionOp->transform, accumulated);

        controller.undoStack()->undo();
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            originalDocument);
        QCOMPARE(RenderEngine::render(controller.document(), 0), originalFrame);

        QTRY_VERIFY(canvas.hasTransformableSelection());
        QVERIFY(canvas.scaleSelection(1.1));
        QVERIFY(canvas.hasPendingSelectionTransform());
        canvas.setTool(CanvasWidget::Tool::Brush);
        QVERIFY(!canvas.hasSelectionTransformSession());
        QVERIFY(canvas.hasTransformableSelection());
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            originalDocument);

        QVERIFY(canvas.scaleSelection(1.1));
        QVERIFY(!canvas.copySelection());
        QVERIFY(canvas.hasSelectionTransformSession());
        canvas.cancelSelectionTransform();
        const int beforeCopyIndex = controller.undoStack()->index();
        QVERIFY(canvas.copySelection());
        QCOMPARE(controller.undoStack()->index(), beforeCopyIndex + 1);
        QCOMPARE(controller.document().layers.size(), 2);
        const Stroke &copyMoveOperation =
            controller.document().layers.last().strokes.last();
        QCOMPARE(copyMoveOperation.mode, StrokeMode::PixelSelection);
        QVERIFY(copyMoveOperation.pixelSelectionOp.has_value());
        QVERIFY(qFuzzyCompare(
            copyMoveOperation.pixelSelectionOp->transform.dx() + 1.0, 13.0));
        controller.undoStack()->undo();
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            originalDocument);

        QTRY_VERIFY(canvas.hasTransformableSelection());
        QVERIFY(canvas.scaleSelection(1.1));
        QVERIFY(controller.resizeCanvas(QSize(130, 100), QPoint(5, 0)));
        QVERIFY(!canvas.hasSelectionTransformSession());
        QTRY_VERIFY(canvas.hasTransformableSelection());

        QVERIFY(canvas.rotateSelection(5.0));
        controller.loadDocument(document);
        QVERIFY(!canvas.hasSelectionTransformSession());
        QVERIFY(!canvas.hasSelection());
    }

    void composesFloatingSelectionActionsInDocumentOrder()
    {
        Document document = Document::createDefault(QSize(120, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke source;
        source.color = QColor(35, 95, 225);
        source.width = 12.0;
        source.points = {
            {QPointF(20.0, 42.0), 1.0}, {QPointF(48.0, 58.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(480, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        canvas.setTool(CanvasWidget::Tool::Lasso);

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                canvas, documentPoint)
                .toPoint();
        };
        dragFreehandQuad(&canvas,
            widgetPoint(QPointF(10.0, 30.0)),
            widgetPoint(QPointF(58.0, 70.0)),
            Qt::NoModifier);
        QTRY_VERIFY(canvas.hasTransformableSelection());
        const QImage sourceMask = CanvasWidgetTestAccess::selectionMask(canvas);
        const Document originalDocument = controller.document();
        const QImage originalFrame = RenderEngine::render(originalDocument, 0);

        const QPointF moveDelta(18.0, 6.0);
        const QPointF moveStart(34.0, 50.0);
        CanvasWidgetTestAccess::beginSelectionMove(canvas, moveStart);
        QVERIFY(canvas.hasSelectionTransformSession());
        const QRectF sourceBounds =
            CanvasWidgetTestAccess::selectionTransformSourceBounds(canvas);
        CanvasWidgetTestAccess::continueSelectionMove(
            canvas, moveStart + moveDelta);
        CanvasWidgetTestAccess::commitSelectionMove(canvas);
        QVERIFY(canvas.hasPendingSelectionTransform());
        const QPointF center =
            CanvasWidgetTestAccess::displayedSelectionBounds(canvas).center();
        QVERIFY(canvas.rotateSelection(90.0));
        const QPointF flipCenter =
            CanvasWidgetTestAccess::displayedSelectionBounds(canvas).center();
        QVERIFY(canvas.flipSelectionHorizontally());

        QTransform move;
        move.translate(moveDelta.x(), moveDelta.y());
        QTransform rotation;
        rotation.translate(center.x(), center.y());
        rotation.rotate(90.0);
        rotation.translate(-center.x(), -center.y());
        QTransform flip;
        flip.translate(flipCenter.x(), flipCenter.y());
        flip.scale(-1.0, 1.0);
        flip.translate(-flipCenter.x(), -flipCenter.y());
        const QTransform expectedTransform = move * rotation * flip;
        QCOMPARE(canvas.pendingSelectionTransform(), expectedTransform);

        const QPointF sourcePoint = sourceBounds.topLeft() + QPointF(7.0, 5.0);
        QPointF expectedPoint = sourcePoint + moveDelta;
        expectedPoint = QPointF(center.x() - (expectedPoint.y() - center.y()),
            center.y() + (expectedPoint.x() - center.x()));
        expectedPoint.setX(2.0 * flipCenter.x() - expectedPoint.x());
        const QPointF actualPoint =
            canvas.pendingSelectionTransform().map(sourcePoint);
        QVERIFY(qAbs(actualPoint.x() - expectedPoint.x()) < 0.0001);
        QVERIFY(qAbs(actualPoint.y() - expectedPoint.y()) < 0.0001);

        const std::optional<PixelSelectionOp> expectedOperation =
            makePixelSelectionOp(sourceMask, expectedTransform, true, true);
        QVERIFY(expectedOperation.has_value());
        Document expectedDocument = originalDocument;
        Stroke operation;
        operation.mode = StrokeMode::PixelSelection;
        operation.pixelSelectionOp = *expectedOperation;
        expectedDocument.layers.first().strokes.append(operation);
        const QImage expectedFrame = RenderEngine::render(expectedDocument, 0);
        QCOMPARE(RenderEngine::render(
                     canvas.documentWithPendingSelectionTransform(), 0),
            expectedFrame);

        QVERIFY(canvas.applySelectionTransform());
        QCOMPARE(RenderEngine::render(controller.document(), 0), expectedFrame);
        controller.undoStack()->undo();
        QCOMPARE(RenderEngine::render(controller.document(), 0), originalFrame);
        controller.undoStack()->redo();
        QCOMPARE(RenderEngine::render(controller.document(), 0), expectedFrame);
    }

    void immediateUndoRedoRestoresTransformedSelectionMask()
    {
        Document document = Document::createDefault(QSize(120, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke source;
        source.width = 12.0;
        source.points = {
            {QPointF(20.0, 50.0), 1.0}, {QPointF(50.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(480, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        canvas.setTool(CanvasWidget::Tool::Lasso);

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                canvas, documentPoint)
                .toPoint();
        };
        dragFreehandQuad(&canvas,
            widgetPoint(QPointF(10.0, 35.0)),
            widgetPoint(QPointF(60.0, 65.0)),
            Qt::NoModifier);
        QTRY_VERIFY(canvas.hasTransformableSelection());
        const QImage originalSelection =
            CanvasWidgetTestAccess::selectionMask(canvas);

        QVERIFY(canvas.scaleSelection(1.4));
        QVERIFY(canvas.rotateSelection(20.0));
        QVERIFY(canvas.applySelectionTransform());
        const QImage transformedSelection =
            CanvasWidgetTestAccess::selectionMask(canvas);
        QVERIFY(transformedSelection != originalSelection);

        controller.undoStack()->undo();
        QCOMPARE(
            CanvasWidgetTestAccess::selectionMask(canvas), originalSelection);

        controller.undoStack()->redo();
        QCOMPARE(CanvasWidgetTestAccess::selectionMask(canvas),
            transformedSelection);
    }

    void undoAfterEmptyVisibilityResultRestoresTransformedSelectionMask()
    {
        Document document = Document::createDefault(QSize(120, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke source;
        source.width = 8.0;
        source.points = {{QPointF(10.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(480, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        canvas.setTool(CanvasWidget::Tool::Lasso);

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                canvas, documentPoint)
                .toPoint();
        };
        dragFreehandQuad(&canvas,
            widgetPoint(QPointF(0.0, 35.0)),
            widgetPoint(QPointF(100.0, 65.0)),
            Qt::NoModifier);
        QTRY_VERIFY(canvas.hasTransformableSelection());
        const QImage originalSelection =
            CanvasWidgetTestAccess::selectionMask(canvas);

        canvas.setSelectionMoveMode(true);
        const QPoint start = widgetPoint(QPointF(50.0, 50.0));
        const QPoint end = widgetPoint(QPointF(0.0, 50.0));
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, start);
        QTest::mouseMove(&canvas, end, 5);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, end);
        QVERIFY(canvas.hasPendingSelectionTransform());
        QVERIFY(canvas.pendingSelectionTransform().dx() < -49.0);
        QVERIFY(canvas.applySelectionTransform());
        const QImage transformedSelection =
            CanvasWidgetTestAccess::selectionMask(canvas);
        QVERIFY(transformedSelection != originalSelection);
        QTRY_VERIFY(!canvas.hasTransformableSelection());

        controller.undoStack()->undo();
        QCOMPARE(
            CanvasWidgetTestAccess::selectionMask(canvas), originalSelection);
        QTRY_VERIFY(canvas.hasTransformableSelection());

        controller.undoStack()->redo();
        QCOMPARE(CanvasWidgetTestAccess::selectionMask(canvas),
            transformedSelection);
    }

    void clipsPartialFloatingTransformAndRetainsFailedSession()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::white;
        document.wobbleAmount = 0.0;
        Stroke source;
        source.color = QColor(35, 95, 225);
        source.width = 10.0;
        source.points = {{QPointF(5.0, 50.0), 1.0}, {QPointF(30.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(420, 420);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            const QPointF center(canvas.rect().center());
            return (
                center + (documentPoint - QPointF(50.0, 50.0)) * canvas.zoom())
                .toPoint();
        };
        const auto sample = [&canvas, &widgetPoint](const QPixmap &pixmap,
                                const QPointF &documentPoint)
        {
            const QImage image = pixmap.toImage();
            const QPoint widgetPosition = widgetPoint(documentPoint);
            const qreal xScale =
                static_cast<qreal>(image.width()) / canvas.width();
            const qreal yScale =
                static_cast<qreal>(image.height()) / canvas.height();
            return image.pixelColor(
                std::clamp(
                    qRound(widgetPosition.x() * xScale), 0, image.width() - 1),
                std::clamp(qRound(widgetPosition.y() * yScale),
                    0,
                    image.height() - 1));
        };
        canvas.setTool(CanvasWidget::Tool::Lasso);
        const QPoint topLeft = widgetPoint(QPointF(0.0, 38.0));
        const QPoint topRight = widgetPoint(QPointF(36.0, 38.0));
        const QPoint bottomRight = widgetPoint(QPointF(36.0, 62.0));
        const QPoint bottomLeft = widgetPoint(QPointF(0.0, 62.0));
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(&canvas, topRight, 5);
        QTest::mouseMove(&canvas, bottomRight, 5);
        QTest::mouseMove(&canvas, bottomLeft, 5);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTRY_VERIFY(canvas.hasTransformableSelection());

        const QByteArray beforeTransform =
            DocumentSerializer::toJson(controller.document());
        const QImage beforeFrame =
            RenderEngine::render(controller.document(), 0);
        const int undoCount = controller.undoStack()->count();
        const int undoIndex = controller.undoStack()->index();

        canvas.setSelectionMoveMode(true);
        const QPoint partialStart = widgetPoint(QPointF(10.0, 50.0));
        const QPoint partialEnd = widgetPoint(QPointF(-10.0, 50.0));
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, partialStart);
        QTest::mouseMove(&canvas, partialEnd, 5);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, partialEnd);
        QVERIFY(canvas.hasPendingSelectionTransform());
        QVERIFY(canvas.pendingSelectionTransform().dx() < -19.0);
        QCOMPARE(
            DocumentSerializer::toJson(controller.document()), beforeTransform);
        const QPixmap partialPreview = canvas.grab();
        QVERIFY(canvas.applySelectionTransform());
        QApplication::processEvents();
        const QPixmap partialCommitted = canvas.grab();
        QCOMPARE(sample(partialPreview, QPointF(3.0, 50.0)),
            sample(partialCommitted, QPointF(3.0, 50.0)));
        QCOMPARE(controller.undoStack()->count(), undoCount + 1);
        QCOMPARE(controller.undoStack()->index(), undoIndex + 1);

        controller.undoStack()->undo();
        QCOMPARE(
            DocumentSerializer::toJson(controller.document()), beforeTransform);
        QCOMPARE(RenderEngine::render(controller.document(), 0), beforeFrame);

        QTRY_VERIFY(canvas.hasTransformableSelection());
        canvas.setSelectionMoveMode(true);
        const QPoint outsideStart = widgetPoint(QPointF(10.0, 50.0));
        const QPoint outsideEnd = widgetPoint(QPointF(-80.0, 50.0));
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, outsideStart);
        QTest::mouseMove(&canvas, outsideEnd, 5);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, outsideEnd);
        QVERIFY(canvas.hasPendingSelectionTransform());
        const QTransform rejected = canvas.pendingSelectionTransform();
        QVERIFY(rejected.dx() < -89.0);
        QVERIFY(!canvas.applySelectionTransform());
        QVERIFY(canvas.hasPendingSelectionTransform());
        QCOMPARE(canvas.pendingSelectionTransform(), rejected);
        QCOMPARE(
            DocumentSerializer::toJson(controller.document()), beforeTransform);
        QCOMPARE(controller.undoStack()->index(), undoIndex);
        canvas.cancelSelectionTransform();
        QVERIFY(!canvas.hasSelectionTransformSession());
        QVERIFY(canvas.hasTransformableSelection());
        QCOMPARE(
            DocumentSerializer::toJson(controller.document()), beforeTransform);
    }

    void lassoModeChangeRestoresThePreviousSelection()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 12.0;
        stroke.points = {
            {QPointF(20.0, 50.0), 1.0}, {QPointF(80.0, 50.0), 1.0}};
        document.layers.first().strokes = {stroke};
        DocumentController controller;
        QVERIFY(controller.loadDocument(document));

        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.setTool(CanvasWidget::Tool::Lasso);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        canvas.selectAll();
        QTRY_VERIFY(canvas.hasTransformableSelection());
        const QImage selectionBefore =
            CanvasWidgetTestAccess::selectionMask(canvas);
        QVERIFY(!selectionBefore.isNull());

        const auto widgetPoint = [&canvas](qreal x, qreal y)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                canvas, QPointF(x, y))
                .toPoint();
        };
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, widgetPoint(30.0, 30.0));
        QTest::mouseMove(&canvas, widgetPoint(70.0, 30.0), 5);
        QTest::mouseMove(&canvas, widgetPoint(70.0, 70.0), 5);
        QVERIFY(CanvasWidgetTestAccess::areaSelectionActive(canvas));

        canvas.setLassoMode(CanvasWidget::LassoMode::Paint);
        QVERIFY(!CanvasWidgetTestAccess::areaSelectionActive(canvas));
        QCOMPARE(
            CanvasWidgetTestAccess::selectionMask(canvas), selectionBefore);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, widgetPoint(70.0, 70.0));
        QCOMPARE(
            CanvasWidgetTestAccess::selectionMask(canvas), selectionBefore);
    }

    void resizeDuringActiveLassoKeepsTheRollbackSnapshot()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 12.0;
        stroke.points = {
            {QPointF(20.0, 50.0), 1.0}, {QPointF(80.0, 50.0), 1.0}};
        document.layers.first().strokes = {stroke};
        DocumentController controller;
        QVERIFY(controller.loadDocument(document));

        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.setTool(CanvasWidget::Tool::Lasso);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        canvas.selectAll();
        QTRY_VERIFY(canvas.hasTransformableSelection());

        const auto widgetPoint = [&canvas](qreal x, qreal y)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                canvas, QPointF(x, y))
                .toPoint();
        };
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, widgetPoint(30.0, 30.0));
        QTest::mouseMove(&canvas, widgetPoint(70.0, 30.0), 5);
        QTest::mouseMove(&canvas, widgetPoint(70.0, 70.0), 5);
        QVERIFY(CanvasWidgetTestAccess::areaSelectionActive(canvas));

        const QSize resized(130, 100);
        QVERIFY(controller.resizeCanvas(resized, QPoint(5, 0)));

        // Escape rolls the live lasso back to the selection that existed
        // before it started; that snapshot has to survive the resize the
        // same way the live selection does.
        canvas.handleEscape();
        QVERIFY(!CanvasWidgetTestAccess::areaSelectionActive(canvas));
        const QImage restored = CanvasWidgetTestAccess::selectionMask(canvas);
        QVERIFY2(!restored.isNull(),
            "the rollback snapshot was lost across the canvas resize");
        QCOMPARE(restored.size(), resized);
        QVERIFY(canvas.hasSelection());
    }

    void unrelatedSelectionDoesNotInheritCopyMoveMode()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 12.0;
        stroke.points = {
            {QPointF(20.0, 50.0), 1.0}, {QPointF(80.0, 50.0), 1.0}};
        document.layers.first().strokes = {stroke};
        DocumentController controller;
        QVERIFY(controller.loadDocument(document));

        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.selectAll();
        QTRY_VERIFY(canvas.hasTransformableSelection());
        QVERIFY(canvas.copySelection());

        // Deselect before the pasted selection's visibility evaluation can
        // deliver, then make an unrelated selection. Its completion must not
        // consume the copy's move-mode arm.
        canvas.deselectSelection();
        QVERIFY(!canvas.selectionMoveMode());
        canvas.selectAll();
        QTRY_VERIFY(canvas.hasTransformableSelection());
        QCoreApplication::processEvents();
        QVERIFY2(!canvas.selectionMoveMode(),
            "an unrelated selection inherited the copy's move-mode arm");
    }

    void requiresExplicitSelectionMoveMode()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);

        QAction moveAction(&canvas);
        moveAction.setObjectName(QStringLiteral("moveSelectionAction"));
        moveAction.setCheckable(true);
        connect(&moveAction,
            &QAction::toggled,
            &canvas,
            &CanvasWidget::setSelectionMoveMode);
        connect(&canvas,
            &CanvasWidget::selectionMoveModeChanged,
            &moveAction,
            &QAction::setChecked);

        auto *actionBar = new SelectionActionBar(&canvas);
        QToolButton *moveButton = actionBar->addActionButton(&moveAction);
        canvas.setSelectionActionBar(actionBar);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        QVERIFY(moveButton);
        QVERIFY(!actionBar->isVisible());

        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(55, 0));
        QTest::mouseMove(&canvas, center + QPoint(55, 0), 5);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(55, 0));

        canvas.setTool(CanvasWidget::Tool::Lasso);
        const QPoint topLeft = center - QPoint(90, 55);
        const QPoint topRight = center + QPoint(90, -55);
        const QPoint bottomRight = center + QPoint(90, 55);
        const QPoint bottomLeft = center + QPoint(-90, 55);
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(&canvas, topRight, 5);
        QTest::mouseMove(&canvas, bottomRight, 5);
        QTest::mouseMove(&canvas, bottomLeft, 5);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);

        QTRY_VERIFY(canvas.hasTransformableSelection());
        QTRY_VERIFY(actionBar->isVisible());
        QVERIFY(!moveAction.isChecked());
        QVERIFY(!canvas.selectionMoveMode());

        const QByteArray beforeInactiveDrag =
            DocumentSerializer::toJson(controller.document());
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, center);
        QTest::mouseMove(&canvas, center + QPoint(30, 10), 5);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(30, 10));
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            beforeInactiveDrag);

        controller.undoStack()->undo();
        QTRY_VERIFY(canvas.hasTransformableSelection());
        QTRY_VERIFY(actionBar->isVisible());
        QTest::mouseClick(moveButton, Qt::LeftButton);
        QVERIFY(moveAction.isChecked());
        QVERIFY(canvas.selectionMoveMode());

        const QByteArray beforeActiveDrag =
            DocumentSerializer::toJson(controller.document());
        const int undoCountBeforeActiveDrag = controller.undoStack()->count();
        const int undoIndexBeforeActiveDrag = controller.undoStack()->index();
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, center);
        QTest::mouseMove(&canvas, center + QPoint(35, 15), 5);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(35, 15));
        QCOMPARE(DocumentSerializer::toJson(controller.document()),
            beforeActiveDrag);
        QVERIFY(canvas.hasPendingSelectionTransform());
        QCOMPARE(controller.undoStack()->count(), undoCountBeforeActiveDrag);
        QCOMPARE(controller.undoStack()->index(), undoIndexBeforeActiveDrag);

        QVERIFY(canvas.applySelectionTransform());
        QVERIFY(!canvas.hasSelectionTransformSession());
        QVERIFY(DocumentSerializer::toJson(controller.document())
                != beforeActiveDrag);
        QCOMPARE(
            controller.undoStack()->count(), undoIndexBeforeActiveDrag + 1);
        QCOMPARE(
            controller.undoStack()->index(), undoIndexBeforeActiveDrag + 1);
    }

    void rejectsSelectionMoveJustOutsideTheCanvas()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 4.0;
        stroke.points = {{QPointF(1.0, 30.0), 1.0}, {QPointF(1.0, 70.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            const QPointF center(canvas.rect().center());
            return (
                center + (documentPoint - QPointF(50.0, 50.0)) * canvas.zoom())
                .toPoint();
        };
        canvas.setTool(CanvasWidget::Tool::Lasso);
        const QPoint topLeft = widgetPoint(QPointF(0.0, 20.0));
        const QPoint topRight = widgetPoint(QPointF(15.0, 20.0));
        const QPoint bottomRight = widgetPoint(QPointF(15.0, 80.0));
        const QPoint bottomLeft = widgetPoint(QPointF(0.0, 80.0));
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(&canvas, topRight, 5);
        QTest::mouseMove(&canvas, bottomRight, 5);
        QTest::mouseMove(&canvas, bottomLeft, 5);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTRY_VERIFY(canvas.hasTransformableSelection());

        canvas.setSelectionMoveMode(true);
        QVERIFY(canvas.selectionMoveMode());
        const QByteArray before =
            DocumentSerializer::toJson(controller.document());
        QSignalSpy messages(&canvas, &CanvasWidget::interactionMessage);
        const QPoint justOutside = widgetPoint(QPointF(-0.25, 50.0));
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, justOutside);
        QTest::mouseMove(&canvas, justOutside + QPoint(20, 0), 5);
        QTest::mouseRelease(&canvas,
            Qt::LeftButton,
            Qt::NoModifier,
            justOutside + QPoint(20, 0));

        QCOMPARE(DocumentSerializer::toJson(controller.document()), before);
        QCOMPARE(messages.size(), 1);
        QCOMPARE(messages.first().first().toString(),
            QStringLiteral("Drag inside the selection to move it."));
    }

    void selectionHitTestingRespectsClipAndLayerVisibility()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 5.0;
        stroke.points = {
            {QPointF(10.0, 50.0), 1.0}, {QPointF(90.0, 50.0), 1.0}};
        stroke.clipMask = QImage(document.size, QImage::Format_Grayscale8);
        stroke.clipMask.fill(0);
        for (int y = 0; y < stroke.clipMask.height(); ++y)
        {
            std::fill_n(stroke.clipMask.scanLine(y), 40, 255);
        }
        document.layers.first().strokes.append(stroke);
        const QUuid layerId = document.activeLayerId;

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            const QPointF center(canvas.rect().center());
            return (
                center + (documentPoint - QPointF(50.0, 50.0)) * canvas.zoom())
                .toPoint();
        };
        const auto lasso = [&canvas, &widgetPoint](const QRectF &documentRect)
        {
            const QPoint topLeft = widgetPoint(documentRect.topLeft());
            const QPoint topRight = widgetPoint(documentRect.topRight());
            const QPoint bottomRight = widgetPoint(documentRect.bottomRight());
            const QPoint bottomLeft = widgetPoint(documentRect.bottomLeft());
            QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
            QTest::mouseMove(&canvas, topRight, 5);
            QTest::mouseMove(&canvas, bottomRight, 5);
            QTest::mouseMove(&canvas, bottomLeft, 5);
            QTest::mouseRelease(
                &canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        };

        canvas.setTool(CanvasWidget::Tool::Lasso);
        lasso(QRectF(60.0, 40.0, 25.0, 20.0));
        QTRY_VERIFY(canvas.hasSelection());
        QVERIFY(!canvas.hasTransformableSelection());

        canvas.deselectSelection();
        controller.setLayerVisible(layerId, false);
        lasso(QRectF(10.0, 40.0, 25.0, 20.0));
        QTRY_VERIFY(canvas.hasSelection());
        QVERIFY(!canvas.hasTransformableSelection());
    }

    void selectionHitTestingUsesProceduralFillCoverage()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;

        Stroke boundary;
        boundary.color = Qt::black;
        boundary.width = 6.0;
        boundary.points = {
            {QPointF(50.0, 0.0), 1.0}, {QPointF(50.0, 99.0), 1.0}};
        boundary.brush.antialiasing = false;

        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(30, 80, 220);
        fill.points = {{QPointF(70.0, 50.0), 1.0}};
        document.layers.first().strokes = {boundary, fill};

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            const QPointF center(canvas.rect().center());
            return (
                center + (documentPoint - QPointF(50.0, 50.0)) * canvas.zoom())
                .toPoint();
        };
        const auto lassoFillCoverage = [&]()
        {
            const QPoint topLeft = widgetPoint(QPointF(58.0, 38.0));
            const QPoint topRight = widgetPoint(QPointF(83.0, 38.0));
            const QPoint bottomRight = widgetPoint(QPointF(83.0, 63.0));
            const QPoint bottomLeft = widgetPoint(QPointF(58.0, 63.0));
            QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
            QTest::mouseMove(&canvas, topRight, 5);
            QTest::mouseMove(&canvas, bottomRight, 5);
            QTest::mouseMove(&canvas, bottomLeft, 5);
            QTest::mouseRelease(
                &canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        };

        canvas.setTool(CanvasWidget::Tool::Lasso);
        lassoFillCoverage();
        QTRY_VERIFY(canvas.hasTransformableSelection());

        canvas.deselectSelection();
        document.layers.first().strokes.last().points = {
            {QPointF(20.0, 50.0), 1.0}};
        controller.loadDocument(document);
        lassoFillCoverage();
        QTRY_VERIFY(canvas.hasSelection());
        QVERIFY(!canvas.hasTransformableSelection());
    }

    void selectionHitTestingUsesFinalLayerPixelsIncludingOperations()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.wobbleAmount = 0.0;

        Stroke paint;
        paint.color = QColor(30, 90, 220);
        paint.width = 12.0;
        paint.points = {{QPointF(10.0, 50.0), 1.0}, {QPointF(35.0, 50.0), 1.0}};
        paint.brush.antialiasing = false;
        document.layers.first().strokes.append(paint);

        QImage sourceMask(document.size, QImage::Format_Grayscale8);
        sourceMask.fill(0);
        for (int y = 40; y < 61; ++y)
        {
            std::fill(
                sourceMask.scanLine(y) + 5, sourceMask.scanLine(y) + 41, 255);
        }
        QTransform shift;
        shift.translate(45.0, 0.0);
        const std::optional<PixelSelectionOp> operation =
            makePixelSelectionOp(sourceMask, shift, true, true);
        QVERIFY(operation.has_value());
        Stroke move;
        move.mode = StrokeMode::PixelSelection;
        move.pixelSelectionOp = *operation;
        document.layers.first().strokes.append(move);

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            const QPointF center(canvas.rect().center());
            return (
                center + (documentPoint - QPointF(50.0, 50.0)) * canvas.zoom())
                .toPoint();
        };
        const auto lasso = [&canvas, &widgetPoint](const QRectF &documentRect)
        {
            const QPoint topLeft = widgetPoint(documentRect.topLeft());
            const QPoint topRight = widgetPoint(documentRect.topRight());
            const QPoint bottomRight = widgetPoint(documentRect.bottomRight());
            const QPoint bottomLeft = widgetPoint(documentRect.bottomLeft());
            QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
            QTest::mouseMove(&canvas, topRight, 5);
            QTest::mouseMove(&canvas, bottomRight, 5);
            QTest::mouseMove(&canvas, bottomLeft, 5);
            QTest::mouseRelease(
                &canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        };

        canvas.setTool(CanvasWidget::Tool::Lasso);
        QSignalSpy messages(&canvas, &CanvasWidget::interactionMessage);
        lasso(QRectF(10.0, 45.0, 20.0, 10.0));
        QTRY_VERIFY(canvas.hasSelection());
        QVERIFY(!canvas.hasTransformableSelection());
        QTRY_VERIFY(!messages.isEmpty());
        QCOMPARE(messages.last().first().toString(),
            QStringLiteral("No content in the selected area."));

        canvas.deselectSelection();
        lasso(QRectF(55.0, 45.0, 25.0, 10.0));
        QTRY_VERIFY(canvas.hasTransformableSelection());
        QCOMPARE(messages.last().first().toString(),
            QStringLiteral("Selected content. Use the action bar to transform "
                           "or remove it."));
    }

    void selectionAvailabilityChecksEveryAnimationFrame()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::transparent;
        document.animationFrames = 12;
        document.wobbleAmount = DocumentLimits::maximumWobbleAmount;
        Stroke animated;
        animated.seed = 0x7d1a2b3c4d5e6f70ULL;
        animated.color = QColor(35, 100, 225);
        animated.width = 2.0;
        animated.points = {
            {QPointF(35.0, 50.0), 1.0}, {QPointF(65.0, 50.0), 1.0}};
        animated.brush.antialiasing = false;
        document.layers.first().strokes.append(animated);

        QVector<QImage> frames;
        frames.reserve(document.animationFrames);
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QImage layerImage;
            QVERIFY(RenderEngine::renderStrokesOnLayer(layerImage,
                document,
                document.layers.first().strokes,
                frame,
                document.size));
            frames.append(std::move(layerImage));
        }

        QPoint laterFrameOnlyPixel(-1, -1);
        const QImage &first = frames.first();
        for (int y = 4; y < first.height() - 4 && laterFrameOnlyPixel.x() < 0;
            ++y)
        {
            for (int x = 4; x < first.width() - 4; ++x)
            {
                bool firstNeighborhoodIsTransparent = true;
                for (int offsetY = -3;
                    offsetY <= 3 && firstNeighborhoodIsTransparent;
                    ++offsetY)
                {
                    for (int offsetX = -3; offsetX <= 3; ++offsetX)
                    {
                        if (first.pixelColor(x + offsetX, y + offsetY).alpha()
                            != 0)
                        {
                            firstNeighborhoodIsTransparent = false;
                            break;
                        }
                    }
                }
                if (!firstNeighborhoodIsTransparent)
                {
                    continue;
                }
                const bool paintedLater = std::any_of(frames.cbegin() + 1,
                    frames.cend(),
                    [x, y](const QImage &frame)
                    {
                        return frame.pixelColor(x, y).alpha() != 0;
                    });
                if (paintedLater)
                {
                    laterFrameOnlyPixel = QPoint(x, y);
                    break;
                }
            }
        }
        QVERIFY(laterFrameOnlyPixel.x() >= 0);

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        QCOMPARE(canvas.currentFrame(), 0);

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            const QPointF center(canvas.rect().center());
            return (
                center + (documentPoint - QPointF(50.0, 50.0)) * canvas.zoom())
                .toPoint();
        };
        const QPoint seed = widgetPoint(QPointF(
            laterFrameOnlyPixel.x() + 0.5, laterFrameOnlyPixel.y() + 0.5));
        canvas.setTool(CanvasWidget::Tool::Wand);
        QTest::mouseClick(&canvas, Qt::LeftButton, Qt::NoModifier, seed);

        QTRY_VERIFY(canvas.hasTransformableSelection());
    }

    void wandReferencesActiveMarkedAndVisibleLayers()
    {
        const auto selectionMask = [](CanvasWidget::WandReference reference,
                                       bool marked) -> std::pair<QImage, bool>
        {
            Document document = Document::createDefault(QSize(100, 100));
            document.background = Qt::transparent;
            document.wobbleAmount = 0.0;
            Layer boundary;
            boundary.name = QStringLiteral("Boundary");
            boundary.initialCanvasSize = document.size;
            boundary.reference = marked;
            Stroke outline;
            outline.width = 4.0;
            outline.brush.antialiasing = false;
            outline.points = {{QPointF(20.0, 20.0), 1.0},
                {QPointF(80.0, 20.0), 1.0},
                {QPointF(80.0, 80.0), 1.0},
                {QPointF(20.0, 80.0), 1.0},
                {QPointF(20.0, 20.0), 1.0}};
            boundary.strokes.append(outline);
            document.layers.append(boundary);

            DocumentController controller;
            controller.loadDocument(document);
            CanvasWidget canvas(&controller);
            canvas.resize(400, 400);
            canvas.setAnimating(false);
            canvas.show();
            if (!QTest::qWaitForWindowExposed(&canvas))
            {
                return {{}, false};
            }
            canvas.fitToWindow();
            canvas.setWandReference(reference);
            canvas.setTool(CanvasWidget::Tool::Wand);
            QTest::mouseClick(&canvas,
                Qt::LeftButton,
                Qt::NoModifier,
                canvas.rect().center());
            if (!canvas.hasSelection())
            {
                return {{}, false};
            }
            canvas.setTool(CanvasWidget::Tool::Bucket);
            QTest::mouseClick(&canvas,
                Qt::LeftButton,
                Qt::NoModifier,
                canvas.rect().center());
            const Layer *active =
                controller.document().layer(document.activeLayerId);
            if (!active || active->strokes.isEmpty())
            {
                return {{}, false};
            }
            return {active->strokes.constLast().clipMask, true};
        };

        const auto activeLayer =
            selectionMask(CanvasWidget::WandReference::ActiveLayer, false);
        QVERIFY(activeLayer.second);
        QVERIFY(activeLayer.first.isNull());

        const auto visibleLayers =
            selectionMask(CanvasWidget::WandReference::AllVisibleLayers, false);
        QVERIFY(visibleLayers.second);
        const QImage &visibleLayersMask = visibleLayers.first;
        QVERIFY(!visibleLayersMask.isNull());
        QCOMPARE(visibleLayersMask.constScanLine(5)[5], static_cast<uchar>(0));
        QCOMPARE(
            visibleLayersMask.constScanLine(50)[50], static_cast<uchar>(255));

        const auto referenceLayers =
            selectionMask(CanvasWidget::WandReference::ReferenceLayers, true);
        QVERIFY(referenceLayers.second);
        const QImage &referenceLayersMask = referenceLayers.first;
        QVERIFY(!referenceLayersMask.isNull());
        QCOMPARE(
            referenceLayersMask.constScanLine(5)[5], static_cast<uchar>(0));
        QCOMPARE(
            referenceLayersMask.constScanLine(50)[50], static_cast<uchar>(255));

        const auto missingReference =
            selectionMask(CanvasWidget::WandReference::ReferenceLayers, false);
        QVERIFY(!missingReference.second);
    }

    void wandAndBucketReachALayerInsideAGroup()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        document.layers.clear();
        Layer group;
        group.kind = LayerKind::Group;
        group.name = QStringLiteral("Group");
        group.initialCanvasSize = document.size;
        Layer child;
        child.name = QStringLiteral("Child");
        child.parentGroupId = group.id;
        child.initialCanvasSize = document.size;
        Stroke outline;
        outline.width = 4.0;
        outline.brush.antialiasing = false;
        outline.points = {{QPointF(20.0, 20.0), 1.0},
            {QPointF(80.0, 20.0), 1.0},
            {QPointF(80.0, 80.0), 1.0},
            {QPointF(20.0, 80.0), 1.0},
            {QPointF(20.0, 20.0), 1.0}};
        child.strokes.append(outline);
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
        canvas.fitToWindow();

        // The active-layer reference used to render the layer on its own
        // without detaching it from its group, so the composition plan saw a
        // parent that was not there, refused the whole frame, and both tools
        // returned in silence for every layer inside a group.
        canvas.setWandReference(CanvasWidget::WandReference::ActiveLayer);
        canvas.setTool(CanvasWidget::Tool::Wand);
        QTest::mouseClick(
            &canvas, Qt::LeftButton, Qt::NoModifier, canvas.rect().center());
        QVERIFY(canvas.hasSelection());

        canvas.setBrushColor(QColor(220, 30, 40));
        canvas.setTool(CanvasWidget::Tool::Bucket);
        QTest::mouseClick(
            &canvas, Qt::LeftButton, Qt::NoModifier, canvas.rect().center());
        const Layer *filled = controller.document().layer(childId);
        QVERIFY(filled);
        QCOMPARE(filled->strokes.size(), 2);
        QCOMPARE(filled->strokes.constLast().mode, StrokeMode::Fill);
        QCOMPARE(
            RenderEngine::render(controller.document(), 0).pixelColor(50, 50),
            canvas.brushColor());
    }

    void selectionVisibilitySkipsRedundantStaticFrames()
    {
        Document document = Document::createDefault(QSize(64, 64));
        document.background = Qt::transparent;
        document.animationFrames = 60;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.points = {
            {QPointF(24.0, 32.0), 1.0}, {QPointF(40.0, 32.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        QImage mask(document.size, QImage::Format_Grayscale8);
        mask.fill(0);
        mask.scanLine(0)[0] = 255;

        SelectionVisibility::Result result = SelectionVisibility::evaluate(
            document, document.layers.first(), mask, 23);
        QVERIFY(result.renderSucceeded);
        QVERIFY(!result.hasVisiblePixels);
        QCOMPARE(result.renderedFrames, 1);
        QCOMPARE(result.renderedPixels, 1ULL);
        QCOMPARE(result.maximumExplicitImageBytes, 4ULL);

        document.wobbleAmount = 1.0;
        result = SelectionVisibility::evaluate(
            document, document.layers.first(), mask, 23);
        QVERIFY(result.renderSucceeded);
        QVERIFY(!result.hasVisiblePixels);
        QCOMPARE(result.renderedFrames, document.animationFrames);
        QCOMPARE(result.renderedPixels,
            static_cast<quint64>(document.animationFrames));
        QCOMPARE(result.maximumExplicitImageBytes, 4ULL);
    }

    void resolvesAnimatedSelectionVisibilityWithoutBlockingUi()
    {
        Document document = Document::createDefault(QSize(2048, 2048));
        document.background = Qt::transparent;
        document.animationFrames = 60;
        document.wobbleAmount = 4.0;
        Stroke stroke;
        stroke.width = 20.0;
        stroke.points = {
            {QPointF(800.0, 1024.0), 1.0}, {QPointF(1248.0, 1024.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(800, 600);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        canvas.setSelectionShape(CanvasWidget::SelectionShape::Rectangle);
        canvas.setTool(CanvasWidget::Tool::Lasso);

        bool eventLoopAdvanced = false;
        QTimer::singleShot(0,
            &canvas,
            [&eventLoopAdvanced]()
            {
                eventLoopAdvanced = true;
            });
        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(100, 50));
        QTest::mouseMove(&canvas, center + QPoint(100, 50), 1);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(100, 50));
        QVERIFY(canvas.hasSelection());
        QTRY_VERIFY(eventLoopAdvanced);
        QTRY_VERIFY_WITH_TIMEOUT(canvas.hasTransformableSelection(), 5000);
    }

    void restoredSelectionReusesTheCachedVisibility()
    {
        Document document = Document::createDefault(QSize(128, 128));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 12.0;
        stroke.points = {
            {QPointF(30.0, 64.0), 1.0}, {QPointF(98.0, 64.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.setSelectionShape(CanvasWidget::SelectionShape::Rectangle);
        canvas.setTool(CanvasWidget::Tool::Lasso);
        canvas.selectAll();
        QTRY_VERIFY(canvas.hasTransformableSelection());

        // Starting a lasso drops the selection and Escape puts the very same
        // mask back on an unchanged document. That answer is already cached, so
        // it has to apply straight away instead of behind another render.
        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(40, 40));
        QTest::mouseMove(&canvas, center, 1);
        QVERIFY(!canvas.hasSelection());
        canvas.handleEscape();
        QVERIFY(canvas.hasSelection());
        QVERIFY2(canvas.hasTransformableSelection(),
            "the restored selection waited on a redundant evaluation");
    }

    void documentChangeCancelsTheSupersededVisibilityEvaluation()
    {
        // Wobble over every frame makes one evaluation an animation-long
        // render, so a superseded run left going competes with its own
        // replacement for the pool.
        Document document = Document::createDefault(QSize(256, 256));
        document.background = Qt::transparent;
        document.animationFrames = 60;
        document.wobbleAmount = 4.0;
        Stroke stroke;
        stroke.width = 12.0;
        stroke.points = {
            {QPointF(60.0, 128.0), 1.0}, {QPointF(196.0, 128.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.selectAll();
        const std::shared_ptr<const std::atomic_bool> superseded =
            CanvasWidgetTestAccess::selectionVisibilityCancellation(canvas);
        QVERIFY(superseded);
        QVERIFY(!superseded->load(std::memory_order_relaxed));

        Stroke added;
        added.width = 8.0;
        added.points = {
            {QPointF(60.0, 40.0), 1.0}, {QPointF(196.0, 40.0), 1.0}};
        QCOMPARE(
            controller.addStroke(controller.document().activeLayerId, added),
            DocumentController::AddStrokeResult::Added);

        QVERIFY2(superseded->load(std::memory_order_relaxed),
            "the superseded evaluation kept rendering on the pool");
        const std::shared_ptr<const std::atomic_bool> replacement =
            CanvasWidgetTestAccess::selectionVisibilityCancellation(canvas);
        QVERIFY(replacement);
        QVERIFY(replacement != superseded);
        QVERIFY(!replacement->load(std::memory_order_relaxed));
    }

    void selectionVisibilityStopsWhenCancelled()
    {
        Document document = Document::createDefault(QSize(64, 64));
        document.background = Qt::transparent;
        document.animationFrames = 60;
        document.wobbleAmount = 1.0;
        Stroke stroke;
        stroke.points = {
            {QPointF(24.0, 32.0), 1.0}, {QPointF(40.0, 32.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        QImage mask(document.size, QImage::Format_Grayscale8);
        mask.fill(0);
        mask.scanLine(0)[0] = 255;

        std::atomic_bool cancelled{true};
        const SelectionVisibility::Result result =
            SelectionVisibility::evaluate(
                document, document.layers.first(), mask, 0, &cancelled);
        QCOMPARE(result.renderedFrames, 0);
        QVERIFY2(!result.renderSucceeded,
            "a cancelled evaluation reported an answer it never finished");
    }

    void scrubbingFramesRefreshesStrokePropertyActions()
    {
        // Wobble carries the stroke in and out of a tight selection, so which
        // strokes it can edit is a property of the displayed frame.
        Document document = Document::createDefault(QSize(128, 128));
        document.background = Qt::transparent;
        document.animationFrames = 8;
        document.wobbleAmount = 12.0;
        Stroke stroke;
        stroke.width = 4.0;
        stroke.points = {
            {QPointF(60.0, 64.0), 1.0}, {QPointF(68.0, 64.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        QImage mask(document.size, QImage::Format_Grayscale8);
        mask.fill(0);
        for (int y = 62; y <= 66; ++y)
        {
            std::fill(mask.scanLine(y) + 58, mask.scanLine(y) + 71, 255);
        }
        const auto editableAt = [&document, &mask](int frame)
        {
            return !SelectionVisibility::editableStrokeIds(
                document, document.layers.first(), mask, frame)
                        .isEmpty();
        };
        QVERIFY(!editableAt(0));
        QVERIFY(editableAt(1));
        QVERIFY(!editableAt(2));

        MainWindow window;
        window.resize(800, 600);
        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        QVERIFY(controller.loadDocument(document));
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *editStrokeProperties = window.findChild<QAction *>(
            QStringLiteral("editStrokePropertiesAction"));
        QVERIFY(canvas);
        QVERIFY(editStrokeProperties);
        canvas->setAnimating(false);
        controller.pushSelectionStateCommand(QStringLiteral("Select"),
            {},
            {},
            controller.document().activeLayerId,
            mask);
        QTRY_VERIFY(canvas->hasTransformableSelection());
        QVERIFY(!editStrokeProperties->isEnabled());

        canvas->setCurrentFrame(1);
        QVERIFY2(editStrokeProperties->isEnabled(),
            "scrubbing onto an editable frame left the action disabled");
        canvas->setCurrentFrame(2);
        QVERIFY2(!editStrokeProperties->isEnabled(),
            "scrubbing off an editable frame left the action enabled");

        // Playback skips the per-frame search; stopping it settles the state
        // on the frame that stays on screen.
        canvas->setAnimating(true);
        canvas->setCurrentFrame(1);
        canvas->setAnimating(false);
        QVERIFY2(editStrokeProperties->isEnabled(),
            "stopping playback left the action stale");
    }

    void packedSelectionSnapshotRoundTripsWithin4kUndoBudget()
    {
        constexpr int edge = 4096;
        QImage mask(QSize(edge, edge), QImage::Format_Grayscale8);
        QVERIFY(!mask.isNull());
        quint32 random = 0x6d2b79f5U;
        for (int y = 0; y < edge; ++y)
        {
            uchar *line = mask.scanLine(y);
            for (int x = 0; x < edge; ++x)
            {
                random ^= random << 13U;
                random ^= random >> 17U;
                random ^= random << 5U;
                line[x] = (random & 1U) != 0U ? 255 : 0;
            }
        }
        mask.scanLine(0)[0] = 255;
        mask.scanLine(edge - 1)[edge - 1] = 255;

        const std::optional<PackedMaskRegion> snapshot = packBinaryMask(mask);
        QVERIFY(snapshot.has_value());
        QCOMPARE(snapshot->bounds, QRect(QPoint(), mask.size()));
        QVERIFY(snapshot->packedMask.size() <= qsizetype(2 * 1024 * 1024));
        QCOMPARE(unpackBinaryMask(*snapshot), mask);

        DocumentController controller;
        controller.newDocument(mask.size());
        const QUuid layerId = controller.document().activeLayerId;
        QImage restored;
        QObject::connect(&controller,
            &DocumentController::selectionHistoryStateRequested,
            &controller,
            [&restored](const QUuid &, const QImage &state)
            {
                restored = state;
            });
        controller.pushSelectionStateCommand(
            QStringLiteral("4K selection snapshot"), {}, {}, layerId, mask);
        QCOMPARE(restored, mask);
        controller.undoStack()->undo();
        QVERIFY(restored.isNull());
        controller.undoStack()->redo();
        QCOMPARE(restored, mask);

        mask.fill(0);
        QVERIFY(!packBinaryMask(mask).has_value());
    }

    void failedCanvasResizeMacroPreservesSelectionAndHistory()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.color = QColor(35, 95, 225);
        stroke.width = 12.0;
        stroke.points = {
            {QPointF(30.0, 50.0), 1.0}, {QPointF(70.0, 50.0), 1.0}};
        stroke.brush.antialiasing = false;
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.setAnimating(false);

        QImage selection(document.size, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 35; y <= 65; ++y)
        {
            std::fill(
                selection.scanLine(y) + 20, selection.scanLine(y) + 81, 255);
        }
        const QUuid layerId = controller.document().activeLayerId;
        controller.pushSelectionStateCommand(
            QStringLiteral("Select"), {}, {}, layerId, selection);
        QVERIFY(canvas.hasSelection());
        QTRY_VERIFY(canvas.hasTransformableSelection());

        const QByteArray beforeDocument =
            DocumentSerializer::toJson(controller.document());
        const int beforeCount = controller.undoStack()->count();
        const int beforeIndex = controller.undoStack()->index();

        controller.undoStack()->beginMacro(QStringLiteral("Resize canvas"));
        canvas.deselectSelection();
        // Selection changes are journaled while a macro is open; the UI is
        // updated only if the whole document transaction commits.
        QVERIFY(canvas.hasSelection());
        QVERIFY(!controller.resizeCanvas(controller.document().size,
            QPoint(static_cast<int>(
                       DocumentLimits::maximumStoredCoordinateMagnitude)
                       + 1,
                0)));
        controller.undoStack()->endMacro();

        QCOMPARE(
            DocumentSerializer::toJson(controller.document()), beforeDocument);
        QVERIFY(canvas.hasSelection());
        QVERIFY(canvas.hasTransformableSelection());
        QCOMPARE(controller.undoStack()->count(), beforeCount);
        QCOMPARE(controller.undoStack()->index(), beforeIndex);
    }

    void emptyLassoDoesNotCreateSelectionHistory()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const int undoCount = controller.undoStack()->count();
        const QPoint center = canvas.rect().center();
        canvas.setTool(CanvasWidget::Tool::Lasso);
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(60, 0));
        QTest::mouseMove(&canvas, center, 5);
        QTest::mouseMove(&canvas, center + QPoint(60, 0), 5);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(60, 0));

        QVERIFY(!canvas.hasSelection());
        QCOMPARE(controller.undoStack()->count(), undoCount);
    }

    void selectionMovePreviewMatchesCommittedPixelOperation()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::white;
        document.wobbleAmount = 0.0;

        Stroke destination;
        destination.color = QColor(220, 35, 35);
        destination.width = 20.0;
        destination.points = {
            {QPointF(60.0, 50.0), 1.0}, {QPointF(90.0, 50.0), 1.0}};
        destination.brush.antialiasing = false;

        Stroke source;
        source.color = QColor(25, 90, 220);
        source.width = 20.0;
        source.points = {
            {QPointF(10.0, 50.0), 1.0}, {QPointF(40.0, 50.0), 1.0}};
        source.brush.antialiasing = false;

        Stroke hole;
        hole.mode = StrokeMode::Erase;
        hole.width = 8.0;
        hole.points = {{QPointF(25.0, 50.0), 1.0}};
        hole.brush.antialiasing = false;
        document.layers.first().strokes = {destination, source, hole};

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(500, 500);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            const QPointF center(canvas.rect().center());
            return (
                center + (documentPoint - QPointF(50.0, 50.0)) * canvas.zoom())
                .toPoint();
        };
        const auto lasso = [&canvas, &widgetPoint](const QRectF &documentRect)
        {
            const QPoint topLeft = widgetPoint(documentRect.topLeft());
            const QPoint topRight = widgetPoint(documentRect.topRight());
            const QPoint bottomRight = widgetPoint(documentRect.bottomRight());
            const QPoint bottomLeft = widgetPoint(documentRect.bottomLeft());
            QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
            QTest::mouseMove(&canvas, topRight, 5);
            QTest::mouseMove(&canvas, bottomRight, 5);
            QTest::mouseMove(&canvas, bottomLeft, 5);
            QTest::mouseRelease(
                &canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        };
        const auto sample =
            [&canvas](const QPixmap &pixmap, const QPoint &widgetPosition)
        {
            const QImage image = pixmap.toImage();
            const qreal xScale =
                static_cast<qreal>(image.width()) / canvas.width();
            const qreal yScale =
                static_cast<qreal>(image.height()) / canvas.height();
            const int x = std::clamp(
                qRound(widgetPosition.x() * xScale), 0, image.width() - 1);
            const int y = std::clamp(
                qRound(widgetPosition.y() * yScale), 0, image.height() - 1);
            return image.pixelColor(x, y);
        };

        canvas.setTool(CanvasWidget::Tool::Lasso);
        lasso(QRectF(5.0, 38.0, 40.0, 24.0));
        QTRY_VERIFY(canvas.hasTransformableSelection());
        canvas.setSelectionMoveMode(true);
        const QByteArray beforeTransform =
            DocumentSerializer::toJson(controller.document());
        const int undoIndexBeforeTransform = controller.undoStack()->index();

        const QPoint dragStart = widgetPoint(QPointF(15.0, 50.0));
        const QPoint dragEnd = widgetPoint(QPointF(65.0, 50.0));
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, dragStart);
        QTest::mouseMove(&canvas, dragEnd, 5);
        QApplication::processEvents();
        const QPixmap preview = canvas.grab();

        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, dragEnd);
        QApplication::processEvents();
        QVERIFY(canvas.hasPendingSelectionTransform());
        QCOMPARE(
            DocumentSerializer::toJson(controller.document()), beforeTransform);
        QCOMPARE(controller.undoStack()->index(), undoIndexBeforeTransform);
        QVERIFY(canvas.applySelectionTransform());
        QApplication::processEvents();
        const QPixmap committed = canvas.grab();
        QCOMPARE(controller.undoStack()->index(), undoIndexBeforeTransform + 1);

        const QPoint blueSample = widgetPoint(QPointF(65.0, 50.0));
        const QPoint holeSample = widgetPoint(QPointF(75.0, 50.0));
        QCOMPARE(sample(preview, blueSample), sample(committed, blueSample));
        QCOMPARE(sample(preview, holeSample), sample(committed, holeSample));
        QCOMPARE(sample(committed, holeSample), destination.color);

        controller.undoStack()->undo();
        QCOMPARE(
            DocumentSerializer::toJson(controller.document()), beforeTransform);
    }

    void appliesChosenSelectionTransformSampling_data()
    {
        QTest::addColumn<int>("samplingValue");
        QTest::newRow("smooth") << static_cast<int>(SamplingMode::Smooth);
        QTest::newRow("pixel-based") << static_cast<int>(SamplingMode::Nearest);
    }

    void appliesChosenSelectionTransformSampling()
    {
        QFETCH(int, samplingValue);
        const SamplingMode sampling = static_cast<SamplingMode>(samplingValue);

        Document document = Document::createDefault(QSize(96, 96));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke source;
        source.color = QColor(35, 105, 220);
        source.width = 28.0;
        source.points = {
            {QPointF(32.0, 48.0), 1.0}, {QPointF(64.0, 48.0), 1.0}};
        source.brush.tipShape = BrushTipShape::Square;
        source.brush.sizeDynamics = 0.0;
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        QCOMPARE(canvas.selectionTransformSampling(), SamplingMode::Smooth);

        canvas.selectAll();
        QTRY_VERIFY(canvas.hasTransformableSelection());
        QVERIFY(canvas.rotateSelection(17.0));
        QVERIFY(canvas.hasPendingSelectionTransform());
        canvas.setSelectionTransformSampling(sampling);
        QCOMPARE(canvas.selectionTransformSampling(), sampling);

        const Document pending = canvas.documentWithPendingSelectionTransform();
        const Stroke &pendingOperation = pending.layers.first().strokes.last();
        QVERIFY(pendingOperation.pixelSelectionOp.has_value());
        QCOMPARE(pendingOperation.pixelSelectionOp->sampling, sampling);
        const QImage pendingFrame = RenderEngine::render(pending, 0);
        QVERIFY(!pendingFrame.isNull());

        QVERIFY(canvas.applySelectionTransform());
        const Stroke &committedOperation =
            controller.document().layers.first().strokes.last();
        QVERIFY(committedOperation.pixelSelectionOp.has_value());
        QCOMPARE(committedOperation.pixelSelectionOp->sampling, sampling);
        const QImage committedFrame =
            RenderEngine::render(controller.document(), 0);
        QCOMPARE(committedFrame, pendingFrame);

        int partialAlphaPixels = 0;
        for (int y = 0; y < committedFrame.height(); ++y)
        {
            const auto *line =
                reinterpret_cast<const QRgb *>(committedFrame.constScanLine(y));
            for (int x = 0; x < committedFrame.width(); ++x)
            {
                const int alpha = qAlpha(line[x]);
                partialAlphaPixels += alpha > 0 && alpha < 255 ? 1 : 0;
            }
        }
        if (sampling == SamplingMode::Smooth)
        {
            QVERIFY(partialAlphaPixels > 0);
        }
        else
        {
            QCOMPARE(partialAlphaPixels, 0);
        }

        controller.undoStack()->undo();
        QCOMPARE(canvas.selectionTransformSampling(), sampling);
        controller.undoStack()->redo();
        QCOMPARE(canvas.selectionTransformSampling(), sampling);
        QCOMPARE(controller.document()
                     .layers.first()
                     .strokes.last()
                     .pixelSelectionOp->sampling,
            sampling);
    }

    void sequentialSmoothSelectionTransformLeavesNoSourceFringe()
    {
        Document document = Document::createDefault(QSize(160, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke source;
        source.color = QColor(35, 95, 225);
        source.width = 20.0;
        source.points = {
            {QPointF(20.0, 50.0), 1.0}, {QPointF(60.0, 50.0), 1.0}};
        source.brush.antialiasing = false;
        document.layers.first().strokes.append(source);

        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(600, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();

        const auto widgetPoint = [&canvas](const QPointF &documentPoint)
        {
            const QPointF center(canvas.rect().center());
            return (
                center + (documentPoint - QPointF(80.0, 50.0)) * canvas.zoom())
                .toPoint();
        };
        const QPoint topLeft = widgetPoint(QPointF(8.0, 35.0));
        const QPoint topRight = widgetPoint(QPointF(72.0, 35.0));
        const QPoint bottomRight = widgetPoint(QPointF(72.0, 65.0));
        const QPoint bottomLeft = widgetPoint(QPointF(8.0, 65.0));
        canvas.setTool(CanvasWidget::Tool::Lasso);
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(&canvas, topRight, 5);
        QTest::mouseMove(&canvas, bottomRight, 5);
        QTest::mouseMove(&canvas, bottomLeft, 5);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTRY_VERIFY(canvas.hasTransformableSelection());

        const QByteArray beforeTransform =
            DocumentSerializer::toJson(controller.document());
        const int undoCountBeforeTransform = controller.undoStack()->count();
        const int undoIndexBeforeTransform = controller.undoStack()->index();
        QVERIFY(canvas.rotateSelection(27.0));
        QTRY_VERIFY(canvas.hasTransformableSelection());
        QCOMPARE(
            DocumentSerializer::toJson(controller.document()), beforeTransform);
        QCOMPARE(controller.undoStack()->count(), undoCountBeforeTransform);

        canvas.setSelectionMoveMode(true);
        const QPoint dragStart = widgetPoint(QPointF(40.0, 50.0));
        const QPoint dragEnd = widgetPoint(QPointF(120.0, 50.0));
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, dragStart);
        QTest::mouseMove(&canvas, dragEnd, 5);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, dragEnd);

        QVERIFY(canvas.hasPendingSelectionTransform());
        QCOMPARE(
            DocumentSerializer::toJson(controller.document()), beforeTransform);
        QCOMPARE(controller.undoStack()->index(), undoIndexBeforeTransform);
        QVERIFY(canvas.applySelectionTransform());
        QCOMPARE(controller.undoStack()->count(), undoCountBeforeTransform + 1);
        QCOMPARE(controller.undoStack()->index(), undoIndexBeforeTransform + 1);
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);
        QCOMPARE(controller.document().layers.first().strokes.last().mode,
            StrokeMode::PixelSelection);

        const QImage rendered = RenderEngine::render(controller.document(), 0);
        QVERIFY(!rendered.isNull());
        bool sourceFringeRemains = false;
        for (int y = 0; y < rendered.height() && !sourceFringeRemains; ++y)
        {
            const auto *line =
                reinterpret_cast<const QRgb *>(rendered.constScanLine(y));
            for (int x = 0; x < 80; ++x)
            {
                if (qAlpha(line[x]) != 0)
                {
                    sourceFringeRemains = true;
                    break;
                }
            }
        }
        QVERIFY(!sourceFringeRemains);
    }

    void clearsSelectionSafelyAcrossCanvasAndImageResizeUndo()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *lassoAction =
            window.findChild<QAction *>(QStringLiteral("lassoAction"));
        QAction *resizeCanvasAction =
            window.findChild<QAction *>(QStringLiteral("resizeCanvasAction"));
        QAction *resizeImageAction =
            window.findChild<QAction *>(QStringLiteral("resizeImageAction"));
        QAction *undoAction =
            window.findChild<QAction *>(QStringLiteral("undoAction"));
        QVERIFY(canvas);
        QVERIFY(lassoAction);
        QVERIFY(resizeCanvasAction);
        QVERIFY(resizeImageAction);
        QVERIFY(undoAction);

        const auto expectedFitZoom = [canvas](const QSize &size)
        {
            return std::clamp(std::min((canvas->width() - 64.0) / size.width(),
                                  (canvas->height() - 64.0) / size.height()),
                0.01,
                16.0);
        };
        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(60, 0));
        QTest::mouseMove(canvas, center + QPoint(60, 0), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(60, 0));
        lassoAction->trigger();
        const QPoint topLeft = center - QPoint(90, 50);
        const QPoint topRight = center + QPoint(90, -50);
        const QPoint bottomRight = center + QPoint(90, 50);
        const QPoint bottomLeft = center + QPoint(-90, 50);
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(canvas, topRight, 5);
        QTest::mouseMove(canvas, bottomRight, 5);
        QTest::mouseMove(canvas, bottomLeft, 5);
        QTest::mouseRelease(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTRY_VERIFY(canvas->hasTransformableSelection());

        bool canvasDialogHandled = false;
        QSize originalCanvasSize;
        QSize resizedCanvasSize;
        canvas->setZoomPercent(200);
        QTimer::singleShot(0,
            &window,
            [&]()
            {
                CanvasSizeDialog *dialog =
                    window.findChild<CanvasSizeDialog *>();
                if (!dialog)
                {
                    return;
                }
                QSpinBox *width = dialog->findChild<QSpinBox *>(
                    QStringLiteral("canvasWidthSpin"));
                QSpinBox *height = dialog->findChild<QSpinBox *>(
                    QStringLiteral("canvasHeightSpin"));
                if (!width || !height)
                {
                    return;
                }
                originalCanvasSize = QSize(width->value(), height->value());
                width->setValue(width->value() + 24);
                resizedCanvasSize = QSize(width->value(), height->value());
                canvasDialogHandled = true;
                dialog->accept();
            });
        resizeCanvasAction->trigger();
        QVERIFY(canvasDialogHandled);
        QVERIFY(!canvas->hasSelection());
        QVERIFY(qAbs(canvas->zoom() - expectedFitZoom(resizedCanvasSize))
                < 0.000001);
        undoAction->trigger();
        QTRY_VERIFY(canvas->hasTransformableSelection());
        QVERIFY(qAbs(canvas->zoom() - expectedFitZoom(originalCanvasSize))
                < 0.000001);

        bool imageDialogHandled = false;
        QSize resizedImageSize;
        canvas->setZoomPercent(175);
        QTimer::singleShot(0,
            &window,
            [&]()
            {
                ImageSizeDialog *dialog = window.findChild<ImageSizeDialog *>();
                if (!dialog)
                {
                    return;
                }
                QDoubleSpinBox *percentage =
                    dialog->findChild<QDoubleSpinBox *>(
                        QStringLiteral("imageScalePercentSpin"));
                QSpinBox *width = dialog->findChild<QSpinBox *>(
                    QStringLiteral("imageWidthSpin"));
                QSpinBox *height = dialog->findChild<QSpinBox *>(
                    QStringLiteral("imageHeightSpin"));
                if (!percentage || !width || !height)
                {
                    return;
                }
                percentage->setValue(110.0);
                resizedImageSize = QSize(width->value(), height->value());
                imageDialogHandled = true;
                dialog->accept();
            });
        resizeImageAction->trigger();
        QVERIFY(imageDialogHandled);
        QVERIFY(!canvas->hasSelection());
        QVERIFY(qAbs(canvas->zoom() - expectedFitZoom(resizedImageSize))
                < 0.000001);
        undoAction->trigger();
        QTRY_VERIFY(canvas->hasTransformableSelection());
        QVERIFY(qAbs(canvas->zoom() - expectedFitZoom(originalCanvasSize))
                < 0.000001);

        canvas->deselectSelection();
        QVERIFY(!canvas->hasSelection());
        lassoAction->trigger();
        QTest::mousePress(canvas, Qt::LeftButton, Qt::NoModifier, topLeft);
        QTest::mouseMove(canvas, topRight, 5);
        QTest::mouseMove(canvas, bottomRight, 5);

        bool activeLassoDialogHandled = false;
        QSize lassoResizeSize;
        canvas->setZoomPercent(150);
        QTimer::singleShot(0,
            &window,
            [&]()
            {
                CanvasSizeDialog *dialog =
                    window.findChild<CanvasSizeDialog *>();
                if (!dialog)
                {
                    return;
                }
                QSpinBox *width = dialog->findChild<QSpinBox *>(
                    QStringLiteral("canvasWidthSpin"));
                QSpinBox *height = dialog->findChild<QSpinBox *>(
                    QStringLiteral("canvasHeightSpin"));
                if (!width || !height)
                {
                    return;
                }
                width->setValue(width->value() + 1);
                lassoResizeSize = QSize(width->value(), height->value());
                activeLassoDialogHandled = true;
                dialog->accept();
            });
        resizeCanvasAction->trigger();
        QVERIFY(activeLassoDialogHandled);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, bottomRight);
        QVERIFY(!canvas->hasSelection());
        QVERIFY(
            qAbs(canvas->zoom() - expectedFitZoom(lassoResizeSize)) < 0.000001);
    }
};

int runUiSelectionTests(int argc, char **argv)
{
    UiSelectionTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "UiSelectionTests.moc"
