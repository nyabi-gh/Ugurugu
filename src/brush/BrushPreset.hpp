// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QString>
#include <QVector>

namespace ugurugu
{

enum class BrushCategory
{
    Pen,
    Marker,
    Airbrush,
    Spray
};

struct BrushPreset
{
    QString id;
    BrushCategory category = BrushCategory::Pen;
    const char *sourceName = "";
    BrushSettings settings;
    qreal defaultSize = 6.0;
};

class BrushPresetCatalog final
{
public:
    static const QVector<BrushPreset> &builtIns();
    static const BrushPreset &defaultPreset();
    static const BrushPreset *find(const QString &id);
    static QString displayName(const BrushPreset &preset);
    static QString categoryName(BrushCategory category);
};

}
