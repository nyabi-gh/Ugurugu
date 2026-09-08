// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/serializer/RasterAssetTable.hpp"
#include "support/RenderTestHelpers.hpp"
#include "support/RenderTestSuites.hpp"

namespace ugurugu
{

class StrokeCoverageTests final : public QObject
{
    Q_OBJECT

private slots:
    void detectsInsertedImagesAsStaticSelectionContent()
    {
        Document document = Document::createDefault(QSize(64, 48));
        document.background = Qt::transparent;
        document.animationFrames = 30;
        document.wobbleAmount = 12.0;
        QImage image(QSize(8, 6), QImage::Format_RGBA8888);
        image.fill(QColor(40, 120, 220, 255));
        const std::optional<RasterAsset> asset =
            serializer_detail::rasterAssetFromImage(image);
        QVERIFY(asset.has_value());
        document.rasterAssets.insert(asset->id, *asset);
        Stroke operation;
        operation.mode = StrokeMode::Image;
        operation.points.clear();
        operation.imageOp = ImageOp{asset->id,
            QTransform::fromTranslate(20.0, 15.0),
            SamplingMode::Nearest};
        document.layers.first().strokes = {operation};
        const QImage selection =
            rectangularMask(document.size, QRect(20, 15, 8, 6));

        const SelectionVisibility::Result result =
            SelectionVisibility::evaluate(
                document, document.layers.first(), selection, 7);
        QVERIFY(result.renderSucceeded);
        QVERIFY(result.hasVisiblePixels);
        QCOMPARE(result.renderedFrames, 1);
    }

    void detectsMergedLayersAsStaticSelectionContent()
    {
        Document document = Document::createDefault(QSize(64, 48));
        document.background = Qt::transparent;
        document.animationFrames = 30;
        document.wobbleAmount = 12.0;
        Stroke lower = makeStroke(StrokeMode::Paint,
            QColor(220, 60, 40),
            12.0,
            1,
            {QPointF(8.0, 16.0), QPointF(56.0, 16.0)});
        Stroke boundary;
        boundary.mode = StrokeMode::CompositeBoundary;
        Stroke upper = makeStroke(StrokeMode::Paint,
            QColor(40, 80, 220),
            12.0,
            2,
            {QPointF(8.0, 32.0), QPointF(56.0, 32.0)});
        document.layers.first().strokes = {lower, boundary, upper};
        const QImage selection =
            rectangularMask(document.size, QRect(QPoint(), document.size));

        const SelectionVisibility::Result result =
            SelectionVisibility::evaluate(
                document, document.layers.first(), selection, 7);
        QVERIFY(result.renderSucceeded);
        QVERIFY(result.hasVisiblePixels);
        QCOMPARE(result.renderedFrames, 1);
    }

    void identifiesOnlyEditableStrokesIntersectingSelection()
    {
        Document document = Document::createDefault(QSize(96, 64));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        const Stroke left = makeStroke(StrokeMode::Paint,
            QColor(210, 40, 60),
            10.0,
            1,
            {QPointF(12.0, 32.0), QPointF(36.0, 32.0)});
        const Stroke right = makeStroke(StrokeMode::Paint,
            QColor(40, 80, 220),
            10.0,
            2,
            {QPointF(60.0, 32.0), QPointF(84.0, 32.0)});
        layer.strokes = {left, right};
        const QImage selection =
            rectangularMask(document.size, QRect(4, 20, 40, 24));

        const QVector<QUuid> ids = SelectionVisibility::editableStrokeIds(
            document, layer, selection, 0);
        QCOMPARE(ids, QVector<QUuid>{left.id});
    }

    void preservesEditableStrokeVisibilityThroughFramebufferOperations()
    {
        Document movedDocument = Document::createDefault(QSize(96, 64));
        movedDocument.background = Qt::transparent;
        movedDocument.wobbleAmount = 0.0;
        Layer &movedLayer = movedDocument.layers.first();
        Stroke moved = makeStroke(StrokeMode::Paint,
            QColor(210, 40, 60),
            16.0,
            11,
            {QPointF(16.0, 24.0)});
        moved.brush.tipShape = BrushTipShape::Square;
        moved.brush.sizeDynamics = 0.0;
        movedLayer.strokes.append(moved);
        const QImage sourceSelection =
            rectangularMask(movedDocument.size, QRect(8, 16, 16, 16));
        QTransform shift;
        shift.translate(48.0, 16.0);
        const std::optional<PixelSelectionOp> moveOperation =
            makePixelSelectionOp(sourceSelection, shift, true, true);
        QVERIFY(moveOperation.has_value());
        Stroke move;
        move.mode = StrokeMode::PixelSelection;
        move.pixelSelectionOp = *moveOperation;
        movedLayer.strokes.append(move);

        QCOMPARE(SelectionVisibility::editableStrokeIds(
                     movedDocument, movedLayer, sourceSelection, 0),
            QVector<QUuid>());
        const QImage destinationSelection =
            rectangularMask(movedDocument.size, QRect(56, 32, 16, 16));
        QCOMPARE(SelectionVisibility::editableStrokeIds(
                     movedDocument, movedLayer, destinationSelection, 0),
            QVector<QUuid>{moved.id});

        Document erasedDocument = Document::createDefault(QSize(64, 64));
        erasedDocument.background = Qt::transparent;
        erasedDocument.wobbleAmount = 0.0;
        Layer &erasedLayer = erasedDocument.layers.first();
        Stroke painted = makeStroke(StrokeMode::Paint,
            QColor(40, 90, 220),
            20.0,
            21,
            {QPointF(32.0, 32.0)});
        painted.brush.tipShape = BrushTipShape::Square;
        painted.brush.sizeDynamics = 0.0;
        Stroke erased = painted;
        erased.id = QUuid::createUuid();
        erased.mode = StrokeMode::Erase;
        erased.seed = 22;
        erasedLayer.strokes = {painted, erased};
        const QImage erasedSelection =
            rectangularMask(erasedDocument.size, QRect(28, 28, 8, 8));
        QCOMPARE(SelectionVisibility::editableStrokeIds(
                     erasedDocument, erasedLayer, erasedSelection, 0),
            QVector<QUuid>{erased.id});

        Document reframedDocument = Document::createDefault(QSize(96, 72));
        reframedDocument.background = Qt::transparent;
        reframedDocument.wobbleAmount = 0.0;
        Layer &reframedLayer = reframedDocument.layers.first();
        reframedLayer.initialCanvasSize = QSize(128, 96);
        Stroke reframed = makeStroke(StrokeMode::Paint,
            QColor(35, 170, 95),
            16.0,
            31,
            {QPointF(32.0, 32.0)});
        reframed.brush.tipShape = BrushTipShape::Square;
        reframed.brush.sizeDynamics = 0.0;
        reframedLayer.strokes.append(reframed);
        Stroke crop;
        crop.mode = StrokeMode::Reframe;
        crop.reframeOp = ReframeOp{ReframeMode::Canvas,
            SamplingMode::Nearest,
            QSize(128, 96),
            reframedDocument.size,
            QPoint(-16, -8)};
        reframedLayer.strokes.append(crop);
        const QImage reframedSelection =
            rectangularMask(reframedDocument.size, QRect(12, 20, 8, 8));
        QCOMPARE(SelectionVisibility::editableStrokeIds(
                     reframedDocument, reframedLayer, reframedSelection, 0),
            QVector<QUuid>{reframed.id});
    }

    void matchesSparseCoverageToLegacyFramebufferReplay()
    {
        const auto verify = [](const Document &document, int strokeIndex)
        {
            const Layer &layer = document.layers.first();
            const RenderEngine::StrokeCoveragePlan plan =
                RenderEngine::prepareStrokeCoverage(document, layer);
            QVERIFY(plan.valid);
            RenderEngine::StrokeCoverageStats stats;
            const RenderEngine::StrokeCoverageRegion sparse =
                RenderEngine::renderSparseStrokeCoverage(document,
                    layer,
                    strokeIndex,
                    0,
                    QRect(QPoint(), document.size),
                    plan,
                    &stats);
            QVERIFY(sparse.valid);
            QCOMPARE(expandedStrokeCoverage(sparse, document.size),
                RenderEngine::renderStrokeCoverage(
                    document, layer, strokeIndex, 0));
            QCOMPARE(stats.fullCanvasFallbacks, 0ULL);
            QCOMPARE(stats.regionalRenders, 1ULL);
        };

        Document moved = Document::createDefault(QSize(96, 72));
        moved.background = Qt::transparent;
        moved.wobbleAmount = 0.0;
        Layer &movedLayer = moved.layers.first();
        Stroke source = makeStroke(StrokeMode::Paint,
            QColor(210, 45, 80),
            18.0,
            1,
            {QPointF(18.0, 20.0), QPointF(42.0, 32.0)});
        movedLayer.strokes.append(source);
        const QImage sourceMask =
            rectangularMask(moved.size, QRect(8, 8, 44, 36));
        QTransform transform;
        transform.translate(24.0, 12.0);
        transform.rotate(11.0);
        transform.scale(0.9, 1.1);
        const std::optional<PixelSelectionOp> pixelOperation =
            makePixelSelectionOp(sourceMask, transform, true, true);
        QVERIFY(pixelOperation.has_value());
        QCOMPARE(pixelOperation->sampling, SamplingMode::Smooth);
        Stroke pixel;
        pixel.mode = StrokeMode::PixelSelection;
        pixel.pixelSelectionOp = *pixelOperation;
        movedLayer.strokes.append(pixel);
        Stroke erase = makeStroke(StrokeMode::Erase,
            Qt::black,
            7.0,
            2,
            {QPointF(55.0, 34.0), QPointF(68.0, 42.0)});
        erase.clipMask = rectangularMask(moved.size, QRect(48, 24, 28, 28));
        movedLayer.strokes.append(erase);
        verify(moved, 0);

        Document copied = moved;
        copied.layers.first().strokes.removeLast();
        copied.layers.first().strokes[1].pixelSelectionOp->clearSource = false;
        verify(copied, 0);

        Document cleared = copied;
        PixelSelectionOp &clearOperation =
            *cleared.layers.first().strokes[1].pixelSelectionOp;
        clearOperation.clearSource = true;
        clearOperation.drawDestination = false;
        verify(cleared, 0);

        Document nearest = moved;
        nearest.layers.first().strokes.removeLast();
        PixelSelectionOp &nearestOperation =
            *nearest.layers.first().strokes[1].pixelSelectionOp;
        nearestOperation.transform = QTransform::fromTranslate(24.0, 12.0);
        nearestOperation.sampling = SamplingMode::Nearest;
        verify(nearest, 0);

        Document canvasReframe = Document::createDefault(QSize(96, 72));
        canvasReframe.background = Qt::transparent;
        canvasReframe.wobbleAmount = 0.0;
        Layer &canvasLayer = canvasReframe.layers.first();
        canvasLayer.initialCanvasSize = QSize(128, 96);
        canvasLayer.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(40, 170, 100),
            20.0,
            3,
            {QPointF(30.0, 30.0), QPointF(72.0, 52.0)}));
        Stroke crop;
        crop.mode = StrokeMode::Reframe;
        crop.reframeOp = ReframeOp{ReframeMode::Canvas,
            SamplingMode::Nearest,
            QSize(128, 96),
            canvasReframe.size,
            QPoint(-16, -8)};
        canvasLayer.strokes.append(crop);
        verify(canvasReframe, 0);

        Document imageReframe = Document::createDefault(QSize(79, 61));
        imageReframe.background = Qt::transparent;
        imageReframe.wobbleAmount = 0.0;
        Layer &imageLayer = imageReframe.layers.first();
        imageLayer.initialCanvasSize = QSize(97, 73);
        imageLayer.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(60, 100, 220),
            17.0,
            4,
            {QPointF(21.0, 18.0), QPointF(66.0, 51.0)}));
        Stroke resize;
        resize.mode = StrokeMode::Reframe;
        resize.reframeOp = ReframeOp{ReframeMode::Image,
            SamplingMode::Smooth,
            QSize(97, 73),
            imageReframe.size,
            {}};
        imageLayer.strokes.append(resize);
        verify(imageReframe, 0);

        Document nearestImageReframe = imageReframe;
        nearestImageReframe.layers.first().strokes.last().reframeOp->sampling =
            SamplingMode::Nearest;
        verify(nearestImageReframe, 0);

        Document maskedFill = Document::createDefault(QSize(96, 72));
        maskedFill.background = Qt::transparent;
        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(255, 180, 40, 117);
        fill.points = {{QPointF(12.0, 18.0), 1.0}};
        fill.fillMask = rectangularMask(maskedFill.size, QRect(17, 13, 29, 21));
        fill.clipMask = rectangularMask(maskedFill.size, QRect(20, 11, 31, 19));
        fill.visibilityClip = QRect(19, 12, 24, 23);
        maskedFill.layers.first().strokes = {fill};
        verify(maskedFill, 0);

        Document proceduralFill = Document::createDefault(QSize(96, 72));
        proceduralFill.background = Qt::transparent;
        fill.fillMask = {};
        fill.clipMask =
            rectangularMask(proceduralFill.size, QRect(8, 9, 37, 31));
        fill.visibilityClip = QRect(11, 7, 28, 29);
        proceduralFill.layers.first().strokes = {fill};
        verify(proceduralFill, 0);
    }

    void matchesNonAntialiasedPackedFillCoverageExactly()
    {
        Document document = Document::createDefault(QSize(32, 32));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        QImage mask(document.size, QImage::Format_Grayscale8);
        mask.fill(0);
        mask.scanLine(10)[10] = 255;
        const std::optional<PackedMaskRegion> packed = packBinaryMask(mask);
        QVERIFY(packed.has_value());

        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = Qt::black;
        fill.brush.antialiasing = false;
        fill.points = {{QPointF(10.0, 10.0), 1.0}};
        fill.fillCoverage = *packed;
        document.layers.first().strokes = {fill};
        const Layer &layer = document.layers.first();

        const RenderEngine::StrokeCoveragePlan plan =
            RenderEngine::prepareStrokeCoverage(document, layer);
        QVERIFY(plan.valid);
        const RenderEngine::StrokeCoverageRegion sparse =
            RenderEngine::renderSparseStrokeCoverage(
                document, layer, 0, 0, QRect(QPoint(), document.size), plan);
        QVERIFY(sparse.valid);
        const QImage expected =
            RenderEngine::renderStrokeCoverage(document, layer, 0, 0);
        QCOMPARE(expandedStrokeCoverage(sparse, document.size), expected);
        QCOMPARE(qAlpha(expected.pixel(9, 10)), 0);
    }

    void resamplesImageRegionsDeterministicallyNearQtReference()
    {
        QRandomGenerator random(0x7391U);
        QImage source(QSize(73, 47), QImage::Format_ARGB32_Premultiplied);
        QVERIFY(!source.isNull());
        for (int y = 0; y < source.height(); ++y)
        {
            auto *line = reinterpret_cast<QRgb *>(source.scanLine(y));
            for (int x = 0; x < source.width(); ++x)
            {
                const int alpha = int(random.bounded(256U));
                line[x] = qRgba(random.bounded(alpha + 1),
                    random.bounded(alpha + 1),
                    random.bounded(alpha + 1),
                    alpha);
            }
        }

        const QSize targetSize(119, 61);
        for (const SamplingMode sampling :
            {SamplingMode::Nearest, SamplingMode::Smooth})
        {
            const QImage full =
                ImageResampler::resample(source, targetSize, sampling);
            QVERIFY(!full.isNull());
            for (const QRect &region :
                {QRect(0, 0, 1, 1),
                    QRect(17, 9, 43, 21),
                    QRect(
                        targetSize.width() - 1, targetSize.height() - 1, 1, 1)})
            {
                QCOMPARE(ImageResampler::resampleRegion(source,
                             source.rect(),
                             source.size(),
                             region,
                             targetSize,
                             sampling),
                    full.copy(region));
            }
        }

        QImage singlePixel(1, 1, QImage::Format_ARGB32_Premultiplied);
        singlePixel.setPixel(0, 0, qRgba(17, 31, 47, 63));
        const QRect farEdge(4095, 4095, 1, 1);
        for (const SamplingMode sampling :
            {SamplingMode::Nearest, SamplingMode::Smooth})
        {
            const QImage edge = ImageResampler::resampleRegion(singlePixel,
                singlePixel.rect(),
                singlePixel.size(),
                farEdge,
                QSize(4096, 4096),
                sampling);
            QCOMPARE(edge.size(), QSize(1, 1));
            QCOMPARE(edge.pixel(0, 0), singlePixel.pixel(0, 0));
        }

        const QImage deterministic =
            ImageResampler::resample(source, targetSize, SamplingMode::Smooth);
        const QImage qtReference =
            qtResizedRasterResult(source, targetSize, true);
        const ImageDifference difference =
            storedImageDifference(deterministic, qtReference, 0);
        qInfo("deterministic image resize differs from Qt by %llu total "
              "channel levels, %.4f average, max %d",
            static_cast<unsigned long long>(difference.channelDifference),
            double(difference.channelDifference)
                / double(difference.comparedChannels),
            difference.maximumChannelDifference);
        QVERIFY(difference.maximumChannelDifference <= 2);
        QVERIFY(difference.channelDifference * 100
                <= difference.comparedChannels * 6);
    }

    void transformsAffineImagesExactlyAcrossFullAndRegionalTargets()
    {
        QRandomGenerator random(0x7391U);
        const QSize canvasSize(64, 48);
        const QRect canvasBounds(QPoint(), canvasSize);
        for (int caseIndex = 0; caseIndex < 500; ++caseIndex)
        {
            const int sourceWidth = 6 + int(random.bounded(18U));
            const int sourceHeight = 6 + int(random.bounded(18U));
            const QRect sourceBounds(
                int(random.bounded(quint32(canvasSize.width() - sourceWidth))),
                int(random.bounded(
                    quint32(canvasSize.height() - sourceHeight))),
                sourceWidth,
                sourceHeight);
            const QRect contentBounds = sourceBounds.adjusted(2, 2, -2, -2);
            const QRect croppedBounds = contentBounds.adjusted(-1, -1, 1, 1);
            QImage source(
                sourceBounds.size(), QImage::Format_ARGB32_Premultiplied);
            source.fill(Qt::transparent);
            for (int y = contentBounds.top(); y <= contentBounds.bottom(); ++y)
            {
                auto *line = reinterpret_cast<QRgb *>(
                    source.scanLine(y - sourceBounds.top()));
                for (int x = contentBounds.left(); x <= contentBounds.right();
                    ++x)
                {
                    const int alpha = 1 + int(random.bounded(255U));
                    line[x - sourceBounds.left()] =
                        qRgba(random.bounded(alpha + 1),
                            random.bounded(alpha + 1),
                            random.bounded(alpha + 1),
                            alpha);
                }
            }
            QImage base(canvasSize, QImage::Format_ARGB32_Premultiplied);
            for (int y = 0; y < base.height(); ++y)
            {
                auto *line = reinterpret_cast<QRgb *>(base.scanLine(y));
                for (int x = 0; x < base.width(); ++x)
                {
                    const int alpha = int(random.bounded(256U));
                    line[x] = qRgba(random.bounded(alpha + 1),
                        random.bounded(alpha + 1),
                        random.bounded(alpha + 1),
                        alpha);
                }
            }

            QTransform transform;
            const QPointF center = sourceBounds.center();
            switch (caseIndex % 6)
            {
            case 0:
                transform.translate(
                    int(random.bounded(17U)) - 8, int(random.bounded(17U)) - 8);
                break;
            case 1:
                transform.translate(qreal(int(random.bounded(17U)) - 8) + 0.375,
                    qreal(int(random.bounded(17U)) - 8) - 0.4375);
                break;
            case 2:
                transform.translate(center.x(), center.y());
                transform.rotate(qreal(int(random.bounded(61U)) - 30));
                transform.translate(-center.x(), -center.y());
                break;
            case 3:
                transform.translate(center.x(), center.y());
                transform.shear(0.08 * (int(random.bounded(9U)) - 4),
                    0.06 * (int(random.bounded(9U)) - 4));
                transform.scale(0.65 + random.generateDouble(),
                    0.65 + random.generateDouble());
                transform.translate(-center.x(), -center.y());
                break;
            case 4:
                transform.translate(
                    caseIndex % 2 == 0 ? 1.0 - 1.0e-12 : 1.0 + 1.0e-12,
                    caseIndex % 4 == 0 ? -1.0e-12 : 1.0e-12);
                break;
            case 5:
                transform.translate(center.x(), center.y());
                transform.rotate(-17.0);
                transform.shear(0.13, -0.09);
                transform.scale(1.21, 0.83);
                transform.translate(-center.x(), -center.y());
                break;
            default:
                Q_UNREACHABLE();
            }
            QVERIFY(transform.isInvertible());
            const SamplingMode sampling = caseIndex % 3 == 0
                                              ? SamplingMode::Nearest
                                              : SamplingMode::Smooth;

            QImage full = base;
            QVERIFY(ImageAffineTransformer::compositeSourceOver(
                full, canvasBounds, source, sourceBounds, transform, sampling));

            QImage assembled = base;
            const QImage cropped =
                source.copy(croppedBounds.translated(-sourceBounds.topLeft()));
            const QRect effectBounds = ImageAffineTransformer::targetBounds(
                croppedBounds, canvasSize, transform, sampling);
            if (!effectBounds.isEmpty())
            {
                QImage regional = base.copy(effectBounds);
                QVERIFY(ImageAffineTransformer::compositeSourceOver(regional,
                    effectBounds,
                    cropped,
                    croppedBounds,
                    transform,
                    sampling));
                QPainter painter(&assembled);
                painter.setCompositionMode(QPainter::CompositionMode_Source);
                painter.drawImage(effectBounds.topLeft(), regional);
                painter.end();
            }
            QCOMPARE(full, assembled);
        }
    }

    void keepsSelectionSamplingAlphaSemantics()
    {
        const QSize canvasSize(64, 64);
        const QRect canvasBounds(QPoint(), canvasSize);
        const QRect sourceBounds(20, 20, 9, 9);
        QImage source(sourceBounds.size(), QImage::Format_ARGB32_Premultiplied);
        source.fill(Qt::transparent);
        QImage selection(canvasSize, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 0; y < source.height(); ++y)
        {
            auto *sourceLine = reinterpret_cast<QRgb *>(source.scanLine(y));
            uchar *selectionLine = selection.scanLine(y + sourceBounds.top());
            for (int x = 0; x < source.width(); ++x)
            {
                if (std::abs(x - 4) + std::abs(y - 4) > 4)
                {
                    continue;
                }
                sourceLine[x] = qRgba(30, 120, 220, 255);
                selectionLine[x + sourceBounds.left()] = 255;
            }
        }

        const QPointF center = QRectF(sourceBounds).center();
        QTransform rotation;
        rotation.translate(center.x(), center.y());
        rotation.rotate(27.0);
        rotation.translate(-center.x(), -center.y());
        for (const SamplingMode sampling :
            {SamplingMode::Smooth, SamplingMode::Nearest})
        {
            const std::optional<PixelSelectionOp> operation =
                makePixelSelectionOp(selection, rotation, true, true, sampling);
            QVERIFY(operation.has_value());
            QCOMPARE(operation->sampling, sampling);

            QImage transformed(canvasSize, QImage::Format_ARGB32_Premultiplied);
            transformed.fill(Qt::transparent);
            QVERIFY(ImageAffineTransformer::compositeSourceOver(transformed,
                canvasBounds,
                source,
                sourceBounds,
                rotation,
                sampling));

            int partialAlphaPixels = 0;
            int opaquePixels = 0;
            for (int y = 0; y < transformed.height(); ++y)
            {
                const auto *line = reinterpret_cast<const QRgb *>(
                    transformed.constScanLine(y));
                for (int x = 0; x < transformed.width(); ++x)
                {
                    const QRgb pixel = line[x];
                    const int alpha = qAlpha(pixel);
                    partialAlphaPixels += alpha > 0 && alpha < 255 ? 1 : 0;
                    opaquePixels += alpha == 255 ? 1 : 0;
                    QVERIFY(qRed(pixel) <= alpha);
                    QVERIFY(qGreen(pixel) <= alpha);
                    QVERIFY(qBlue(pixel) <= alpha);
                }
            }
            QVERIFY(opaquePixels > 0);
            if (sampling == SamplingMode::Smooth)
            {
                QVERIFY(partialAlphaPixels > 0);
            }
            else
            {
                QCOMPARE(partialAlphaPixels, 0);
            }
        }

        const QRgb translucentPixel = qRgba(30, 20, 10, 96);
        QImage translucent(1, 1, QImage::Format_ARGB32_Premultiplied);
        translucent.setPixel(0, 0, translucentPixel);
        QImage translated(8, 8, QImage::Format_ARGB32_Premultiplied);
        translated.fill(Qt::transparent);
        QVERIFY(ImageAffineTransformer::compositeSourceOver(translated,
            translated.rect(),
            translucent,
            QRect(2, 2, 1, 1),
            QTransform::fromTranslate(3.0, 2.0),
            SamplingMode::Nearest));
        QCOMPARE(translated.pixel(5, 4), translucentPixel);

        QImage translucentBlock(3, 3, QImage::Format_ARGB32_Premultiplied);
        translucentBlock.fill(translucentPixel);
        QImage rotatedBlock(12, 12, QImage::Format_ARGB32_Premultiplied);
        rotatedBlock.fill(Qt::transparent);
        QTransform blockRotation;
        blockRotation.translate(5.5, 5.5);
        blockRotation.rotate(27.0);
        blockRotation.translate(-1.5, -1.5);
        QVERIFY(ImageAffineTransformer::compositeSourceOver(rotatedBlock,
            rotatedBlock.rect(),
            translucentBlock,
            QRect(0, 0, 3, 3),
            blockRotation,
            SamplingMode::Nearest));
        int preservedTranslucentPixels = 0;
        for (int y = 0; y < rotatedBlock.height(); ++y)
        {
            const auto *line =
                reinterpret_cast<const QRgb *>(rotatedBlock.constScanLine(y));
            for (int x = 0; x < rotatedBlock.width(); ++x)
            {
                const int alpha = qAlpha(line[x]);
                QVERIFY(alpha == 0 || alpha == 96);
                preservedTranslucentPixels += alpha == 96 ? 1 : 0;
            }
        }
        QVERIFY(preservedTranslucentPixels > 0);
    }

    void keepsAffineSelectionSupportAlignedAtHalfAlpha()
    {
        const QSize canvasSize(72, 56);
        const QImage selection =
            rectangularMask(canvasSize, QRect(11, 9, 27, 19));
        const QRect sourceBounds(11, 9, 27, 19);
        QImage payload(
            sourceBounds.size(), QImage::Format_ARGB32_Premultiplied);
        payload.fill(Qt::white);
        const QVector<QTransform> transforms = {
            QTransform::fromTranslate(7.0, -3.0),
            QTransform::fromTranslate(7.375, -3.4375),
            QTransform(1.15, -0.17, 0.12, 0.83, 4.0, 3.0),
            QTransform(0.91, 0.26, -0.19, 1.08, -2.0, 5.0)};
        int falsePositives = 0;
        int falseNegatives = 0;
        for (qsizetype transformIndex = 0; transformIndex < transforms.size();
            ++transformIndex)
        {
            const QTransform &transform = transforms[transformIndex];
            const SamplingMode sampling =
                samplingForSelectionTransform(transform);
            QImage transformed(canvasSize, QImage::Format_ARGB32_Premultiplied);
            transformed.fill(Qt::transparent);
            QVERIFY(ImageAffineTransformer::compositeSourceOver(transformed,
                QRect(QPoint(), canvasSize),
                payload,
                sourceBounds,
                transform,
                sampling));
            const QImage support = transformedSelectionSupport(
                selection, canvasSize, transform, sampling);
            QVERIFY(!support.isNull());
            for (int y = 0; y < canvasSize.height(); ++y)
            {
                const auto *transformedLine = reinterpret_cast<const QRgb *>(
                    transformed.constScanLine(y));
                const uchar *supportLine = support.constScanLine(y);
                for (int x = 0; x < canvasSize.width(); ++x)
                {
                    const bool transformedContains =
                        qAlpha(transformedLine[x]) >= 128;
                    const bool supportContains = supportLine[x] >= 128;
                    falsePositives +=
                        !transformedContains && supportContains ? 1 : 0;
                    falseNegatives +=
                        transformedContains && !supportContains ? 1 : 0;
                }
            }
        }
        qInfo("selection support differs from deterministic payload at >=128 "
              "by %d false positives and %d false negatives",
            falsePositives,
            falseNegatives);
        QCOMPARE(falsePositives, 0);
        QCOMPARE(falseNegatives, 0);
    }

    void keepsAffineTransformNearQtReference()
    {
        const QSize canvasSize(80, 64);
        const QRect canvasBounds(QPoint(), canvasSize);
        const QRect sourceBounds(17, 13, 23, 19);
        QImage source(sourceBounds.size(), QImage::Format_ARGB32_Premultiplied);
        source.fill(Qt::transparent);
        QRandomGenerator random(0x2814U);
        for (int y = 1; y < source.height() - 1; ++y)
        {
            auto *line = reinterpret_cast<QRgb *>(source.scanLine(y));
            for (int x = 1; x < source.width() - 1; ++x)
            {
                const int alpha = int(random.bounded(256U));
                line[x] = qRgba(random.bounded(alpha + 1),
                    random.bounded(alpha + 1),
                    random.bounded(alpha + 1),
                    alpha);
            }
        }
        QImage base(canvasSize, QImage::Format_ARGB32_Premultiplied);
        for (int y = 0; y < base.height(); ++y)
        {
            auto *line = reinterpret_cast<QRgb *>(base.scanLine(y));
            for (int x = 0; x < base.width(); ++x)
            {
                const int alpha = int(random.bounded(256U));
                line[x] = qRgba(random.bounded(alpha + 1),
                    random.bounded(alpha + 1),
                    random.bounded(alpha + 1),
                    alpha);
            }
        }
        const QVector<QTransform> transforms = {
            QTransform::fromTranslate(9.0, -4.0),
            QTransform::fromTranslate(9.375, -4.4375),
            QTransform(1.21, -0.14, 0.09, 0.82, 3.0, 6.0),
            QTransform(0.88, 0.31, -0.22, 1.13, -4.0, 2.0)};
        quint64 totalDifference = 0;
        quint64 comparedChannels = 0;
        qsizetype differentPixels = 0;
        qsizetype comparedPixels = 0;
        int maximumDifference = 0;
        for (qsizetype transformIndex = 0; transformIndex < transforms.size();
            ++transformIndex)
        {
            const QTransform &transform = transforms[transformIndex];
            const SamplingMode sampling =
                samplingForSelectionTransform(transform);
            QImage deterministic = base;
            QVERIFY(ImageAffineTransformer::compositeSourceOver(deterministic,
                canvasBounds,
                source,
                sourceBounds,
                transform,
                sampling));
            const QImage qtReference = qtAffineComposite(
                base, canvasBounds, source, sourceBounds, transform, sampling);
            const ImageDifference difference =
                storedImageDifference(deterministic, qtReference, 0);
            totalDifference += difference.channelDifference;
            comparedChannels += difference.comparedChannels;
            differentPixels += difference.visiblyDifferentPixels;
            comparedPixels += static_cast<qsizetype>(canvasSize.width())
                              * static_cast<qsizetype>(canvasSize.height());
            maximumDifference = std::max(
                maximumDifference, difference.maximumChannelDifference);
        }
        qInfo("deterministic affine transform differs from Qt by %llu total "
              "channel levels, %.4f average, across %lld/%lld pixels, max %d",
            static_cast<unsigned long long>(totalDifference),
            double(totalDifference) / double(comparedChannels),
            static_cast<long long>(differentPixels),
            static_cast<long long>(comparedPixels),
            maximumDifference);
        QVERIFY(maximumDifference <= 10);
        QVERIFY(totalDifference * 100 <= comparedChannels * 5);
        QVERIFY(differentPixels * 100 <= comparedPixels * 4);
    }

    void filtersLargeStrokeSetsBeforeRenderingSelectionCoverage()
    {
        constexpr int edge = DocumentLimits::maximumCanvasEdge;
        constexpr int strokeCount = 12000;
        Document document = Document::createDefault(QSize(edge, edge));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        layer.strokes.reserve(strokeCount + 1);
        for (int index = 0; index < strokeCount; ++index)
        {
            layer.strokes.append(makeStroke(StrokeMode::Paint,
                Qt::black,
                2.0,
                static_cast<quint64>(index) + 1,
                {QPointF(32.0 + index % 128, 32.0 + index / 128 % 128)}));
        }
        const Stroke target = makeStroke(StrokeMode::Paint,
            QColor(220, 50, 80),
            8.0,
            0xfedcba98ULL,
            {QPointF(4000.0, 4000.0)});
        layer.strokes.append(target);
        const QRect selectionBounds(3996, 3996, 9, 9);
        const QImage selection =
            rectangularMask(document.size, selectionBounds);

        QElapsedTimer timer;
        timer.start();
        const QVector<QUuid> ids = SelectionVisibility::editableStrokeIds(
            document, layer, selection, 0);
        const qint64 elapsed = timer.elapsed();

        QCOMPARE(ids, QVector<QUuid>{target.id});
        QVERIFY2(elapsed < 5000,
            qPrintable(QStringLiteral("editable stroke lookup took %1 ms")
                    .arg(elapsed)));
        const QImage coverage =
            RenderEngine::renderStrokeCoverageRegion(document,
                layer,
                static_cast<int>(layer.strokes.size()) - 1,
                0,
                selectionBounds);
        QCOMPARE(coverage.size(), selectionBounds.size());
    }

    void boundsPerStrokeCoverageForFullCanvasSelection()
    {
        constexpr int edge = DocumentLimits::maximumCanvasEdge;
        constexpr int strokeCount = 1024;
        Document document = Document::createDefault(QSize(edge, edge));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        layer.strokes.reserve(strokeCount);
        QVector<QUuid> expectedIds;
        expectedIds.reserve(strokeCount);
        for (int index = 0; index < strokeCount; ++index)
        {
            const int row = index / 32;
            Stroke stroke = makeStroke(StrokeMode::Paint,
                Qt::black,
                2.0,
                static_cast<quint64>(index) + 1,
                {QPointF(32.0 + index % 32 * 24,
                    32.0 + static_cast<qreal>(row) * 24)});
            expectedIds.append(stroke.id);
            layer.strokes.append(std::move(stroke));
        }
        QImage selection(document.size, QImage::Format_Grayscale8);
        selection.fill(255);

        QElapsedTimer timer;
        timer.start();
        const QVector<QUuid> ids = SelectionVisibility::editableStrokeIds(
            document, layer, selection, 0);
        const qint64 elapsed = timer.elapsed();

        QCOMPARE(ids, expectedIds);
        QVERIFY2(elapsed < 5000,
            qPrintable(QStringLiteral("full selection lookup took %1 ms")
                    .arg(elapsed)));
    }

    void boundsMaskedAndProceduralFillCoverage()
    {
        constexpr int edge = DocumentLimits::maximumCanvasEdge;
        Document document = Document::createDefault(QSize(edge, edge));
        document.background = Qt::transparent;
        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(210, 90, 35, 173);
        fill.points = {{QPointF(24.0, 24.0), 1.0}};
        fill.fillMask = rectangularMask(document.size, QRect(12, 14, 64, 48));
        document.layers.first().strokes = {fill};
        QImage selection =
            rectangularMask(document.size, QRect(20, 20, 24, 20));

        SelectionVisibility::EditableStrokeStats stats;
        QCOMPARE(SelectionVisibility::editableStrokeIds(
                     document, document.layers.first(), selection, 0, &stats),
            QVector<QUuid>{fill.id});
        QCOMPARE(stats.fullCanvasFallbacks, 0ULL);
        QVERIFY(stats.maximumExplicitImageBytes < 1024ULL * 1024ULL);

        document.layers.first().strokes.first().fillMask = {};
        stats = {};
        QCOMPARE(SelectionVisibility::editableStrokeIds(
                     document, document.layers.first(), selection, 0, &stats),
            QVector<QUuid>{fill.id});
        QCOMPARE(stats.fullCanvasFallbacks, 0ULL);
        QVERIFY(stats.maximumExplicitImageBytes < 1024ULL * 1024ULL);
    }

    void reportsFarCopyCoverageImageBound()
    {
        constexpr int edge = DocumentLimits::maximumCanvasEdge;
        Document document = Document::createDefault(QSize(edge, edge));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke paint = makeStroke(
            StrokeMode::Paint, Qt::black, 8.0, 91, {QPointF(24.0, 24.0)});
        paint.brush.antialiasing = false;
        PixelSelectionOp copy;
        copy.canvasSize = document.size;
        copy.sourceBounds = QRect(0, 0, 64, 64);
        copy.packedMask = QByteArray(
            static_cast<qsizetype>(8) * 64, std::bit_cast<char>(quint8{0xff}));
        copy.transform = QTransform::fromTranslate(3960.0, 3960.0);
        copy.sampling = SamplingMode::Nearest;
        copy.clearSource = false;
        copy.drawDestination = true;
        QVERIFY(isValidPixelSelectionOp(copy));
        Stroke operation;
        operation.mode = StrokeMode::PixelSelection;
        operation.pixelSelectionOp = copy;
        document.layers.first().strokes = {paint, operation};

        const RenderEngine::StrokeCoveragePlan plan =
            RenderEngine::prepareStrokeCoverage(
                document, document.layers.first());
        QVERIFY(plan.valid);
        RenderEngine::StrokeCoverageStats stats;
        QElapsedTimer timer;
        timer.start();
        const RenderEngine::StrokeCoverageRegion coverage =
            RenderEngine::renderSparseStrokeCoverage(document,
                document.layers.first(),
                0,
                0,
                QRect(3960, 3960, 96, 96),
                plan,
                &stats);
        const qint64 elapsed = timer.elapsed();
        qInfo("4K far-copy coverage took %lld ms, max image %llu bytes",
            static_cast<long long>(elapsed),
            static_cast<unsigned long long>(stats.maximumExplicitImageBytes));
        QVERIFY(coverage.valid);
        QVERIFY(!coverage.image.isNull());
        QCOMPARE(stats.fullCanvasFallbacks, 0ULL);
        QVERIFY(stats.maximumExplicitImageBytes <= 64ULL * 1024ULL * 1024ULL);
        QVERIFY(elapsed < 10000);
    }

    void benchmarksFullCanvasSelectionAfterFramebufferOperation()
    {
        constexpr int edge = DocumentLimits::maximumCanvasEdge;
        constexpr int strokeCount = DocumentLimits::maximumStrokesPerLayer - 1;
        Document document = Document::createDefault(QSize(edge, edge));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        layer.strokes.reserve(strokeCount + 1);
        QVector<QUuid> expectedIds;
        expectedIds.reserve(strokeCount);
        for (int index = 0; index < strokeCount; ++index)
        {
            Stroke stroke = makeStroke(StrokeMode::Paint,
                Qt::black,
                2.0,
                static_cast<quint64>(index) + 1,
                {QPointF(32.0 + index % 128 * 31,
                    32.0 + static_cast<qreal>(index / 128 % 128) * 31)});
            expectedIds.append(stroke.id);
            layer.strokes.append(std::move(stroke));
        }
        QImage selection(document.size, QImage::Format_Grayscale8);
        selection.fill(255);
        const std::optional<PixelSelectionOp> pixelOperation =
            makePixelSelectionOp(selection, QTransform(), true, true);
        QVERIFY(pixelOperation.has_value());
        Stroke operation;
        operation.mode = StrokeMode::PixelSelection;
        operation.pixelSelectionOp = *pixelOperation;
        layer.strokes.append(std::move(operation));

        QElapsedTimer timer;
        timer.start();
        SelectionVisibility::EditableStrokeStats stats;
        const QVector<QUuid> ids = SelectionVisibility::editableStrokeIds(
            document, layer, selection, 0, &stats);
        const qint64 elapsed = timer.elapsed();

        qInfo("19999-stroke pixel-selection lookup took %lld ms, %llu "
              "operations, %llu effects, max image %llu bytes",
            static_cast<long long>(elapsed),
            static_cast<unsigned long long>(
                stats.pixelSelectionOperationsReplayed),
            static_cast<unsigned long long>(stats.effectCandidatesExamined),
            static_cast<unsigned long long>(stats.maximumExplicitImageBytes));
        QCOMPARE(ids, expectedIds);
        QCOMPARE(stats.fullCanvasFallbacks, 0ULL);
        QCOMPARE(stats.regionalRenders, quint64(strokeCount));
        QCOMPARE(stats.pixelSelectionOperationsReplayed, quint64(strokeCount));
        QCOMPARE(stats.reframeOperationsReplayed, 0ULL);
        QCOMPARE(stats.effectCandidatesExamined, quint64(strokeCount));
        QVERIFY(stats.maximumExplicitImageBytes < 1024ULL * 1024ULL);

        Document reframeDocument = Document::createDefault(QSize(edge, edge));
        reframeDocument.background = Qt::transparent;
        reframeDocument.wobbleAmount = 0.0;
        Layer &reframeLayer = reframeDocument.layers.first();
        reframeLayer.initialCanvasSize = QSize(edge - 1, edge - 1);
        reframeLayer.strokes = layer.strokes;
        reframeLayer.strokes.removeLast();
        Stroke reframe;
        reframe.mode = StrokeMode::Reframe;
        reframe.reframeOp = ReframeOp{ReframeMode::Image,
            SamplingMode::Smooth,
            QSize(edge - 1, edge - 1),
            reframeDocument.size,
            {}};
        reframeLayer.strokes.append(std::move(reframe));

        timer.restart();
        SelectionVisibility::EditableStrokeStats reframeStats;
        const QVector<QUuid> reframeIds =
            SelectionVisibility::editableStrokeIds(
                reframeDocument, reframeLayer, selection, 0, &reframeStats);
        const qint64 reframeElapsed = timer.elapsed();

        qInfo("19999-stroke image-reframe lookup took %lld ms, %llu "
              "operations, %llu effects, max image %llu bytes",
            static_cast<long long>(reframeElapsed),
            static_cast<unsigned long long>(
                reframeStats.reframeOperationsReplayed),
            static_cast<unsigned long long>(
                reframeStats.effectCandidatesExamined),
            static_cast<unsigned long long>(
                reframeStats.maximumExplicitImageBytes));
        QCOMPARE(reframeIds, expectedIds);
        QCOMPARE(reframeStats.fullCanvasFallbacks, 0ULL);
        QCOMPARE(reframeStats.regionalRenders, quint64(strokeCount));
        QCOMPARE(reframeStats.pixelSelectionOperationsReplayed, 0ULL);
        QCOMPARE(reframeStats.reframeOperationsReplayed, quint64(strokeCount));
        QCOMPARE(reframeStats.effectCandidatesExamined, quint64(strokeCount));
        QVERIFY(reframeStats.maximumExplicitImageBytes < 1024ULL * 1024ULL);
    }

    void benchmarksMixedPaintAndEraseCoverageEffects()
    {
        constexpr int edge = DocumentLimits::maximumCanvasEdge;
        constexpr int paintCount = 10000;
        constexpr int eraseCount = 10000;
        Document document = Document::createDefault(QSize(edge, edge));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        layer.strokes.reserve(paintCount + eraseCount);
        for (int index = 0; index < paintCount; ++index)
        {
            const int row = index / 100;
            layer.strokes.append(makeStroke(StrokeMode::Paint,
                Qt::black,
                2.0,
                static_cast<quint64>(index) + 1,
                {QPointF(32.0 + index % 100 * 16,
                    32.0 + static_cast<qreal>(row) * 16)}));
        }
        for (int index = 0; index < eraseCount; ++index)
        {
            const int row = index / 100;
            layer.strokes.append(makeStroke(StrokeMode::Erase,
                Qt::black,
                2.0,
                static_cast<quint64>(paintCount) + static_cast<quint64>(index)
                    + 1,
                {QPointF(2400.0 + index % 100 * 16,
                    32.0 + static_cast<qreal>(row) * 16)}));
        }
        QImage selection(document.size, QImage::Format_Grayscale8);
        selection.fill(255);

        QElapsedTimer timer;
        timer.start();
        SelectionVisibility::EditableStrokeStats stats;
        const QVector<QUuid> ids = SelectionVisibility::editableStrokeIds(
            document, layer, selection, 0, &stats);
        const qint64 elapsed = timer.elapsed();

        qInfo("10000-paint/10000-erase lookup took %lld ms, %llu erases, "
              "%llu effects, max image %llu bytes",
            static_cast<long long>(elapsed),
            static_cast<unsigned long long>(stats.eraseOperationsReplayed),
            static_cast<unsigned long long>(stats.effectCandidatesExamined),
            static_cast<unsigned long long>(stats.maximumExplicitImageBytes));
        QCOMPARE(ids.size(), paintCount + eraseCount);
        QCOMPARE(stats.fullCanvasFallbacks, 0ULL);
        QCOMPARE(stats.regionalRenders, quint64(paintCount + eraseCount));
        QVERIFY(stats.eraseOperationsReplayed < 1000000ULL);
        QVERIFY(stats.effectCandidatesExamined < 2000000ULL);

        Document pixelDocument = Document::createDefault(QSize(edge, edge));
        pixelDocument.background = Qt::transparent;
        pixelDocument.wobbleAmount = 0.0;
        Layer &pixelLayer = pixelDocument.layers.first();
        pixelLayer.strokes.reserve(static_cast<qsizetype>(paintCount) * 2);
        QVector<QUuid> expectedIds;
        expectedIds.reserve(paintCount);
        for (int index = 0; index < paintCount; ++index)
        {
            const int row = index / 100;
            Stroke paint = makeStroke(StrokeMode::Paint,
                Qt::black,
                2.0,
                static_cast<quint64>(index) + 1,
                {QPointF(32.0 + index % 100 * 16,
                    32.0 + static_cast<qreal>(row) * 16)});
            expectedIds.append(paint.id);
            pixelLayer.strokes.append(std::move(paint));
            PixelSelectionOp pixelOperation;
            pixelOperation.canvasSize = pixelDocument.size;
            pixelOperation.sourceBounds =
                QRect(2400 + index % 100 * 16, 32 + index / 100 * 16, 1, 1);
            pixelOperation.packedMask =
                QByteArray(1, std::bit_cast<char>(quint8{0x80}));
            pixelOperation.sampling = SamplingMode::Nearest;
            QVERIFY(isValidPixelSelectionOp(pixelOperation));
            Stroke operation;
            operation.mode = StrokeMode::PixelSelection;
            operation.pixelSelectionOp = std::move(pixelOperation);
            pixelLayer.strokes.append(std::move(operation));
        }

        timer.restart();
        SelectionVisibility::EditableStrokeStats pixelStats;
        const QVector<QUuid> pixelIds = SelectionVisibility::editableStrokeIds(
            pixelDocument, pixelLayer, selection, 0, &pixelStats);
        const qint64 pixelElapsed = timer.elapsed();

        qInfo("10000-paint/10000-pixel-selection lookup took %lld ms, %llu "
              "operations, %llu effects, max image %llu bytes",
            static_cast<long long>(pixelElapsed),
            static_cast<unsigned long long>(
                pixelStats.pixelSelectionOperationsReplayed),
            static_cast<unsigned long long>(
                pixelStats.effectCandidatesExamined),
            static_cast<unsigned long long>(
                pixelStats.maximumExplicitImageBytes));
        QCOMPARE(pixelIds, expectedIds);
        QCOMPARE(pixelStats.fullCanvasFallbacks, 0ULL);
        QCOMPARE(pixelStats.regionalRenders, quint64(paintCount));
        QCOMPARE(pixelStats.pixelSelectionOperationsReplayed, 0ULL);
        QVERIFY(pixelStats.effectCandidatesExamined < 100000ULL);
    }
};

int runStrokeCoverageTests(int argc, char **argv)
{
    StrokeCoverageTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "StrokeCoverageTests.moc"
