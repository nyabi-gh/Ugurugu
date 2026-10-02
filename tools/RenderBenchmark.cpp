// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Times whole-animation renders of one document the way playback warmup and
// export drive the engine, and prints a digest so A/B runs can prove the
// pixels did not change. Layers can be pinned still (wobble 0) or padded with
// empty layers to model backgrounds and unused layers in real documents.

#include "io/DocumentSerializer.hpp"
#include "render/RenderEngine.hpp"

#include <QByteArray>
#include <QCryptographicHash>
#include <QElapsedTimer>
#include <QFile>
#include <QImage>
#include <QString>
#include <QStringList>

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <vector>

namespace
{

struct Options
{
    QString path;
    QSize outputSize;
    int staticLayers = 0;
    int emptyLayers = 0;
    int strokesPerLayer = -1;
    int rounds = 3;
    bool nativeExact = false;
    bool editBetweenRounds = false;
    qint64 staticCacheBytes = -1;
};

double percentile(std::vector<double> values, double fraction)
{
    if (values.empty())
    {
        return 0.0;
    }
    std::sort(values.begin(), values.end());
    const auto index = static_cast<std::size_t>(
        fraction * static_cast<double>(values.size() - 1) + 0.5);
    return values[std::min(index, values.size() - 1)];
}

bool parse(int argc, char **argv, Options &options)
{
    for (int index = 1; index < argc; ++index)
    {
        const QString argument = QString::fromLocal8Bit(argv[index]);
        const auto next = [&]() -> QString
        {
            return index + 1 < argc ? QString::fromLocal8Bit(argv[++index])
                                    : QString();
        };
        if (argument == QLatin1String("--size"))
        {
            const QStringList parts = next().split(QLatin1Char('x'));
            if (parts.size() != 2)
            {
                return false;
            }
            options.outputSize = QSize(parts[0].toInt(), parts[1].toInt());
        }
        else if (argument == QLatin1String("--static-layers"))
        {
            options.staticLayers = next().toInt();
        }
        else if (argument == QLatin1String("--empty-layers"))
        {
            options.emptyLayers = next().toInt();
        }
        else if (argument == QLatin1String("--strokes-per-layer"))
        {
            options.strokesPerLayer = next().toInt();
        }
        else if (argument == QLatin1String("--rounds"))
        {
            options.rounds = std::max(1, next().toInt());
        }
        else if (argument == QLatin1String("--exact"))
        {
            options.nativeExact = true;
        }
        else if (argument == QLatin1String("--edit"))
        {
            options.editBetweenRounds = true;
        }
        else if (argument == QLatin1String("--static-cache-mib"))
        {
            options.staticCacheBytes = next().toLongLong() * 1024 * 1024;
        }
        else if (options.path.isEmpty())
        {
            options.path = argument;
        }
        else
        {
            return false;
        }
    }
    return !options.path.isEmpty();
}

}

int main(int argc, char **argv)
{
    Options options;
    if (!parse(argc, argv, options))
    {
        std::fprintf(stderr,
            "usage: %s <document.ugu> [--size WxH] [--static-layers N] "
            "[--empty-layers N] [--strokes-per-layer N] [--rounds N] [--exact] [--edit] "
            "[--static-cache-mib N]\n",
            argv[0]);
        return 2;
    }

    QFile file(options.path);
    if (!file.open(QIODevice::ReadOnly))
    {
        std::fprintf(stderr, "cannot read %s\n", qPrintable(options.path));
        return 1;
    }
    QString error;
    auto loaded = ugurugu::DocumentSerializer::fromJson(file.readAll(), &error);
    if (!loaded)
    {
        std::fprintf(stderr, "open failed: %s\n", qPrintable(error));
        return 1;
    }
    ugurugu::Document document = *loaded;
    if (options.strokesPerLayer >= 0)
    {
        for (ugurugu::Layer &layer : document.layers)
        {
            if (layer.strokes.size() > options.strokesPerLayer)
            {
                layer.strokes.resize(options.strokesPerLayer);
            }
        }
    }
    for (int index = 0;
        index < std::min<int>(options.staticLayers, document.layers.size());
        ++index)
    {
        document.layers[index].wobbleAmount = 0.0;
    }
    for (int index = 0; index < options.emptyLayers; ++index)
    {
        ugurugu::Layer layer;
        layer.name = QStringLiteral("Empty %1").arg(index + 1);
        layer.initialCanvasSize = document.size;
        document.layers.append(layer);
    }
    if (options.staticCacheBytes >= 0)
    {
        ugurugu::RenderEngine::setStaticLayerCacheBudget(
            options.staticCacheBytes);
    }
    const QSize outputSize =
        options.outputSize.isValid() ? options.outputSize : document.size;
    const auto mode = options.nativeExact
                          ? ugurugu::RenderEngine::ScaledRenderMode::NativeExact
                          : ugurugu::RenderEngine::ScaledRenderMode::
                                DisplayPreview;
    const int frameCount = std::max(1, document.animationFrames);

    std::printf("document %dx%d layers=%lld frames=%d output=%dx%d mode=%s "
                "static=%d empty=%d\n",
        document.size.width(),
        document.size.height(),
        static_cast<long long>(document.layers.size()),
        frameCount,
        outputSize.width(),
        outputSize.height(),
        options.nativeExact ? "exact" : "preview",
        options.staticLayers,
        options.emptyLayers);

    for (int round = 0; round < options.rounds; ++round)
    {
        if (options.editBetweenRounds && round > 0)
        {
            // Commits one short stroke to the topmost paint layer, the way a
            // finished pen stroke invalidates every cached frame.
            for (auto layer = document.layers.rbegin();
                layer != document.layers.rend();
                ++layer)
            {
                if (layer->kind == ugurugu::LayerKind::Paint
                    && !layer->strokes.isEmpty())
                {
                    ugurugu::Stroke stroke = layer->strokes.constLast();
                    stroke.id = QUuid::createUuid();
                    for (ugurugu::StrokePoint &point : stroke.points)
                    {
                        point.position += QPointF(3.0, 2.0);
                    }
                    layer->strokes.append(stroke);
                    break;
                }
            }
        }
        std::vector<double> frameMs;
        QCryptographicHash digest(QCryptographicHash::Sha256);
        QElapsedTimer total;
        total.start();
        quint64 allocations = 0;
        for (int frame = 0; frame < frameCount; ++frame)
        {
            ugurugu::RenderEngine::ScaledRenderStats stats;
            QElapsedTimer timer;
            timer.start();
            const QImage image = ugurugu::RenderEngine::renderScaled(
                document, frame, outputSize, mode, &stats);
            frameMs.push_back(timer.nsecsElapsed() / 1.0e6);
            allocations += stats.hierarchySurfaceAllocations;
            for (int row = 0; row < image.height(); ++row)
            {
                digest.addData(QByteArrayView(
                    reinterpret_cast<const char *>(image.constScanLine(row)),
                    image.width() * 4));
            }
        }
        std::printf("round %d total_ms=%.1f frame_p50=%.2f frame_p95=%.2f "
                    "frame_max=%.2f surface_allocs=%llu digest=%s\n",
            round,
            total.nsecsElapsed() / 1.0e6,
            percentile(frameMs, 0.5),
            percentile(frameMs, 0.95),
            *std::max_element(frameMs.begin(), frameMs.end()),
            static_cast<unsigned long long>(allocations),
            digest.result().toHex().left(16).constData());
    }
    return 0;
}
