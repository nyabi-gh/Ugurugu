// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "io/WawaV10Reader.hpp"

#include <QByteArray>
#include <QString>

#include <optional>

namespace ugurugu
{

struct WawaImportSummary
{
    int layers = 0;
    int baseImages = 0;
    int paintStrokes = 0;
    int eraserStrokes = 0;
    int polygonFills = 0;
    int skippedOperations = 0;
    int clampedWidths = 0;
};

struct WawaImportResult
{
    Document document;
    WawaImportSummary summary;
};

class WawaV10Importer final
{
public:
    static std::optional<WawaImportResult> import(
        const QByteArray &data, QString *error = nullptr);
    static std::optional<WawaImportResult> convert(
        const WawaProject &project, QString *error = nullptr);
};

}
