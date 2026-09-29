// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QImage>

namespace ugurugu
{

class ImageResampler
{
public:
    static QImage resample(
        const QImage &source, const QSize &targetSize, SamplingMode sampling);
    static QImage resampleRegion(const QImage &source,
        const QRect &sourceBounds,
        const QSize &sourceCanvasSize,
        const QRect &targetBounds,
        const QSize &targetCanvasSize,
        SamplingMode sampling);
};

}
