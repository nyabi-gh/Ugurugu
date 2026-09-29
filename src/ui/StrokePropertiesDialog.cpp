// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/StrokePropertiesDialog.hpp"

#include "document/DocumentLimits.hpp"
#include "ui/SpinBoxCaretGuard.hpp"

#include <QCheckBox>
#include <QColorDialog>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QPushButton>
#include <QVBoxLayout>

namespace ugurugu
{

StrokePropertiesDialog::StrokePropertiesDialog(
    const Values &values, QWidget *parent)
    : QDialog(parent)
{
    setObjectName(QStringLiteral("StrokePropertiesDialog"));
    setWindowTitle(tr("Edit Stroke Properties"));
    setModal(true);

    auto *layout = new QVBoxLayout(this);
    auto *form = new QFormLayout;

    m_colorCheck = new QCheckBox(tr("Color"), this);
    m_colorCheck->setEnabled(values.colorSupported);
    m_colorCheck->setChecked(values.color.has_value());
    m_color = values.color.value_or(QColor(Qt::black));
    m_colorButton = new QPushButton(this);
    m_colorButton->setObjectName(QStringLiteral("strokeColorButton"));
    m_colorButton->setEnabled(values.colorSupported);
    m_colorButton->setMinimumWidth(96);
    updateColorButton();
    form->addRow(m_colorCheck, m_colorButton);

    m_widthCheck = new QCheckBox(tr("Width"), this);
    m_widthCheck->setEnabled(values.widthSupported);
    m_widthCheck->setChecked(values.width.has_value());
    m_widthSpin = new QDoubleSpinBox(this);
    m_widthSpin->setObjectName(QStringLiteral("strokeWidthSpin"));
    m_widthSpin->setRange(
        DocumentLimits::minimumStrokeWidth, DocumentLimits::maximumStrokeWidth);
    m_widthSpin->setDecimals(2);
    m_widthSpin->setSuffix(tr(" px"));
    installSuffixCaretGuard(m_widthSpin);
    m_widthSpin->setValue(values.width.value_or(6.0));
    m_widthSpin->setEnabled(values.widthSupported);
    form->addRow(m_widthCheck, m_widthSpin);
    layout->addLayout(form);

    m_buttons = new QDialogButtonBox(
        QDialogButtonBox::Ok | QDialogButtonBox::Cancel, this);
    layout->addWidget(m_buttons);

    connect(m_colorButton,
        &QPushButton::clicked,
        this,
        &StrokePropertiesDialog::chooseColor);
    connect(m_widthSpin,
        &QDoubleSpinBox::valueChanged,
        this,
        [this]()
        {
            m_widthCheck->setChecked(true);
        });
    for (QCheckBox *check : {m_colorCheck, m_widthCheck})
    {
        connect(check,
            &QCheckBox::toggled,
            this,
            &StrokePropertiesDialog::updateAcceptState);
    }
    connect(m_buttons, &QDialogButtonBox::accepted, this, &QDialog::accept);
    connect(m_buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    updateAcceptState();
}

std::optional<QColor> StrokePropertiesDialog::color() const
{
    return m_colorCheck->isChecked() ? std::optional<QColor>(m_color)
                                     : std::nullopt;
}

std::optional<qreal> StrokePropertiesDialog::selectedWidth() const
{
    return m_widthCheck->isChecked()
               ? std::optional<qreal>(m_widthSpin->value())
               : std::nullopt;
}

void StrokePropertiesDialog::chooseColor()
{
    const QColor selected = QColorDialog::getColor(m_color,
        this,
        tr("Select Stroke Color"),
        QColorDialog::ShowAlphaChannel);
    if (!selected.isValid())
    {
        return;
    }
    m_color = selected;
    m_colorCheck->setChecked(true);
    updateColorButton();
}

void StrokePropertiesDialog::updateColorButton()
{
    m_colorButton->setText(m_color.name(QColor::HexArgb));
    m_colorButton->setStyleSheet(QStringLiteral("background-color: %1;")
            .arg(m_color.name(QColor::HexArgb)));
}

void StrokePropertiesDialog::updateAcceptState()
{
    m_buttons->button(QDialogButtonBox::Ok)
        ->setEnabled(m_colorCheck->isChecked() || m_widthCheck->isChecked());
}

}
