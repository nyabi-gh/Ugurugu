// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ImageSizeDialog.hpp"

#include "document/DocumentLimits.hpp"
#include "ui/SpinBoxCaretGuard.hpp"
#include "ui/Theme.hpp"

#include <QCheckBox>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QPainter>
#include <QPainterPath>
#include <QPushButton>
#include <QSignalBlocker>
#include <QSpinBox>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>

namespace ugurugu
{

namespace
{

constexpr int minimumDialogEdge = DocumentLimits::minimumCanvasEdge;
constexpr int previewMargin = 18;

QRectF centeredFittedRect(
    const QSize &source, const QSize &largest, const QRectF &viewport)
{
    if (!source.isValid() || !largest.isValid() || viewport.isEmpty())
    {
        return {};
    }
    const qreal scale = std::min(viewport.width() / largest.width(),
        viewport.height() / largest.height());
    const QSizeF displayed(source.width() * scale, source.height() * scale);
    return QRectF(
        viewport.center()
            - QPointF(displayed.width() / 2.0, displayed.height() / 2.0),
        displayed);
}

}

class ImageGeometryPreview final : public QWidget
{
public:
    explicit ImageGeometryPreview(QWidget *parent = nullptr)
        : QWidget(parent)
    {
        setObjectName(QStringLiteral("imageGeometryPreview"));
        setAccessibleName(ImageSizeDialog::tr("Image size preview"));
        setMinimumSize(280, 230);
        setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Expanding);
    }

    void setGeometryState(const QSize &currentSize, const QSize &targetSize)
    {
        m_currentSize = currentSize;
        m_targetSize = targetSize;
        update();
    }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing, true);

        const QRectF frame = QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5);
        QPainterPath framePath;
        framePath.addRoundedRect(frame, 10.0, 10.0);
        painter.fillPath(framePath, Theme::canvasBackground());
        painter.setPen(QPen(Theme::border(), 1.0));
        painter.drawPath(framePath);

        if (!m_currentSize.isValid() || !m_targetSize.isValid())
        {
            return;
        }

        const QSize largest(
            std::max(m_currentSize.width(), m_targetSize.width()),
            std::max(m_currentSize.height(), m_targetSize.height()));
        const QRectF viewport = frame.adjusted(
            previewMargin, previewMargin, -previewMargin, -previewMargin);
        const QRectF displayedCurrent =
            centeredFittedRect(m_currentSize, largest, viewport);
        const QRectF displayedTarget =
            centeredFittedRect(m_targetSize, largest, viewport);

        painter.setPen(Qt::NoPen);
        QColor targetFill = Theme::accent();
        targetFill.setAlpha(46);
        painter.setBrush(targetFill);
        painter.drawRect(displayedTarget);

        QPen currentPen(Theme::textMuted(), 1.0, Qt::DashLine);
        currentPen.setCosmetic(true);
        painter.setBrush(Qt::NoBrush);
        painter.setPen(currentPen);
        painter.drawRect(displayedCurrent);

        QPen targetPen(Theme::accent(), 2.0);
        targetPen.setCosmetic(true);
        painter.setPen(targetPen);
        painter.drawRect(displayedTarget);
    }

private:
    QSize m_currentSize;
    QSize m_targetSize;
};

ImageSizeDialog::ImageSizeDialog(const QSize &currentSize, QWidget *parent)
    : QDialog(parent)
    , m_currentSize(std::clamp(currentSize.width(),
                        minimumDialogEdge,
                        DocumentLimits::maximumCanvasEdge),
          std::clamp(currentSize.height(),
              minimumDialogEdge,
              DocumentLimits::maximumCanvasEdge))
{
    setObjectName(QStringLiteral("imageSizeDialog"));
    setWindowTitle(tr("Change image size"));
    setModal(true);

    auto *mainLayout = new QVBoxLayout(this);
    mainLayout->setContentsMargins(18, 18, 18, 14);
    mainLayout->setSpacing(14);

    auto *description = new QLabel(
        tr("Scale the artwork and brush sizes to new pixel dimensions."), this);
    description->setWordWrap(true);
    mainLayout->addWidget(description);

    auto *contentLayout = new QHBoxLayout;
    contentLayout->setSpacing(22);
    mainLayout->addLayout(contentLayout, 1);

    auto *controls = new QWidget(this);
    controls->setObjectName(QStringLiteral("imageSizeControls"));
    auto *controlsLayout = new QVBoxLayout(controls);
    controlsLayout->setContentsMargins(0, 0, 0, 0);
    controlsLayout->setSpacing(10);
    contentLayout->addWidget(controls);

    auto *sizeHeading = new QLabel(tr("IMAGE SIZE"), controls);
    sizeHeading->setProperty("fieldLabel", true);
    controlsLayout->addWidget(sizeHeading);

    auto *sizeForm = new QFormLayout;
    sizeForm->setContentsMargins(0, 0, 0, 0);
    sizeForm->setHorizontalSpacing(14);
    sizeForm->setVerticalSpacing(8);

    m_widthSpin = new QSpinBox(controls);
    m_widthSpin->setObjectName(QStringLiteral("imageWidthSpin"));
    m_widthSpin->setRange(minimumDialogEdge, DocumentLimits::maximumCanvasEdge);
    m_widthSpin->setValue(m_currentSize.width());
    m_widthSpin->setSuffix(tr(" px"));
    installSuffixCaretGuard(m_widthSpin);
    m_widthSpin->setAccessibleName(tr("Image width"));
    sizeForm->addRow(tr("Width"), m_widthSpin);

    m_heightSpin = new QSpinBox(controls);
    m_heightSpin->setObjectName(QStringLiteral("imageHeightSpin"));
    m_heightSpin->setRange(
        minimumDialogEdge, DocumentLimits::maximumCanvasEdge);
    m_heightSpin->setValue(m_currentSize.height());
    m_heightSpin->setSuffix(tr(" px"));
    installSuffixCaretGuard(m_heightSpin);
    m_heightSpin->setAccessibleName(tr("Image height"));
    sizeForm->addRow(tr("Height"), m_heightSpin);

    m_percentageSpin = new QDoubleSpinBox(controls);
    m_percentageSpin->setObjectName(QStringLiteral("imageScalePercentSpin"));
    m_percentageSpin->setDecimals(1);
    m_percentageSpin->setSingleStep(10.0);
    m_percentageSpin->setSuffix(tr(" %"));
    installSuffixCaretGuard(m_percentageSpin);
    m_percentageSpin->setAccessibleName(tr("Uniform scale"));
    m_percentageSpin->setToolTip(
        tr("Changing this value scales both dimensions uniformly."));
    const qreal minimumPercentage =
        std::max(100.0 * minimumDialogEdge / m_currentSize.width(),
            100.0 * minimumDialogEdge / m_currentSize.height());
    const qreal maximumPercentage = std::min(
        100.0 * DocumentLimits::maximumCanvasEdge / m_currentSize.width(),
        100.0 * DocumentLimits::maximumCanvasEdge / m_currentSize.height());
    m_percentageSpin->setRange(minimumPercentage, maximumPercentage);
    m_percentageSpin->setValue(100.0);
    sizeForm->addRow(tr("Uniform scale"), m_percentageSpin);
    controlsLayout->addLayout(sizeForm);

    m_keepAspectCheck = new QCheckBox(tr("Keep aspect ratio"), controls);
    m_keepAspectCheck->setObjectName(QStringLiteral("imageKeepAspectCheck"));
    m_keepAspectCheck->setAccessibleName(tr("Keep image aspect ratio"));
    const qreal aspectRatio =
        static_cast<qreal>(m_currentSize.width()) / m_currentSize.height();
    const int smallestAspectWidth =
        std::clamp(qCeil(minimumDialogEdge * aspectRatio),
            minimumDialogEdge,
            DocumentLimits::maximumCanvasEdge);
    const int largestAspectWidth =
        std::clamp(qFloor(DocumentLimits::maximumCanvasEdge * aspectRatio),
            minimumDialogEdge,
            DocumentLimits::maximumCanvasEdge);
    const int smallestAspectHeight =
        std::clamp(qCeil(minimumDialogEdge / aspectRatio),
            minimumDialogEdge,
            DocumentLimits::maximumCanvasEdge);
    const int largestAspectHeight =
        std::clamp(qFloor(DocumentLimits::maximumCanvasEdge / aspectRatio),
            minimumDialogEdge,
            DocumentLimits::maximumCanvasEdge);
    const bool aspectResizable = smallestAspectWidth != largestAspectWidth
                                 || smallestAspectHeight != largestAspectHeight;
    m_keepAspectCheck->setChecked(aspectResizable);
    m_keepAspectCheck->setEnabled(aspectResizable);
    if (!aspectResizable)
    {
        m_keepAspectCheck->setToolTip(
            tr("No other size can keep this aspect ratio within the "
               "canvas size limits."));
    }
    controlsLayout->addWidget(m_keepAspectCheck);
    controlsLayout->addStretch(1);

    auto *previewColumn = new QWidget(this);
    auto *previewLayout = new QVBoxLayout(previewColumn);
    previewLayout->setContentsMargins(0, 0, 0, 0);
    previewLayout->setSpacing(8);
    contentLayout->addWidget(previewColumn, 1);

    m_preview = new ImageGeometryPreview(previewColumn);
    previewLayout->addWidget(m_preview, 1);

    m_sizeSummaryLabel = new QLabel(previewColumn);
    m_sizeSummaryLabel->setObjectName(QStringLiteral("imageSizeSummaryLabel"));
    m_sizeSummaryLabel->setAlignment(Qt::AlignCenter);
    m_sizeSummaryLabel->setTextInteractionFlags(Qt::TextSelectableByMouse);
    previewLayout->addWidget(m_sizeSummaryLabel);

    m_scaleSummaryLabel = new QLabel(previewColumn);
    m_scaleSummaryLabel->setObjectName(
        QStringLiteral("imageScaleSummaryLabel"));
    m_scaleSummaryLabel->setAlignment(Qt::AlignCenter);
    m_scaleSummaryLabel->setTextInteractionFlags(Qt::TextSelectableByMouse);
    previewLayout->addWidget(m_scaleSummaryLabel);

    m_distortionWarningLabel = new QLabel(previewColumn);
    m_distortionWarningLabel->setObjectName(
        QStringLiteral("imageDistortionWarningLabel"));
    m_distortionWarningLabel->setAlignment(Qt::AlignCenter);
    m_distortionWarningLabel->setWordWrap(true);
    m_distortionWarningLabel->setMinimumHeight(
        m_distortionWarningLabel->fontMetrics().lineSpacing() * 2);
    previewLayout->addWidget(m_distortionWarningLabel);

    m_buttons = new QDialogButtonBox(
        QDialogButtonBox::Ok | QDialogButtonBox::Cancel, this);
    QPushButton *okButton = m_buttons->button(QDialogButtonBox::Ok);
    okButton->setObjectName(QStringLiteral("imageSizeOkButton"));
    okButton->setDefault(true);
    m_buttons->button(QDialogButtonBox::Cancel)
        ->setObjectName(QStringLiteral("imageSizeCancelButton"));
    connect(m_buttons, &QDialogButtonBox::accepted, this, &QDialog::accept);
    connect(m_buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    mainLayout->addWidget(m_buttons);

    connect(m_widthSpin,
        &QSpinBox::valueChanged,
        this,
        [this](int)
        {
            updateFromWidth();
        });
    connect(m_heightSpin,
        &QSpinBox::valueChanged,
        this,
        [this](int)
        {
            updateFromHeight();
        });
    connect(m_percentageSpin,
        &QDoubleSpinBox::valueChanged,
        this,
        [this](double)
        {
            updateFromPercentage();
        });
    connect(m_keepAspectCheck,
        &QCheckBox::toggled,
        this,
        &ImageSizeDialog::setKeepAspectRatio);

    updateDimensionRanges();
    updatePresentation();
    resize(700, 440);
}

ImageSizeDialog::Result ImageSizeDialog::currentResult() const
{
    return {imageSize(), horizontalScale(), verticalScale()};
}

QSize ImageSizeDialog::imageSize() const
{
    return QSize(m_widthSpin->value(), m_heightSpin->value());
}

qreal ImageSizeDialog::horizontalScale() const
{
    return static_cast<qreal>(m_widthSpin->value()) / m_currentSize.width();
}

qreal ImageSizeDialog::verticalScale() const
{
    return static_cast<qreal>(m_heightSpin->value()) / m_currentSize.height();
}

void ImageSizeDialog::setKeepAspectRatio(bool keep)
{
    if (m_syncing)
    {
        return;
    }
    m_syncing = true;
    updateDimensionRanges();
    if (keep)
    {
        const qreal ratio =
            static_cast<qreal>(m_currentSize.width()) / m_currentSize.height();
        const int width = std::clamp(m_widthSpin->value(),
            m_widthSpin->minimum(),
            m_widthSpin->maximum());
        m_widthSpin->setValue(width);
        m_heightSpin->setValue(std::clamp(qRound(width / ratio),
            m_heightSpin->minimum(),
            m_heightSpin->maximum()));
    }
    m_syncing = false;
    updatePercentageFromDimensions();
    updatePresentation();
}

void ImageSizeDialog::updateFromWidth()
{
    if (m_syncing)
    {
        return;
    }
    m_syncing = true;
    if (m_keepAspectCheck->isChecked())
    {
        const qreal ratio =
            static_cast<qreal>(m_currentSize.width()) / m_currentSize.height();
        m_heightSpin->setValue(std::clamp(qRound(m_widthSpin->value() / ratio),
            m_heightSpin->minimum(),
            m_heightSpin->maximum()));
    }
    m_syncing = false;
    updatePercentageFromDimensions();
    updatePresentation();
}

void ImageSizeDialog::updateFromHeight()
{
    if (m_syncing)
    {
        return;
    }
    m_syncing = true;
    if (m_keepAspectCheck->isChecked())
    {
        const qreal ratio =
            static_cast<qreal>(m_currentSize.width()) / m_currentSize.height();
        m_widthSpin->setValue(std::clamp(qRound(m_heightSpin->value() * ratio),
            m_widthSpin->minimum(),
            m_widthSpin->maximum()));
    }
    m_syncing = false;
    updatePercentageFromDimensions();
    updatePresentation();
}

void ImageSizeDialog::updateFromPercentage()
{
    if (m_syncing)
    {
        return;
    }
    const qreal factor = m_percentageSpin->value() / 100.0;
    m_syncing = true;
    m_widthSpin->setValue(std::clamp(qRound(m_currentSize.width() * factor),
        m_widthSpin->minimum(),
        m_widthSpin->maximum()));
    m_heightSpin->setValue(std::clamp(qRound(m_currentSize.height() * factor),
        m_heightSpin->minimum(),
        m_heightSpin->maximum()));
    m_syncing = false;
    updatePresentation();
}

void ImageSizeDialog::updateDimensionRanges()
{
    const bool keepAspect = m_keepAspectCheck && m_keepAspectCheck->isChecked();
    if (!keepAspect)
    {
        m_widthSpin->setRange(
            minimumDialogEdge, DocumentLimits::maximumCanvasEdge);
        m_heightSpin->setRange(
            minimumDialogEdge, DocumentLimits::maximumCanvasEdge);
        return;
    }

    const qreal ratio =
        static_cast<qreal>(m_currentSize.width()) / m_currentSize.height();
    const int minimumWidth = std::clamp(qCeil(minimumDialogEdge * ratio),
        minimumDialogEdge,
        DocumentLimits::maximumCanvasEdge);
    const int maximumWidth =
        std::clamp(qFloor(DocumentLimits::maximumCanvasEdge * ratio),
            minimumDialogEdge,
            DocumentLimits::maximumCanvasEdge);
    const int minimumHeight = std::clamp(qCeil(minimumDialogEdge / ratio),
        minimumDialogEdge,
        DocumentLimits::maximumCanvasEdge);
    const int maximumHeight =
        std::clamp(qFloor(DocumentLimits::maximumCanvasEdge / ratio),
            minimumDialogEdge,
            DocumentLimits::maximumCanvasEdge);
    m_widthSpin->setRange(std::min(minimumWidth, maximumWidth),
        std::max(minimumWidth, maximumWidth));
    m_heightSpin->setRange(std::min(minimumHeight, maximumHeight),
        std::max(minimumHeight, maximumHeight));
}

void ImageSizeDialog::updatePresentation()
{
    const QSize targetSize = imageSize();
    const qreal horizontalPercent = horizontalScale() * 100.0;
    const qreal verticalPercent = verticalScale() * 100.0;
    m_sizeSummaryLabel->setText(tr("%1 × %2 px  →  %3 × %4 px")
            .arg(m_currentSize.width())
            .arg(m_currentSize.height())
            .arg(targetSize.width())
            .arg(targetSize.height()));
    m_scaleSummaryLabel->setText(tr("Width %1%  ·  Height %2%")
            .arg(horizontalPercent, 0, 'f', 1)
            .arg(verticalPercent, 0, 'f', 1));
    m_preview->setGeometryState(m_currentSize, targetSize);

    const bool distorted =
        !m_keepAspectCheck->isChecked()
        && !qFuzzyCompare(horizontalScale(), verticalScale());
    if (distorted)
    {
        m_distortionWarningLabel->setText(
            tr("The aspect ratio will change and the artwork will be "
               "distorted."));
        m_distortionWarningLabel->setStyleSheet(
            QStringLiteral("color: %1;").arg(Theme::accent().name()));
    }
    else
    {
        m_distortionWarningLabel->setText(
            tr("Artwork and brush sizes scale with the image."));
        m_distortionWarningLabel->setStyleSheet(
            QStringLiteral("color: %1;").arg(Theme::textMuted().name()));
    }

    m_buttons->button(QDialogButtonBox::Ok)
        ->setEnabled(targetSize != m_currentSize);
}

void ImageSizeDialog::updatePercentageFromDimensions()
{
    const qreal horizontalPercent = horizontalScale() * 100.0;
    const qreal verticalPercent = verticalScale() * 100.0;
    const qreal representative =
        m_keepAspectCheck->isChecked()
            ? horizontalPercent
            : std::sqrt(horizontalPercent * verticalPercent);
    const QSignalBlocker blocker(m_percentageSpin);
    m_percentageSpin->setValue(std::clamp(representative,
        m_percentageSpin->minimum(),
        m_percentageSpin->maximum()));
}

}
