// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

#include "document/DocumentLimits.hpp"
#include "document/DocumentOperations.hpp"
#include "document/TextStrokeBuilder.hpp"
#include "wasm/BridgeDocument.hpp"

#include <QRandomGenerator>

#include <cmath>
#include <utility>

using namespace ugurugu::wasm;

extern "C"
{
    // Seven doubles per command: opcode, x, y, x1, y1, x2, y2.
    // 0 move, 1 line, 2 cubic, 3 quadratic, 4 close. This keeps Qt font
    // discovery and QGuiApplication out of the headless worker.
    EMSCRIPTEN_KEEPALIVE int ugu_text_path(BridgeDocument *handle,
        int index,
        const double *commands,
        int count,
        double x,
        double y,
        double width,
        int filled,
        std::uint32_t color)
    {
        const auto &document = handle->controller->document();
        const auto *layer = layerAtIndex(handle, index);
        if (!layer || layer->kind != ugurugu::LayerKind::Paint
            || !ugurugu::DocumentOperations::isLayerRenderable(
                document, *layer))
        {
            setError(StatusLayerNotDrawable,
                QByteArrayLiteral("choose a visible paint layer for text"));
            return 0;
        }
        if (!commands || count <= 0 || count > 32768 * 7 || count % 7 != 0
            || !std::isfinite(x) || !std::isfinite(y) || !std::isfinite(width)
            || x < 0 || y < 0 || x > document.size.width()
            || y > document.size.height() || width < 1 || width > 128)
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("invalid text geometry"));
            return 0;
        }
        QPainterPath path;
        path.setFillRule(Qt::WindingFill);
        bool contourOpen = false;
        for (int offset = 0; offset < count; offset += 7)
        {
            const double *c = commands + offset;
            for (int value = 0; value < 7; ++value)
            {
                if (!std::isfinite(c[value]) || std::abs(c[value]) > 1000000)
                {
                    setError(StatusInvalidArgument,
                        QByteArrayLiteral("invalid text path"));
                    return 0;
                }
            }
            if (c[0] != 0 && !contourOpen)
            {
                setError(StatusInvalidArgument,
                    QByteArrayLiteral("text contour has no start"));
                return 0;
            }
            if (c[0] == 0)
            {
                path.moveTo(c[1], c[2]);
                contourOpen = true;
            }
            else if (c[0] == 1)
            {
                path.lineTo(c[1], c[2]);
            }
            else if (c[0] == 2)
            {
                path.cubicTo(c[3], c[4], c[5], c[6], c[1], c[2]);
            }
            else if (c[0] == 3)
            {
                path.quadTo(c[3], c[4], c[1], c[2]);
            }
            else if (c[0] == 4)
            {
                path.closeSubpath();
                contourOpen = false;
            }
            else
            {
                setError(StatusInvalidArgument,
                    QByteArrayLiteral("unknown text path command"));
                return 0;
            }
        }
        if (!path.boundingRect().translated(x, y).intersects(
                QRectF(QPointF(), document.size)))
        {
            setError(StatusEmptyRegion,
                QByteArrayLiteral("place the text inside the canvas"));
            return 0;
        }
        ugurugu::TextStrokeBuilder::Options options;
        options.anchor = QPointF(x, y);
        options.color = QColor::fromRgba(color);
        options.outlineWidth = width;
        options.brush = handle->brushTemplate.brush;
        options.brush.antialiasing = handle->brushAntialiasing;
        options.filled = filled != 0;
        options.canvasSize = document.size;
        options.baseSeed = QRandomGenerator::global()->generate64();
        auto strokes =
            ugurugu::TextStrokeBuilder::buildFromPath(std::move(path), options);
        if (strokes.isEmpty())
        {
            setError(StatusEmptyRegion,
                QByteArrayLiteral("the text has no drawable outlines"));
            return 0;
        }
        const QUuid layerId = layer->id;
        const qsizetype expectedCount = layer->strokes.size() + strokes.size();
        auto *history = handle->controller->undoStack();
        history->beginMacro(QStringLiteral("Add text"));
        for (auto &stroke : strokes)
        {
            if (selectionAppliesTo(handle, layerId))
            {
                stroke.clipMask = handle->selectionMask;
            }
            const auto result =
                handle->controller->addStroke(layerId, std::move(stroke));
            using Result = ugurugu::DocumentController::AddStrokeResult;
            if (result != Result::Added
                && result != Result::AddedWithResampledPoints)
            {
                // The controller rolls back the entire macro on a rejected
                // stroke.
                break;
            }
        }
        history->endMacro();
        const auto *committed = handle->controller->document().layer(layerId);
        if (!committed || committed->strokes.size() != expectedCount)
        {
            setError(StatusStrokeRejected,
                QByteArrayLiteral(
                    "text exceeds the document budget; use fewer characters"));
            return 0;
        }
        invalidateSplit(handle);
        clearError();
        return 1;
    }

    // Browser codecs supply straight RGBA pixels. Keep the original raster;
    // insertImage fits and centers it using a reversible image operation.
    EMSCRIPTEN_KEEPALIVE int ugu_insert_image(BridgeDocument *handle,
        const std::uint8_t *rgba,
        int size,
        int width,
        int height,
        const char *name)
    {
        if (!rgba || !name || width <= 0 || height <= 0 || width > 4096
            || height > 4096 || qint64(width) * height * 4 != size)
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("invalid image size or pixels"));
            return 0;
        }
        const QImage image(
            rgba, width, height, width * 4, QImage::Format_RGBA8888);
        using Result = ugurugu::DocumentController::InsertImageResult;
        const Result result =
            handle->controller->insertImage(image, QString::fromUtf8(name));
        if (result != Result::Inserted)
        {
            setError(StatusDocumentInvalid,
                result == Result::RejectedLayerLimit
                    ? QByteArrayLiteral("the project layer limit was reached")
                    : QByteArrayLiteral("the image could not fit within the "
                                        "document budget"));
            return 0;
        }
        installSelection(handle, QImage(), QUuid());
        invalidateSplit(handle);
        clearError();
        return 1;
    }
}
