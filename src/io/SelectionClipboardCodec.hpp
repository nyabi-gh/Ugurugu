// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QByteArray>
#include <QCoreApplication>
#include <QImage>
#include <QString>
#include <QStringList>

#include <optional>

class QMimeData;

namespace ugurugu
{

// Clipboard payload for a masked layer selection. The payload is a complete
// single-layer document in the regular project JSON schema, so pasting
// reuses the serializer's full validation, mask round-trip and schema
// version checks instead of introducing a second wire format.
class SelectionClipboardCodec
{
    Q_DECLARE_TR_FUNCTIONS(ugurugu::SelectionClipboardCodec)

public:
    struct Copy
    {
        QByteArray payload;
        // Selection-bounded ARGB32 frame for pasting into external apps.
        QImage raster;
        // The same layer the payload serializes, for callers that place the
        // copy into the current document without a clipboard round trip.
        Layer layer;
        QSize canvasSize;
        QMap<QString, RasterAsset> rasterAssets;
    };

    struct Pasted
    {
        QSize canvasSize;
        Layer layer;
        QMap<QString, RasterAsset> rasterAssets;
    };

    static QString mimeType();
    static QStringList legacyMimeTypes();
    // The type to read the payload from, or an empty string when mimeData
    // carries neither the current nor a legacy selection.
    static QString availableMimeType(const QMimeData &mimeData);
    static std::optional<Copy> makeCopy(const Document &document,
        const QUuid &layerId,
        const QImage &selectionMask,
        int frameIndex,
        QString *error = nullptr);
    static std::optional<Pasted> decode(
        const QByteArray &payload, QString *error = nullptr);
};

}
