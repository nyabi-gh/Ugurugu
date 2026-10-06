// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Writes what the C++ engine makes of a document in a form the Rust port can
// compare against without reading .ugu: a neutral scene with every default
// and layer override resolved, the prepared stroke geometry per frame, the
// rendered frames, each stroke rendered alone, and the stabilizer's response
// to recorded pen traces. Development only; see docs/RUST_PORT_PLAN.md §4.2.

#include "input/StrokeStabilizer.hpp"
#include "io/DocumentSerializer.hpp"
#include "io/serializer/RasterAssetTable.hpp"
#include "render/RenderEngine.hpp"
#include "render/StrokeRenderer.hpp"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QStringList>
#include <QSysInfo>
#include <QTransform>

#include <cstdio>
#include <optional>

namespace
{

using namespace ugurugu;

constexpr int formatVersion = 1;

struct Options
{
    QString command;
    QString input;
    QString outputDirectory;
    QString frames = QStringLiteral("all");
    QSize size;
};

void printUsage(const char *program)
{
    std::fprintf(stderr,
        "usage: %s <command> <input> --out <directory> [--frames all|N,M,...]"
        " [--size WxH]\n"
        "commands:\n"
        "  scene <document>      scene.json\n"
        "  geometry <document>   geometry.jsonl, one prepared stroke per "
        "frame\n"
        "  frames <document>     frames/frame-NNNN.png\n"
        "  strokes <document>    strokes/LNN-SNNNN-fNNNN.png, each stroke "
        "alone\n"
        "  all <document>        all four above\n"
        "  stabilize <trace>     stabilize.json from a pen trace\n",
        program);
}

std::optional<Options> parseOptions(int argc, char **argv)
{
    if (argc < 3)
    {
        return std::nullopt;
    }
    Options options;
    options.command = QString::fromLocal8Bit(argv[1]);
    options.input = QString::fromLocal8Bit(argv[2]);
    for (int index = 3; index < argc; ++index)
    {
        const QString flag = QString::fromLocal8Bit(argv[index]);
        if (index + 1 >= argc)
        {
            return std::nullopt;
        }
        const QString value = QString::fromLocal8Bit(argv[++index]);
        if (flag == QLatin1String("--out"))
        {
            options.outputDirectory = value;
        }
        else if (flag == QLatin1String("--frames"))
        {
            options.frames = value;
        }
        else if (flag == QLatin1String("--size"))
        {
            const QStringList parts = value.split(QLatin1Char('x'));
            bool widthOk = false;
            bool heightOk = false;
            const int width = parts.size() == 2 ? parts[0].toInt(&widthOk) : 0;
            const int height =
                parts.size() == 2 ? parts[1].toInt(&heightOk) : 0;
            if (!widthOk || !heightOk || width <= 0 || height <= 0)
            {
                return std::nullopt;
            }
            options.size = QSize(width, height);
        }
        else
        {
            return std::nullopt;
        }
    }
    if (options.outputDirectory.isEmpty())
    {
        return std::nullopt;
    }
    return options;
}

std::optional<QVector<int>> selectedFrames(
    const QString &selection, int frameCount)
{
    QVector<int> frames;
    if (selection == QLatin1String("all"))
    {
        for (int frame = 0; frame < frameCount; ++frame)
        {
            frames.append(frame);
        }
        return frames;
    }
    for (const QString &part : selection.split(QLatin1Char(',')))
    {
        bool ok = false;
        const int frame = part.toInt(&ok);
        if (!ok || frame < 0 || frame >= frameCount)
        {
            return std::nullopt;
        }
        frames.append(frame);
    }
    return frames;
}

bool writeFile(const QString &path, const QByteArray &bytes)
{
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)
        || file.write(bytes) != bytes.size())
    {
        std::fprintf(stderr, "cannot write %s\n", qPrintable(path));
        return false;
    }
    return true;
}

bool writePng(const QString &path, const QImage &image)
{
    if (!image.convertToFormat(QImage::Format_RGBA8888).save(path, "PNG"))
    {
        std::fprintf(stderr, "cannot write %s\n", qPrintable(path));
        return false;
    }
    return true;
}

QJsonArray sizeJson(const QSize &size)
{
    return {size.width(), size.height()};
}

QJsonArray rectJson(const QRect &rect)
{
    return {rect.x(), rect.y(), rect.width(), rect.height()};
}

QJsonArray colorJson(const QColor &color)
{
    const QRgb rgba = color.rgba();
    return {qRed(rgba), qGreen(rgba), qBlue(rgba), qAlpha(rgba)};
}

// Row-major, Qt's m11 m12 m13 / m21 m22 m23 / m31 m32 m33: a point maps as
// x' = m11 x + m21 y + m31, y' = m12 x + m22 y + m32.
QJsonArray transformJson(const QTransform &transform)
{
    return {transform.m11(),
        transform.m12(),
        transform.m13(),
        transform.m21(),
        transform.m22(),
        transform.m23(),
        transform.m31(),
        transform.m32(),
        transform.m33()};
}

QString samplingName(SamplingMode sampling)
{
    return sampling == SamplingMode::Nearest ? QStringLiteral("nearest")
                                             : QStringLiteral("smooth");
}

// Unpadded rows of the image converted to the given format, base64.
QJsonObject pixelsJson(const QImage &image, QImage::Format format)
{
    const QImage converted = image.convertToFormat(format);
    const int bytesPerPixel = converted.depth() / 8;
    QByteArray rows;
    rows.reserve(
        qsizetype(converted.width()) * bytesPerPixel * converted.height());
    for (int row = 0; row < converted.height(); ++row)
    {
        rows.append(
            reinterpret_cast<const char *>(converted.constScanLine(row)),
            qsizetype(converted.width()) * bytesPerPixel);
    }
    return {{QStringLiteral("size"), sizeJson(converted.size())},
        {QStringLiteral("data"), QString::fromLatin1(rows.toBase64())}};
}

QJsonObject motionJson(const MotionSettings &motion)
{
    static const char *const styles[] = {"classic", "smooth", "stepped"};
    return {{QStringLiteral("style"),
                QLatin1String(styles[static_cast<int>(motion.style)])},
        {QStringLiteral("poseCount"), motion.poseCount},
        {QStringLiteral("detail"), motion.detail},
        {QStringLiteral("linked"), motion.linked},
        {QStringLiteral("randomness"), motion.randomness},
        {QStringLiteral("brokenLine"), motion.brokenLine},
        {QStringLiteral("breakAmount"), motion.breakAmount},
        {QStringLiteral("breakRange"), motion.breakRange}};
}

QJsonObject brushJson(const BrushSettings &brush)
{
    static const char *const engines[] = {"line", "airbrush", "spray"};
    return {{QStringLiteral("engine"),
                QLatin1String(engines[static_cast<int>(brush.engine)])},
        {QStringLiteral("tipShape"),
            brush.tipShape == BrushTipShape::Round ? QStringLiteral("round")
                                                   : QStringLiteral("square")},
        {QStringLiteral("opacity"), brush.opacity},
        {QStringLiteral("flow"), brush.flow},
        {QStringLiteral("hardness"), brush.hardness},
        {QStringLiteral("spacing"), brush.spacing},
        {QStringLiteral("scatter"), brush.scatter},
        {QStringLiteral("particleSize"), brush.particleSize},
        {QStringLiteral("density"), brush.density},
        {QStringLiteral("sizeDynamics"), brush.sizeDynamics},
        {QStringLiteral("opacityDynamics"), brush.opacityDynamics},
        {QStringLiteral("sizeJitter"), brush.sizeJitter},
        {QStringLiteral("animatedJitter"), brush.animatedJitter},
        {QStringLiteral("wobbleScale"), brush.wobbleScale},
        {QStringLiteral("antialiasing"), brush.antialiasing}};
}

QJsonArray pointsJson(const QVector<StrokePoint> &points)
{
    QJsonArray array;
    for (const StrokePoint &point : points)
    {
        array.append(
            QJsonArray{point.position.x(), point.position.y(), point.pressure});
    }
    return array;
}

QJsonObject packedMaskJson(
    const QSize &canvasSize, const QRect &bounds, const QByteArray &packedMask)
{
    return {{QStringLiteral("canvasSize"), sizeJson(canvasSize)},
        {QStringLiteral("bounds"), rectJson(bounds)},
        {QStringLiteral("packedMask"),
            QString::fromLatin1(packedMask.toBase64())}};
}

QJsonObject operationJson(const Stroke &stroke)
{
    static const char *const modes[] = {"paint",
        "erase",
        "fill",
        "image",
        "pixelSelection",
        "reframe",
        "compositeBoundary"};
    QJsonObject object{{QStringLiteral("mode"),
                           QLatin1String(modes[static_cast<int>(stroke.mode)])},
        {QStringLiteral("id"), stroke.id.toString(QUuid::WithoutBraces)},
        // u64 does not survive a JSON number.
        {QStringLiteral("seed"), QString::number(stroke.seed)},
        {QStringLiteral("color"), colorJson(stroke.color)},
        {QStringLiteral("width"), stroke.width},
        {QStringLiteral("brush"), brushJson(stroke.brush)},
        {QStringLiteral("points"), pointsJson(stroke.points)}};
    if (stroke.visibilityClip)
    {
        object.insert(
            QStringLiteral("visibilityClip"), rectJson(*stroke.visibilityClip));
    }
    if (!stroke.clipMask.isNull())
    {
        object.insert(QStringLiteral("clipMask"),
            pixelsJson(stroke.clipMask, QImage::Format_Grayscale8));
    }
    if (!stroke.fillMask.isNull())
    {
        object.insert(QStringLiteral("fillMask"),
            pixelsJson(stroke.fillMask, QImage::Format_Grayscale8));
    }
    if (stroke.fillCoverage)
    {
        object.insert(QStringLiteral("fillCoverage"),
            packedMaskJson(stroke.fillCoverage->canvasSize,
                stroke.fillCoverage->bounds,
                stroke.fillCoverage->packedMask));
    }
    if (stroke.pixelSelectionOp)
    {
        const PixelSelectionOp &op = *stroke.pixelSelectionOp;
        QJsonObject selection =
            packedMaskJson(op.canvasSize, op.sourceBounds, op.packedMask);
        selection.insert(
            QStringLiteral("transform"), transformJson(op.transform));
        selection.insert(QStringLiteral("sampling"), samplingName(op.sampling));
        selection.insert(QStringLiteral("clearSource"), op.clearSource);
        selection.insert(QStringLiteral("drawDestination"), op.drawDestination);
        object.insert(QStringLiteral("pixelSelection"), selection);
    }
    if (stroke.reframeOp)
    {
        const ReframeOp &op = *stroke.reframeOp;
        object.insert(QStringLiteral("reframe"),
            QJsonObject{
                {QStringLiteral("mode"),
                    op.mode == ReframeMode::Canvas ? QStringLiteral("canvas")
                                                   : QStringLiteral("image")},
                {QStringLiteral("sampling"), samplingName(op.sampling)},
                {QStringLiteral("sourceSize"), sizeJson(op.sourceSize)},
                {QStringLiteral("targetSize"), sizeJson(op.targetSize)},
                {QStringLiteral("contentOffset"),
                    QJsonArray{op.contentOffset.x(), op.contentOffset.y()}}});
    }
    if (stroke.imageOp)
    {
        object.insert(QStringLiteral("image"),
            QJsonObject{{QStringLiteral("assetId"), stroke.imageOp->assetId},
                {QStringLiteral("transform"),
                    transformJson(stroke.imageOp->transform)},
                {QStringLiteral("sampling"),
                    samplingName(stroke.imageOp->sampling)}});
    }
    return object;
}

QJsonObject layerJson(const Document &document, const Layer &layer)
{
    static const char *const blends[] = {
        "normal", "multiply", "screen", "overlay"};
    QJsonArray operations;
    for (const Stroke &stroke : layer.strokes)
    {
        operations.append(operationJson(stroke));
    }
    QJsonObject object{
        {QStringLiteral("id"), layer.id.toString(QUuid::WithoutBraces)},
        {QStringLiteral("name"), layer.name},
        {QStringLiteral("kind"),
            layer.kind == LayerKind::Group ? QStringLiteral("group")
                                           : QStringLiteral("paint")},
        {QStringLiteral("clipToBelow"), layer.clipToLayerBelow},
        {QStringLiteral("visible"), layer.visible},
        {QStringLiteral("reference"), layer.reference},
        {QStringLiteral("opacity"), layer.opacity},
        {QStringLiteral("blend"),
            QLatin1String(blends[static_cast<int>(layer.blendMode)])},
        {QStringLiteral("overridesWobble"),
            layer.wobbleAmount.has_value() || layer.motion.has_value()},
        {QStringLiteral("wobbleAmount"),
            effectiveWobbleAmount(document, layer)},
        {QStringLiteral("motion"),
            motionJson(effectiveMotion(document, layer))},
        {QStringLiteral("initialCanvasSize"),
            sizeJson(layer.initialCanvasSize)},
        {QStringLiteral("operations"), operations}};
    if (!layer.parentGroupId.isNull())
    {
        object.insert(QStringLiteral("parent"),
            layer.parentGroupId.toString(QUuid::WithoutBraces));
    }
    return object;
}

QJsonObject sceneJson(const Document &document)
{
    QJsonArray assets;
    for (const RasterAsset &asset : document.rasterAssets)
    {
        const std::optional<QImage> image =
            serializer_detail::decodeRasterAsset(asset);
        QJsonObject object{{QStringLiteral("id"), asset.id}};
        if (image)
        {
            const QJsonObject pixels =
                pixelsJson(*image, QImage::Format_RGBA8888_Premultiplied);
            object.insert(QStringLiteral("size"), pixels[u"size"]);
            object.insert(QStringLiteral("premultipliedRgba"), pixels[u"data"]);
        }
        assets.append(object);
    }
    QJsonArray layers;
    for (const Layer &layer : document.layers)
    {
        layers.append(layerJson(document, layer));
    }
    return {
        {QStringLiteral("format"), QStringLiteral("ugurugu-reference-scene")},
        {QStringLiteral("version"), formatVersion},
        {QStringLiteral("size"), sizeJson(document.size)},
        {QStringLiteral("background"), colorJson(document.background)},
        {QStringLiteral("frames"), document.animationFrames},
        {QStringLiteral("framesPerSecond"), document.framesPerSecond},
        {QStringLiteral("wobbleAmount"), document.wobbleAmount},
        {QStringLiteral("motion"), motionJson(document.motion)},
        {QStringLiteral("assets"), assets},
        {QStringLiteral("layers"), layers}};
}

bool hasPreparedGeometry(const Stroke &stroke)
{
    return stroke.mode == StrokeMode::Paint || stroke.mode == StrokeMode::Erase
           || stroke.mode == StrokeMode::Fill;
}

bool exportScene(const Document &document, const QDir &output)
{
    return writeFile(output.filePath(QStringLiteral("scene.json")),
        QJsonDocument(sceneJson(document)).toJson(QJsonDocument::Indented));
}

bool exportGeometry(
    const Document &document, const QVector<int> &frames, const QDir &output)
{
    QByteArray lines;
    for (int layerIndex = 0; layerIndex < document.layers.size(); ++layerIndex)
    {
        const Layer &layer = document.layers[layerIndex];
        const Document layerDocument = documentForLayer(document, layer);
        for (int strokeIndex = 0; strokeIndex < layer.strokes.size();
            ++strokeIndex)
        {
            const Stroke &stroke = layer.strokes[strokeIndex];
            if (!hasPreparedGeometry(stroke))
            {
                continue;
            }
            for (const int frame : frames)
            {
                const StrokeRenderer::PreparedStroke prepared =
                    StrokeRenderer::prepare(stroke, frame, layerDocument);
                QJsonArray segments;
                for (const quint8 segment : prepared.visibleSegments)
                {
                    segments.append(int(segment));
                }
                const QJsonObject line{{QStringLiteral("layer"), layerIndex},
                    {QStringLiteral("operation"), strokeIndex},
                    {QStringLiteral("frame"), frame},
                    {QStringLiteral("valid"), prepared.valid},
                    {QStringLiteral("normalizedFrame"),
                        prepared.normalizedFrame},
                    {QStringLiteral("width"), prepared.width},
                    {QStringLiteral("variablePressure"),
                        prepared.variablePressure},
                    {QStringLiteral("points"), pointsJson(prepared.points)},
                    {QStringLiteral("visibleSegments"), segments}};
                lines.append(
                    QJsonDocument(line).toJson(QJsonDocument::Compact));
                lines.append('\n');
            }
        }
    }
    return writeFile(output.filePath(QStringLiteral("geometry.jsonl")), lines);
}

QSize outputSize(const Document &document, const Options &options)
{
    return options.size.isValid() ? options.size : document.size;
}

bool exportFrames(const Document &document,
    const QVector<int> &frames,
    const Options &options,
    const QDir &output)
{
    if (!output.mkpath(QStringLiteral("frames")))
    {
        return false;
    }
    const QSize size = outputSize(document, options);
    for (const int frame : frames)
    {
        const QImage image =
            size == document.size
                ? RenderEngine::render(document, frame)
                : RenderEngine::renderScaled(document,
                      frame,
                      size,
                      RenderEngine::ScaledRenderMode::NativeExact);
        const QString name =
            QStringLiteral("frames/frame-%1.png").arg(frame, 4, 10, QChar('0'));
        if (image.isNull() || !writePng(output.filePath(name), image))
        {
            return false;
        }
    }
    return true;
}

bool exportStrokes(const Document &document,
    const QVector<int> &frames,
    const Options &options,
    const QDir &output)
{
    if (!output.mkpath(QStringLiteral("strokes")))
    {
        return false;
    }
    const QSize size = outputSize(document, options);
    for (int layerIndex = 0; layerIndex < document.layers.size(); ++layerIndex)
    {
        const Layer &layer = document.layers[layerIndex];
        const Document layerDocument = documentForLayer(document, layer);
        for (int strokeIndex = 0; strokeIndex < layer.strokes.size();
            ++strokeIndex)
        {
            const Stroke &stroke = layer.strokes[strokeIndex];
            if (stroke.mode != StrokeMode::Paint)
            {
                continue;
            }
            for (const int frame : frames)
            {
                QImage image(size, QImage::Format_ARGB32_Premultiplied);
                image.fill(Qt::transparent);
                if (!RenderEngine::renderStrokesOnLayer(
                        image, layerDocument, {stroke}, frame, size))
                {
                    std::fprintf(stderr,
                        "cannot render layer %d stroke %d frame %d\n",
                        layerIndex,
                        strokeIndex,
                        frame);
                    return false;
                }
                const QString name = QStringLiteral("strokes/L%1-S%2-f%3.png")
                                         .arg(layerIndex, 2, 10, QChar('0'))
                                         .arg(strokeIndex, 4, 10, QChar('0'))
                                         .arg(frame, 4, 10, QChar('0'));
                if (!writePng(output.filePath(name), image))
                {
                    return false;
                }
            }
        }
    }
    return true;
}

// Trace: {"strength": s, "strokes": [[[x, y, timestampMs], ...], ...]}.
// Each stroke runs begin, update for the middle samples, then finish, the
// order the canvas feeds a pen-down, its moves and the pen-up.
bool exportStabilize(const QByteArray &traceBytes, const QDir &output)
{
    QJsonParseError parseError;
    const QJsonDocument trace =
        QJsonDocument::fromJson(traceBytes, &parseError);
    if (parseError.error != QJsonParseError::NoError || !trace.isObject())
    {
        std::fprintf(stderr,
            "invalid trace: %s\n",
            qPrintable(parseError.errorString()));
        return false;
    }
    StrokeStabilizer stabilizer;
    stabilizer.setStrength(trace[u"strength"].toDouble());
    QJsonArray strokes;
    for (const QJsonValue strokeValue : trace[u"strokes"].toArray())
    {
        const QJsonArray samples = strokeValue.toArray();
        QJsonArray filtered;
        stabilizer.reset();
        for (qsizetype index = 0; index < samples.size(); ++index)
        {
            const QJsonArray sample = samples[index].toArray();
            const QPointF position(sample[0].toDouble(), sample[1].toDouble());
            const auto timestamp = quint64(sample[2].toInteger());
            const QPointF result = index == 0
                                       ? stabilizer.begin(position, timestamp)
                                   : index == samples.size() - 1
                                       ? stabilizer.finish(position, timestamp)
                                       : stabilizer.update(position, timestamp);
            filtered.append(QJsonArray{result.x(), result.y()});
        }
        strokes.append(filtered);
    }
    const QJsonObject result{
        {QStringLiteral("strength"), stabilizer.strength()},
        {QStringLiteral("strokes"), strokes}};
    return writeFile(output.filePath(QStringLiteral("stabilize.json")),
        QJsonDocument(result).toJson(QJsonDocument::Indented));
}

// Where the reference came from: the Win-vs-mac tolerance baseline compares
// exports of the same scene across these.
bool exportManifest(const Options &options, const QDir &output)
{
    const QJsonObject manifest{
        {QStringLiteral("format"), QStringLiteral("ugurugu-reference-export")},
        {QStringLiteral("version"), formatVersion},
        {QStringLiteral("command"), options.command},
        {QStringLiteral("input"), QFileInfo(options.input).fileName()},
        {QStringLiteral("frames"), options.frames},
        {QStringLiteral("qt"), QLatin1String(qVersion())},
        {QStringLiteral("os"), QSysInfo::prettyProductName()},
        {QStringLiteral("cpu"), QSysInfo::currentCpuArchitecture()},
        {QStringLiteral("pngAlpha"), QStringLiteral("straight")}};
    return writeFile(output.filePath(QStringLiteral("manifest.json")),
        QJsonDocument(manifest).toJson(QJsonDocument::Indented));
}

}

int main(int argc, char **argv)
{
    const std::optional<Options> options = parseOptions(argc, argv);
    if (!options)
    {
        printUsage(argv[0]);
        return 2;
    }
    QDir output(options->outputDirectory);
    if (!output.mkpath(QStringLiteral(".")))
    {
        std::fprintf(
            stderr, "cannot create %s\n", qPrintable(options->outputDirectory));
        return 1;
    }
    QFile file(options->input);
    if (!file.open(QIODevice::ReadOnly))
    {
        std::fprintf(stderr, "cannot read %s\n", qPrintable(options->input));
        return 1;
    }
    const QByteArray bytes = file.readAll();
    if (!exportManifest(*options, output))
    {
        return 1;
    }
    if (options->command == QLatin1String("stabilize"))
    {
        return exportStabilize(bytes, output) ? 0 : 1;
    }

    QString error;
    const std::optional<Document> document =
        DocumentSerializer::fromJson(bytes, &error);
    if (!document)
    {
        std::fprintf(stderr, "open failed: %s\n", qPrintable(error));
        return 1;
    }
    const std::optional<QVector<int>> frames =
        selectedFrames(options->frames, document->animationFrames);
    if (!frames)
    {
        std::fprintf(
            stderr, "invalid --frames %s\n", qPrintable(options->frames));
        return 2;
    }

    const QString &command = options->command;
    const bool all = command == QLatin1String("all");
    bool ok = true;
    bool known = all;
    if (all || command == QLatin1String("scene"))
    {
        known = true;
        ok = ok && exportScene(*document, output);
    }
    if (all || command == QLatin1String("geometry"))
    {
        known = true;
        ok = ok && exportGeometry(*document, *frames, output);
    }
    if (all || command == QLatin1String("frames"))
    {
        known = true;
        ok = ok && exportFrames(*document, *frames, *options, output);
    }
    if (all || command == QLatin1String("strokes"))
    {
        known = true;
        ok = ok && exportStrokes(*document, *frames, *options, output);
    }
    if (!known)
    {
        printUsage(argv[0]);
        return 2;
    }
    return ok ? 0 : 1;
}
