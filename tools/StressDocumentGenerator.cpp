// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Writes a deterministic stress document for the web port measurements: many
// layers and dense strokes near the coordinate spread of a real drawing so
// open time, peak memory, and stroke latency probes exercise realistic load.

#include "document/Document.hpp"
#include "document/DocumentLimits.hpp"
#include "io/DocumentSerializer.hpp"

#include <QByteArray>
#include <QColor>
#include <QFile>
#include <QSize>
#include <QString>
#include <QUuid>

#include <cmath>
#include <cstdio>
#include <cstdlib>

namespace
{

constexpr int layerCount = 4;
constexpr int strokesPerLayer = 500;
constexpr int pointsPerStroke = 100;

// Deterministic output requires stable identities and seeds across runs, so
// UUIDs are derived from names and randomness comes from this generator.
quint64 nextRandom(quint64 &state)
{
    state = state * 6364136223846793005ULL + 1442695040888963407ULL;
    return state >> 16;
}

QUuid namedUuid(const QString &name)
{
    static const QUuid nameSpace = QUuid::fromString(
        QStringLiteral("b6f9c9a4-7d2e-4d38-9a4f-2f6f4c1a7b53"));
    return QUuid::createUuidV5(nameSpace, name);
}

ugurugu::Stroke stressStroke(int layerIndex, int strokeIndex, int edge)
{
    ugurugu::Stroke stroke;
    stroke.id = namedUuid(
        QStringLiteral("stroke-%1-%2").arg(layerIndex).arg(strokeIndex));
    quint64 state = static_cast<quint64>(layerIndex) * 100003ULL
                    + static_cast<quint64>(strokeIndex) * 7919ULL + 1ULL;
    stroke.seed = nextRandom(state);
    stroke.width = 2.0 + static_cast<qreal>(nextRandom(state) % 220) / 10.0;
    stroke.color = QColor(static_cast<int>(nextRandom(state) % 256),
        static_cast<int>(nextRandom(state) % 256),
        static_cast<int>(nextRandom(state) % 256),
        200 + static_cast<int>(nextRandom(state) % 56));
    if (strokeIndex % 11 == 10)
    {
        stroke.mode = ugurugu::StrokeMode::Erase;
    }
    if (strokeIndex % 7 == 3)
    {
        stroke.brush.engine = ugurugu::BrushEngine::Airbrush;
    }
    else if (strokeIndex % 7 == 5)
    {
        stroke.brush.engine = ugurugu::BrushEngine::Spray;
    }

    const qreal span = static_cast<qreal>(edge);
    const qreal originX = static_cast<qreal>(nextRandom(state) % 1000) / 1000.0;
    const qreal originY = static_cast<qreal>(nextRandom(state) % 1000) / 1000.0;
    const qreal radius =
        span * (0.05 + static_cast<qreal>(nextRandom(state) % 300) / 1000.0);
    const qreal turns =
        1.0 + static_cast<qreal>(nextRandom(state) % 300) / 100.0;
    stroke.points.reserve(pointsPerStroke);
    for (int index = 0; index < pointsPerStroke; ++index)
    {
        const qreal progress =
            static_cast<qreal>(index) / (pointsPerStroke - 1);
        const qreal angle = progress * turns * 6.283185307179586;
        const qreal wobble = radius * progress;
        qreal x = originX * span + std::cos(angle) * wobble;
        qreal y = originY * span + std::sin(angle) * wobble;
        x = std::clamp(x, 0.0, span - 1.0);
        y = std::clamp(y, 0.0, span - 1.0);
        const qreal pressure =
            0.35 + 0.65 * std::sin(progress * 3.141592653589793);
        stroke.points.append(ugurugu::StrokePoint{QPointF(x, y), pressure});
    }
    return stroke;
}

}

int main(int argc, char **argv)
{
    if (argc != 3)
    {
        std::fprintf(stderr, "usage: %s <canvas-edge> <output.ugu>\n", argv[0]);
        return 2;
    }

    char *edgeEnd = nullptr;
    const long parsedEdge = std::strtol(argv[1], &edgeEnd, 10);
    if (edgeEnd == argv[1] || *edgeEnd != '\0'
        || parsedEdge < ugurugu::DocumentLimits::minimumCanvasEdge
        || parsedEdge > ugurugu::DocumentLimits::maximumCanvasEdge)
    {
        std::fprintf(stderr, "canvas edge out of range: %s\n", argv[1]);
        return 2;
    }
    const int edge = static_cast<int>(parsedEdge);

    ugurugu::Document document;
    document.size = QSize(edge, edge);
    for (int layerIndex = 0; layerIndex < layerCount; ++layerIndex)
    {
        ugurugu::Layer layer;
        layer.id = namedUuid(QStringLiteral("layer-%1").arg(layerIndex));
        layer.name = QStringLiteral("Stress %1").arg(layerIndex + 1);
        layer.initialCanvasSize = document.size;
        layer.strokes.reserve(strokesPerLayer);
        for (int strokeIndex = 0; strokeIndex < strokesPerLayer; ++strokeIndex)
        {
            layer.strokes.append(stressStroke(layerIndex, strokeIndex, edge));
        }
        document.layers.append(std::move(layer));
    }
    document.activeLayerId = document.layers.first().id;

    const QByteArray bytes = ugurugu::DocumentSerializer::toJson(document);
    if (bytes.isEmpty())
    {
        std::fprintf(stderr, "serialization produced no bytes\n");
        return 1;
    }

    QFile output(QString::fromLocal8Bit(argv[2]));
    if (!output.open(QIODevice::WriteOnly | QIODevice::Truncate)
        || output.write(bytes) != bytes.size())
    {
        std::fprintf(stderr, "cannot write %s\n", argv[2]);
        return 1;
    }

    std::printf("%s: %dx%d, %d layers, %d strokes, %d points, %lld bytes\n",
        argv[2],
        edge,
        edge,
        layerCount,
        layerCount * strokesPerLayer,
        layerCount * strokesPerLayer * pointsPerStroke,
        static_cast<long long>(bytes.size()));
    return 0;
}
