// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/GifExportDialog.hpp"

#include "io/AnimationExportPolicy.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QLabel>
#include <QPushButton>
#include <QVBoxLayout>

#include <algorithm>

namespace ugurugu
{

namespace
{

// Percentages rather than free entry: GIF is a delivery format, and the whole
// point of the control is to get under the memory budget quickly.
constexpr int scalePercentages[] = {100, 75, 50, 33, 25};

bool documentHasTransparentBackground(const Document &document)
{
    return document.background.alpha() < 255;
}

QSize scaledSize(const QSize &size, int percentage)
{
    if (percentage >= 100)
    {
        return size;
    }
    return QSize(std::max(1, size.width() * percentage / 100),
        std::max(1, size.height() * percentage / 100));
}

}

GifExportDialog::GifExportDialog(
    const Document &document, const QString &windowTitle, QWidget *parent)
    : QDialog(parent)
    , m_document(document)
    , m_documentSize(document.size)
    , m_frameCount(document.animationFrames)
    , m_documentHasTransparency(documentHasTransparentBackground(document))
{
    setWindowTitle(windowTitle);
    setModal(true);

    auto *layout = new QVBoxLayout(this);
    auto *form = new QFormLayout;

    m_scaleBox = new QComboBox(this);
    for (const int percentage : scalePercentages)
    {
        const QSize size = scaledSize(m_documentSize, percentage);
        m_scaleBox->addItem(tr("%1%  (%2 × %3)")
                                .arg(percentage)
                                .arg(size.width())
                                .arg(size.height()),
            percentage);
    }
    form->addRow(tr("Size"), m_scaleBox);

    m_transparencyBox = new QCheckBox(tr("Keep transparent areas"), this);
    m_transparencyBox->setChecked(true);
    m_transparencyBox->setEnabled(m_documentHasTransparency);
    if (!m_documentHasTransparency)
    {
        m_transparencyBox->setToolTip(
            tr("The canvas background is opaque, so there is nothing to keep "
               "transparent."));
    }
    form->addRow(QString(), m_transparencyBox);

    layout->addLayout(form);

    m_summaryLabel = new QLabel(this);
    m_summaryLabel->setWordWrap(true);
    layout->addWidget(m_summaryLabel);

    m_buttonBox = new QDialogButtonBox(
        QDialogButtonBox::Ok | QDialogButtonBox::Cancel, this);
    layout->addWidget(m_buttonBox);
    connect(m_buttonBox, &QDialogButtonBox::accepted, this, &QDialog::accept);
    connect(m_buttonBox, &QDialogButtonBox::rejected, this, &QDialog::reject);
    connect(m_scaleBox,
        &QComboBox::currentIndexChanged,
        this,
        &GifExportDialog::updatePresentation);

    // Preselect the largest scale that fits, so a canvas that cannot be
    // exported natively opens on a choice that works instead of on an error.
    int selected = 0;
    for (int index = 0; index < m_scaleBox->count(); ++index)
    {
        const QSize size =
            scaledSize(m_documentSize, m_scaleBox->itemData(index).toInt());
        if (AnimationExportPolicy::fitsMemoryBudget(m_document, size))
        {
            selected = index;
            break;
        }
    }
    m_scaleBox->setCurrentIndex(selected);
    updatePresentation();
}

QSize GifExportDialog::sizeForCurrentScale() const
{
    return scaledSize(m_documentSize, m_scaleBox->currentData().toInt());
}

void GifExportDialog::updatePresentation()
{
    const QSize size = sizeForCurrentScale();
    const bool fits = AnimationExportPolicy::fitsMemoryBudget(m_document, size);
    const long double bytes =
        AnimationExportPolicy::estimatedWorkingBytes(m_document, size);
    const double mebibytes = static_cast<double>(bytes / (1024.0L * 1024.0L));

    if (fits)
    {
        m_summaryLabel->setText(tr("%1 frames, about %2 MiB while encoding.")
                .arg(m_frameCount)
                .arg(mebibytes, 0, 'f', 0));
    }
    else
    {
        m_summaryLabel->setText(
            tr("This size needs about %1 MiB while encoding, which is more "
               "than the export budget. Choose a smaller size.")
                .arg(mebibytes, 0, 'f', 0));
    }
    if (auto *ok = m_buttonBox->button(QDialogButtonBox::Ok))
    {
        ok->setEnabled(fits);
    }
}

GifExportDialog::Result GifExportDialog::currentResult() const
{
    Result result;
    result.outputSize = sizeForCurrentScale();
    result.preserveTransparency =
        m_documentHasTransparency && m_transparencyBox->isChecked();
    return result;
}

}
