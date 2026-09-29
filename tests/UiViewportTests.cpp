// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/RenderTestHelpers.hpp"
#include "support/UiTestHelpers.hpp"
#include "support/UiTestSuites.hpp"

#include <QMouseEvent>
#include <QPolygonF>
#include <QTouchEvent>
#include <QWheelEvent>
#include <QtTest/qtesttouch.h>

#include <cmath>
#include <limits>
#include <memory>
#include <numbers>

namespace ugurugu
{

namespace
{

bool pointsAreClose(
    const QPointF &actual, const QPointF &expected, qreal tolerance = 0.000001)
{
    return std::hypot(actual.x() - expected.x(), actual.y() - expected.y())
           <= tolerance;
}

std::optional<Document> animatedFramebufferHistoryDocument()
{
    const QSize initialSize(96, 64);
    const QSize finalSize(96, 72);
    Document document = Document::createDefault(finalSize);
    document.background = QColor(244, 241, 232);
    document.animationFrames = 4;
    document.wobbleAmount = 3.0;
    document.layers.clear();

    for (int layerIndex = 0; layerIndex < 3; ++layerIndex)
    {
        Layer layer;
        layer.name = QStringLiteral("History %1").arg(layerIndex + 1);
        layer.initialCanvasSize = initialSize;

        for (int strokeIndex = 0; strokeIndex < 24; ++strokeIndex)
        {
            const qreal x = 6.0 + (strokeIndex * 17 + layerIndex * 9) % 84;
            const qreal y = 6.0 + (strokeIndex * 11 + layerIndex * 7) % 52;
            const QPointF end(std::min(94.0, x + 8.0), std::min(62.0, y + 5.0));
            layer.strokes.append(makeStroke(
                strokeIndex % 7 == 0 ? StrokeMode::Erase : StrokeMode::Paint,
                QColor::fromHsv(
                    (layerIndex * 100 + strokeIndex * 13) % 360, 170, 210),
                4.0 + strokeIndex % 4,
                static_cast<quint64>(layerIndex) * 1000ULL
                    + static_cast<quint64>(strokeIndex) + 1ULL,
                {QPointF(x, y), end}));
        }

        Stroke reframe;
        reframe.mode = StrokeMode::Reframe;
        reframe.reframeOp = ReframeOp{ReframeMode::Canvas,
            SamplingMode::Nearest,
            initialSize,
            finalSize,
            QPoint(0, 4)};
        layer.strokes.append(reframe);

        for (int strokeIndex = 0; strokeIndex < 8; ++strokeIndex)
        {
            const qreal x = 10.0 + strokeIndex * 10.0;
            const qreal y = 10.0 + (strokeIndex * 9 + layerIndex * 5) % 54;
            layer.strokes.append(makeStroke(StrokeMode::Paint,
                QColor::fromHsv(
                    (layerIndex * 110 + strokeIndex * 23) % 360, 190, 220),
                5.0,
                static_cast<quint64>(layerIndex) * 1000ULL + 100ULL
                    + static_cast<quint64>(strokeIndex),
                {QPointF(x, y), QPointF(x + 7.0, std::min(70.0, y + 4.0))}));
        }

        const QImage selection =
            rectangularMask(finalSize, QRect(4, 4, 88, 64));
        QTransform shift;
        shift.translate(2.0, 1.0);
        const std::optional<PixelSelectionOp> operation = makePixelSelectionOp(
            selection, shift, true, true, SamplingMode::Nearest);
        if (!operation)
        {
            return std::nullopt;
        }
        Stroke selectionStroke;
        selectionStroke.mode = StrokeMode::PixelSelection;
        selectionStroke.pixelSelectionOp = *operation;
        layer.strokes.append(selectionStroke);
        document.layers.append(std::move(layer));
    }

    document.activeLayerId = document.layers[1].id;
    return document;
}

}

class UiViewportTests final : public QObject
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
        settings.remove(QStringLiteral("shortcuts/rotateCanvasLeftAction"));
        settings.remove(QStringLiteral("shortcuts/rotateCanvasRightAction"));
        settings.remove(QStringLiteral("shortcuts/resetCanvasRotationAction"));
        settings.sync();
    }

    void cleanup()
    {
        QSettings settings;
        settings.remove(QStringLiteral("drawingTools"));
        settings.remove(QStringLiteral("brush/recentColors"));
        settings.remove(QStringLiteral("brush/colorHistory"));
        settings.remove(QStringLiteral("canvas/strokeStabilization"));
        settings.remove(QStringLiteral("shortcuts/rotateCanvasLeftAction"));
        settings.remove(QStringLiteral("shortcuts/rotateCanvasRightAction"));
        settings.remove(QStringLiteral("shortcuts/resetCanvasRotationAction"));
        settings.sync();
    }

    void mirrorsTheCanvasAsAViewOnlyToggle()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *mirrorAction =
            window.findChild<QAction *>(QStringLiteral("mirrorCanvasAction"));
        QToolButton *mirrorButton = window.findChild<QToolButton *>(
            QStringLiteral("mirrorCanvasButton"));
        QVERIFY(canvas);
        QVERIFY(mirrorAction);
        QVERIFY(mirrorButton);
        QVERIFY(mirrorAction->isCheckable());
        QCOMPARE(mirrorAction->shortcut(), QKeySequence(QStringLiteral("M")));
        QVERIFY(!canvas->isCanvasMirrored());
        QVERIFY(!window.isWindowModified());

        mirrorAction->trigger();
        QVERIFY(canvas->isCanvasMirrored());
        QVERIFY(mirrorAction->isChecked());
        QVERIFY(canvas->zoom() > 0.0);
        QVERIFY(!window.isWindowModified());

        mirrorAction->trigger();
        QVERIFY(!canvas->isCanvasMirrored());
        QVERIFY(!mirrorAction->isChecked());
        QVERIFY(!window.isWindowModified());
    }

    void mapsDocumentCoordinatesThroughRotationAndMirroring_data()
    {
        QTest::addColumn<qreal>("rotation");
        QTest::addColumn<bool>("mirrored");

        QTest::newRow("37-degrees") << 37.0 << false;
        QTest::newRow("37-degrees-mirrored") << 37.0 << true;
        QTest::newRow("90-degrees") << 90.0 << false;
        QTest::newRow("90-degrees-mirrored") << 90.0 << true;
    }

    void mapsDocumentCoordinatesThroughRotationAndMirroring()
    {
        QFETCH(qreal, rotation);
        QFETCH(bool, mirrored);

        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(160, 80)));
        CanvasWidget canvas(&controller);
        canvas.resize(500, 400);
        canvas.setAnimating(false);
        canvas.setZoomPercent(175);
        canvas.setCanvasRotation(rotation);
        canvas.setCanvasMirrored(mirrored);

        const QPointF documentCenter(80.0, 40.0);
        const QPointF widgetCenter(250.0, 200.0);
        QVERIFY(pointsAreClose(
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentCenter),
            widgetCenter));

        const QVector<QPointF> documentPoints = {
            QPointF(0.0, 0.0),
            QPointF(13.25, 17.75),
            documentCenter,
            QPointF(149.5, 63.25),
            QPointF(160.0, 80.0),
        };
        for (const QPointF &documentPoint : documentPoints)
        {
            const QPointF widgetPoint =
                CanvasWidgetTestAccess::mapFromDocument(canvas, documentPoint);
            QVERIFY(pointsAreClose(
                CanvasWidgetTestAccess::mapToDocument(canvas, widgetPoint),
                documentPoint));
        }

        const QPointF mappedAxis =
            CanvasWidgetTestAccess::mapFromDocument(
                canvas, documentCenter + QPointF(10.0, 0.0))
            - widgetCenter;
        const qreal radians = rotation * std::numbers::pi_v<qreal> / 180.0;
        const qreal direction = mirrored ? -1.0 : 1.0;
        const QPointF expectedAxis(direction * 17.5 * std::cos(radians),
            direction * 17.5 * std::sin(radians));
        QVERIFY(pointsAreClose(mappedAxis, expectedAxis));
    }

    void keepsPreviewRenderSizeAtRightAngleRotation()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(1024, 512)));
        CanvasWidget canvas(&controller);
        canvas.resize(800, 600);
        canvas.setAnimating(false);
        canvas.setZoomPercent(150);

        const QSize unrotatedSize =
            CanvasWidgetTestAccess::previewRenderSize(canvas);
        QVERIFY(unrotatedSize.isValid());
        canvas.setCanvasRotation(90.0);
        QCOMPARE(
            CanvasWidgetTestAccess::previewRenderSize(canvas), unrotatedSize);
        canvas.setCanvasMirrored(true);
        QCOMPARE(
            CanvasWidgetTestAccess::previewRenderSize(canvas), unrotatedSize);
    }

    void rotatesCanvasWithShiftSpaceMouseDragWithoutDrawing()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        canvas.setFocus(Qt::OtherFocusReason);
        QTest::keyPress(&canvas, Qt::Key_Shift);
        QTest::keyPress(&canvas, Qt::Key_Space, Qt::ShiftModifier);
        const QPoint start = canvas.rect().center();
        const QPoint end = start + QPoint(100, 0);
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::ShiftModifier, start);
        QTest::mouseMove(&canvas, end, 5);
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::ShiftModifier, end);
        QTest::keyRelease(&canvas, Qt::Key_Space, Qt::ShiftModifier);
        QTest::keyRelease(&canvas, Qt::Key_Shift);

        QVERIFY(std::abs(canvas.canvasRotation()) > 1.0);
        QVERIFY(controller.document().layers.first().strokes.isEmpty());
    }

    void rotatesCanvasWithShiftSpaceTabletDragAndResumesDrawing()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        QPointingDevice stylus(QStringLiteral("Rotation stylus"),
            4,
            QInputDevice::DeviceType::Stylus,
            QPointingDevice::PointerType::Pen,
            QInputDevice::Capability::Position
                | QInputDevice::Capability::Pressure,
            1,
            1);
        const auto sendTabletEvent = [&canvas, &stylus](QEvent::Type type,
                                         const QPointF &position,
                                         qreal pressure,
                                         Qt::KeyboardModifiers modifiers,
                                         Qt::MouseButton button,
                                         Qt::MouseButtons buttons)
        {
            const QPointF globalPosition =
                canvas.mapToGlobal(position.toPoint());
            QTabletEvent event(type,
                &stylus,
                position,
                globalPosition,
                pressure,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                modifiers,
                button,
                buttons);
            QApplication::sendEvent(&canvas, &event);
        };

        canvas.setFocus(Qt::OtherFocusReason);
        QTest::keyPress(&canvas, Qt::Key_Shift);
        QTest::keyPress(&canvas, Qt::Key_Space, Qt::ShiftModifier);
        const QPointF rotationStart = canvas.rect().center();
        const QPointF rotationEnd = rotationStart + QPointF(90.0, 0.0);
        sendTabletEvent(QEvent::TabletPress,
            rotationStart,
            0.7,
            Qt::ShiftModifier,
            Qt::LeftButton,
            Qt::LeftButton);
        sendTabletEvent(QEvent::TabletMove,
            rotationEnd,
            0.6,
            Qt::ShiftModifier,
            Qt::NoButton,
            Qt::LeftButton);
        sendTabletEvent(QEvent::TabletRelease,
            rotationEnd,
            0.0,
            Qt::ShiftModifier,
            Qt::LeftButton,
            Qt::NoButton);
        QTest::keyRelease(&canvas, Qt::Key_Space, Qt::ShiftModifier);
        QTest::keyRelease(&canvas, Qt::Key_Shift);

        QVERIFY(std::abs(canvas.canvasRotation()) > 1.0);
        QVERIFY(controller.document().layers.first().strokes.isEmpty());

        const QPointF documentStart(30.0, 50.0);
        const QPointF documentEnd(70.0, 50.0);
        const QPointF drawingStart =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentStart);
        const QPointF drawingEnd =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentEnd);
        sendTabletEvent(QEvent::TabletPress,
            drawingStart,
            0.8,
            Qt::NoModifier,
            Qt::LeftButton,
            Qt::LeftButton);
        sendTabletEvent(QEvent::TabletMove,
            drawingEnd,
            0.8,
            Qt::NoModifier,
            Qt::NoButton,
            Qt::LeftButton);
        sendTabletEvent(QEvent::TabletRelease,
            drawingEnd,
            0.0,
            Qt::NoModifier,
            Qt::LeftButton,
            Qt::NoButton);

        const QVector<Stroke> &strokes =
            controller.document().layers.first().strokes;
        QCOMPARE(strokes.size(), 1);
        QVERIFY(pointsAreClose(
            strokes.first().points.first().position, documentStart, 0.75));
        QVERIFY(pointsAreClose(
            strokes.first().points.last().position, documentEnd, 0.75));
        QCOMPARE(strokes.first().points.last().pressure, 0.8);
    }

    void appliesTwoFingerPanZoomAndRotationTogether()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.setCanvasRotation(37.0);
        canvas.setCanvasMirrored(true);

        std::unique_ptr<QPointingDevice> touchScreen(
            QTest::createTouchDevice());
        QVERIFY(touchScreen);
        auto touch = QTest::touchEvent(&canvas, touchScreen.get(), false);
        const QPoint startFirst(150, 200);
        const QPoint startSecond(250, 200);
        const QPointF startCenter =
            (QPointF(startFirst) + QPointF(startSecond)) / 2.0;
        const QPointF documentAnchor =
            CanvasWidgetTestAccess::mapToDocument(canvas, startCenter);
        const qreal initialZoom = canvas.zoom();
        const qreal initialRotation = canvas.canvasRotation();

        touch.press(0, startFirst, &canvas).press(1, startSecond, &canvas);
        QVERIFY(touch.commit());

        const QPoint updatedFirst(190, 220);
        const QPoint updatedSecond(350, 300);
        const QPointF updatedCenter =
            (QPointF(updatedFirst) + QPointF(updatedSecond)) / 2.0;
        touch.move(0, updatedFirst, &canvas).move(1, updatedSecond, &canvas);
        QVERIFY(touch.commit());

        const QPointF startVector = QPointF(startSecond - startFirst);
        const QPointF updatedVector = QPointF(updatedSecond - updatedFirst);
        const qreal expectedScale =
            std::hypot(updatedVector.x(), updatedVector.y())
            / std::hypot(startVector.x(), startVector.y());
        const qreal expectedRotation =
            initialRotation
            + std::atan2(updatedVector.y(), updatedVector.x()) * 180.0
                  / std::numbers::pi_v<qreal>;
        QVERIFY(qAbs(canvas.zoom() - initialZoom * expectedScale) < 0.000001);
        QVERIFY(qAbs(canvas.canvasRotation() - expectedRotation) < 0.000001);
        QVERIFY(pointsAreClose(
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentAnchor),
            updatedCenter));

        touch.release(0, updatedFirst, &canvas)
            .release(1, updatedSecond, &canvas);
        QVERIFY(touch.commit());
        QVERIFY(controller.document().layers.first().strokes.isEmpty());
    }

#if defined(Q_OS_WIN)
    void appliesRawTouchPadGesturesOnlyWithTwoFingers()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        std::unique_ptr<QPointingDevice> touchPad(
            QTest::createTouchDevice(QInputDevice::DeviceType::TouchPad,
                QInputDevice::Capability::Position));
        QVERIFY(touchPad);
        auto touch = QTest::touchEvent(&canvas, touchPad.get(), false);
        const qreal initialZoom = canvas.zoom();
        const qreal initialRotation = canvas.canvasRotation();
        const QPointF initialCenter = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(50.0, 50.0));

        touch.press(0, QPoint(150, 200), &canvas);
        QVERIFY(touch.commit());
        touch.move(0, QPoint(170, 220), &canvas);
        QVERIFY(touch.commit());
        QCOMPARE(canvas.zoom(), initialZoom);
        QCOMPARE(canvas.canvasRotation(), initialRotation);
        QVERIFY(pointsAreClose(CanvasWidgetTestAccess::mapFromDocument(
                                   canvas, QPointF(50.0, 50.0)),
            initialCenter));
        QVERIFY(controller.document().layers.first().strokes.isEmpty());
        touch.release(0, QPoint(170, 220), &canvas);
        QVERIFY(touch.commit());

        const QPoint baselineFirst(170, 220);
        const QPoint baselineSecond(270, 220);
        const QPointF baselineCenter =
            (QPointF(baselineFirst) + QPointF(baselineSecond)) * 0.5;
        const QPointF documentAnchor =
            CanvasWidgetTestAccess::mapToDocument(canvas, baselineCenter);
        auto gesture = QTest::touchEvent(&canvas, touchPad.get(), false);
        gesture.press(0, baselineFirst, &canvas)
            .press(1, baselineSecond, &canvas);
        QVERIFY(gesture.commit());

        const QPoint updatedFirst(200, 240);
        const QPoint updatedSecond(360, 320);
        const QPointF updatedCenter =
            (QPointF(updatedFirst) + QPointF(updatedSecond)) * 0.5;
        gesture.move(0, updatedFirst, &canvas).move(1, updatedSecond, &canvas);
        QVERIFY(gesture.commit());
        const QPointF updatedVector = QPointF(updatedSecond - updatedFirst);
        const qreal expectedScale =
            std::hypot(updatedVector.x(), updatedVector.y()) / 100.0;
        const qreal expectedRotation =
            std::atan2(updatedVector.y(), updatedVector.x()) * 180.0
            / std::numbers::pi_v<qreal>;
        QVERIFY(qAbs(canvas.zoom() - initialZoom * expectedScale) < 0.000001);
        QVERIFY(qAbs(canvas.canvasRotation() - expectedRotation) < 0.000001);
        QVERIFY(pointsAreClose(
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentAnchor),
            updatedCenter));

        gesture.stationary(0).release(1, updatedSecond, &canvas);
        QVERIFY(gesture.commit());
        const qreal zoomAfterRelease = canvas.zoom();
        const qreal rotationAfterRelease = canvas.canvasRotation();
        gesture.move(0, QPoint(250, 300), &canvas);
        QVERIFY(gesture.commit());
        QCOMPARE(canvas.zoom(), zoomAfterRelease);
        QCOMPARE(canvas.canvasRotation(), rotationAfterRelease);
        gesture.release(0, QPoint(250, 300), &canvas);
        QVERIFY(gesture.commit());
        QVERIFY(controller.document().layers.first().strokes.isEmpty());
    }
#endif

    void rebaselinesWhenTouchPointCountChangesWithoutDrawing()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        std::unique_ptr<QPointingDevice> touchScreen(
            QTest::createTouchDevice());
        QVERIFY(touchScreen);
        auto touch = QTest::touchEvent(&canvas, touchScreen.get(), false);
        const qreal initialZoom = canvas.zoom();
        const qreal initialRotation = canvas.canvasRotation();
        const QPointF initialMappedCenter =
            CanvasWidgetTestAccess::mapFromDocument(
                canvas, QPointF(50.0, 50.0));

        touch.press(0, QPoint(120, 160), &canvas);
        QVERIFY(touch.commit());
        touch.move(0, QPoint(140, 180), &canvas);
        QVERIFY(touch.commit());
        QCOMPARE(canvas.zoom(), initialZoom);
        QCOMPARE(canvas.canvasRotation(), initialRotation);
        QVERIFY(pointsAreClose(CanvasWidgetTestAccess::mapFromDocument(
                                   canvas, QPointF(50.0, 50.0)),
            initialMappedCenter));
        QVERIFY(controller.document().layers.first().strokes.isEmpty());

        const QPoint baselineFirst(140, 180);
        const QPoint baselineSecond(240, 180);
        const QPointF baselineCenter =
            (QPointF(baselineFirst) + QPointF(baselineSecond)) / 2.0;
        const QPointF documentAnchor =
            CanvasWidgetTestAccess::mapToDocument(canvas, baselineCenter);
        touch.stationary(0).press(1, baselineSecond, &canvas);
        QVERIFY(touch.commit());
        QCOMPARE(canvas.zoom(), initialZoom);
        QCOMPARE(canvas.canvasRotation(), initialRotation);
        QVERIFY(pointsAreClose(CanvasWidgetTestAccess::mapFromDocument(
                                   canvas, QPointF(50.0, 50.0)),
            initialMappedCenter));

        const QPoint updatedFirst(180, 220);
        const QPoint updatedSecond(300, 280);
        const QPointF updatedVector = QPointF(updatedSecond - updatedFirst);
        const QPointF updatedCenter =
            (QPointF(updatedFirst) + QPointF(updatedSecond)) / 2.0;
        touch.move(0, updatedFirst, &canvas).move(1, updatedSecond, &canvas);
        QVERIFY(touch.commit());

        const qreal expectedScale =
            std::hypot(updatedVector.x(), updatedVector.y()) / 100.0;
        const qreal expectedRotation =
            initialRotation
            + std::atan2(updatedVector.y(), updatedVector.x()) * 180.0
                  / std::numbers::pi_v<qreal>;
        QVERIFY(qAbs(canvas.zoom() - initialZoom * expectedScale) < 0.000001);
        QVERIFY(qAbs(canvas.canvasRotation() - expectedRotation) < 0.000001);
        QVERIFY(pointsAreClose(
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentAnchor),
            updatedCenter));

        touch.stationary(0).release(1, updatedSecond, &canvas);
        QVERIFY(touch.commit());
        const qreal zoomAfterRemoval = canvas.zoom();
        const qreal rotationAfterRemoval = canvas.canvasRotation();
        const QPointF mappedCenterAfterRemoval =
            CanvasWidgetTestAccess::mapFromDocument(
                canvas, QPointF(50.0, 50.0));
        touch.move(0, QPoint(220, 300), &canvas);
        QVERIFY(touch.commit());
        QCOMPARE(canvas.zoom(), zoomAfterRemoval);
        QCOMPARE(canvas.canvasRotation(), rotationAfterRemoval);
        QVERIFY(pointsAreClose(CanvasWidgetTestAccess::mapFromDocument(
                                   canvas, QPointF(50.0, 50.0)),
            mappedCenterAfterRemoval));
        QVERIFY(controller.document().layers.first().strokes.isEmpty());

        touch.release(0, QPoint(220, 300), &canvas);
        QVERIFY(touch.commit());
    }

    void ignoresTwoFingerGesturesDuringAPenStroke()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        QPointingDevice stylus(QStringLiteral("Touch isolation stylus"),
            52,
            QInputDevice::DeviceType::Stylus,
            QPointingDevice::PointerType::Pen,
            QInputDevice::Capability::Position
                | QInputDevice::Capability::Pressure,
            1,
            1);
        std::unique_ptr<QPointingDevice> touchScreen(
            QTest::createTouchDevice());
        QVERIFY(touchScreen);
        auto touch = QTest::touchEvent(&canvas, touchScreen.get(), false);
        const auto sendTabletEvent = [&canvas, &stylus](QEvent::Type type,
                                         const QPointF &position,
                                         qreal pressure,
                                         Qt::MouseButton button,
                                         Qt::MouseButtons buttons)
        {
            QTabletEvent event(type,
                &stylus,
                position,
                canvas.mapToGlobal(position),
                pressure,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                Qt::NoModifier,
                button,
                buttons);
            QApplication::sendEvent(&canvas, &event);
        };

        const QPointF documentStart(20.0, 50.0);
        const QPointF documentMiddle(35.0, 50.0);
        const QPointF documentEnd(75.0, 50.0);
        const QPointF drawingStart =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentStart);
        const QPointF drawingMiddle =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentMiddle);
        const QPointF drawingEnd =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentEnd);
        sendTabletEvent(QEvent::TabletPress,
            drawingStart,
            0.6,
            Qt::LeftButton,
            Qt::LeftButton);
        sendTabletEvent(QEvent::TabletMove,
            drawingMiddle,
            0.7,
            Qt::NoButton,
            Qt::LeftButton);

        const qreal zoomDuringStroke = canvas.zoom();
        const qreal rotationDuringStroke = canvas.canvasRotation();
        const QPointF mappedCenterDuringStroke =
            CanvasWidgetTestAccess::mapFromDocument(
                canvas, QPointF(50.0, 50.0));
        touch.press(0, QPoint(150, 180), &canvas)
            .press(1, QPoint(250, 180), &canvas);
        QVERIFY(touch.commit());
        touch.move(0, QPoint(100, 260), &canvas)
            .move(1, QPoint(320, 340), &canvas);
        QVERIFY(touch.commit());
        touch.release(0, QPoint(100, 260), &canvas)
            .release(1, QPoint(320, 340), &canvas);
        QVERIFY(touch.commit());

        QCOMPARE(canvas.zoom(), zoomDuringStroke);
        QCOMPARE(canvas.canvasRotation(), rotationDuringStroke);
        QVERIFY(pointsAreClose(CanvasWidgetTestAccess::mapFromDocument(
                                   canvas, QPointF(50.0, 50.0)),
            mappedCenterDuringStroke));

        sendTabletEvent(
            QEvent::TabletMove, drawingEnd, 0.8, Qt::NoButton, Qt::LeftButton);
        sendTabletEvent(QEvent::TabletRelease,
            drawingEnd,
            0.0,
            Qt::LeftButton,
            Qt::NoButton);

        const QVector<Stroke> &strokes =
            controller.document().layers.first().strokes;
        QCOMPARE(strokes.size(), 1);
        QVERIFY(pointsAreClose(
            strokes.first().points.first().position, documentStart, 0.75));
        QVERIFY(pointsAreClose(
            strokes.first().points.last().position, documentEnd, 0.75));
        QCOMPARE(strokes.first().points.last().pressure, 0.8);
    }

    void givesPenPressPriorityOverAnActiveTouchGesture()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        std::unique_ptr<QPointingDevice> touchScreen(
            QTest::createTouchDevice());
        QVERIFY(touchScreen);
        auto touch = QTest::touchEvent(&canvas, touchScreen.get(), false);
        touch.press(0, QPoint(150, 180), &canvas)
            .press(1, QPoint(250, 180), &canvas);
        QVERIFY(touch.commit());
        touch.move(0, QPoint(170, 200), &canvas)
            .move(1, QPoint(290, 240), &canvas);
        QVERIFY(touch.commit());

        QPointingDevice stylus(QStringLiteral("Touch priority stylus"),
            55,
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
                                         Qt::MouseButtons buttons)
        {
            QTabletEvent event(type,
                &stylus,
                position,
                canvas.mapToGlobal(position),
                pressure,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                Qt::NoModifier,
                button,
                buttons);
            QApplication::sendEvent(&canvas, &event);
        };
        const QPointF documentStart(30.0, 50.0);
        const QPointF documentEnd(70.0, 50.0);
        const QPointF drawingStart =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentStart);
        const QPointF drawingEnd =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentEnd);
        sendTabletEvent(QEvent::TabletPress,
            drawingStart,
            0.55,
            Qt::LeftButton,
            Qt::LeftButton);

        const qreal zoomAfterPenPress = canvas.zoom();
        const qreal rotationAfterPenPress = canvas.canvasRotation();
        const QPointF mappedCenterAfterPenPress =
            CanvasWidgetTestAccess::mapFromDocument(
                canvas, QPointF(50.0, 50.0));
        touch.move(0, QPoint(30, 350), &canvas)
            .move(1, QPoint(380, 30), &canvas);
        QVERIFY(touch.commit());
        QCOMPARE(canvas.zoom(), zoomAfterPenPress);
        QCOMPARE(canvas.canvasRotation(), rotationAfterPenPress);
        QVERIFY(pointsAreClose(CanvasWidgetTestAccess::mapFromDocument(
                                   canvas, QPointF(50.0, 50.0)),
            mappedCenterAfterPenPress));

        sendTabletEvent(
            QEvent::TabletMove, drawingEnd, 0.85, Qt::NoButton, Qt::LeftButton);
        sendTabletEvent(QEvent::TabletRelease,
            drawingEnd,
            0.0,
            Qt::LeftButton,
            Qt::NoButton);
        touch.move(0, QPoint(20, 380), &canvas)
            .move(1, QPoint(390, 20), &canvas);
        QVERIFY(touch.commit());
        QCOMPARE(canvas.zoom(), zoomAfterPenPress);
        QCOMPARE(canvas.canvasRotation(), rotationAfterPenPress);

        touch.release(0, QPoint(20, 380), &canvas)
            .release(1, QPoint(390, 20), &canvas);
        QVERIFY(touch.commit());
        const QVector<Stroke> &strokes =
            controller.document().layers.first().strokes;
        QCOMPARE(strokes.size(), 1);
        QVERIFY(pointsAreClose(
            strokes.first().points.first().position, documentStart, 0.75));
        QVERIFY(pointsAreClose(
            strokes.first().points.last().position, documentEnd, 0.75));
        QCOMPARE(strokes.first().points.last().pressure, 0.85);
    }

    void keepsMouseInputFromATouchPadDeviceWorking()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        QPointingDevice touchPad(QStringLiteral("Test touch pad"),
            56,
            QInputDevice::DeviceType::TouchPad,
            QPointingDevice::PointerType::Finger,
            QInputDevice::Capability::Position,
            5,
            3);
        const auto sendMouseEvent = [&canvas, &touchPad](QEvent::Type type,
                                        const QPointF &position,
                                        Qt::MouseButton button,
                                        Qt::MouseButtons buttons)
        {
            QMouseEvent event(type,
                position,
                canvas.mapToGlobal(position),
                button,
                buttons,
                Qt::NoModifier,
                &touchPad);
            QApplication::sendEvent(&canvas, &event);
        };
        const QPointF documentStart(25.0, 50.0);
        const QPointF documentEnd(75.0, 50.0);
        const QPointF start =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentStart);
        const QPointF end =
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentEnd);

        sendMouseEvent(
            QEvent::MouseButtonPress, start, Qt::LeftButton, Qt::LeftButton);
        sendMouseEvent(QEvent::MouseMove, end, Qt::NoButton, Qt::LeftButton);
        sendMouseEvent(
            QEvent::MouseButtonRelease, end, Qt::LeftButton, Qt::NoButton);

        const QVector<Stroke> &strokes =
            controller.document().layers.first().strokes;
        QCOMPARE(strokes.size(), 1);
        QVERIFY(pointsAreClose(
            strokes.first().points.first().position, documentStart, 0.75));
        QVERIFY(pointsAreClose(
            strokes.first().points.last().position, documentEnd, 0.75));
    }

    void recoversTwoFingerGestureStateAfterTouchCancel()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        std::unique_ptr<QPointingDevice> touchScreen(
            QTest::createTouchDevice());
        QVERIFY(touchScreen);
        auto touch = QTest::touchEvent(&canvas, touchScreen.get(), false);
        touch.press(0, QPoint(160, 200), &canvas)
            .press(1, QPoint(240, 200), &canvas);
        QVERIFY(touch.commit());
        touch.move(0, QPoint(140, 180), &canvas)
            .move(1, QPoint(260, 220), &canvas);
        QVERIFY(touch.commit());
        QTouchEvent cancelEvent(QEvent::TouchCancel, touchScreen.get());
        QApplication::sendEvent(&canvas, &cancelEvent);

        const qreal zoomAfterCancel = canvas.zoom();
        const qreal rotationAfterCancel = canvas.canvasRotation();
        const QPointF mappedCenterAfterCancel =
            CanvasWidgetTestAccess::mapFromDocument(
                canvas, QPointF(50.0, 50.0));
        touch.move(0, QPoint(10, 10), &canvas)
            .move(1, QPoint(390, 390), &canvas);
        QVERIFY(touch.commit());
        QCOMPARE(canvas.zoom(), zoomAfterCancel);
        QCOMPARE(canvas.canvasRotation(), rotationAfterCancel);
        QVERIFY(pointsAreClose(CanvasWidgetTestAccess::mapFromDocument(
                                   canvas, QPointF(50.0, 50.0)),
            mappedCenterAfterCancel));
        touch.release(0, QPoint(10, 10), &canvas)
            .release(1, QPoint(390, 390), &canvas);
        QVERIFY(touch.commit());

        auto restartedTouch =
            QTest::touchEvent(&canvas, touchScreen.get(), false);
        const QPoint restartFirst(100, 150);
        const QPoint restartSecond(200, 150);
        const QPointF restartCenter =
            (QPointF(restartFirst) + QPointF(restartSecond)) / 2.0;
        const QPointF documentAnchor =
            CanvasWidgetTestAccess::mapToDocument(canvas, restartCenter);
        restartedTouch.press(0, restartFirst, &canvas)
            .press(1, restartSecond, &canvas);
        QVERIFY(restartedTouch.commit());

        const QPoint updatedFirst(170, 190);
        const QPoint updatedSecond(270, 290);
        const QPointF updatedCenter =
            (QPointF(updatedFirst) + QPointF(updatedSecond)) / 2.0;
        restartedTouch.move(0, updatedFirst, &canvas)
            .move(1, updatedSecond, &canvas);
        QVERIFY(restartedTouch.commit());

        QVERIFY(
            qAbs(canvas.zoom() - zoomAfterCancel * std::sqrt(2.0)) < 0.000001);
        QVERIFY(qAbs(canvas.canvasRotation() - (rotationAfterCancel + 45.0))
                < 0.000001);
        QVERIFY(pointsAreClose(
            CanvasWidgetTestAccess::mapFromDocument(canvas, documentAnchor),
            updatedCenter));

        restartedTouch.release(0, updatedFirst, &canvas)
            .release(1, updatedSecond, &canvas);
        QVERIFY(restartedTouch.commit());
    }

    void syncsCanvasRotationControlsWithoutEditingDocument()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *rotateLeftAction = window.findChild<QAction *>(
            QStringLiteral("rotateCanvasLeftAction"));
        QAction *rotateRightAction = window.findChild<QAction *>(
            QStringLiteral("rotateCanvasRightAction"));
        QAction *resetRotationAction = window.findChild<QAction *>(
            QStringLiteral("resetCanvasRotationAction"));
        QDoubleSpinBox *rotationSpin = window.findChild<QDoubleSpinBox *>(
            QStringLiteral("canvasRotationSpin"));
        QVERIFY(canvas);
        QVERIFY(rotateLeftAction);
        QVERIFY(rotateRightAction);
        QVERIFY(resetRotationAction);
        QVERIFY(rotationSpin);
        QCOMPARE(
            rotateLeftAction->shortcut(), QKeySequence(QStringLiteral("-")));
        QCOMPARE(
            rotateRightAction->shortcut(), QKeySequence(QStringLiteral("^")));

        QSignalSpy documentChanges(&MainWindowTestAccess::controller(window),
            &DocumentController::documentChanged);
        QCOMPARE(canvas->canvasRotation(), 0.0);
        QCOMPARE(rotationSpin->value(), 0.0);

        rotateLeftAction->trigger();
        QCOMPARE(canvas->canvasRotation(), -5.0);
        QCOMPARE(rotationSpin->value(), -5.0);
        rotateRightAction->trigger();
        QCOMPARE(canvas->canvasRotation(), 0.0);
        QCOMPARE(rotationSpin->value(), 0.0);

        rotationSpin->setValue(37.0);
        QCOMPARE(canvas->canvasRotation(), 37.0);
        resetRotationAction->trigger();
        QCOMPARE(canvas->canvasRotation(), 0.0);
        QCOMPARE(rotationSpin->value(), 0.0);
        QCOMPARE(documentChanges.size(), 0);
        QVERIFY(!window.isWindowModified());
    }

    void normalizesCanvasRotationAndRejectsNonfiniteAngles()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        QSignalSpy rotations(&canvas, &CanvasWidget::canvasRotationChanged);

        canvas.setCanvasRotation(725.0);
        QCOMPARE(canvas.canvasRotation(), 5.0);
        canvas.setCanvasRotation(-725.0);
        QCOMPARE(canvas.canvasRotation(), -5.0);
        canvas.setCanvasRotation(std::numeric_limits<qreal>::infinity());
        QCOMPARE(canvas.canvasRotation(), -5.0);
        canvas.setCanvasRotation(std::numeric_limits<qreal>::quiet_NaN());
        QCOMPARE(canvas.canvasRotation(), -5.0);
        QCOMPARE(rotations.size(), 2);
    }

    void fitsTheRotatedCanvasWithoutResettingItsViewState()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(320, 120)));
        CanvasWidget canvas(&controller);
        canvas.resize(500, 400);
        canvas.setCanvasRotation(45.0);
        canvas.setCanvasMirrored(true);
        canvas.fitToWindow();

        QPolygonF corners;
        for (const QPointF &point : {QPointF(0.0, 0.0),
                 QPointF(320.0, 0.0),
                 QPointF(320.0, 120.0),
                 QPointF(0.0, 120.0)})
        {
            corners.append(
                CanvasWidgetTestAccess::mapFromDocument(canvas, point));
        }
        const QRectF bounds = corners.boundingRect();
        QVERIFY(bounds.left() >= 31.0);
        QVERIFY(bounds.top() >= 31.0);
        QVERIFY(bounds.right() <= canvas.width() - 31.0);
        QVERIFY(bounds.bottom() <= canvas.height() - 31.0);
        QVERIFY(qAbs(bounds.height() - (canvas.height() - 64.0)) < 0.0001);
        QCOMPARE(canvas.canvasRotation(), 45.0);
        QVERIFY(canvas.isCanvasMirrored());
    }

    void rotatesWithShiftWheelAndIgnoresWheelDuringAStroke()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        const QPointF center = canvas.rect().center();
        const QPointF globalCenter = canvas.mapToGlobal(center.toPoint());
        QWheelEvent rotateWheel(center,
            globalCenter,
            QPoint(),
            QPoint(0, 120),
            Qt::NoButton,
            Qt::ShiftModifier,
            Qt::NoScrollPhase,
            false);
        QApplication::sendEvent(&canvas, &rotateWheel);
        QCOMPARE(canvas.canvasRotation(), 5.0);

        const qreal zoomBeforeStroke = canvas.zoom();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center.toPoint());
        QWheelEvent ignoredWheel(center,
            globalCenter,
            QPoint(),
            QPoint(0, 120),
            Qt::NoButton,
            Qt::ShiftModifier,
            Qt::NoScrollPhase,
            false);
        QApplication::sendEvent(&canvas, &ignoredWheel);
        QCOMPARE(canvas.canvasRotation(), 5.0);
        QCOMPARE(canvas.zoom(), zoomBeforeStroke);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center.toPoint());
        QCOMPARE(controller.document().layers.first().strokes.size(), 1);
    }

    void clipsTheSoftwareCheckerToTheRotatedCanvas()
    {
        Document document = Document::createDefault(QSize(160, 80));
        document.background = Qt::transparent;
        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.setZoomPercent(100);
        canvas.setCanvasRotation(45.0);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        QVERIFY(!CanvasWidgetTestAccess::usingGpuDisplay(canvas));

        QPolygonF canvasPolygon;
        for (const QPointF &point : {QPointF(0.0, 0.0),
                 QPointF(160.0, 0.0),
                 QPointF(160.0, 80.0),
                 QPointF(0.0, 80.0)})
        {
            canvasPolygon.append(
                CanvasWidgetTestAccess::mapFromDocument(canvas, point));
        }
        const QRectF bounds = canvasPolygon.boundingRect();
        const QPointF outside = bounds.topLeft() + QPointF(5.0, 5.0);
        QVERIFY(!canvasPolygon.containsPoint(outside, Qt::OddEvenFill));

        const QImage image = canvas.grab().toImage();
        const qreal ratio = image.devicePixelRatio();
        const auto pixelColorAt = [&image, ratio](const QPointF &position)
        {
            return image.pixelColor(
                qRound(position.x() * ratio), qRound(position.y() * ratio));
        };
        QCOMPARE(pixelColorAt(outside), Theme::canvasBackground());
        QVERIFY(pixelColorAt(canvasPolygon.boundingRect().center())
                != Theme::canvasBackground());
    }

    void disablesDocumentAndLayerWobbleForTheCanvasView()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;
        MotionSettings motion = controller.document().motion;
        controller.setLayerWobbleOverride(layerId, 5.0, motion);
        CanvasWidget canvas(&controller);

        canvas.setWobbleAnimationEnabled(false);
        const Document display =
            CanvasWidgetTestAccess::displayDocument(canvas);
        const Layer *displayLayer = display.layer(layerId);
        QVERIFY(displayLayer);
        QCOMPARE(display.wobbleAmount, 0.0);
        QCOMPARE(effectiveWobbleAmount(display, *displayLayer), 0.0);
    }

    void disablesLayerWobbleInPendingTransformSnapshots()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(96, 96)));
        const QUuid layerId = controller.document().activeLayerId;
        Stroke stroke;
        stroke.width = 10.0;
        stroke.points = {
            {QPointF(20.0, 48.0), 1.0}, {QPointF(76.0, 48.0), 1.0}};
        QCOMPARE(controller.addStroke(layerId, stroke),
            DocumentController::AddStrokeResult::Added);
        MotionSettings motion = controller.document().motion;
        controller.setLayerWobbleOverride(layerId, 5.0, motion);

        CanvasWidget canvas(&controller);
        canvas.resize(300, 300);
        canvas.setAnimating(false);
        canvas.setWobbleAnimationEnabled(false);
        canvas.selectAll();
        QTRY_VERIFY(canvas.hasTransformableSelection());
        QVERIFY(canvas.scaleSelection(1.5));
        QVERIFY(canvas.hasPendingSelectionTransform());

        // The eyedropper and the still-image export sample this snapshot; it
        // has to match the wobble-disabled view, layer overrides included,
        // while still carrying the pending selection transform.
        const Document snapshot =
            canvas.displayDocumentWithPendingSelectionTransform();
        const Layer *layer = snapshot.layer(layerId);
        QVERIFY(layer);
        QCOMPARE(snapshot.wobbleAmount, 0.0);
        QCOMPARE(effectiveWobbleAmount(snapshot, *layer), 0.0);
        QVERIFY(!layer->strokes.isEmpty());
        QCOMPARE(layer->strokes.last().mode, StrokeMode::PixelSelection);
    }

    void keepsAnimatingWhileThePenIsDownWhenEnabled()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(128, 128)));
        CanvasWidget canvas(&controller);
        canvas.resize(320, 320);
        canvas.setAnimateWhileDrawing(true);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        QTRY_VERIFY_WITH_TIMEOUT(
            !CanvasWidgetTestAccess::frameCacheWarmupActive(canvas), 5000);

        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(30, 0));
        const int frameWhilePressed = canvas.currentFrame();
        QTRY_VERIFY_WITH_TIMEOUT(
            canvas.currentFrame() != frameWhilePressed, 2000);
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(30, 0));
    }

    void avoidsSynchronousPreviewReplayWhileAnimatingADrawnStroke_data()
    {
        QTest::addColumn<bool>("useLayerRasterFallback");

        QTest::newRow("layer-split") << false;
        QTest::newRow("layer-raster-fallback") << true;
    }

    void avoidsSynchronousPreviewReplayWhileAnimatingADrawnStroke()
    {
        QFETCH(bool, useLayerRasterFallback);
        const std::optional<Document> fixture =
            animatedFramebufferHistoryDocument();
        QVERIFY(fixture.has_value());
        Document document = *fixture;
        if (useLayerRasterFallback)
        {
            document.layers.last().blendMode = LayerBlendMode::Multiply;
        }
        QCOMPARE(
            RenderEngine::supportsLayerSplit(document, document.activeLayerId),
            !useLayerRasterFallback);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(384, 288);
        canvas.setAnimating(false);
        canvas.setAnimateWhileDrawing(true);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        QTRY_VERIFY_WITH_TIMEOUT(
            !CanvasWidgetTestAccess::frameCacheWarmupActive(canvas), 5000);

        const QPoint start =
            CanvasWidgetTestAccess::mapFromDocument(canvas, QPointF(18.0, 34.0))
                .toPoint();
        const QPoint end =
            CanvasWidgetTestAccess::mapFromDocument(canvas, QPointF(78.0, 44.0))
                .toPoint();
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::NoModifier, start);
        QTest::mouseMove(&canvas, end, 1);
        QVERIFY(CanvasWidgetTestAccess::drawing(canvas));

        const auto initial =
            CanvasWidgetTestAccess::resolveDisplayedFrame(canvas);
        QVERIFY(!initial.image.isNull());
        const quint64 synchronousRenderBaseline =
            CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas);

        canvas.setAnimating(true);
        CanvasWidgetTestAccess::stopAnimationTimer(canvas);
        const int heldFrame = canvas.currentFrame();
        const int nextFrame =
            (heldFrame + 1) % controller.document().animationFrames;
        CanvasWidgetTestAccess::discardPreparedInteractionFrame(canvas);

        CanvasWidgetTestAccess::advanceFrame(canvas);
        QCOMPARE(canvas.currentFrame(), heldFrame);
        QCOMPARE(CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas),
            synchronousRenderBaseline);

        const qsizetype pointsBeforeMissMove =
            CanvasWidgetTestAccess::activeStroke(canvas).points.size();
        const QPoint missMove =
            CanvasWidgetTestAccess::mapFromDocument(canvas, QPointF(76.0, 58.0))
                .toPoint();
        QTest::mouseMove(&canvas, missMove, 1);
        QCOMPARE(canvas.currentFrame(), heldFrame);
        QVERIFY(CanvasWidgetTestAccess::activeStroke(canvas).points.size()
                > pointsBeforeMissMove);
        QVERIFY(
            CanvasWidgetTestAccess::activeStrokePreviewIncludesStroke(canvas));
        QCOMPARE(CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas),
            synchronousRenderBaseline);

        QTRY_VERIFY_WITH_TIMEOUT(
            CanvasWidgetTestAccess::hasPreparedInteractionFrame(
                canvas, nextFrame),
            5000);
        QCOMPARE(
            CanvasWidgetTestAccess::preparedInteractionUsesLayerRasters(canvas),
            useLayerRasterFallback);
        CanvasWidgetTestAccess::advanceFrame(canvas);
        QCOMPARE(canvas.currentFrame(), nextFrame);

        const auto actual =
            CanvasWidgetTestAccess::resolveDisplayedFrame(canvas);
        QVERIFY(!actual.image.isNull());
        QCOMPARE(CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas),
            synchronousRenderBaseline);

        const Stroke activeStroke =
            CanvasWidgetTestAccess::activeStroke(canvas);
        QVERIFY(activeStroke.points.size() >= 2);
        Document expectedDocument =
            CanvasWidgetTestAccess::displayDocument(canvas);
        Layer *activeLayer =
            expectedDocument.layer(expectedDocument.activeLayerId);
        QVERIFY(activeLayer);
        activeLayer->strokes.append(activeStroke);
        const QImage expected = RenderEngine::renderScaled(
            expectedDocument, nextFrame, actual.image.size());
        QCOMPARE(actual.image, expected);

        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::NoModifier, missMove);
    }

    void doesNotPromoteAStrokeFreeFrameWhenReleasedBeforePreparation()
    {
        const std::optional<Document> fixture =
            animatedFramebufferHistoryDocument();
        QVERIFY(fixture.has_value());

        DocumentController controller;
        QVERIFY(controller.loadDocument(*fixture));
        const qsizetype initialStrokeCount =
            controller.document().layers[1].strokes.size();
        CanvasWidget canvas(&controller);
        canvas.resize(384, 288);
        canvas.setAnimating(false);
        canvas.setAnimateWhileDrawing(true);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        QTRY_VERIFY_WITH_TIMEOUT(
            !CanvasWidgetTestAccess::frameCacheWarmupActive(canvas), 5000);
        QVERIFY(CanvasWidgetTestAccess::hasCachedFrame(
            canvas, canvas.currentFrame()));

        canvas.setAnimating(true);
        CanvasWidgetTestAccess::stopAnimationTimer(canvas);
        const int frame = canvas.currentFrame();
        const QPointF start = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(20.0, 30.0));
        const QPointF end = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(76.0, 48.0));
        CanvasWidgetTestAccess::beginStroke(canvas, start, 100);
        QVERIFY(CanvasWidgetTestAccess::drawing(canvas));

        CanvasWidgetTestAccess::discardCurrentInteractionFrame(canvas);
        const quint64 synchronousRenderBaseline =
            CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas);
        CanvasWidgetTestAccess::continueStroke(canvas, end, 200);
        QVERIFY(
            !CanvasWidgetTestAccess::activeStrokePreviewIncludesStroke(canvas));
        QCOMPARE(CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas),
            synchronousRenderBaseline);

        CanvasWidgetTestAccess::endStroke(canvas, end, 300);
        QVERIFY(!CanvasWidgetTestAccess::drawing(canvas));
        QCOMPARE(controller.document().layers[1].strokes.size(),
            initialStrokeCount + 1);
        QCOMPARE(CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas),
            synchronousRenderBaseline);

        QTRY_VERIFY_WITH_TIMEOUT(
            CanvasWidgetTestAccess::hasCachedFrame(canvas, frame), 5000);
        const QImage actual =
            CanvasWidgetTestAccess::cachedFrame(canvas, frame);
        QVERIFY(!actual.isNull());
        const QImage expected = RenderEngine::renderScaled(
            CanvasWidgetTestAccess::displayDocument(canvas),
            frame,
            actual.size());
        QVERIFY(!expected.isNull());
        QCOMPARE(actual, expected);
    }

    void resumesPlaybackAfterChangingMotionLinkage()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(128, 128)));
        const QUuid layerId = controller.document().activeLayerId;
        Stroke stroke;
        stroke.points = {
            {QPointF(16.0, 64.0), 1.0}, {QPointF(112.0, 64.0), 1.0}};
        QCOMPARE(controller.addStroke(layerId, stroke),
            DocumentController::AddStrokeResult::Added);

        CanvasWidget canvas(&controller);
        canvas.resize(320, 320);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        QTRY_VERIFY_WITH_TIMEOUT(
            !CanvasWidgetTestAccess::frameCacheWarmupActive(canvas), 5000);

        controller.setMotionLinked(0.35);
        const int frameAfterEdit = canvas.currentFrame();
        QTRY_VERIFY_WITH_TIMEOUT(canvas.currentFrame() != frameAfterEdit, 3000);
    }

    void zoomsWithKeyboardActions()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QAction *zoomInAction =
            window.findChild<QAction *>(QStringLiteral("zoomInAction"));
        QAction *zoomOutAction =
            window.findChild<QAction *>(QStringLiteral("zoomOutAction"));
        QVERIFY(canvas);
        QVERIFY(zoomInAction);
        QVERIFY(zoomOutAction);

        const qreal initialZoom = canvas->zoom();
        zoomInAction->trigger();
        QVERIFY(canvas->zoom() > initialZoom);
        zoomOutAction->trigger();
        zoomOutAction->trigger();
        QVERIFY(canvas->zoom() < initialZoom);
        QVERIFY(!window.isWindowModified());
    }

    void distinguishesActualZoomFromFit()
    {
        DocumentController controller;
        controller.newDocument(QSize(1000, 800));
        CanvasWidget canvas(&controller);
        canvas.resize(500, 400);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        canvas.setZoomPercent(250);
        QVERIFY(qAbs(canvas.zoom() - 2.5) < 0.0001);
        canvas.resetZoom();
        QVERIFY(qAbs(canvas.zoom() - 1.0) < 0.0001);

        canvas.fitToWindow();
        const qreal fittedZoom = canvas.zoom();
        QVERIFY(qAbs(fittedZoom - 1.0) > 0.0001);
        QVERIFY(qAbs(fittedZoom - 2.5) > 0.0001);

        canvas.resetZoom();
        QVERIFY(qAbs(canvas.zoom() - 1.0) < 0.0001);
    }

    void defersPreviewRerenderUntilZoomInputIsIdle()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(4096, 4096)));
        CanvasWidget canvas(&controller);
        canvas.resize(800, 600);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        canvas.setZoomPercent(25);
        QTRY_VERIFY(!CanvasWidgetTestAccess::zoomRenderPending(canvas));
        canvas.grab();
        const QSize initialRenderSize =
            CanvasWidgetTestAccess::cachedRenderSize(canvas);
        QVERIFY(initialRenderSize.isValid());

        canvas.setZoomPercent(50);
        canvas.setZoomPercent(100);
        QVERIFY(CanvasWidgetTestAccess::zoomRenderPending(canvas));
        QCOMPARE(CanvasWidgetTestAccess::previewRenderSize(canvas),
            initialRenderSize);

        QTRY_VERIFY(!CanvasWidgetTestAccess::zoomRenderPending(canvas));
        QTRY_VERIFY(CanvasWidgetTestAccess::previewRenderSize(canvas)
                    != initialRenderSize);
    }

    void keepsRegionalStrokePreviewFreeOfSeams()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(256, 256)));
        const QUuid layerId = controller.document().activeLayerId;

        Stroke background;
        background.color = QColor(255, 0, 0);
        background.width = 512.0;
        background.points = {{QPointF(0.0, 0.0), 1.0},
            {QPointF(128.0, 128.0), 1.0},
            {QPointF(256.0, 256.0), 1.0}};
        QCOMPARE(controller.addStroke(layerId, background),
            DocumentController::AddStrokeResult::Added);

        CanvasWidget canvas(&controller);
        canvas.resize(300, 300);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.setAnimating(false);
        canvas.fitToWindow();
        canvas.setBrushColor(QColor(0, 0, 255));
        canvas.setBrushWidth(8.0);

        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(30, 0));
        for (int step = -20; step <= 30; step += 10)
        {
            QTest::mouseMove(&canvas, center + QPoint(step, step / 3), 5);
        }
        const QPoint pointerPosition = center + QPoint(30, 10);

        int seamPixels = 0;
        for (const int zoomPercent : {87, 93, 101, 113})
        {
            canvas.setZoomPercent(zoomPercent);
            const QImage frame = canvas.grab().toImage();

            QRect paintedBounds;
            for (int y = 0; y < frame.height(); ++y)
            {
                for (int x = 0; x < frame.width(); ++x)
                {
                    const QColor color = frame.pixelColor(x, y);
                    if (color.red() > 150 && color.green() < 80
                        && color.blue() < 80)
                    {
                        paintedBounds |= QRect(x, y, 1, 1);
                    }
                }
            }
            QVERIFY(paintedBounds.width() > 100);
            QVERIFY(paintedBounds.height() > 100);

            const QRect interior = paintedBounds.adjusted(4, 4, -4, -4);
            for (int y = interior.top(); y <= interior.bottom(); ++y)
            {
                for (int x = interior.left(); x <= interior.right(); ++x)
                {
                    if ((QPoint(x, y) - pointerPosition).manhattanLength() < 40)
                    {
                        continue;
                    }
                    const QColor color = frame.pixelColor(x, y);
                    if (color.red() > 90 && color.green() > 90
                        && color.blue() > 90)
                    {
                        ++seamPixels;
                    }
                }
            }
        }
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, pointerPosition);
        QCOMPARE(seamPixels, 0);
    }

    void promotesCompletedStrokePreviewIntoFrameCache()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(4096, 4096)));
        CanvasWidget canvas(&controller);
        canvas.resize(800, 600);
        canvas.setAnimating(false);
        canvas.setZoomPercent(25);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        QTRY_VERIFY(!CanvasWidgetTestAccess::zoomRenderPending(canvas));

        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(80, 0));
        for (int step = -60; step <= 80; step += 20)
        {
            QTest::mouseMove(&canvas, center + QPoint(step, step / 4), 1);
        }

        QElapsedTimer timer;
        timer.start();
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(80, 20));
        QApplication::processEvents();
        const qreal penUpMilliseconds =
            static_cast<qreal>(timer.nsecsElapsed()) / 1000000.0;

        QCOMPARE(controller.document().layers.first().strokes.size(), 1);
        QVERIFY(CanvasWidgetTestAccess::hasCachedFrame(
            canvas, canvas.currentFrame()));
        qInfo().nospace() << "4K pen-up cache promotion and next paint took "
                          << penUpMilliseconds << " ms";
    }

    void defersStaleFrameWarmupWhilePausedUntilPlaybackStarts()
    {
        Document document = Document::createDefault(QSize(256, 256));
        document.animationFrames = 30;
        document.wobbleAmount = 1.6;
        Stroke background;
        background.color = QColor(220, 70, 50);
        background.width = 24.0;
        background.points = {{QPointF(40.0, 80.0), 1.0},
            {QPointF(120.0, 190.0), 1.0},
            {QPointF(210.0, 70.0), 1.0}};
        document.layers.first().strokes.append(background);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(320, 320);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        QTRY_COMPARE_WITH_TIMEOUT(
            CanvasWidgetTestAccess::cachedFrameCount(canvas),
            qsizetype{30},
            10000);
        QTRY_VERIFY_WITH_TIMEOUT(
            !CanvasWidgetTestAccess::frameCacheWarmupActive(canvas), 10000);
        QCOMPARE(
            CanvasWidgetTestAccess::frameCacheWarmupWorkerCount(canvas), 0);

        canvas.setAnimating(false);
        QVERIFY(!canvas.isAnimating());
        QCOMPARE(
            CanvasWidgetTestAccess::frameCacheWarmupWorkerCount(canvas), 0);

        const QPointF start = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(60.0, 200.0));
        const QPointF end = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(200.0, 60.0));
        CanvasWidgetTestAccess::beginStroke(canvas, start, 100);
        CanvasWidgetTestAccess::continueStroke(canvas, end, 200);
        CanvasWidgetTestAccess::endStroke(canvas, end, 300);
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);
        QVERIFY(!canvas.isAnimating());

        QTest::qWait(100);
        QVERIFY(!CanvasWidgetTestAccess::frameCacheWarmupActive(canvas));
        QCOMPARE(
            CanvasWidgetTestAccess::frameCacheWarmupWorkerCount(canvas), 0);
        QCOMPARE(
            CanvasWidgetTestAccess::cachedFrameCount(canvas), qsizetype{30});
        QCOMPARE(
            CanvasWidgetTestAccess::staleFrameCount(canvas), qsizetype{29});
        QVERIFY(CanvasWidgetTestAccess::hasCachedFrame(
            canvas, canvas.currentFrame()));
        const QImage promotedFrame =
            CanvasWidgetTestAccess::cachedFrame(canvas, canvas.currentFrame());
        QVERIFY(!promotedFrame.isNull());
        QCOMPARE(promotedFrame,
            RenderEngine::renderScaled(
                CanvasWidgetTestAccess::displayDocument(canvas),
                canvas.currentFrame(),
                promotedFrame.size()));

        const qsizetype staleBeforeResume =
            CanvasWidgetTestAccess::staleFrameCount(canvas);
        const quint64 synchronousRenderBaseline =
            CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas);
        canvas.setAnimating(true);
        CanvasWidgetTestAccess::stopAnimationTimer(canvas);
        QTRY_VERIFY_WITH_TIMEOUT(
            CanvasWidgetTestAccess::frameCacheWarmupWorkerCount(canvas) > 0
                || CanvasWidgetTestAccess::staleFrameCount(canvas)
                       < staleBeforeResume,
            5000);
        QTRY_COMPARE_WITH_TIMEOUT(
            CanvasWidgetTestAccess::staleFrameCount(canvas),
            qsizetype{0},
            10000);
        QTRY_COMPARE_WITH_TIMEOUT(
            CanvasWidgetTestAccess::frameCacheWarmupWorkerCount(canvas),
            0,
            10000);
        QVERIFY(!CanvasWidgetTestAccess::frameCacheWarmupActive(canvas));
        QCOMPARE(
            CanvasWidgetTestAccess::cachedFrameCount(canvas), qsizetype{30});
        QCOMPARE(CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas),
            synchronousRenderBaseline);
        for (int frame = 0; frame < 30; ++frame)
        {
            QVERIFY(CanvasWidgetTestAccess::hasCachedFrame(canvas, frame));
        }
    }

    void skipsTheInteractionWarmupAfterAFullyPromotedStroke()
    {
        Document document = Document::createDefault(QSize(256, 256));
        document.animationFrames = 30;
        document.wobbleAmount = 1.6;
        Stroke background;
        background.color = QColor(220, 70, 50);
        background.width = 24.0;
        background.points = {{QPointF(40.0, 80.0), 1.0},
            {QPointF(120.0, 190.0), 1.0},
            {QPointF(210.0, 70.0), 1.0}};
        document.layers.first().strokes.append(background);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(320, 320);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        QTRY_COMPARE_WITH_TIMEOUT(
            CanvasWidgetTestAccess::cachedFrameCount(canvas),
            qsizetype{30},
            10000);
        canvas.setAnimating(false);
        QTRY_VERIFY_WITH_TIMEOUT(
            !CanvasWidgetTestAccess::interactionFrameWarmupActive(canvas)
                && CanvasWidgetTestAccess::hasCurrentInteractionBase(
                    canvas, canvas.currentFrame()),
            10000);

        const quint64 synchronousRenderBaseline =
            CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas);
        const QPointF start = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(60.0, 200.0));
        const QPointF end = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(200.0, 60.0));
        CanvasWidgetTestAccess::beginStroke(canvas, start, 100);
        CanvasWidgetTestAccess::continueStroke(canvas, end, 200);
        CanvasWidgetTestAccess::endStroke(canvas, end, 300);
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);

        // The pen-up promoted the current frame and a reusable base, so the
        // commit must not have queued an interaction worker, now or deferred.
        QVERIFY(!CanvasWidgetTestAccess::interactionFrameWarmupActive(canvas));
        QTest::qWait(100);
        QVERIFY(!CanvasWidgetTestAccess::interactionFrameWarmupActive(canvas));
        QVERIFY(CanvasWidgetTestAccess::hasCurrentInteractionBase(
            canvas, canvas.currentFrame()));
        QVERIFY(CanvasWidgetTestAccess::hasCachedFrame(
            canvas, canvas.currentFrame()));
        const QImage promotedFrame =
            CanvasWidgetTestAccess::cachedFrame(canvas, canvas.currentFrame());
        QCOMPARE(promotedFrame,
            RenderEngine::renderScaled(
                CanvasWidgetTestAccess::displayDocument(canvas),
                canvas.currentFrame(),
                promotedFrame.size()));
        QCOMPARE(CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas),
            synchronousRenderBaseline);

        // The next stroke starts with zero interaction workers and reuses the
        // promoted base without any synchronous preview render.
        CanvasWidgetTestAccess::beginStroke(canvas, end, 400);
        CanvasWidgetTestAccess::continueStroke(canvas, start, 500);
        CanvasWidgetTestAccess::resolveDisplayedFrame(canvas);
        QVERIFY(!CanvasWidgetTestAccess::interactionFrameWarmupActive(canvas));
        CanvasWidgetTestAccess::endStroke(canvas, start, 600);
        QCOMPARE(controller.document().layers.first().strokes.size(), 3);
        QVERIFY(!CanvasWidgetTestAccess::interactionFrameWarmupActive(canvas));
        QCOMPARE(CanvasWidgetTestAccess::synchronousPreviewRenderCount(canvas),
            synchronousRenderBaseline);
    }

    void keepsPreparingInteractionFramesOutsideAFullyPromotedCommit()
    {
        Document document = Document::createDefault(QSize(256, 256));
        document.animationFrames = 30;
        document.wobbleAmount = 1.6;
        Stroke background;
        background.color = QColor(220, 70, 50);
        background.width = 24.0;
        background.points = {{QPointF(40.0, 80.0), 1.0},
            {QPointF(120.0, 190.0), 1.0},
            {QPointF(210.0, 70.0), 1.0}};
        document.layers.first().strokes.append(background);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(320, 320);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        canvas.fitToWindow();
        QTRY_COMPARE_WITH_TIMEOUT(
            CanvasWidgetTestAccess::cachedFrameCount(canvas),
            qsizetype{30},
            10000);
        canvas.setAnimating(false);
        QTRY_VERIFY_WITH_TIMEOUT(
            !CanvasWidgetTestAccess::interactionFrameWarmupActive(canvas)
                && CanvasWidgetTestAccess::hasCurrentInteractionBase(
                    canvas, canvas.currentFrame()),
            10000);

        const QPointF start = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(60.0, 200.0));
        const QPointF end = CanvasWidgetTestAccess::mapFromDocument(
            canvas, QPointF(200.0, 60.0));
        CanvasWidgetTestAccess::beginStroke(canvas, start, 100);
        CanvasWidgetTestAccess::continueStroke(canvas, end, 200);
        CanvasWidgetTestAccess::endStroke(canvas, end, 300);
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);

        // Scrubbing to another frame while paused still prepares that frame's
        // interaction base.
        canvas.setCurrentFrame(1);
        QTRY_VERIFY_WITH_TIMEOUT(
            CanvasWidgetTestAccess::hasCurrentInteractionBase(canvas, 1),
            10000);
        QVERIFY(CanvasWidgetTestAccess::hasCachedFrame(canvas, 1));
        const QImage scrubbedFrame =
            CanvasWidgetTestAccess::cachedFrame(canvas, 1);
        QCOMPARE(scrubbedFrame,
            RenderEngine::renderScaled(
                CanvasWidgetTestAccess::displayDocument(canvas),
                1,
                scrubbedFrame.size()));

        // An invalidation outside a stroke commit, here an undo, still
        // prepares the interaction base.
        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.first().strokes.size(), 1);
        QTRY_VERIFY_WITH_TIMEOUT(
            CanvasWidgetTestAccess::hasCurrentInteractionBase(
                canvas, canvas.currentFrame()),
            10000);
    }

    void warmsAnimatedFourKFramesOffTheUiThreadAfterErasing()
    {
        Document document = Document::createDefault(QSize(4096, 4096));
        document.animationFrames = 30;
        document.wobbleAmount = 1.6;
        Stroke background;
        background.color = QColor(220, 70, 50);
        background.width = 320.0;
        background.points = {{QPointF(300.0, 1200.0), 1.0},
            {QPointF(1100.0, 2800.0), 1.0},
            {QPointF(2100.0, 1300.0), 1.0},
            {QPointF(3100.0, 2900.0), 1.0},
            {QPointF(3800.0, 1500.0), 1.0}};
        document.layers.first().strokes.append(background);

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(800, 600);
        canvas.setZoomPercent(25);
        canvas.setTool(CanvasWidget::Tool::Eraser);
        canvas.setEraserWidth(100.0);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        QTRY_VERIFY_WITH_TIMEOUT(
            !CanvasWidgetTestAccess::frameCacheWarmupActive(canvas), 10000);
        QTRY_COMPARE_WITH_TIMEOUT(
            CanvasWidgetTestAccess::cachedFrameCount(canvas),
            qsizetype{30},
            10000);

        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(70, 0));
        for (int step = 0; step < 160; ++step)
        {
            const qreal angle = step * 0.31;
            const QPoint offset(
                qRound(std::cos(angle) * 70.0), qRound(std::sin(angle) * 45.0));
            QTest::mouseMove(&canvas, center + offset);
        }

        QElapsedTimer timer;
        timer.start();
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(70, 0));
        QApplication::processEvents(QEventLoop::AllEvents, 5);
        const qreal penUpMilliseconds =
            static_cast<qreal>(timer.nsecsElapsed()) / 1000000.0;

        QCOMPARE(controller.document().layers.first().strokes.size(), 2);
        QVERIFY(CanvasWidgetTestAccess::hasCachedFrame(
            canvas, canvas.currentFrame()));
        QVERIFY(CanvasWidgetTestAccess::frameCacheWarmupActive(canvas)
                || CanvasWidgetTestAccess::cachedFrameCount(canvas)
                       == qsizetype{30});
#ifdef NDEBUG
        QVERIFY2(penUpMilliseconds < 500.0,
            qPrintable(QStringLiteral("4K animated pen-up blocked for %1 ms")
                    .arg(penUpMilliseconds)));
#endif
        QTRY_VERIFY_WITH_TIMEOUT(
            !CanvasWidgetTestAccess::frameCacheWarmupActive(canvas), 10000);
        QTRY_COMPARE_WITH_TIMEOUT(
            CanvasWidgetTestAccess::cachedFrameCount(canvas),
            qsizetype{30},
            10000);
        qInfo().nospace() << "4K 100px animated eraser pen-up returned in "
                          << penUpMilliseconds
                          << " ms; remaining frames were warmed asynchronously";
    }

    void shrinkingTheAnimationPublishesTheNormalizedFrame()
    {
        Document document = Document::createDefault(QSize(64, 64));
        document.animationFrames = 8;
        DocumentController controller;
        QVERIFY(controller.loadDocument(document));

        CanvasWidget canvas(&controller);
        canvas.setAnimating(false);
        canvas.setCurrentFrame(5);
        QCOMPARE(canvas.currentFrame(), 5);

        // Whoever is told about the frame keeps it; normalizing in silence
        // would leave every consumer that is connected ahead of the canvas
        // holding a frame the document no longer has.
        QSignalSpy frameChanges(&canvas, &CanvasWidget::currentFrameChanged);
        controller.setAnimationFrames(3);
        QCOMPARE(canvas.currentFrame(), 2);
        QCOMPARE(frameChanges.size(), 1);
        QCOMPARE(frameChanges.at(0).at(0).toInt(), 2);
    }

    void keepsEveryPreviewSurfaceInsideTheDeclaredBudget()
    {
        const QSize canvasSize(4096, 4096);
        Document document = Document::createDefault(canvasSize);
        document.animationFrames = 3;
        document.wobbleAmount = 3.0;
        for (int index = 0; index < 3; ++index)
        {
            Layer layer;
            layer.name = QStringLiteral("Paint %1").arg(index + 1);
            layer.kind = LayerKind::Paint;
            layer.initialCanvasSize = canvasSize;
            Stroke stroke;
            stroke.width = 48.0;
            stroke.points = {{QPointF(200.0 + index * 300.0, 200.0), 1.0},
                {QPointF(3800.0, 3800.0 - index * 300.0), 1.0}};
            layer.strokes.append(stroke);
            document.layers.append(std::move(layer));
        }
        DocumentController controller;
        QVERIFY(controller.loadDocument(document));

        CanvasWidget canvas(&controller);
        canvas.resize(800, 600);
        canvas.setAnimating(false);
        canvas.setZoomPercent(100);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        QTRY_VERIFY(!CanvasWidgetTestAccess::zoomRenderPending(canvas));

        PreviewSurfaceUsage peak;
        const auto sample = [&canvas, &peak]()
        {
            const PreviewSurfaceUsage usage =
                CanvasWidgetTestAccess::previewSurfaceUsage(canvas);
            if (usage.totalBytes() > peak.totalBytes())
            {
                peak = usage;
            }
        };

        for (int frame = 0; frame < controller.document().animationFrames;
            ++frame)
        {
            canvas.setCurrentFrame(frame);
            canvas.repaint();
            sample();
        }

        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(80, 0));
        for (int step = -60; step <= 80; step += 20)
        {
            QTest::mouseMove(&canvas, center + QPoint(step, step / 4), 1);
            canvas.repaint();
            sample();
        }
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(80, 20));
        canvas.repaint();
        sample();

        // Sampled while the pick is still held, because releasing drops the
        // native-size frame the picker renders.
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::AltModifier, center);
        canvas.repaint();
        sample();
        QTest::mouseRelease(&canvas, Qt::LeftButton, Qt::AltModifier, center);

        const qint64 budgetBytes =
            static_cast<qint64>(MemoryBudget::previewCacheKiB()) * 1024;
        constexpr qreal mib = 1024.0 * 1024.0;
        qInfo().nospace()
            << "4K preview peak " << static_cast<qreal>(peak.totalBytes()) / mib
            << " MiB of " << static_cast<qreal>(budgetBytes) / mib
            << " MiB budget (frames "
            << static_cast<qreal>(peak.frameCacheBytes) / mib << ", split "
            << static_cast<qreal>(peak.layerSplitBytes) / mib << ", rasters "
            << static_cast<qreal>(peak.layerRasterBytes) / mib
            << ", prepared interaction "
            << static_cast<qreal>(peak.preparedInteractionBytes) / mib
            << ", composed "
            << static_cast<qreal>(peak.composedPreviewBytes) / mib
            << ", colour pick " << static_cast<qreal>(peak.colorPickBytes) / mib
            << ", tiles " << static_cast<qreal>(peak.strokeTileBytes) / mib
            << ")";
        QVERIFY(peak.totalBytes() <= budgetBytes);
    }

    void fitsCanvasToViewportOnFirstShow()
    {
        MainWindow window;
        QApplication::processEvents();

        window.resize(1280, 820);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);
        QVERIFY(canvas->width() > 200);
        QVERIFY(canvas->height() > 200);
        const QSize documentSize =
            MainWindowTestAccess::controller(window).document().size;
        const qreal expectedZoom =
            std::clamp(std::min((canvas->width() - 64.0) / documentSize.width(),
                           (canvas->height() - 64.0) / documentSize.height()),
                0.01,
                16.0);
        QTRY_VERIFY(qAbs(canvas->zoom() - expectedZoom) < 0.0001);
    }

    void syncsMainWindowZoomControls()
    {
        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));

        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QSlider *zoomSlider =
            window.findChild<QSlider *>(QStringLiteral("zoomSlider"));
        QSpinBox *zoomSpin =
            window.findChild<QSpinBox *>(QStringLiteral("zoomPercentSpin"));
        QAction *actualSizeAction =
            window.findChild<QAction *>(QStringLiteral("actualSizeAction"));
        QVERIFY(canvas);
        QVERIFY(zoomSlider);
        QVERIFY(zoomSpin);
        QVERIFY(actualSizeAction);

        zoomSpin->setValue(250);
        QVERIFY(qAbs(canvas->zoom() - 2.5) < 0.0001);
        QCOMPARE(zoomSpin->value(), 250);
        const int sliderAt250 = zoomSlider->value();

        canvas->setZoomPercent(175);
        QCOMPARE(zoomSpin->value(), 175);
        QVERIFY(zoomSlider->value() != sliderAt250);

        int sliderTarget = zoomSlider->maximum() * 3 / 4;
        if (sliderTarget == zoomSlider->value())
        {
            sliderTarget = zoomSlider->maximum() / 2;
        }
        zoomSlider->setValue(sliderTarget);
        QCOMPARE(zoomSpin->value(), qRound(canvas->zoom() * 100.0));
        QVERIFY(qAbs(canvas->zoom() - 1.75) > 0.0001);

        actualSizeAction->trigger();
        QCOMPARE(zoomSpin->value(), 100);
        QVERIFY(qAbs(canvas->zoom() - 1.0) < 0.0001);
        QVERIFY(!window.isWindowModified());
    }

    void pausesAnimationWhilePanning()
    {
        Document document = Document::createDefault(QSize(128, 128));
        document.framesPerSecond = 50.0;
        document.animationFrames = 30;
        DocumentController controller;
        controller.loadDocument(document);
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        QTRY_VERIFY_WITH_TIMEOUT(
            !CanvasWidgetTestAccess::frameCacheWarmupActive(canvas), 10000);

        const QPoint center = canvas.rect().center();
        QTest::mousePress(&canvas, Qt::MiddleButton, Qt::NoModifier, center);
        const int heldFrame = canvas.currentFrame();
        QTest::qWait(100);
        QCOMPARE(canvas.currentFrame(), heldFrame);

        QSignalSpy frameChanges(&canvas, &CanvasWidget::currentFrameChanged);
        QTest::mouseRelease(&canvas, Qt::MiddleButton, Qt::NoModifier, center);
        QTRY_VERIFY_WITH_TIMEOUT(!frameChanges.isEmpty(), 1000);
    }

    void repaintsOnlyExposedStripsWhilePanning()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(4096, 4096)));
        CanvasWidget canvas(&controller);
        canvas.resize(800, 600);
        canvas.setAnimating(false);
        canvas.setZoomPercent(100);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        if (CanvasWidgetTestAccess::usingGpuDisplay(canvas))
        {
            QSKIP("Exposed-strip scrolling is a software-backing-store "
                  "optimization; the GPU display pans with a transform.");
        }
        QTRY_VERIFY(!CanvasWidgetTestAccess::zoomRenderPending(canvas));

        PaintRegionTracker tracker;
        canvas.installEventFilter(&tracker);
        const QPoint center = canvas.rect().center();
        QTest::mousePress(&canvas, Qt::MiddleButton, Qt::NoModifier, center);
        QApplication::processEvents();
        tracker.reset();

        QTest::mouseMove(&canvas, center + QPoint(12, 7), 5);
        QTRY_VERIFY(tracker.eventCount() > 0);
        QVERIFY(tracker.largestArea()
                < static_cast<qint64>(canvas.width()) * canvas.height() / 3);

        QTest::mouseRelease(
            &canvas, Qt::MiddleButton, Qt::NoModifier, center + QPoint(12, 7));
        canvas.removeEventFilter(&tracker);
    }

    void fallsBackToSoftwareDisplayOnHeadlessPlatforms()
    {
        const QString platform = QGuiApplication::platformName();
        if (platform != QStringLiteral("offscreen")
            && platform != QStringLiteral("minimal"))
        {
            QSKIP("Only headless platforms must reject the GPU display.");
        }
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(64, 64)));
        CanvasWidget canvas(&controller);
        QVERIFY(!CanvasWidgetTestAccess::usingGpuDisplay(canvas));
    }

    void reportsRegionalDirtyBoundsForTheDisplayedFrame()
    {
        // Large enough that the preview render spans several 256-pixel
        // incremental-renderer tiles; a short stroke then dirties a proper
        // subset of the frame instead of the single tile covering it all.
        Document document = Document::createDefault(QSize(1024, 1024));
        document.animationFrames = 3;
        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        CanvasWidget canvas(&controller);
        canvas.resize(800, 800);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));
        if (CanvasWidgetTestAccess::usingGpuDisplay(canvas))
        {
            QSKIP("The GPU display consumes dirty bounds on its own "
                  "schedule; this test drives the software resolve.");
        }
        canvas.fitToWindow();
        QTRY_VERIFY_WITH_TIMEOUT(
            !CanvasWidgetTestAccess::frameCacheWarmupActive(canvas), 5000);

        canvas.repaint();
        const auto steady =
            CanvasWidgetTestAccess::resolveDisplayedFrame(canvas);
        QVERIFY(!steady.image.isNull());
        QVERIFY(steady.dirtyBounds.isEmpty());

        const QPoint center = canvas.rect().center();
        QTest::mousePress(
            &canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(20, 0));
        QTest::mouseMove(&canvas, center - QPoint(10, 0));
        const auto firstStrokeResolve =
            CanvasWidgetTestAccess::resolveDisplayedFrame(canvas);
        QVERIFY(!firstStrokeResolve.image.isNull());

        QTest::mouseMove(&canvas, center + QPoint(5, 3));
        const auto tail = CanvasWidgetTestAccess::resolveDisplayedFrame(canvas);
        QVERIFY(!tail.image.isNull());
        QVERIFY(!tail.dirtyBounds.isEmpty());
        QVERIFY(tail.dirtyBounds != tail.image.rect());
        QVERIFY(tail.image.rect().contains(tail.dirtyBounds));

        const auto settled =
            CanvasWidgetTestAccess::resolveDisplayedFrame(canvas);
        QVERIFY(settled.dirtyBounds.isEmpty());
        QTest::mouseRelease(
            &canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(5, 3));
        QApplication::processEvents();

        canvas.repaint();
        canvas.setCurrentFrame(1);
        const auto nextFrame =
            CanvasWidgetTestAccess::resolveDisplayedFrame(canvas);
        QVERIFY(!nextFrame.image.isNull());
        QCOMPARE(nextFrame.dirtyBounds, nextFrame.image.rect());
    }

    void zoomsWithPanModifierCtrlDrag()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 400);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        const qreal initialZoom = canvas.zoom();
        canvas.setPanModifierActive(true);
        const QPoint start(200, 200);
        QTest::mousePress(&canvas, Qt::LeftButton, Qt::ControlModifier, start);
        QTest::mouseMove(&canvas, start + QPoint(120, 0), 5);
        QTest::mouseRelease(&canvas,
            Qt::LeftButton,
            Qt::ControlModifier,
            start + QPoint(120, 0));
        canvas.setPanModifierActive(false);
        QVERIFY(canvas.zoom() > initialZoom);

        const Layer &layer = controller.document().layers.first();
        QVERIFY(layer.strokes.isEmpty());
    }

    void resolvesTheStrokePreviewOncePerPaintWhileDrawing()
    {
        DocumentController controller;
        controller.newDocument(QSize(160, 120));
        CanvasWidget canvas(&controller);
        canvas.resize(400, 320);
        canvas.setAnimating(false);
        canvas.show();
        QVERIFY(QTest::qWaitForWindowExposed(&canvas));

        const auto widgetPoint = [&](const QPointF &documentPosition)
        {
            return CanvasWidgetTestAccess::mapFromDocument(
                canvas, documentPosition);
        };
        // Posted straight to the widget: a pointer report must not be able to
        // slip a paint in before the next one arrives, which is the whole
        // situation under test.
        const auto report = [&](const QPointF &documentPosition)
        {
            const QPointF position = widgetPoint(documentPosition);
            QMouseEvent event(QEvent::MouseMove,
                position,
                canvas.mapToGlobal(position),
                Qt::NoButton,
                Qt::LeftButton,
                Qt::NoModifier);
            QApplication::sendEvent(&canvas, &event);
        };

        QTest::mousePress(&canvas,
            Qt::LeftButton,
            Qt::NoModifier,
            widgetPoint(QPointF(20.0, 30.0)).toPoint());
        CanvasWidgetTestAccess::resolveDisplayedFrame(canvas);

        report(QPointF(60.0, 40.0));
        // The first report after a paint still resolves, so the interaction
        // frame warmup and the tail-sized repaint keep their timing.
        QVERIFY(
            CanvasWidgetTestAccess::activeStrokePreviewIncludesStroke(canvas));

        report(QPointF(90.0, 70.0));
        report(QPointF(120.0, 45.0));
        // Resolving a preview re-renders every tile the tail covers, and that
        // cost grows with the stroke. Reports that arrive before the paint
        // only widen the repaint.
        QVERIFY(
            !CanvasWidgetTestAccess::activeStrokePreviewIncludesStroke(canvas));

        const auto resolved =
            CanvasWidgetTestAccess::resolveDisplayedFrame(canvas);
        QVERIFY(
            CanvasWidgetTestAccess::activeStrokePreviewIncludesStroke(canvas));
        QVERIFY(!resolved.image.isNull());

        // The deferred resolve has to land on the pixels the skipped ones
        // would have produced.
        const Stroke activeStroke =
            CanvasWidgetTestAccess::activeStroke(canvas);
        QCOMPARE(activeStroke.points.size(), 4);
        Document expectedDocument =
            CanvasWidgetTestAccess::displayDocument(canvas);
        Layer *activeLayer =
            expectedDocument.layer(expectedDocument.activeLayerId);
        QVERIFY(activeLayer);
        activeLayer->strokes.append(activeStroke);
        QCOMPARE(resolved.image,
            RenderEngine::renderScaled(expectedDocument,
                canvas.currentFrame(),
                resolved.image.size()));

        QTest::mouseRelease(&canvas,
            Qt::LeftButton,
            Qt::NoModifier,
            widgetPoint(QPointF(120.0, 45.0)).toPoint());
    }
};

int runUiViewportTests(int argc, char **argv)
{
    UiViewportTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "UiViewportTests.moc"
