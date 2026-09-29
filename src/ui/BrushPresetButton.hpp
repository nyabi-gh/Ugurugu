// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QAbstractButton>
#include <QImage>
#include <QVector>

namespace ugurugu
{

struct BrushPreset;

class BrushPresetButton final : public QAbstractButton
{
    Q_OBJECT

public:
    static constexpr int previewFrameCount = 6;

    BrushPresetButton(const BrushPreset &preset, QWidget *parent = nullptr);

    QString presetId() const;
    void setPreviewFrame(int frame);
    void setTabletPressureEnabled(bool enabled);
    QSize sizeHint() const override;
    bool event(QEvent *event) override;

protected:
    void paintEvent(QPaintEvent *event) override;

private:
    const QImage &frameImage(int frame);

    const BrushPreset *m_preset;
    QVector<QImage> m_frames;
    int m_frame = 0;
    bool m_tabletPressureEnabled = true;
};

}
