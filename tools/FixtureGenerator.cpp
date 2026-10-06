// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Writes the Rust 3.0 comparison fixtures (plan 10.2) in the 2.2.13 format,
// so the current app can be measured on the same content. Fixture 3 is
// ugurugu_stress_document_generator. Each document is saved, loaded back
// through the app's own validation, and described in a manifest next to it.
// This is a one-off measurement tool, not a compatibility target.

#include "document/Document.hpp"
#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"
#include "io/DocumentSerializer.hpp"
#include "io/serializer/RasterAssetTable.hpp"

#include <QColor>
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

QJsonObject manifest(const QString &id, const Document &document, qint64 bytes)
{
    const char *blendNames[] = {"normal", "multiply", "screen", "overlay"};
    QJsonArray layers;
    int strokes = 0;
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
    if (argc != 3)
    {
        std::fprintf(stderr, "usage: %s <1|2|4|5> <output.ugu>\n", argv[0]);
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
    document->activeLayerId = document->layers.first().id;

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
