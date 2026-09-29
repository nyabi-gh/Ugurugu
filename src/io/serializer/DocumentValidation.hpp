// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "io/serializer/SerializerSchema.hpp"

#include <QJsonArray>
#include <QString>

namespace ugurugu
{
namespace serializer_detail
{

class RasterAssetTable;

// Totals gathered while validating, so a caller that already validated a
// document does not have to walk it again to learn its budget usage.
struct DocumentValidationStats
{
    qsizetype totalStrokeCount = 0;
    qsizetype totalPointCount = 0;
    quint64 distinctMaskBytes = 0;
};

// Bounds the collection sizes in parsed JSON before anything is materialised,
// so a hostile file cannot force large allocations just by declaring them.
bool validateCollectionBudgets(const QJsonArray &layers, QString *error);

// Full semantic check against DocumentLimits and the layer hierarchy rules.
// Only a document that passes may enter editable state or be written out.
// `fileSchemaVersion` selects the rules that applied when the document was
// written; pass the current schemaVersion for a freshly built document.
bool validateDocument(const Document &document,
    int fileSchemaVersion,
    QString *error,
    DocumentValidationStats *stats = nullptr,
    const RasterAssetTable *validatedRasterAssets = nullptr);

// Fills in the per-layer canvas size that documents written before the field
// existed omit. Run before validation, never after.
void normalizeLayerInitialCanvasSizes(Document &document);

}

}
