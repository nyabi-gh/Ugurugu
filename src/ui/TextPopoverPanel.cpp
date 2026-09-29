// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/TextPopoverPanel.hpp"

#include "ui/CanvasWidget.hpp"
#include "ui/SpinBoxCaretGuard.hpp"

#include <QCheckBox>
#include <QDoubleSpinBox>
#include <QFontComboBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSignalBlocker>
#include <QVBoxLayout>

namespace ugurugu
{

TextPopoverPanel::TextPopoverPanel(CanvasWidget *canvas, QWidget *parent)
    : QWidget(parent)
{
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(8);

    auto *contentLabel = new QLabel(tr("TEXT"), this);
    contentLabel->setProperty("fieldLabel", true);
    layout->addWidget(contentLabel);

    auto *contentEdit = new QPlainTextEdit(this);
    contentEdit->setObjectName(QStringLiteral("textContentEdit"));
    contentEdit->setPlaceholderText(tr("Type the text to place…"));
    contentEdit->setPlainText(canvas->textContent());
    contentEdit->setTabChangesFocus(true);
    contentEdit->setFixedHeight(72);
    layout->addWidget(contentEdit);
    connect(contentEdit,
        &QPlainTextEdit::textChanged,
        canvas,
        [canvas, contentEdit]()
        {
            canvas->setTextContent(contentEdit->toPlainText());
        });
    connect(canvas,
        &CanvasWidget::textContentChanged,
        contentEdit,
        [contentEdit](const QString &text)
        {
            if (contentEdit->toPlainText() != text)
            {
                const QSignalBlocker blocker(contentEdit);
                contentEdit->setPlainText(text);
            }
        });

    auto *fontLabel = new QLabel(tr("FONT"), this);
    fontLabel->setProperty("fieldLabel", true);
    layout->addWidget(fontLabel);

    auto *fontCombo = new QFontComboBox(this);
    fontCombo->setObjectName(QStringLiteral("textFontCombo"));
    fontCombo->setCurrentFont(canvas->textFont());
    layout->addWidget(fontCombo);
    connect(fontCombo,
        &QFontComboBox::currentFontChanged,
        canvas,
        [canvas](const QFont &font)
        {
            canvas->setTextFontFamily(font.family());
        });
    connect(canvas,
        &CanvasWidget::textFontFamilyChanged,
        fontCombo,
        [fontCombo](const QString &family)
        {
            if (fontCombo->currentFont().family() != family)
            {
                const QSignalBlocker blocker(fontCombo);
                fontCombo->setCurrentFont(QFont(family));
            }
        });

    auto *sizeRow = new QWidget(this);
    auto *sizeLayout = new QHBoxLayout(sizeRow);
    sizeLayout->setContentsMargins(0, 0, 0, 0);
    auto *sizeLabel = new QLabel(tr("Size"), sizeRow);
    auto *sizeSpin = new QDoubleSpinBox(sizeRow);
    sizeSpin->setObjectName(QStringLiteral("textFontSizeSpin"));
    sizeSpin->setRange(8.0, 512.0);
    sizeSpin->setDecimals(0);
    sizeSpin->setSuffix(tr(" px"));
    installSuffixCaretGuard(sizeSpin);
    sizeSpin->setValue(canvas->textFontSize());
    sizeLayout->addWidget(sizeLabel);
    sizeLayout->addWidget(sizeSpin, 1);
    layout->addWidget(sizeRow);
    connect(sizeSpin,
        &QDoubleSpinBox::valueChanged,
        canvas,
        &CanvasWidget::setTextFontSize);
    connect(canvas,
        &CanvasWidget::textFontSizeChanged,
        sizeSpin,
        [sizeSpin](qreal size)
        {
            if (!qFuzzyCompare(sizeSpin->value(), size))
            {
                const QSignalBlocker blocker(sizeSpin);
                sizeSpin->setValue(size);
            }
        });

    auto *filledCheck = new QCheckBox(tr("Fill the letters"), this);
    filledCheck->setObjectName(QStringLiteral("textFilledCheck"));
    filledCheck->setChecked(canvas->textFilled());
    layout->addWidget(filledCheck);
    connect(
        filledCheck, &QCheckBox::toggled, canvas, &CanvasWidget::setTextFilled);
    connect(canvas,
        &CanvasWidget::textFilledChanged,
        filledCheck,
        [filledCheck](bool filled)
        {
            if (filledCheck->isChecked() != filled)
            {
                const QSignalBlocker blocker(filledCheck);
                filledCheck->setChecked(filled);
            }
        });

    auto *buttonRow = new QWidget(this);
    auto *buttonLayout = new QHBoxLayout(buttonRow);
    buttonLayout->setContentsMargins(0, 0, 0, 0);
    auto *applyButton = new QPushButton(tr("Place text"), buttonRow);
    applyButton->setObjectName(QStringLiteral("textApplyButton"));
    auto *cancelButton = new QPushButton(tr("Cancel"), buttonRow);
    cancelButton->setObjectName(QStringLiteral("textCancelButton"));
    applyButton->setEnabled(canvas->hasTextPlacement());
    cancelButton->setEnabled(canvas->hasTextPlacement());
    buttonLayout->addWidget(applyButton, 1);
    buttonLayout->addWidget(cancelButton);
    layout->addWidget(buttonRow);
    connect(applyButton,
        &QPushButton::clicked,
        canvas,
        &CanvasWidget::applyTextPlacement);
    connect(cancelButton,
        &QPushButton::clicked,
        canvas,
        &CanvasWidget::cancelTextPlacement);
    connect(canvas,
        &CanvasWidget::textPlacementChanged,
        buttonRow,
        [applyButton, cancelButton](bool active)
        {
            applyButton->setEnabled(active);
            cancelButton->setEnabled(active);
        });

    auto *hint = new QLabel(
        tr("Click the canvas to place the text, then drag it into "
           "position. The letters use the brush color and width, and "
           "wobble like drawn lines."),
        this);
    hint->setWordWrap(true);
    hint->setProperty("fieldLabel", true);
    layout->addWidget(hint);
}

}
