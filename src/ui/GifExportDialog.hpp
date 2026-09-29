// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QDialog>
#include <QSize>

class QCheckBox;
class QComboBox;
class QDialogButtonBox;
class QLabel;

namespace ugurugu
{

// Export options for one animated image.
//
// The scale choice exists because the encoder holds every frame at once, so a
// large canvas can exceed the export memory budget at its native size. The
// dialog therefore reports the estimate and blocks OK on a scale that would
// not fit, rather than letting the export fail after the user waits for it.
class GifExportDialog final : public QDialog
{
    Q_OBJECT

public:
    struct Result
    {
        QSize outputSize;
        bool preserveTransparency = true;
    };

    explicit GifExportDialog(const Document &document,
        const QString &windowTitle,
        QWidget *parent = nullptr);

    Result currentResult() const;

private:
    void updatePresentation();
    QSize sizeForCurrentScale() const;

    // Held whole rather than as size and frame count: the memory estimate
    // depends on the layer hierarchy, and the copy shares the document data.
    Document m_document;
    QSize m_documentSize;
    int m_frameCount = 1;
    bool m_documentHasTransparency = false;
    QComboBox *m_scaleBox = nullptr;
    QCheckBox *m_transparencyBox = nullptr;
    QLabel *m_summaryLabel = nullptr;
    QDialogButtonBox *m_buttonBox = nullptr;
};

}
