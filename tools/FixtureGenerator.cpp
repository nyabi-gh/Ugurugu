// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Writes the Rust 3.0 comparison fixtures (plan 10.2) in the 2.2.13 format,
// so the current app can be measured on the same content. Fixture 3 is
// ugurugu_stress_document_generator. Each document is saved, loaded back
// through the app's own validation, and described in a manifest next to it.
// The named fixtures cover every schema 13 feature the 3.0 import maps or
// refuses (docs/rust/ugu-import.md); `frames` renders a document's frames
// with 2.2.13's exact renderer for the comparison.

#include "brush/BrushPreset.hpp"
#include "brush/EraserPreset.hpp"
#include "document/Document.hpp"
#include "document/DocumentController.hpp"
#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"
#include "io/DocumentSerializer.hpp"
#include "io/serializer/RasterAssetTable.hpp"
#include "render/RenderEngine.hpp"

#include <QColor>
#include <QCoreApplication>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QPainter>
#include <QString>
#include <QTransform>
#include <QUuid>

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <functional>
#include <vector>

namespace
{

using namespace ugurugu;

constexpr qreal tau = 6.283185307179586;

struct Random
{
    quint64 state;

    quint64 next()
    {
        state = state * 6364136223846793005ULL + 1442695040888963407ULL;
        return state >> 16;
    }

    qreal unit()
    {
        return static_cast<qreal>(next() % 1000000) / 1000000.0;
    }

    qreal range(qreal low, qreal high)
    {
        return low + (high - low) * unit();
    }
};

QUuid namedUuid(const QString &name)
{
    static const QUuid nameSpace = QUuid::fromString(
        QStringLiteral("0d6f3d2e-3a43-4b8e-9d55-4c6a7f2b9e11"));
    return QUuid::createUuidV5(nameSpace, name);
}

struct StrokeShape
{
    BrushEngine engine = BrushEngine::Line;
    StrokeMode mode = StrokeMode::Paint;
    int points = 60;
    qreal minimumWidth = 2.0;
    qreal maximumWidth = 16.0;
    int minimumAlpha = 160;
    // Path length relative to the canvas edge.
    qreal reach = 0.25;
};

// A wandering stroke whose pressure rises and falls like a hand-drawn one.
Stroke makeStroke(const QString &name, QSize canvas, const StrokeShape &shape)
{
    Random random{qHash(name) * 2654435761ULL + 1ULL};
    Stroke stroke;
    stroke.id = namedUuid(name);
    stroke.seed = random.next();
    stroke.mode = shape.mode;
    stroke.brush.engine = shape.engine;
    stroke.width = random.range(shape.minimumWidth, shape.maximumWidth);
    stroke.color = QColor(static_cast<int>(random.next() % 256),
        static_cast<int>(random.next() % 256),
        static_cast<int>(random.next() % 256),
        shape.minimumAlpha
            + static_cast<int>(random.next() % (256 - shape.minimumAlpha)));
    const qreal edge = std::min(canvas.width(), canvas.height());
    const qreal step = edge * shape.reach / std::max(1, shape.points - 1);
    QPointF position(random.range(0.05, 0.95) * canvas.width(),
        random.range(0.05, 0.95) * canvas.height());
    qreal heading = random.range(0.0, tau);
    stroke.points.reserve(shape.points);
    for (int index = 0; index < shape.points; ++index)
    {
        heading += random.range(-0.3, 0.3);
        position += QPointF(std::cos(heading), std::sin(heading)) * step;
        position.setX(std::clamp(position.x(), 0.0, canvas.width() - 1.0));
        position.setY(std::clamp(position.y(), 0.0, canvas.height() - 1.0));
        const qreal progress =
            shape.points > 1 ? qreal(index) / (shape.points - 1) : 0.5;
        stroke.points.append(
            StrokePoint{position, 0.3 + 0.7 * std::sin(progress * tau / 2.0)});
    }
    return stroke;
}

Layer makeLayer(const QString &name, QSize canvas)
{
    Layer layer;
    layer.id = namedUuid(QStringLiteral("layer:") + name);
    layer.name = name;
    layer.initialCanvasSize = canvas;
    return layer;
}

Layer makeGroup(const QString &name, QSize canvas)
{
    Layer group = makeLayer(name, canvas);
    group.kind = LayerKind::Group;
    return group;
}

void addStrokes(Layer &layer,
    QSize canvas,
    int count,
    const std::function<StrokeShape(int)> &shapeOf)
{
    layer.strokes.reserve(layer.strokes.size() + count);
    for (int index = 0; index < count; ++index)
    {
        layer.strokes.append(
            makeStroke(layer.name + QStringLiteral("/%1").arg(index),
                canvas,
                shapeOf(index)));
    }
}

// Pen, airbrush and spray in a fixed rotation, with an eraser now and then.
StrokeShape mixedShape(int index, int points)
{
    StrokeShape shape;
    shape.points = points;
    switch (index % 10)
    {
    case 6:
    case 7:
        shape.engine = BrushEngine::Airbrush;
        shape.minimumWidth = 12.0;
        shape.maximumWidth = 48.0;
        break;
    case 8:
        shape.engine = BrushEngine::Spray;
        shape.minimumWidth = 16.0;
        shape.maximumWidth = 40.0;
        break;
    case 9:
        shape.mode = StrokeMode::Erase;
        shape.minimumWidth = 8.0;
        shape.maximumWidth = 24.0;
        break;
    default:
        break;
    }
    return shape;
}

Document baseDocument(QSize canvas)
{
    Document document;
    document.size = canvas;
    return document;
}

// 1: one layer of plain pen strokes on a small canvas.
Document simpleStrokes()
{
    const QSize canvas(1024, 1024);
    Document document = baseDocument(canvas);
    Layer layer = makeLayer(QStringLiteral("Simple"), canvas);
    addStrokes(layer,
        canvas,
        300,
        [](int)
        {
            StrokeShape shape;
            shape.points = 60;
            shape.maximumWidth = 12.0;
            return shape;
        });
    document.layers.append(std::move(layer));
    return document;
}

// 2: sixteen layers using every blend mode, opacity, two groups and clipping.
Document mixedWork()
{
    const QSize canvas(2048, 2048);
    Document document = baseDocument(canvas);
    const LayerBlendMode blends[] = {LayerBlendMode::Normal,
        LayerBlendMode::Multiply,
        LayerBlendMode::Screen,
        LayerBlendMode::Overlay};
    const auto paint = [&](int index)
    {
        Layer layer = makeLayer(QStringLiteral("Paint %1").arg(index), canvas);
        layer.blendMode = blends[index % 4];
        layer.opacity = 0.6 + 0.4 * ((index * 37) % 10) / 9.0;
        addStrokes(layer,
            canvas,
            120,
            [](int stroke)
            {
                return mixedShape(stroke, 80);
            });
        return layer;
    };
    // Children come before their group, as DocumentController lays them out.
    Layer groupA = makeGroup(QStringLiteral("Group A"), canvas);
    Layer groupB = makeGroup(QStringLiteral("Group B"), canvas);
    groupB.blendMode = LayerBlendMode::Multiply;
    groupB.opacity = 0.85;
    for (int index = 0; index < 14; ++index)
    {
        Layer layer = paint(index);
        if (index >= 3 && index <= 6)
        {
            layer.parentGroupId = groupA.id;
        }
        if (index >= 9 && index <= 11)
        {
            layer.parentGroupId = groupB.id;
        }
        // Clipped to the layer below within the same parent.
        layer.clipToLayerBelow =
            index == 4 || index == 5 || index == 10 || index == 13;
        document.layers.append(std::move(layer));
        if (index == 6)
        {
            document.layers.append(groupA);
        }
        if (index == 11)
        {
            document.layers.append(groupB);
        }
    }
    return document;
}

// 4: twenty thousand short strokes, the most a document may hold. The long
// undo history is built at run time by drawing them; it is not stored.
Document shortStrokes()
{
    const QSize canvas(2048, 2048);
    Document document = baseDocument(canvas);
    Layer layer = makeLayer(QStringLiteral("Short strokes"), canvas);
    addStrokes(layer,
        canvas,
        DocumentLimits::maximumTotalStrokes,
        [](int)
        {
            StrokeShape shape;
            shape.points = 6;
            shape.reach = 0.01;
            shape.maximumWidth = 8.0;
            return shape;
        });
    document.layers.append(std::move(layer));
    return document;
}

QImage testImage(QSize size)
{
    QImage image(size, QImage::Format_ARGB32);
    for (int y = 0; y < size.height(); ++y)
    {
        auto *row = reinterpret_cast<QRgb *>(image.scanLine(y));
        for (int x = 0; x < size.width(); ++x)
        {
            const int ring = static_cast<int>(
                std::hypot(x - size.width() / 2.0, y - size.height() / 2.0)
                / 24.0);
            row[x] = qRgba((x * 255) / size.width(),
                (y * 255) / size.height(),
                ring % 2 ? 220 : 40,
                ring % 3 ? 255 : 180);
        }
    }
    return image;
}

// 5: the largest canvas with a placed image, a masked selection move and a
// group whose layers clip to its base.
std::optional<Document> imageMaskGroup()
{
    const QSize canvas(
        DocumentLimits::maximumCanvasEdge, DocumentLimits::maximumCanvasEdge);
    Document document = baseDocument(canvas);

    std::optional<RasterAsset> asset =
        serializer_detail::rasterAssetFromImage(testImage(QSize(2048, 2048)));
    if (!asset)
    {
        return std::nullopt;
    }
    Layer image = makeLayer(QStringLiteral("Image"), canvas);
    Stroke place;
    place.id = namedUuid(QStringLiteral("image-op"));
    place.mode = StrokeMode::Image;
    place.points.clear();
    place.imageOp = ImageOp{asset->id,
        QTransform(1.8, 0.2, -0.2, 1.8, 400.0, 100.0),
        SamplingMode::Smooth};
    image.strokes.append(std::move(place));
    document.rasterAssets.insert(asset->id, *asset);
    document.layers.append(std::move(image));

    Layer painted = makeLayer(QStringLiteral("Painted"), canvas);
    addStrokes(painted,
        canvas,
        400,
        [](int stroke)
        {
            StrokeShape shape = mixedShape(stroke, 100);
            shape.reach = 0.2;
            return shape;
        });
    QImage selection(canvas, QImage::Format_Grayscale8);
    selection.fill(0);
    {
        QPainter painter(&selection);
        painter.setRenderHint(QPainter::Antialiasing, false);
        painter.setBrush(Qt::white);
        painter.setPen(Qt::NoPen);
        painter.drawEllipse(QRectF(800, 900, 1400, 1100));
    }
    std::optional<PixelSelectionOp> move = makePixelSelectionOp(
        selection, QTransform::fromTranslate(600.0, -300.0), true, true);
    if (!move)
    {
        return std::nullopt;
    }
    Stroke moveStroke;
    moveStroke.id = namedUuid(QStringLiteral("selection-move"));
    moveStroke.mode = StrokeMode::PixelSelection;
    moveStroke.points.clear();
    moveStroke.pixelSelectionOp = *move;
    painted.strokes.append(std::move(moveStroke));
    document.layers.append(std::move(painted));

    Layer group = makeGroup(QStringLiteral("Clipped group"), canvas);
    group.blendMode = LayerBlendMode::Overlay;
    for (int index = 0; index < 3; ++index)
    {
        Layer layer =
            makeLayer(QStringLiteral("Group layer %1").arg(index), canvas);
        layer.parentGroupId = group.id;
        layer.clipToLayerBelow = index > 0;
        layer.blendMode =
            index == 2 ? LayerBlendMode::Screen : LayerBlendMode::Normal;
        addStrokes(layer,
            canvas,
            150,
            [](int stroke)
            {
                StrokeShape shape = mixedShape(stroke, 100);
                shape.minimumWidth = 20.0;
                shape.maximumWidth = 120.0;
                return shape;
            });
        document.layers.append(std::move(layer));
    }
    document.layers.append(std::move(group));
    return document;
}

QImage maskOf(
    QSize canvas, bool antialiased, const std::function<void(QPainter &)> &draw)
{
    QImage mask(canvas, QImage::Format_Grayscale8);
    mask.fill(0);
    QPainter painter(&mask);
    painter.setRenderHint(QPainter::Antialiasing, antialiased);
    painter.setPen(Qt::NoPen);
    painter.setBrush(Qt::white);
    draw(painter);
    return mask;
}

QVector<QUuid> strokeIds(const Layer &layer)
{
    QVector<QUuid> ids;
    for (const Stroke &stroke : layer.strokes)
    {
        ids.append(stroke.id);
    }
    return ids;
}

// 2.2.13 composites each piece of a translucent line stroke whose pressure
// varies, so the overlaps darken; 3.0 draws it at one opacity (m2-plan
// decision 2). The named fixtures check the import, so such strokes keep one
// pressure there.
void evenTranslucentPressure(Document &document)
{
    for (Layer &layer : document.layers)
    {
        for (Stroke &stroke : layer.strokes)
        {
            if (stroke.brush.engine != BrushEngine::Line
                || (stroke.color.alpha() == 255 && stroke.brush.opacity >= 1.0)
                || stroke.points.isEmpty())
            {
                continue;
            }
            const qreal pressure =
                stroke.points[stroke.points.size() / 2].pressure;
            for (StrokePoint &point : stroke.points)
            {
                point.pressure = pressure;
            }
        }
    }
}

// motion: every brush and eraser preset on a layer that follows the
// document's wobble, beside layers overriding it with each motion style.
Document motionStyles()
{
    const QSize canvas(480, 360);
    Document document = baseDocument(canvas);
    document.animationFrames = 12;
    document.framesPerSecond = 12.0;
    document.wobbleAmount = 2.4;

    Layer presets = makeLayer(QStringLiteral("Presets"), canvas);
    int index = 0;
    for (const BrushPreset &preset : BrushPresetCatalog::builtIns())
    {
        StrokeShape shape;
        shape.points = 40;
        shape.reach = 0.35;
        Stroke stroke =
            makeStroke(QStringLiteral("preset/") + preset.id, canvas, shape);
        stroke.brush = preset.settings;
        stroke.width = std::max(2.0, preset.defaultSize * 1.5);
        switch (index++ % 4)
        {
        case 1:
            stroke.brush.antialiasing = !stroke.brush.antialiasing;
            break;
        case 2:
            stroke.brush.wobbleScale = 0.3;
            break;
        case 3:
            stroke.brush.tipShape = BrushTipShape::Square;
            break;
        default:
            break;
        }
        presets.strokes.append(std::move(stroke));
    }
    for (const EraserPreset &preset : EraserPresetCatalog::builtIns())
    {
        StrokeShape shape;
        shape.points = 30;
        shape.mode = StrokeMode::Erase;
        Stroke stroke =
            makeStroke(QStringLiteral("eraser/") + preset.id, canvas, shape);
        stroke.brush = preset.settings;
        stroke.width = std::max(8.0, preset.defaultSize * 2.0);
        presets.strokes.append(std::move(stroke));
    }
    document.layers.append(std::move(presets));

    struct Override
    {
        const char *name;
        qreal wobble;
        MotionSettings motion;
    };
    MotionSettings smooth;
    smooth.style = MotionStyle::Smooth;
    smooth.poseCount = 5;
    smooth.detail = 9;
    smooth.linked = 0.6;
    smooth.randomness = 0.3;
    MotionSettings stepped;
    stepped.style = MotionStyle::Stepped;
    stepped.poseCount = 4;
    stepped.detail = 20;
    stepped.randomness = 0.8;
    MotionSettings broken;
    broken.brokenLine = true;
    broken.breakAmount = 0.5;
    broken.breakRange = 30.0;
    MotionSettings smoothBroken = smooth;
    smoothBroken.brokenLine = true;
    smoothBroken.breakAmount = 0.8;
    smoothBroken.breakRange = 12.0;
    const Override overrides[] = {{"Smooth", 3.0, smooth},
        {"Stepped", 2.0, stepped},
        {"Broken", 2.4, broken},
        {"Smooth broken", 4.0, smoothBroken},
        {"Still", 0.0, MotionSettings{}}};
    for (const Override &entry : overrides)
    {
        Layer layer = makeLayer(QString::fromLatin1(entry.name), canvas);
        layer.wobbleAmount = entry.wobble;
        layer.motion = entry.motion;
        addStrokes(layer,
            canvas,
            12,
            [](int stroke)
            {
                return mixedShape(stroke, 50);
            });
        document.layers.append(std::move(layer));
    }

    Layer reference = makeLayer(QStringLiteral("Reference"), canvas);
    reference.reference = true;
    addStrokes(reference,
        canvas,
        6,
        [](int)
        {
            return StrokeShape{};
        });
    document.layers.append(std::move(reference));
    Layer hidden = makeLayer(QStringLiteral("Hidden"), canvas);
    hidden.visible = false;
    addStrokes(hidden,
        canvas,
        6,
        [](int)
        {
            return StrokeShape{};
        });
    document.layers.append(std::move(hidden));
    evenTranslucentPressure(document);
    return document;
}

// fill: frozen fills with and without antialiasing, a fill limited by a
// selection, a schema-5 fill mask, and strokes clipped by masks and
// rectangles.
Document fills()
{
    const QSize canvas(320, 240);
    Document document = baseDocument(canvas);
    document.animationFrames = 6;
    document.wobbleAmount = 1.6;

    Layer lines = makeLayer(QStringLiteral("Lines"), canvas);
    addStrokes(lines,
        canvas,
        10,
        [](int)
        {
            StrokeShape shape;
            shape.maximumWidth = 6.0;
            shape.reach = 0.6;
            return shape;
        });
    document.layers.append(std::move(lines));

    Layer filled = makeLayer(QStringLiteral("Fills"), canvas);
    const auto fill = [&](const QString &name,
                          QColor color,
                          bool antialias,
                          const QImage &coverage)
    {
        Stroke stroke;
        stroke.id = namedUuid(name);
        stroke.mode = StrokeMode::Fill;
        stroke.color = color;
        stroke.brush.antialiasing = antialias;
        stroke.points = {
            {QPointF(canvas.width() / 2.0, canvas.height() / 2.0), 1.0}};
        stroke.fillCoverage = packBinaryMask(coverage);
        return stroke;
    };
    filled.strokes.append(fill(QStringLiteral("fill/ellipse"),
        QColor(30, 120, 220, 200),
        false,
        maskOf(canvas,
            false,
            [](QPainter &painter)
            {
                painter.drawEllipse(QRectF(20, 30, 150, 110));
            })));
    filled.strokes.append(fill(QStringLiteral("fill/star"),
        QColor(240, 180, 20),
        true,
        maskOf(canvas,
            false,
            [](QPainter &painter)
            {
                QPolygonF star;
                for (int point = 0; point < 10; ++point)
                {
                    const qreal radius = point % 2 ? 35.0 : 90.0;
                    const qreal angle = tau * point / 10.0;
                    star << QPointF(220 + radius * std::sin(angle),
                        120 - radius * std::cos(angle));
                }
                painter.drawPolygon(star);
            })));
    Stroke limited = fill(QStringLiteral("fill/limited"),
        QColor(200, 40, 90, 160),
        true,
        maskOf(canvas,
            false,
            [](QPainter &painter)
            {
                painter.drawRect(QRect(60, 120, 200, 100));
            }));
    limited.clipMask = maskOf(canvas,
        true,
        [](QPainter &painter)
        {
            painter.drawEllipse(QRectF(90, 100, 150, 130));
        });
    filled.strokes.append(std::move(limited));
    Stroke legacy;
    legacy.id = namedUuid(QStringLiteral("fill/legacy"));
    legacy.mode = StrokeMode::Fill;
    legacy.color = QColor(40, 170, 90, 220);
    legacy.points = {{QPointF(250, 50), 1.0}};
    legacy.fillMask = maskOf(canvas,
        true,
        [](QPainter &painter)
        {
            painter.drawEllipse(QRectF(200, 10, 100, 70));
        });
    filled.strokes.append(std::move(legacy));
    document.layers.append(std::move(filled));

    Layer clipped = makeLayer(QStringLiteral("Clipped"), canvas);
    const QImage circle = maskOf(canvas,
        true,
        [](QPainter &painter)
        {
            painter.drawEllipse(QRectF(40, 20, 180, 180));
        });
    for (int index = 0; index < 8; ++index)
    {
        StrokeShape shape;
        shape.mode = index == 5 ? StrokeMode::Erase : StrokeMode::Paint;
        shape.minimumWidth = 8.0;
        shape.maximumWidth = 24.0;
        shape.reach = 0.7;
        Stroke stroke =
            makeStroke(QStringLiteral("clipped/%1").arg(index), canvas, shape);
        if (index % 3 != 1)
        {
            stroke.clipMask = circle;
        }
        if (index % 3 != 0)
        {
            stroke.visibilityClip = QRect(100 + index * 7, 40, 150, 140);
        }
        clipped.strokes.append(std::move(stroke));
    }
    document.layers.append(std::move(clipped));
    evenTranslucentPressure(document);
    return document;
}

QImage selectionEllipse(QSize canvas, QRectF bounds)
{
    return maskOf(canvas,
        false,
        [bounds](QPainter &painter)
        {
            painter.drawEllipse(bounds);
        });
}

Stroke straightStroke(const QString &name,
    StrokeMode mode,
    QColor color,
    qreal width,
    QPointF from,
    QPointF to)
{
    Stroke stroke;
    stroke.id = namedUuid(name);
    stroke.seed = qHash(name);
    stroke.mode = mode;
    stroke.color = color;
    stroke.width = width;
    for (int index = 0; index <= 20; ++index)
    {
        stroke.points.append(
            StrokePoint{from + (to - from) * (index / 20.0), 1.0});
    }
    return stroke;
}

struct Steps
{
    DocumentController &controller;
    bool ok = true;

    void check(bool result, const char *step)
    {
        if (!result && ok)
        {
            std::fprintf(stderr, "step failed: %s\n", step);
            ok = false;
        }
    }

    void add(const QUuid &layerId, Stroke stroke, const char *step)
    {
        const auto result = controller.addStroke(layerId, std::move(stroke));
        check(result == DocumentController::AddStrokeResult::Added
                  || result
                         == DocumentController::AddStrokeResult::
                             AddedWithResampledPoints,
            step);
    }

    QVector<QUuid> ids(const QUuid &layerId) const
    {
        return strokeIds(*controller.document().layer(layerId));
    }
};

// edit: images, merge boundaries, selection moves that keep or clear their
// source, a cleared selection, a crop and a resize, and a group mixing every
// blend mode with clipping, all made through 2.2.13's own controller.
std::optional<Document> edits()
{
    QSize canvas(360, 270);
    Document document = baseDocument(canvas);
    document.animationFrames = 8;
    document.wobbleAmount = 1.5;

    std::optional<RasterAsset> asset =
        serializer_detail::rasterAssetFromImage(testImage(QSize(120, 90)));
    if (!asset)
    {
        return std::nullopt;
    }
    Layer image = makeLayer(QStringLiteral("Image"), canvas);
    Stroke smooth;
    smooth.id = namedUuid(QStringLiteral("edit/image-smooth"));
    smooth.mode = StrokeMode::Image;
    smooth.points.clear();
    smooth.imageOp = ImageOp{asset->id,
        QTransform(1.3, 0.4, -0.4, 1.3, 120.0, 10.0),
        SamplingMode::Smooth};
    image.strokes.append(std::move(smooth));
    Stroke nearest;
    nearest.id = namedUuid(QStringLiteral("edit/image-nearest"));
    nearest.mode = StrokeMode::Image;
    nearest.points.clear();
    nearest.imageOp = ImageOp{asset->id,
        QTransform(0.7, 0.0, 0.0, 1.6, 10.0, 120.0),
        SamplingMode::Nearest};
    image.strokes.append(std::move(nearest));
    document.rasterAssets.insert(asset->id, *asset);
    document.layers.append(std::move(image));

    const auto paintLayer = [&](const QString &name, int count)
    {
        Layer layer = makeLayer(name, canvas);
        addStrokes(layer,
            canvas,
            count,
            [](int stroke)
            {
                StrokeShape shape = mixedShape(stroke, 40);
                shape.reach = 0.3;
                return shape;
            });
        return layer;
    };
    Layer base = paintLayer(QStringLiteral("Base"), 30);
    Layer upper = paintLayer(QStringLiteral("Upper"), 16);
    Layer upperTwo = paintLayer(QStringLiteral("Upper 2"), 12);
    const QUuid baseId = base.id;
    const QUuid upperId = upper.id;
    const QUuid upperTwoId = upperTwo.id;
    document.layers.append(std::move(base));
    document.layers.append(std::move(upper));
    document.layers.append(std::move(upperTwo));

    Layer group = makeGroup(QStringLiteral("Blend group"), canvas);
    group.blendMode = LayerBlendMode::Overlay;
    group.opacity = 0.9;
    const LayerBlendMode modes[] = {LayerBlendMode::Multiply,
        LayerBlendMode::Screen,
        LayerBlendMode::Normal,
        LayerBlendMode::Overlay};
    QUuid multiplyId;
    QUuid clipId;
    for (int index = 0; index < 4; ++index)
    {
        Layer layer = paintLayer(QStringLiteral("Blend %1").arg(index), 10);
        layer.parentGroupId = group.id;
        layer.blendMode = modes[index];
        layer.clipToLayerBelow = index == 1 || index == 2;
        layer.opacity = index == 2 ? 0.7 : 1.0;
        if (index == 0)
        {
            multiplyId = layer.id;
        }
        if (index == 1)
        {
            clipId = layer.id;
        }
        document.layers.append(std::move(layer));
    }
    document.layers.append(std::move(group));
    Layer top = paintLayer(QStringLiteral("Top"), 10);
    top.blendMode = LayerBlendMode::Screen;
    top.opacity = 0.8;
    document.layers.append(std::move(top));
    document.activeLayerId = baseId;
    evenTranslucentPressure(document);

    DocumentController controller;
    QString error;
    if (!controller.loadDocument(document, &error))
    {
        std::fprintf(stderr, "cannot load: %s\n", qPrintable(error));
        return std::nullopt;
    }
    Steps steps{controller};
    steps.check(controller.duplicateStrokes(baseId,
                    steps.ids(baseId),
                    QPointF(40.0, -25.0),
                    selectionEllipse(canvas, QRectF(60, 90, 120, 90))),
        "duplicate selection (keeps source)");
    steps.check(controller.mergeLayerDown(upperId), "merge Upper");
    steps.check(controller.mergeLayerDown(upperTwoId), "merge Upper 2");
    steps.add(baseId,
        straightStroke(QStringLiteral("edit/after-merge"),
            StrokeMode::Paint,
            QColor(20, 20, 160),
            10.0,
            QPointF(20, 240),
            QPointF(330, 200)),
        "paint after merge");
    steps.add(baseId,
        straightStroke(QStringLiteral("edit/erase-after-merge"),
            StrokeMode::Erase,
            Qt::black,
            14.0,
            QPointF(180, 10),
            QPointF(200, 260)),
        "erase after merge");
    QTransform rotate;
    rotate.translate(180, 135);
    rotate.rotate(20);
    rotate.translate(-170, -125);
    steps.check(controller.transformSelection(baseId,
                    steps.ids(baseId),
                    rotate,
                    maskOf(canvas,
                        false,
                        [](QPainter &painter)
                        {
                            painter.drawRect(QRect(120, 60, 140, 120));
                        }),
                    SamplingMode::Smooth),
        "rotate selection smooth");
    steps.check(controller.removeSelectedContent(baseId,
                    steps.ids(baseId),
                    selectionEllipse(canvas, QRectF(250, 150, 90, 100))),
        "clear selection");
    steps.check(controller.transformSelection(multiplyId,
                    steps.ids(multiplyId),
                    QTransform(1.5, 0.0, 0.0, 1.5, -60.0, -40.0),
                    selectionEllipse(canvas, QRectF(100, 60, 160, 140)),
                    SamplingMode::Nearest),
        "scale selection nearest");
    steps.check(controller.resizeCanvas(QSize(400, 300), QPoint(25, 12)),
        "crop to a larger canvas");
    canvas = controller.document().size;
    steps.add(baseId,
        straightStroke(QStringLiteral("edit/after-crop"),
            StrokeMode::Paint,
            QColor(200, 30, 30, 180),
            8.0,
            QPointF(5, 5),
            QPointF(395, 295)),
        "paint after crop");
    steps.check(controller.resizeImage(QSize(300, 225)), "resize the image");
    canvas = controller.document().size;
    steps.add(baseId,
        straightStroke(QStringLiteral("edit/after-resize"),
            StrokeMode::Paint,
            QColor(10, 140, 60),
            6.0,
            QPointF(10, 200),
            QPointF(290, 30)),
        "paint after resize");
    steps.check(controller.removeSelectedContent(clipId,
                    steps.ids(clipId),
                    selectionEllipse(canvas, QRectF(30, 30, 120, 120))),
        "clear selection on a clipped layer");
    if (!steps.ok)
    {
        return std::nullopt;
    }
    return controller.document();
}

// depth: groups nested to 2.2.13's deepest level, each with its own blend.
Document depth()
{
    const QSize canvas(256, 256);
    Document document = baseDocument(canvas);
    document.animationFrames = 4;
    const LayerBlendMode modes[] = {LayerBlendMode::Normal,
        LayerBlendMode::Multiply,
        LayerBlendMode::Screen,
        LayerBlendMode::Overlay};
    QVector<Layer> groups;
    for (int level = 0; level < DocumentLimits::maximumLayerDepth; ++level)
    {
        Layer group = makeGroup(QStringLiteral("Level %1").arg(level), canvas);
        group.blendMode = modes[level % 4];
        group.opacity = 1.0 - 0.05 * level;
        if (level > 0)
        {
            group.parentGroupId = groups.last().id;
        }
        groups.append(group);
    }
    Layer under = makeLayer(QStringLiteral("Under"), canvas);
    addStrokes(under,
        canvas,
        20,
        [](int)
        {
            StrokeShape shape;
            shape.minimumWidth = 10.0;
            shape.maximumWidth = 30.0;
            return shape;
        });
    document.layers.append(std::move(under));
    for (int level = 0; level < groups.size(); ++level)
    {
        Layer layer = makeLayer(QStringLiteral("Paint %1").arg(level), canvas);
        layer.parentGroupId = groups[level].id;
        layer.blendMode = modes[(level + 1) % 4];
        addStrokes(layer,
            canvas,
            6,
            [](int)
            {
                StrokeShape shape;
                shape.minimumWidth = 10.0;
                shape.maximumWidth = 40.0;
                return shape;
            });
        document.layers.append(std::move(layer));
    }
    // Children come before their group, so the deepest group goes first.
    for (int level = groups.size() - 1; level >= 0; --level)
    {
        document.layers.append(groups[level]);
    }
    evenTranslucentPressure(document);
    return document;
}

// procedural: a fill that floods the layer again on every frame, as files
// first saved before schema 4 still hold. 3.0 refuses it.
Document procedural()
{
    const QSize canvas(160, 120);
    Document document = baseDocument(canvas);
    document.animationFrames = 4;
    document.wobbleAmount = 3.0;
    Layer layer = makeLayer(QStringLiteral("Ring"), canvas);
    Stroke ring;
    ring.id = namedUuid(QStringLiteral("procedural/ring"));
    ring.width = 6.0;
    for (int index = 0; index <= 40; ++index)
    {
        const qreal angle = tau * index / 40.0;
        ring.points.append(StrokePoint{
            QPointF(80 + 50 * std::cos(angle), 60 + 40 * std::sin(angle)),
            1.0});
    }
    layer.strokes.append(std::move(ring));
    Stroke fill;
    fill.id = namedUuid(QStringLiteral("procedural/fill"));
    fill.mode = StrokeMode::Fill;
    fill.color = QColor(250, 200, 40);
    fill.points = {{QPointF(80, 60), 1.0}};
    layer.strokes.append(std::move(fill));
    document.layers.append(std::move(layer));
    return document;
}

// reframe-in-section: a crop between two merge boundaries, which 2.2.13 makes
// by merging, cropping and merging a layer added after the crop. 3.0 refuses
// it.
std::optional<Document> reframeInSection()
{
    const QSize canvas(200, 150);
    Document document = baseDocument(canvas);
    document.animationFrames = 4;
    Layer base = makeLayer(QStringLiteral("Base"), canvas);
    base.strokes.append(straightStroke(QStringLiteral("section/base"),
        StrokeMode::Paint,
        Qt::black,
        8.0,
        QPointF(10, 20),
        QPointF(190, 20)));
    Layer upper = makeLayer(QStringLiteral("Upper"), canvas);
    upper.strokes.append(straightStroke(QStringLiteral("section/upper"),
        StrokeMode::Paint,
        Qt::red,
        8.0,
        QPointF(10, 60),
        QPointF(190, 60)));
    const QUuid baseId = base.id;
    const QUuid upperId = upper.id;
    document.layers.append(std::move(base));
    document.layers.append(std::move(upper));
    document.activeLayerId = baseId;

    DocumentController controller;
    QString error;
    if (!controller.loadDocument(document, &error))
    {
        std::fprintf(stderr, "cannot load: %s\n", qPrintable(error));
        return std::nullopt;
    }
    Steps steps{controller};
    steps.check(controller.mergeLayerDown(upperId), "merge Upper");
    steps.check(
        controller.resizeCanvas(QSize(220, 160), QPoint(10, 5)), "crop");
    controller.addLayer();
    const QUuid addedId = controller.document().activeLayerId;
    steps.check(addedId != baseId, "add a layer");
    steps.add(addedId,
        straightStroke(QStringLiteral("section/added"),
            StrokeMode::Paint,
            Qt::blue,
            8.0,
            QPointF(10, 120),
            QPointF(210, 120)),
        "paint the added layer");
    steps.check(controller.mergeLayerDown(addedId), "merge the added layer");
    if (!steps.ok)
    {
        return std::nullopt;
    }
    return controller.document();
}

// Writes every frame as frame-NN.png with 2.2.13's exact renderer.
int renderFrames(const QString &documentPath, const QString &directory)
{
    // Opened five times; the median is compared with `ugu-doc import`.
    QString error;
    std::optional<Document> document;
    std::vector<double> times;
    for (int round = 0; round < 5; ++round)
    {
        QElapsedTimer timer;
        timer.start();
        document = DocumentSerializer::load(documentPath, &error);
        times.push_back(timer.nsecsElapsed() / 1e6);
    }
    std::sort(times.begin(), times.end());
    std::printf("opened in %.1f ms (median of 5, %.1f..%.1f)\n",
        times[2],
        times[0],
        times[4]);
    if (!document)
    {
        std::fprintf(stderr, "open failed: %s\n", qPrintable(error));
        return 1;
    }
    QDir().mkpath(directory);
    for (int frame = 0; frame < document->animationFrames; ++frame)
    {
        const QImage image = RenderEngine::render(*document, frame)
                                 .convertToFormat(QImage::Format_RGBA8888);
        const QString path = QDir(directory).filePath(
            QStringLiteral("frame-%1.png").arg(frame, 2, 10, QLatin1Char('0')));
        if (!image.save(path))
        {
            std::fprintf(stderr, "cannot write %s\n", qPrintable(path));
            return 1;
        }
    }
    std::printf("%d frames of %dx%d\n",
        document->animationFrames,
        document->size.width(),
        document->size.height());
    return 0;
}

QJsonObject manifest(const QString &id, const Document &document, qint64 bytes)
{
    const char *blendNames[] = {"normal", "multiply", "screen", "overlay"};
    QJsonArray layers;
    qsizetype strokes = 0;
    qsizetype points = 0;
    for (const Layer &layer : document.layers)
    {
        QJsonObject entry{{QStringLiteral("name"), layer.name},
            {QStringLiteral("kind"),
                layer.kind == LayerKind::Group ? QStringLiteral("group")
                                               : QStringLiteral("paint")},
            {QStringLiteral("blend"),
                QString::fromLatin1(
                    blendNames[static_cast<int>(layer.blendMode)])},
            {QStringLiteral("opacity"), layer.opacity},
            {QStringLiteral("clipToLayerBelow"), layer.clipToLayerBelow},
            {QStringLiteral("strokes"), layer.strokes.size()}};
        if (!layer.parentGroupId.isNull())
        {
            entry.insert(QStringLiteral("parent"),
                document.layer(layer.parentGroupId)->name);
        }
        layers.append(entry);
        strokes += layer.strokes.size();
        for (const Stroke &stroke : layer.strokes)
        {
            points += stroke.points.size();
        }
    }
    qint64 assetBytes = 0;
    for (const RasterAsset &asset : document.rasterAssets)
    {
        assetBytes += asset.compressedRgba.size();
    }
    return QJsonObject{{QStringLiteral("fixture"), id},
        {QStringLiteral("format"), QStringLiteral("ugurugu 2.2.13 JSON")},
        {QStringLiteral("canvas"),
            QJsonArray{document.size.width(), document.size.height()}},
        {QStringLiteral("animationFrames"), document.animationFrames},
        {QStringLiteral("framesPerSecond"), document.framesPerSecond},
        {QStringLiteral("wobbleAmount"), document.wobbleAmount},
        {QStringLiteral("layers"), layers},
        {QStringLiteral("strokes"), strokes},
        {QStringLiteral("points"), static_cast<qint64>(points)},
        {QStringLiteral("rasterAssets"), document.rasterAssets.size()},
        {QStringLiteral("rasterAssetBytes"), assetBytes},
        {QStringLiteral("fileBytes"), bytes}};
}

}

int main(int argc, char **argv)
{
    QCoreApplication application(argc, argv);
    if (argc == 4 && std::strcmp(argv[1], "frames") == 0)
    {
        return renderFrames(
            QString::fromLocal8Bit(argv[2]), QString::fromLocal8Bit(argv[3]));
    }
    if (argc != 3)
    {
        std::fprintf(stderr,
            "usage: %s <1|2|4|5|motion|fill|edit|depth|procedural|"
            "reframe-in-section> <output.ugu>\n"
            "       %s frames <document.ugu> <directory>\n",
            argv[0],
            argv[0]);
        return 2;
    }
    const QString id = QString::fromLatin1(argv[1]);
    std::optional<Document> document;
    if (id == QLatin1String("1"))
    {
        document = simpleStrokes();
    }
    else if (id == QLatin1String("2"))
    {
        document = mixedWork();
    }
    else if (id == QLatin1String("4"))
    {
        document = shortStrokes();
    }
    else if (id == QLatin1String("5"))
    {
        document = imageMaskGroup();
    }
    else if (id == QLatin1String("motion"))
    {
        document = motionStyles();
    }
    else if (id == QLatin1String("fill"))
    {
        document = fills();
    }
    else if (id == QLatin1String("edit"))
    {
        document = edits();
    }
    else if (id == QLatin1String("depth"))
    {
        document = depth();
    }
    else if (id == QLatin1String("procedural"))
    {
        document = procedural();
    }
    else if (id == QLatin1String("reframe-in-section"))
    {
        document = reframeInSection();
    }
    else
    {
        std::fprintf(stderr,
            "unknown fixture %s; fixture 3 is "
            "ugurugu_stress_document_generator 2048\n",
            argv[1]);
        return 2;
    }
    if (!document)
    {
        std::fprintf(stderr, "cannot build fixture %s\n", argv[1]);
        return 1;
    }
    if (document->activeLayerId.isNull())
    {
        document->activeLayerId = document->layers.first().id;
    }

    const QString path = QString::fromLocal8Bit(argv[2]);
    QString error;
    if (!DocumentSerializer::save(path, *document, &error))
    {
        std::fprintf(stderr, "cannot save: %s\n", qPrintable(error));
        return 1;
    }
    const std::optional<Document> loaded =
        DocumentSerializer::load(path, &error);
    if (!loaded)
    {
        std::fprintf(
            stderr, "the app rejects the fixture: %s\n", qPrintable(error));
        return 1;
    }

    const qint64 bytes = QFileInfo(path).size();
    const QJsonObject description = manifest(id, *loaded, bytes);
    QFile manifestFile(path + QStringLiteral(".manifest.json"));
    if (!manifestFile.open(QIODevice::WriteOnly | QIODevice::Truncate))
    {
        std::fprintf(stderr, "cannot write the manifest\n");
        return 1;
    }
    manifestFile.write(QJsonDocument(description).toJson());
    std::printf("fixture %s: %dx%d, %lld layers, %d strokes, %lld points, "
                "%lld asset bytes, %lld file bytes\n",
        argv[1],
        loaded->size.width(),
        loaded->size.height(),
        static_cast<long long>(loaded->layers.size()),
        description.value(QStringLiteral("strokes")).toInt(),
        static_cast<long long>(
            description.value(QStringLiteral("points")).toInteger()),
        static_cast<long long>(
            description.value(QStringLiteral("rasterAssetBytes")).toInteger()),
        static_cast<long long>(bytes));
    return 0;
}
