// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "ui/CanvasTypes.hpp"
#include "ui/PopoverOptionButton.hpp"

namespace ugurugu
{

class WandReferenceButton final : public PopoverOptionButton
{
    Q_OBJECT

public:
    WandReferenceButton(CanvasWandReference reference,
        QString title,
        QString description,
        QWidget *parent = nullptr);

    CanvasWandReference reference() const;

protected:
    void paintPreview(QPainter &painter, const QRectF &bounds) const override;

private:
    CanvasWandReference m_reference;
};

}
