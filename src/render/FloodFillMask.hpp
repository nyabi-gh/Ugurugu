// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QImage>

namespace ugurugu::FloodFillMask
{

enum class Comparison
{
    AlphaBoundary,
    Color
};

QImage fromImage(const QImage &image,
    const QPoint &seed,
    Comparison comparison,
    int tolerance);

}
