// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QImage>

namespace ugurugu
{

class LayerThumbnailRenderer final
{
public:
    // The band the delegate reserves for a thumbnail, in the coordinates the
    // delegate lays out in. The pixels behind it come from multiplying this
    // by the screen's own scale.
    static constexpr QSize targetSize{48, 32};

    static QSize renderSize(const QSize &documentSize, qreal devicePixelRatio);
    static QImage renderImage(
        const Document &document, const Layer &layer, qreal devicePixelRatio);
};

}
