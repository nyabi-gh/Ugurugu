// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Builds the scene matrix the Rust port's feel is compared on (see
// docs/RUST_PORT_PLAN.md §4.2): every brush preset under four pressure
// profiles, antialiased lines, the motion styles and their settings, airbrush
// accumulation, erasers, and the layer features that change how strokes
// composite. Each scene is built through DocumentController with ids and
// seeds drawn from a scope keyed by its name, so the same build writes the
// same files. Writes <out>/<name>.ugu and <out>/matrix.json.

#include "brush/BrushPreset.hpp"
#include "brush/EraserPreset.hpp"
#include "document/DocumentController.hpp"
#include "document/Identity.hpp"
#include "document/SelectionOperation.hpp"
#include "io/DocumentSerializer.hpp"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>

#include <cmath>
#include <cstdio>
#include <exception>
#include <functional>
#include <numbers>

namespace
{

using namespace ugurugu;

const QSize canvasSize(160, 120);
constexpr int frameCount = 12;
const QColor ink(29, 33, 41);

struct Scene
{
    QString name;
    QString group;
    QJsonObject parameters;
    std::function<bool(DocumentController &)> build;
};

enum class Pressure
{
    Constant,
    Rising,
    Falling,
    Tremor
};

QString pressureName(Pressure pressure)
{
    switch (pressure)
    {
    case Pressure::Constant:
        return QStringLiteral("constant");
    case Pressure::Rising:
        return QStringLiteral("rising");
    case Pressure::Falling:
        return QStringLiteral("falling");
    case Pressure::Tremor:
        return QStringLiteral("tremor");
    }
    return {};
}

qreal pressureAt(Pressure pressure, int index, qreal t)
{
    switch (pressure)
    {
    case Pressure::Constant:
        return 0.75;
    case Pressure::Rising:
        return 0.1 + 0.9 * t;
    case Pressure::Falling:
        return 1.0 - 0.9 * t;
    case Pressure::Tremor:
        return 0.55 + 0.35 * std::sin(index * 0.9);
    }
    return 1.0;
}

quint64 sceneSeed(const QString &name)
{
    quint64 hash = 0xcbf29ce484222325ULL;
    for (const char byte : name.toUtf8())
    {
        hash = (hash ^ quint8(byte)) * 0x100000001b3ULL;
    }
    return hash;
}

// One gentle S across the canvas, sampled about as densely as pen input.
QVector<StrokePoint> wavePoints(Pressure pressure,
    const QPointF &from = QPointF(20, 60),
    const QPointF &to = QPointF(140, 60),
    qreal amplitude = 22.0)
{
    constexpr int count = 80;
    QVector<StrokePoint> points;
    for (int index = 0; index < count; ++index)
    {
        const qreal t = qreal(index) / (count - 1);
        const QPointF along = from + (to - from) * t;
        const QPointF line = to - from;
        const qreal length = std::hypot(line.x(), line.y());
        const QPointF normal(-line.y() / length, line.x() / length);
        const qreal offset =
            amplitude * std::sin(t * 2.0 * std::numbers::pi * 0.8);
        points.append(
            {along + normal * offset, pressureAt(pressure, index, t)});
    }
    return points;
}

Stroke strokeWith(const BrushSettings &brush,
    qreal width,
    QVector<StrokePoint> points,
    const QColor &color = ink,
    StrokeMode mode = StrokeMode::Paint)
{
    Stroke stroke;
    stroke.seed = Identity::newSeed();
    stroke.mode = mode;
    stroke.color = color;
    stroke.width = width;
    stroke.brush = brush;
    stroke.points = std::move(points);
    return stroke;
}

BrushSettings presetBrush(const QString &id)
{
    const BrushPreset *preset = BrushPresetCatalog::find(id);
    return preset ? preset->settings : BrushSettings();
}

bool add(DocumentController &controller, const QUuid &layer, Stroke stroke)
{
    const auto result = controller.addStroke(layer, std::move(stroke));
    return result == DocumentController::AddStrokeResult::Added
           || result
                  == DocumentController::AddStrokeResult::
                      AddedWithResampledPoints;
}

QUuid activeLayer(const DocumentController &controller)
{
    return controller.document().activeLayerId;
}

QUuid addLayerAbove(DocumentController &controller)
{
    controller.addLayer();
    return activeLayer(controller);
}

Stroke boldStroke(const QPointF &from, const QPointF &to, const QColor &color)
{
    return strokeWith(presetBrush(QStringLiteral("bold-ink")),
        18.0,
        wavePoints(Pressure::Constant, from, to, 6.0),
        color);
}

void addBrushScenes(QVector<Scene> &scenes)
{
    const Pressure pressures[] = {Pressure::Constant,
        Pressure::Rising,
        Pressure::Falling,
        Pressure::Tremor};
    for (const BrushPreset &preset : BrushPresetCatalog::builtIns())
    {
        for (const Pressure pressure : pressures)
        {
            scenes.append({QStringLiteral("brush-%1-%2")
                               .arg(preset.id, pressureName(pressure)),
                QStringLiteral("brush"),
                {{QStringLiteral("preset"), preset.id},
                    {QStringLiteral("pressure"), pressureName(pressure)}},
                [preset, pressure](DocumentController &controller)
                {
                    return add(controller,
                        activeLayer(controller),
                        strokeWith(preset.settings,
                            preset.defaultSize,
                            wavePoints(pressure)));
                }});
        }
    }
    for (const BrushPreset &preset : BrushPresetCatalog::builtIns())
    {
        if (preset.settings.engine != BrushEngine::Line)
        {
            continue;
        }
        scenes.append({QStringLiteral("antialiased-%1").arg(preset.id),
            QStringLiteral("antialiased"),
            {{QStringLiteral("preset"), preset.id},
                {QStringLiteral("pressure"), pressureName(Pressure::Tremor)}},
            [preset](DocumentController &controller)
            {
                BrushSettings brush = preset.settings;
                brush.antialiasing = true;
                return add(controller,
                    activeLayer(controller),
                    strokeWith(brush,
                        preset.defaultSize,
                        wavePoints(Pressure::Tremor)));
            }});
    }
}

void addMotionScenes(QVector<Scene> &scenes)
{
    struct Style
    {
        MotionStyle style;
        const char *name;
    };
    const Style styles[] = {{MotionStyle::Classic, "classic"},
        {MotionStyle::Smooth, "smooth"},
        {MotionStyle::Stepped, "stepped"}};
    const qreal amounts[] = {0.8, 1.6, 4.0};
    const auto lineScene = [](qreal amount, MotionSettings motion)
    {
        return [amount, motion](DocumentController &controller)
        {
            if (!controller.applyMotionPreset(amount, motion))
            {
                return false;
            }
            return add(controller,
                activeLayer(controller),
                strokeWith(presetBrush(QStringLiteral("ink-pen")),
                    6.0,
                    wavePoints(Pressure::Constant)));
        };
    };
    for (const Style &style : styles)
    {
        for (const qreal amount : amounts)
        {
            MotionSettings motion;
            motion.style = style.style;
            scenes.append({QStringLiteral("motion-%1-%2")
                               .arg(QLatin1String(style.name))
                               .arg(amount),
                QStringLiteral("motion"),
                {{QStringLiteral("style"), QLatin1String(style.name)},
                    {QStringLiteral("amount"), amount}},
                lineScene(amount, motion)});
        }
    }

    struct Variant
    {
        const char *name;
        std::function<void(MotionSettings &)> apply;
    };
    const Variant variants[] = {
        {"broken-line",
            [](MotionSettings &motion)
            {
                motion.brokenLine = true;
            }},
        {"broken-line-wide",
            [](MotionSettings &motion)
            {
                motion.brokenLine = true;
                motion.breakAmount = 0.8;
                motion.breakRange = 60.0;
            }},
        {"randomness",
            [](MotionSettings &motion)
            {
                motion.randomness = 0.6;
            }},
        {"loosely-linked",
            [](MotionSettings &motion)
            {
                motion.linked = 0.3;
            }},
        {"coarse-detail",
            [](MotionSettings &motion)
            {
                motion.detail = 4;
            }},
        {"fine-detail",
            [](MotionSettings &motion)
            {
                motion.detail = 24;
            }},
        {"three-poses",
            [](MotionSettings &motion)
            {
                motion.poseCount = 3;
            }},
    };
    for (const Style &style : styles)
    {
        for (const Variant &variant : variants)
        {
            MotionSettings motion;
            motion.style = style.style;
            variant.apply(motion);
            scenes.append({QStringLiteral("motion-%1-%2")
                               .arg(QLatin1String(style.name),
                                   QLatin1String(variant.name)),
                QStringLiteral("motion"),
                {{QStringLiteral("style"), QLatin1String(style.name)},
                    {QStringLiteral("variant"), QLatin1String(variant.name)}},
                lineScene(1.6, motion)});
        }
    }
}

// The same dab stacked k times: how low-flow airbrush ink builds up is set by
// the 8-bit compositing, and this is the curve that shows it.
void addAccumulationScenes(QVector<Scene> &scenes)
{
    for (const BrushPreset &preset : BrushPresetCatalog::builtIns())
    {
        if (preset.settings.engine != BrushEngine::Airbrush)
        {
            continue;
        }
        for (const int layers : {1, 2, 4, 8, 16})
        {
            scenes.append({QStringLiteral("accumulation-%1-%2")
                               .arg(preset.id)
                               .arg(layers, 2, 10, QChar('0')),
                QStringLiteral("accumulation"),
                {{QStringLiteral("preset"), preset.id},
                    {QStringLiteral("dabs"), layers}},
                [preset, layers](DocumentController &controller)
                {
                    controller.setWobbleAmount(0.0);
                    for (int index = 0; index < layers; ++index)
                    {
                        if (!add(controller,
                                activeLayer(controller),
                                strokeWith(preset.settings,
                                    preset.defaultSize,
                                    {{QPointF(80, 60), 1.0}})))
                        {
                            return false;
                        }
                    }
                    return true;
                }});
        }
    }
}

void addEraserScenes(QVector<Scene> &scenes)
{
    for (const EraserPreset &preset : EraserPresetCatalog::builtIns())
    {
        for (const Pressure pressure : {Pressure::Constant, Pressure::Tremor})
        {
            scenes.append({QStringLiteral("eraser-%1-%2")
                               .arg(preset.id, pressureName(pressure)),
                QStringLiteral("eraser"),
                {{QStringLiteral("preset"), preset.id},
                    {QStringLiteral("pressure"), pressureName(pressure)}},
                [preset, pressure](DocumentController &controller)
                {
                    const QUuid layer = activeLayer(controller);
                    return add(controller,
                               layer,
                               boldStroke(
                                   QPointF(20, 60), QPointF(140, 60), ink))
                           && add(controller,
                               layer,
                               strokeWith(preset.settings,
                                   preset.defaultSize,
                                   wavePoints(pressure,
                                       QPointF(80, 10),
                                       QPointF(80, 110),
                                       14.0),
                                   ink,
                                   StrokeMode::Erase));
                }});
        }
    }
}

QImage maskWhere(const std::function<bool(qreal, qreal)> &inside)
{
    QImage mask(canvasSize, QImage::Format_Grayscale8);
    for (int y = 0; y < mask.height(); ++y)
    {
        uchar *row = mask.scanLine(y);
        for (int x = 0; x < mask.width(); ++x)
        {
            row[x] = inside(x + 0.5, y + 0.5) ? 255 : 0;
        }
    }
    return mask;
}

QImage rectangleMask(const QRectF &rect)
{
    return maskWhere(
        [rect](qreal x, qreal y)
        {
            return rect.contains(x, y);
        });
}

QImage ellipseMask(const QRectF &rect)
{
    return maskWhere(
        [rect](qreal x, qreal y)
        {
            const qreal dx = (x - rect.center().x()) / (rect.width() / 2);
            const qreal dy = (y - rect.center().y()) / (rect.height() / 2);
            return dx * dx + dy * dy <= 1.0;
        });
}

QImage gradientImage()
{
    QImage image(40, 30, QImage::Format_ARGB32_Premultiplied);
    for (int y = 0; y < image.height(); ++y)
    {
        for (int x = 0; x < image.width(); ++x)
        {
            image.setPixel(
                x, y, qPremultiply(qRgba(x * 6, 255 - y * 8, 120, 160 + x)));
        }
    }
    return image;
}

void addLayerScenes(QVector<Scene> &scenes)
{
    const QColor blue(40, 90, 200);
    const QColor red(210, 60, 50);
    const auto twoLayers =
        [blue, red](const std::function<void(
                DocumentController &, const QUuid &lower, const QUuid &upper)>
                &configure)
    {
        return [blue, red, configure](DocumentController &controller)
        {
            const QUuid lower = activeLayer(controller);
            if (!add(controller,
                    lower,
                    boldStroke(QPointF(20, 60), QPointF(140, 60), blue)))
            {
                return false;
            }
            const QUuid upper = addLayerAbove(controller);
            if (!add(controller,
                    upper,
                    boldStroke(QPointF(80, 10), QPointF(80, 110), red)))
            {
                return false;
            }
            configure(controller, lower, upper);
            return true;
        };
    };

    scenes.append({QStringLiteral("layer-clip"),
        QStringLiteral("layer"),
        {},
        twoLayers(
            [](DocumentController &controller,
                const QUuid &,
                const QUuid &upper)
            {
                controller.setLayerClipToBelow(upper, true);
            })});
    scenes.append({QStringLiteral("layer-opacity"),
        QStringLiteral("layer"),
        {},
        twoLayers(
            [](DocumentController &controller,
                const QUuid &,
                const QUuid &upper)
            {
                controller.setLayerOpacity(upper, 0.4);
            })});
    scenes.append({QStringLiteral("layer-group-opacity"),
        QStringLiteral("layer"),
        {},
        twoLayers(
            [](DocumentController &controller,
                const QUuid &,
                const QUuid &upper)
            {
                controller.addLayerGroup(upper);
                controller.setLayerOpacity(activeLayer(controller), 0.5);
            })});
    const std::pair<LayerBlendMode, const char *> blends[] = {
        {LayerBlendMode::Normal, "normal"},
        {LayerBlendMode::Multiply, "multiply"},
        {LayerBlendMode::Screen, "screen"},
        {LayerBlendMode::Overlay, "overlay"}};
    for (const auto &[mode, name] : blends)
    {
        scenes.append(
            {QStringLiteral("layer-blend-%1").arg(QLatin1String(name)),
                QStringLiteral("layer"),
                {{QStringLiteral("blend"), QLatin1String(name)}},
                twoLayers(
                    [mode](DocumentController &controller,
                        const QUuid &,
                        const QUuid &upper)
                    {
                        controller.setLayerBlendMode(upper, mode);
                    })});
    }
    scenes.append({QStringLiteral("layer-wobble-override"),
        QStringLiteral("layer"),
        {},
        twoLayers(
            [](DocumentController &controller,
                const QUuid &lower,
                const QUuid &)
            {
                MotionSettings still;
                still.style = MotionStyle::Smooth;
                controller.setLayerWobbleOverride(lower, 0.0, still);
            })});
    // Merging a layer that erased its own ink seals each source into a
    // section, so the eraser keeps reaching only what it reached before.
    scenes.append({QStringLiteral("layer-merged-sections"),
        QStringLiteral("layer"),
        {},
        twoLayers(
            [](DocumentController &controller,
                const QUuid &,
                const QUuid &upper)
            {
                const EraserPreset &eraser =
                    EraserPresetCatalog::defaultPreset();
                add(controller,
                    upper,
                    strokeWith(eraser.settings,
                        14.0,
                        wavePoints(Pressure::Constant,
                            QPointF(20, 40),
                            QPointF(140, 80),
                            4.0),
                        ink,
                        StrokeMode::Erase));
                controller.mergeLayerDown(upper);
            })});
    scenes.append({QStringLiteral("operation-reframe"),
        QStringLiteral("operation"),
        {},
        [blue, red](DocumentController &controller)
        {
            const QUuid layer = activeLayer(controller);
            return add(controller,
                       layer,
                       boldStroke(QPointF(20, 60), QPointF(140, 60), blue))
                   && controller.resizeCanvas(canvasSize, QPoint(-24, 12))
                   && add(controller,
                       layer,
                       boldStroke(QPointF(80, 10), QPointF(80, 110), red));
        }});
    scenes.append({QStringLiteral("operation-selection-transform"),
        QStringLiteral("operation"),
        {},
        [blue](DocumentController &controller)
        {
            const QUuid layer = activeLayer(controller);
            if (!add(controller,
                    layer,
                    boldStroke(QPointF(20, 60), QPointF(140, 60), blue)))
            {
                return false;
            }
            QVector<QUuid> strokes;
            for (const Stroke &stroke :
                controller.document().layer(layer)->strokes)
            {
                strokes.append(stroke.id);
            }
            QTransform transform;
            transform.translate(70, 50);
            transform.rotate(20);
            transform.translate(-60, -50);
            return controller.transformSelection(layer,
                strokes,
                transform,
                rectangleMask(QRect(30, 30, 60, 50)),
                SamplingMode::Smooth);
        }});
    scenes.append({QStringLiteral("operation-image"),
        QStringLiteral("operation"),
        {},
        [blue](DocumentController &controller)
        {
            if (!add(controller,
                    activeLayer(controller),
                    boldStroke(QPointF(20, 60), QPointF(140, 60), blue)))
            {
                return false;
            }
            if (controller.insertImage(gradientImage(), QStringLiteral("g.png"))
                != DocumentController::InsertImageResult::Inserted)
            {
                return false;
            }
            const QUuid layer = activeLayer(controller);
            const Layer *imageLayer = controller.document().layer(layer);
            if (!imageLayer || imageLayer->strokes.isEmpty())
            {
                return false;
            }
            QTransform transform;
            transform.translate(50, 30);
            transform.rotate(-15);
            transform.scale(1.5, 1.5);
            return controller.setImageTransform(layer,
                imageLayer->strokes.last().id,
                transform,
                SamplingMode::Smooth);
        }});
    scenes.append({QStringLiteral("operation-fill"),
        QStringLiteral("operation"),
        {},
        [blue](DocumentController &controller)
        {
            const std::optional<PackedMaskRegion> coverage =
                packBinaryMask(ellipseMask(QRect(40, 25, 80, 70)));
            if (!coverage)
            {
                return false;
            }
            Stroke fill = strokeWith(presetBrush(QStringLiteral("ink-pen")),
                6.0,
                {{QPointF(coverage->bounds.center()), 1.0}},
                blue,
                StrokeMode::Fill);
            fill.fillCoverage = *coverage;
            return add(controller, activeLayer(controller), std::move(fill))
                   && add(controller,
                       activeLayer(controller),
                       strokeWith(presetBrush(QStringLiteral("ink-pen")),
                           4.0,
                           wavePoints(Pressure::Constant)));
        }});
}

QJsonObject sceneEntry(const Scene &scene)
{
    QJsonObject entry = scene.parameters;
    entry.insert(QStringLiteral("name"), scene.name);
    entry.insert(QStringLiteral("group"), scene.group);
    return entry;
}

bool writeScene(const Scene &scene, const QDir &output)
{
    const Identity::DeterministicScope identity(sceneSeed(scene.name));
    DocumentController controller;
    QString error;
    if (!controller.newDocument(canvasSize, &error))
    {
        std::fprintf(stderr,
            "%s: new document failed: %s\n",
            qPrintable(scene.name),
            qPrintable(error));
        return false;
    }
    controller.setAnimationFrames(frameCount);
    controller.setFramesPerSecond(frameCount);
    if (!scene.build(controller))
    {
        std::fprintf(
            stderr, "%s: an edit was rejected\n", qPrintable(scene.name));
        return false;
    }
    const QString path = output.filePath(scene.name + QStringLiteral(".ugu"));
    if (!controller.saveDocument(path, &error))
    {
        std::fprintf(stderr,
            "%s: save failed: %s\n",
            qPrintable(scene.name),
            qPrintable(error));
        return false;
    }
    QFile written(path);
    if (!written.open(QIODevice::ReadOnly)
        || !DocumentSerializer::fromJson(written.readAll(), &error))
    {
        std::fprintf(stderr,
            "%s: does not reopen: %s\n",
            qPrintable(scene.name),
            qPrintable(error));
        return false;
    }
    return true;
}

int run(int argc, char **argv)
{
    if (argc != 2)
    {
        std::fprintf(stderr, "usage: %s <output directory>\n", argv[0]);
        return 2;
    }
    QDir output(QString::fromLocal8Bit(argv[1]));
    if (!output.mkpath(QStringLiteral(".")))
    {
        std::fprintf(stderr, "cannot create %s\n", argv[1]);
        return 1;
    }

    QVector<Scene> scenes;
    addBrushScenes(scenes);
    addMotionScenes(scenes);
    addAccumulationScenes(scenes);
    addEraserScenes(scenes);
    addLayerScenes(scenes);

    QJsonArray entries;
    for (const Scene &scene : scenes)
    {
        if (!writeScene(scene, output))
        {
            return 1;
        }
        entries.append(sceneEntry(scene));
    }
    const QJsonObject matrix{
        {QStringLiteral("format"), QStringLiteral("ugurugu-reference-matrix")},
        {QStringLiteral("version"), 1},
        {QStringLiteral("canvasSize"),
            QJsonArray{canvasSize.width(), canvasSize.height()}},
        {QStringLiteral("frames"), frameCount},
        {QStringLiteral("scenes"), entries}};
    QFile file(output.filePath(QStringLiteral("matrix.json")));
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)
        || file.write(QJsonDocument(matrix).toJson(QJsonDocument::Indented))
               < 0)
    {
        std::fprintf(stderr, "cannot write matrix.json\n");
        return 1;
    }
    std::printf("%lld scenes\n", static_cast<long long>(scenes.size()));
    return 0;
}

}

int main(int argc, char **argv)
{
    try
    {
        return run(argc, argv);
    }
    catch (const std::exception &exception)
    {
        std::fprintf(stderr, "error: %s\n", exception.what());
    }
    catch (...)
    {
        std::fprintf(stderr, "error: unknown exception\n");
    }
    return 1;
}
