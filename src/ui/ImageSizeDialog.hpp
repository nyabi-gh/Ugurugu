// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QDialog>
#include <QSize>

class QCheckBox;
class QDialogButtonBox;
class QDoubleSpinBox;
class QLabel;
class QSpinBox;

namespace ugurugu
{

class ImageGeometryPreview;

class ImageSizeDialog final : public QDialog
{
    Q_OBJECT

public:
    struct Result
    {
        QSize size;
        qreal horizontalScale = 1.0;
        qreal verticalScale = 1.0;
    };

    explicit ImageSizeDialog(
        const QSize &currentSize, QWidget *parent = nullptr);

    Result currentResult() const;
    QSize imageSize() const;
    qreal horizontalScale() const;
    qreal verticalScale() const;

private:
    void setKeepAspectRatio(bool keep);
    void updateFromWidth();
    void updateFromHeight();
    void updateFromPercentage();
    void updateDimensionRanges();
    void updatePresentation();
    void updatePercentageFromDimensions();

    QSize m_currentSize;
    QSpinBox *m_widthSpin = nullptr;
    QSpinBox *m_heightSpin = nullptr;
    QDoubleSpinBox *m_percentageSpin = nullptr;
    QCheckBox *m_keepAspectCheck = nullptr;
    QLabel *m_scaleSummaryLabel = nullptr;
    QLabel *m_sizeSummaryLabel = nullptr;
    QLabel *m_distortionWarningLabel = nullptr;
    ImageGeometryPreview *m_preview = nullptr;
    QDialogButtonBox *m_buttons = nullptr;
    bool m_syncing = false;
};

}
