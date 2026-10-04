// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QString>
#include <QStringList>

namespace ugurugu::ImageImportFormats
{

// The suffixes Insert image offers: the formats Ugurugu means to read,
// limited to those an image plugin in this installation can decode, so a
// package missing a plugin stops offering its format.
QStringList suffixes();

// The suffixes as a file dialog name filter pattern, "*.png *.jpg …".
QString nameFilterPattern();

}
