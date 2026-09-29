// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QByteArray>
#include <QCoreApplication>

#include <cstddef>
#include <cstdint>

namespace ugurugu::fuzzing
{

// Qt resolves image format plugins and locale data through the application
// instance, so one is installed before the first input runs.
inline void initialize(int *argc, char ***argv)
{
    static QCoreApplication application(*argc, *argv);
}

// Copies rather than wrapping the libFuzzer buffer: the parsers may rely on
// QByteArray::constData() being null-terminated, which fromRawData would not
// provide, and a spurious overread would only mask real findings.
inline QByteArray toByteArray(const uint8_t *data, size_t size)
{
    return QByteArray(
        reinterpret_cast<const char *>(data), static_cast<qsizetype>(size));
}

}
