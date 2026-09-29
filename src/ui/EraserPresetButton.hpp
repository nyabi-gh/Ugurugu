// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QAbstractButton>
#include <QImage>

namespace ugurugu
{

struct EraserPreset;

class EraserPresetButton final : public QAbstractButton
{
    Q_OBJECT

public:
    EraserPresetButton(const EraserPreset &preset, QWidget *parent = nullptr);

    QString presetId() const;
    QSize sizeHint() const override;
    bool event(QEvent *event) override;

protected:
    void paintEvent(QPaintEvent *event) override;

private:
    const QImage &previewImage();

    const EraserPreset *m_preset;
    QImage m_preview;
};

}
