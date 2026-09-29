// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ColorDock.hpp"

#include "ui/CanvasWidget.hpp"
#include "ui/ColorPairSwatch.hpp"
#include "ui/ColorWheel.hpp"
#include "ui/PaletteDockTitleBar.hpp"
#include "ui/ResponsiveGrid.hpp"

#include <QButtonGroup>
#include <QHBoxLayout>
#include <QLabel>
#include <QLatin1StringView>
#include <QSettings>
#include <QSizePolicy>
#include <QToolButton>
#include <QVBoxLayout>

namespace ugurugu
{

namespace
{

constexpr QLatin1StringView colorWheelShapeKey("dock/colorWheelShape");

}

ColorDock::ColorDock(CanvasWidget *canvas, QWidget *parent)
    : QDockWidget(tr("Color"), parent)
    , m_canvas(canvas)
{
    setObjectName(QStringLiteral("ColorDock"));
    setFeatures(QDockWidget::DockWidgetMovable
                | QDockWidget::DockWidgetFloatable
                | QDockWidget::DockWidgetClosable);
    setMinimumWidth(150);
    installCompactPaletteTitleBar(this);

    auto *body = new QWidget(this);
    auto *layout = new QVBoxLayout(body);
    layout->setContentsMargins(8, 8, 8, 8);
    layout->setSpacing(6);

    m_colorWheel = new ColorWheel(body);
    layout->addWidget(m_colorWheel);

    auto *shapeGrid = new ResponsiveGrid(76, 2, 4, body);
    shapeGrid->setObjectName(QStringLiteral("colorShapeGrid"));
    auto *squareButton = new QToolButton(shapeGrid);
    squareButton->setObjectName(QStringLiteral("colorWheelSquareButton"));
    squareButton->setText(tr("HSV Square"));
    squareButton->setCheckable(true);
    squareButton->setProperty("categoryTab", true);
    auto *triangleButton = new QToolButton(shapeGrid);
    triangleButton->setObjectName(QStringLiteral("colorWheelTriangleButton"));
    triangleButton->setText(tr("HSV Triangle"));
    triangleButton->setCheckable(true);
    triangleButton->setProperty("categoryTab", true);
    auto *shapeGroup = new QButtonGroup(shapeGrid);
    shapeGroup->setExclusive(true);
    shapeGroup->addButton(squareButton);
    shapeGroup->addButton(triangleButton);
    shapeGrid->addWidget(squareButton);
    shapeGrid->addWidget(triangleButton);
    layout->addWidget(shapeGrid);

    auto *colorGrid = new ResponsiveGrid(116, 2, 8, body);
    colorGrid->setObjectName(QStringLiteral("currentColorGrid"));
    m_colorSwatch = new ColorPairSwatch(colorGrid);
    colorGrid->addWidget(m_colorSwatch);
    auto *colorTextWidget = new QWidget(colorGrid);
    auto *colorText = new QVBoxLayout(colorTextWidget);
    colorText->setContentsMargins(0, 0, 0, 0);
    colorText->setSpacing(1);
    auto *colorLabel = new QLabel(tr("Current color"), colorTextWidget);
    colorLabel->setProperty("fieldLabel", true);
    colorText->addWidget(colorLabel);
    m_colorValue = new QLabel(colorTextWidget);
    m_colorValue->setTextInteractionFlags(Qt::TextSelectableByMouse);
    colorText->addWidget(m_colorValue);
    colorGrid->addWidget(colorTextWidget);
    layout->addWidget(colorGrid);

    layout->addStretch(1);
    setWidget(body);

    connect(m_colorWheel,
        &ColorWheel::colorChanged,
        this,
        [this](const QColor &color)
        {
            if (!m_updatingColor)
            {
                m_canvas->setBrushColor(color);
            }
        });
    connect(m_canvas,
        &CanvasWidget::brushColorChanged,
        this,
        [this](const QColor &color)
        {
            m_updatingColor = true;
            m_colorWheel->setColor(color);
            m_colorSwatch->setCurrentColor(color);
            m_colorValue->setText(color.name(QColor::HexArgb).toUpper());
            m_updatingColor = false;
        });
    connect(m_colorSwatch,
        &ColorPairSwatch::colorSelected,
        m_canvas,
        &CanvasWidget::setBrushColor);

    connect(squareButton,
        &QToolButton::clicked,
        this,
        [this]()
        {
            m_colorWheel->setShape(ColorWheel::Shape::Square);
        });
    connect(triangleButton,
        &QToolButton::clicked,
        this,
        [this]()
        {
            m_colorWheel->setShape(ColorWheel::Shape::Triangle);
        });
    connect(m_colorWheel,
        &ColorWheel::shapeChanged,
        this,
        [squareButton, triangleButton](ColorWheel::Shape shape)
        {
            squareButton->setChecked(shape == ColorWheel::Shape::Square);
            triangleButton->setChecked(shape == ColorWheel::Shape::Triangle);
            QSettings().setValue(colorWheelShapeKey,
                shape == ColorWheel::Shape::Triangle
                    ? QStringLiteral("triangle")
                    : QStringLiteral("square"));
        });

    const bool triangle = QSettings().value(colorWheelShapeKey).toString()
                          == QStringLiteral("triangle");
    m_colorWheel->setShape(
        triangle ? ColorWheel::Shape::Triangle : ColorWheel::Shape::Square);
    squareButton->setChecked(!triangle);
    triangleButton->setChecked(triangle);
    m_colorWheel->setColor(m_canvas->brushColor());
    m_colorSwatch->setCurrentColor(m_canvas->brushColor());
    m_colorValue->setText(
        m_canvas->brushColor().name(QColor::HexArgb).toUpper());
}

}
