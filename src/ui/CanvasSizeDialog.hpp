// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QDialog>
#include <QPoint>
#include <QSize>

class QButtonGroup;
class QCheckBox;
class QDialogButtonBox;
class QLabel;
class QSpinBox;

namespace ugurugu
{

class CanvasGeometryPreview;

class CanvasSizeDialog final : public QDialog
{
    Q_OBJECT

public:
    struct Result
    {
        QSize size;
        QPoint contentOffset;
    };

    explicit CanvasSizeDialog(
        const QSize &currentSize, QWidget *parent = nullptr);

    Result currentResult() const;
    QSize canvasSize() const;
    QPoint contentOffset() const;

private:
    void setRelativeMode(bool relative);
    void handleSizeChanged();
    void handleOffsetChanged();
    void applyAnchor(int anchorId);
    void syncAnchorFromOffset();
    void updatePresentation();
    QSize requestedSize() const;
    QPoint offsetForAnchor(int anchorId, const QSize &size) const;

    QSize m_currentSize;
    QCheckBox *m_relativeCheck = nullptr;
    QLabel *m_widthLabel = nullptr;
    QLabel *m_heightLabel = nullptr;
    QSpinBox *m_widthSpin = nullptr;
    QSpinBox *m_heightSpin = nullptr;
    QSpinBox *m_offsetXSpin = nullptr;
    QSpinBox *m_offsetYSpin = nullptr;
    QButtonGroup *m_anchorGroup = nullptr;
    QLabel *m_summaryLabel = nullptr;
    QLabel *m_offsetSummaryLabel = nullptr;
    QLabel *m_changeHintLabel = nullptr;
    CanvasGeometryPreview *m_preview = nullptr;
    QDialogButtonBox *m_buttons = nullptr;
    bool m_syncing = false;
};

}
