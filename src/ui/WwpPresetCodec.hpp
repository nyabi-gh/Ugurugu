// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QByteArray>
#include <QMap>
#include <QString>
#include <QVariant>

#include <optional>

class QSettings;

namespace ugurugu
{

struct WwpPreset
{
    QMap<QString, QVariant> drawingTools;
    qreal wobbleAmount = 0.0;
    MotionSettings motion;
};

class WwpPresetCodec final
{
public:
    static constexpr qint64 maximumBytes = 1024LL * 1024;

    static WwpPreset capture(QSettings &settings, const Document &document);
    static QByteArray encode(const WwpPreset &preset);
    static std::optional<WwpPreset> decode(
        const QByteArray &data, QString *error = nullptr);
    static void applyDrawingTools(const WwpPreset &preset, QSettings &settings);
};

}
