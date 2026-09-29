// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QImage>

namespace ugurugu
{

class ImageAffineTransformer final
{
public:
    static QRect targetBounds(const QRect &sourceBounds,
        const QSize &targetCanvasSize,
        const QTransform &transform,
        SamplingMode sampling);

    static bool compositeSourceOver(QImage &target,
        const QRect &targetImageBounds,
        const QImage &source,
        const QRect &sourceImageBounds,
        const QTransform &transform,
        SamplingMode sampling);

    static QImage transformMask(const QImage &source,
        const QRect &sourceImageBounds,
        const QRect &targetImageBounds,
        const QTransform &transform,
        SamplingMode sampling);
};

}
