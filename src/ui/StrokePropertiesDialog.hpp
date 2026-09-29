// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QColor>
#include <QDialog>

#include <optional>

class QCheckBox;
class QDialogButtonBox;
class QDoubleSpinBox;
class QPushButton;

namespace ugurugu
{

class StrokePropertiesDialog final : public QDialog
{
    Q_OBJECT

public:
    struct Values
    {
        bool colorSupported = false;
        bool widthSupported = false;
        std::optional<QColor> color;
        std::optional<qreal> width;
    };

    explicit StrokePropertiesDialog(
        const Values &values, QWidget *parent = nullptr);

    std::optional<QColor> color() const;
    std::optional<qreal> selectedWidth() const;

private:
    void chooseColor();
    void updateColorButton();
    void updateAcceptState();

    QColor m_color = Qt::black;
    QCheckBox *m_colorCheck = nullptr;
    QCheckBox *m_widthCheck = nullptr;
    QPushButton *m_colorButton = nullptr;
    QDoubleSpinBox *m_widthSpin = nullptr;
    QDialogButtonBox *m_buttons = nullptr;
};

}
