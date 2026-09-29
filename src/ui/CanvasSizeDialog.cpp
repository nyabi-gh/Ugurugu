// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/CanvasSizeDialog.hpp"

#include "document/DocumentLimits.hpp"
#include "ui/SpinBoxCaretGuard.hpp"
#include "ui/Theme.hpp"

#include <QAbstractButton>
#include <QButtonGroup>
#include <QCheckBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QGridLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QPainter>
#include <QPainterPath>
#include <QPushButton>
#include <QSignalBlocker>
#include <QSpinBox>
#include <QToolButton>
#include <QVBoxLayout>

#include <algorithm>

namespace ugurugu
{

namespace
{

constexpr int minimumDialogEdge = DocumentLimits::minimumCanvasEdge;
constexpr int anchorColumnCount = 3;
constexpr int previewMargin = 18;

class DimensionSpinBox final : public QSpinBox
{
public:
    explicit DimensionSpinBox(QWidget *parent = nullptr)
        : QSpinBox(parent)
    {
    }

    void setSignedDisplay(bool signedDisplay)
    {
        if (m_signedDisplay == signedDisplay)
        {
            return;
        }
        m_signedDisplay = signedDisplay;
        update();
    }

protected:
    QString textFromValue(int value) const override
    {
        QString text = locale().toString(value);
        if (m_signedDisplay && value > 0)
        {
            text.prepend(QLatin1Char('+'));
        }
        return text;
    }

private:
    bool m_signedDisplay = false;
};

int alignedOffset(int delta, int alignment)
{
    switch (alignment)
    {
    case 0:
        return 0;
    case 1:
        return delta / 2;
    case 2:
        return delta;
    default:
        return 0;
    }
}

QString signedPixels(int value)
{
    return QStringLiteral("%1%2 px")
        .arg(value > 0 ? QStringLiteral("+") : QString())
        .arg(value);
}

QRectF fittedRect(
    const QRectF &worldRect, const QRectF &viewport, const QRectF &source)
{
    if (worldRect.isEmpty() || viewport.isEmpty() || source.isEmpty())
    {
        return {};
    }
    const qreal scale = std::min(viewport.width() / worldRect.width(),
        viewport.height() / worldRect.height());
    const QPointF worldCenter = worldRect.center();
    const QPointF viewportCenter = viewport.center();
    const auto mapPoint = [&](const QPointF &point)
    {
        return viewportCenter + (point - worldCenter) * scale;
    };
    return QRectF(mapPoint(source.topLeft()), mapPoint(source.bottomRight()))
        .normalized();
}

}

class CanvasGeometryPreview final : public QWidget
{
public:
    explicit CanvasGeometryPreview(QWidget *parent = nullptr)
        : QWidget(parent)
    {
        setObjectName(QStringLiteral("canvasGeometryPreview"));
        setAccessibleName(CanvasSizeDialog::tr("Canvas size preview"));
        setMinimumSize(280, 230);
        setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Expanding);
    }

    void setGeometryState(const QSize &currentSize,
        const QSize &targetSize,
        const QPoint &contentOffset)
    {
        m_currentSize = currentSize;
        m_targetSize = targetSize;
        m_contentOffset = contentOffset;
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

        const QRectF targetRect{QPointF(), QSizeF(m_targetSize)};
        const QRectF artworkRect{
            QPointF(m_contentOffset), QSizeF(m_currentSize)};
        QRectF world = targetRect.united(artworkRect);
        const qreal padding =
            std::max(1.0, std::max(world.width(), world.height()) * 0.08);
        world.adjust(-padding, -padding, padding, padding);

        const QRectF viewport = frame.adjusted(
            previewMargin, previewMargin, -previewMargin, -previewMargin);
        const QRectF displayedTarget = fittedRect(world, viewport, targetRect);
        const QRectF displayedArtwork =
            fittedRect(world, viewport, artworkRect);

        painter.setPen(Qt::NoPen);
        painter.setBrush(QColor(0xEE, 0xEE, 0xF0));
        painter.drawRect(displayedTarget);

        QColor artworkFill = Theme::accent();
        artworkFill.setAlpha(54);
        painter.setBrush(artworkFill);
        painter.drawRect(displayedArtwork);

        QPen artworkPen(Theme::textMuted(), 1.0, Qt::DashLine);
        artworkPen.setCosmetic(true);
        painter.setBrush(Qt::NoBrush);
        painter.setPen(artworkPen);
        painter.drawRect(displayedArtwork);

        QPen targetPen(Theme::accent(), 2.0);
        targetPen.setCosmetic(true);
        painter.setPen(targetPen);
        painter.drawRect(displayedTarget);
    }

private:
    QSize m_currentSize;
    QSize m_targetSize;
    QPoint m_contentOffset;
};

CanvasSizeDialog::CanvasSizeDialog(const QSize &currentSize, QWidget *parent)
    : QDialog(parent)
    , m_currentSize(std::clamp(currentSize.width(),
                        minimumDialogEdge,
                        DocumentLimits::maximumCanvasEdge),
          std::clamp(currentSize.height(),
              minimumDialogEdge,
              DocumentLimits::maximumCanvasEdge))
{
    setObjectName(QStringLiteral("canvasSizeDialog"));
    setWindowTitle(tr("Change canvas size"));
    setModal(true);

    auto *mainLayout = new QVBoxLayout(this);
    mainLayout->setContentsMargins(18, 18, 18, 14);
    mainLayout->setSpacing(14);

    auto *description = new QLabel(
        tr("Change the canvas bounds without scaling the artwork."), this);
    description->setWordWrap(true);
    mainLayout->addWidget(description);

    auto *contentLayout = new QHBoxLayout;
    contentLayout->setSpacing(22);
    mainLayout->addLayout(contentLayout, 1);

    auto *controls = new QWidget(this);
    controls->setObjectName(QStringLiteral("canvasSizeControls"));
    auto *controlsLayout = new QVBoxLayout(controls);
    controlsLayout->setContentsMargins(0, 0, 0, 0);
    controlsLayout->setSpacing(10);
    contentLayout->addWidget(controls);

    auto *sizeHeading = new QLabel(tr("CANVAS SIZE"), controls);
    sizeHeading->setProperty("fieldLabel", true);
    controlsLayout->addWidget(sizeHeading);

    m_relativeCheck = new QCheckBox(tr("Relative size"), controls);
    m_relativeCheck->setObjectName(QStringLiteral("canvasRelativeSizeCheck"));
    m_relativeCheck->setAccessibleName(tr("Use relative canvas size"));
    controlsLayout->addWidget(m_relativeCheck);

    auto *sizeForm = new QFormLayout;
    sizeForm->setContentsMargins(0, 0, 0, 0);
    sizeForm->setHorizontalSpacing(14);
    sizeForm->setVerticalSpacing(8);

    m_widthLabel = new QLabel(tr("Width"), controls);
    m_widthSpin = new DimensionSpinBox(controls);
    m_widthSpin->setObjectName(QStringLiteral("canvasWidthSpin"));
    m_widthSpin->setRange(minimumDialogEdge, DocumentLimits::maximumCanvasEdge);
    m_widthSpin->setValue(m_currentSize.width());
    m_widthSpin->setSuffix(tr(" px"));
    installSuffixCaretGuard(m_widthSpin);
    m_widthSpin->setAccessibleName(tr("Canvas width"));
    m_widthLabel->setBuddy(m_widthSpin);
    sizeForm->addRow(m_widthLabel, m_widthSpin);

    m_heightLabel = new QLabel(tr("Height"), controls);
    m_heightSpin = new DimensionSpinBox(controls);
    m_heightSpin->setObjectName(QStringLiteral("canvasHeightSpin"));
    m_heightSpin->setRange(
        minimumDialogEdge, DocumentLimits::maximumCanvasEdge);
    m_heightSpin->setValue(m_currentSize.height());
    m_heightSpin->setSuffix(tr(" px"));
    installSuffixCaretGuard(m_heightSpin);
    m_heightSpin->setAccessibleName(tr("Canvas height"));
    m_heightLabel->setBuddy(m_heightSpin);
    sizeForm->addRow(m_heightLabel, m_heightSpin);
    controlsLayout->addLayout(sizeForm);

    auto *anchorHeading = new QLabel(tr("REFERENCE POINT"), controls);
    anchorHeading->setProperty("fieldLabel", true);
    controlsLayout->addWidget(anchorHeading);

    auto *placementLayout = new QHBoxLayout;
    placementLayout->setSpacing(14);

    auto *anchorWidget = new QWidget(controls);
    auto *anchorLayout = new QGridLayout(anchorWidget);
    anchorLayout->setContentsMargins(0, 0, 0, 0);
    anchorLayout->setHorizontalSpacing(3);
    anchorLayout->setVerticalSpacing(3);
    m_anchorGroup = new QButtonGroup(this);
    m_anchorGroup->setExclusive(true);

    const QStringList anchorNames = {tr("Top left"),
        tr("Top center"),
        tr("Top right"),
        tr("Middle left"),
        tr("Center"),
        tr("Middle right"),
        tr("Bottom left"),
        tr("Bottom center"),
        tr("Bottom right")};
    const QStringList anchorObjectNames = {
        QStringLiteral("canvasAnchorTopLeft"),
        QStringLiteral("canvasAnchorTopCenter"),
        QStringLiteral("canvasAnchorTopRight"),
        QStringLiteral("canvasAnchorMiddleLeft"),
        QStringLiteral("canvasAnchorCenter"),
        QStringLiteral("canvasAnchorMiddleRight"),
        QStringLiteral("canvasAnchorBottomLeft"),
        QStringLiteral("canvasAnchorBottomCenter"),
        QStringLiteral("canvasAnchorBottomRight")};
    for (int anchorId = 0; anchorId < anchorNames.size(); ++anchorId)
    {
        auto *button = new QToolButton(anchorWidget);
        button->setObjectName(anchorObjectNames[anchorId]);
        button->setText(QStringLiteral("●"));
        button->setCheckable(true);
        button->setFixedSize(30, 30);
        button->setToolTip(anchorNames[anchorId]);
        button->setAccessibleName(anchorNames[anchorId]);
        button->setStyleSheet(
            QStringLiteral("QToolButton { background: %1; color: %2; "
                           "border: 1px solid %3; border-radius: 5px; }"
                           "QToolButton:hover { background: %4; color: %5; }"
                           "QToolButton:focus { border-color: %6; }"
                           "QToolButton:checked { background: %6; color: %7; "
                           "border-color: %6; }")
                .arg(Theme::statusBackground().name(),
                    Theme::textMuted().name(),
                    Theme::border().name(),
                    Theme::hoverBackground().name(),
                    Theme::textPrimary().name(),
                    Theme::accent().name(),
                    Theme::accentText().name()));
        m_anchorGroup->addButton(button, anchorId);
        anchorLayout->addWidget(
            button, anchorId / anchorColumnCount, anchorId % anchorColumnCount);
    }
    m_anchorGroup->button(4)->setChecked(true);
    placementLayout->addWidget(anchorWidget, 0, Qt::AlignTop);

    auto *offsetForm = new QFormLayout;
    offsetForm->setContentsMargins(0, 0, 0, 0);
    offsetForm->setHorizontalSpacing(10);
    offsetForm->setVerticalSpacing(8);

    m_offsetXSpin = new DimensionSpinBox(controls);
    m_offsetXSpin->setObjectName(QStringLiteral("canvasOffsetXSpin"));
    m_offsetXSpin->setRange(
        -DocumentLimits::maximumCanvasEdge, DocumentLimits::maximumCanvasEdge);
    m_offsetXSpin->setSuffix(tr(" px"));
    installSuffixCaretGuard(m_offsetXSpin);
    static_cast<DimensionSpinBox *>(m_offsetXSpin)->setSignedDisplay(true);
    m_offsetXSpin->setAccessibleName(tr("Artwork horizontal offset"));
    offsetForm->addRow(tr("X offset"), m_offsetXSpin);

    m_offsetYSpin = new DimensionSpinBox(controls);
    m_offsetYSpin->setObjectName(QStringLiteral("canvasOffsetYSpin"));
    m_offsetYSpin->setRange(
        -DocumentLimits::maximumCanvasEdge, DocumentLimits::maximumCanvasEdge);
    m_offsetYSpin->setSuffix(tr(" px"));
    installSuffixCaretGuard(m_offsetYSpin);
    static_cast<DimensionSpinBox *>(m_offsetYSpin)->setSignedDisplay(true);
    m_offsetYSpin->setAccessibleName(tr("Artwork vertical offset"));
    offsetForm->addRow(tr("Y offset"), m_offsetYSpin);
    placementLayout->addLayout(offsetForm);
    controlsLayout->addLayout(placementLayout);
    controlsLayout->addStretch(1);

    auto *previewColumn = new QWidget(this);
    auto *previewLayout = new QVBoxLayout(previewColumn);
    previewLayout->setContentsMargins(0, 0, 0, 0);
    previewLayout->setSpacing(8);
    contentLayout->addWidget(previewColumn, 1);

    m_preview = new CanvasGeometryPreview(previewColumn);
    previewLayout->addWidget(m_preview, 1);

    m_summaryLabel = new QLabel(previewColumn);
    m_summaryLabel->setObjectName(QStringLiteral("canvasSizeSummaryLabel"));
    m_summaryLabel->setAlignment(Qt::AlignCenter);
    m_summaryLabel->setTextInteractionFlags(Qt::TextSelectableByMouse);
    previewLayout->addWidget(m_summaryLabel);

    m_offsetSummaryLabel = new QLabel(previewColumn);
    m_offsetSummaryLabel->setObjectName(
        QStringLiteral("canvasOffsetSummaryLabel"));
    m_offsetSummaryLabel->setAlignment(Qt::AlignCenter);
    m_offsetSummaryLabel->setTextInteractionFlags(Qt::TextSelectableByMouse);
    previewLayout->addWidget(m_offsetSummaryLabel);

    m_changeHintLabel = new QLabel(previewColumn);
    m_changeHintLabel->setObjectName(QStringLiteral("canvasChangeHintLabel"));
    m_changeHintLabel->setAlignment(Qt::AlignCenter);
    m_changeHintLabel->setWordWrap(true);
    m_changeHintLabel->setMinimumHeight(
        m_changeHintLabel->fontMetrics().lineSpacing() * 2);
    previewLayout->addWidget(m_changeHintLabel);

    m_buttons = new QDialogButtonBox(
        QDialogButtonBox::Ok | QDialogButtonBox::Cancel, this);
    QPushButton *okButton = m_buttons->button(QDialogButtonBox::Ok);
    okButton->setObjectName(QStringLiteral("canvasSizeOkButton"));
    okButton->setDefault(true);
    m_buttons->button(QDialogButtonBox::Cancel)
        ->setObjectName(QStringLiteral("canvasSizeCancelButton"));
    connect(m_buttons, &QDialogButtonBox::accepted, this, &QDialog::accept);
    connect(m_buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    mainLayout->addWidget(m_buttons);

    connect(m_relativeCheck,
        &QCheckBox::toggled,
        this,
        &CanvasSizeDialog::setRelativeMode);
    connect(m_widthSpin,
        &QSpinBox::valueChanged,
        this,
        [this](int)
        {
            handleSizeChanged();
        });
    connect(m_heightSpin,
        &QSpinBox::valueChanged,
        this,
        [this](int)
        {
            handleSizeChanged();
        });
    connect(m_offsetXSpin,
        &QSpinBox::valueChanged,
        this,
        [this](int)
        {
            handleOffsetChanged();
        });
    connect(m_offsetYSpin,
        &QSpinBox::valueChanged,
        this,
        [this](int)
        {
            handleOffsetChanged();
        });
    connect(m_anchorGroup,
        &QButtonGroup::idClicked,
        this,
        &CanvasSizeDialog::applyAnchor);

    applyAnchor(4);
    updatePresentation();
    resize(700, 470);
}

CanvasSizeDialog::Result CanvasSizeDialog::currentResult() const
{
    return {canvasSize(), contentOffset()};
}

QSize CanvasSizeDialog::canvasSize() const
{
    return requestedSize();
}

QPoint CanvasSizeDialog::contentOffset() const
{
    return QPoint(m_offsetXSpin->value(), m_offsetYSpin->value());
}

void CanvasSizeDialog::setRelativeMode(bool relative)
{
    const QSize targetSize =
        relative ? QSize(m_widthSpin->value(), m_heightSpin->value())
                 : QSize(m_currentSize.width() + m_widthSpin->value(),
                       m_currentSize.height() + m_heightSpin->value());
    m_syncing = true;

    auto *widthSpin = static_cast<DimensionSpinBox *>(m_widthSpin);
    auto *heightSpin = static_cast<DimensionSpinBox *>(m_heightSpin);
    widthSpin->setSignedDisplay(relative);
    heightSpin->setSignedDisplay(relative);

    if (relative)
    {
        m_widthLabel->setText(tr("Width change"));
        m_heightLabel->setText(tr("Height change"));
        m_widthSpin->setRange(minimumDialogEdge - m_currentSize.width(),
            DocumentLimits::maximumCanvasEdge - m_currentSize.width());
        m_heightSpin->setRange(minimumDialogEdge - m_currentSize.height(),
            DocumentLimits::maximumCanvasEdge - m_currentSize.height());
        m_widthSpin->setValue(targetSize.width() - m_currentSize.width());
        m_heightSpin->setValue(targetSize.height() - m_currentSize.height());
        m_widthSpin->setAccessibleName(tr("Canvas width change"));
        m_heightSpin->setAccessibleName(tr("Canvas height change"));
    }
    else
    {
        m_widthLabel->setText(tr("Width"));
        m_heightLabel->setText(tr("Height"));
        m_widthSpin->setRange(
            minimumDialogEdge, DocumentLimits::maximumCanvasEdge);
        m_heightSpin->setRange(
            minimumDialogEdge, DocumentLimits::maximumCanvasEdge);
        m_widthSpin->setValue(targetSize.width());
        m_heightSpin->setValue(targetSize.height());
        m_widthSpin->setAccessibleName(tr("Canvas width"));
        m_heightSpin->setAccessibleName(tr("Canvas height"));
    }

    m_syncing = false;
    handleSizeChanged();
}

void CanvasSizeDialog::handleSizeChanged()
{
    if (m_syncing)
    {
        return;
    }
    const int anchorId = m_anchorGroup->checkedId();
    if (anchorId >= 0)
    {
        const QPoint anchored = offsetForAnchor(anchorId, requestedSize());
        m_syncing = true;
        m_offsetXSpin->setValue(anchored.x());
        m_offsetYSpin->setValue(anchored.y());
        m_syncing = false;
    }
    updatePresentation();
}

void CanvasSizeDialog::handleOffsetChanged()
{
    if (m_syncing)
    {
        return;
    }
    syncAnchorFromOffset();
    updatePresentation();
}

void CanvasSizeDialog::applyAnchor(int anchorId)
{
    if (anchorId < 0 || anchorId >= anchorColumnCount * anchorColumnCount)
    {
        return;
    }
    const QPoint anchored = offsetForAnchor(anchorId, requestedSize());
    m_syncing = true;
    m_offsetXSpin->setValue(anchored.x());
    m_offsetYSpin->setValue(anchored.y());
    if (QAbstractButton *button = m_anchorGroup->button(anchorId))
    {
        button->setChecked(true);
    }
    m_syncing = false;
    updatePresentation();
}

void CanvasSizeDialog::syncAnchorFromOffset()
{
    const QPoint offset = contentOffset();
    int matchingId = -1;
    const int currentId = m_anchorGroup->checkedId();
    if (currentId >= 0 && offsetForAnchor(currentId, requestedSize()) == offset)
    {
        matchingId = currentId;
    }
    else
    {
        for (int anchorId = 0; anchorId < anchorColumnCount * anchorColumnCount;
            ++anchorId)
        {
            if (offsetForAnchor(anchorId, requestedSize()) == offset)
            {
                matchingId = anchorId;
                break;
            }
        }
    }

    m_anchorGroup->setExclusive(false);
    for (QAbstractButton *button : m_anchorGroup->buttons())
    {
        button->setChecked(m_anchorGroup->id(button) == matchingId);
    }
    m_anchorGroup->setExclusive(true);
}

void CanvasSizeDialog::updatePresentation()
{
    const QSize targetSize = requestedSize();
    const QPoint offset = contentOffset();
    m_summaryLabel->setText(tr("%1 × %2 px  →  %3 × %4 px")
            .arg(m_currentSize.width())
            .arg(m_currentSize.height())
            .arg(targetSize.width())
            .arg(targetSize.height()));
    m_offsetSummaryLabel->setText(tr("Artwork offset: X %1, Y %2")
            .arg(signedPixels(offset.x()), signedPixels(offset.y())));
    m_preview->setGeometryState(m_currentSize, targetSize, offset);

    const QRect targetRect(QPoint(), targetSize);
    const QRect artworkRect(offset, m_currentSize);
    if (!targetRect.intersects(artworkRect))
    {
        m_changeHintLabel->setText(
            tr("The artwork is entirely outside the new canvas."));
        m_changeHintLabel->setStyleSheet(
            QStringLiteral("color: %1;").arg(Theme::accent().name()));
    }
    else if (!targetRect.contains(artworkRect))
    {
        m_changeHintLabel->setText(
            tr("Artwork outside the new canvas will be clipped."));
        m_changeHintLabel->setStyleSheet(
            QStringLiteral("color: %1;").arg(Theme::accent().name()));
    }
    else if (targetRect != artworkRect)
    {
        m_changeHintLabel->setText(
            tr("The expanded area uses the canvas background."));
        m_changeHintLabel->setStyleSheet(
            QStringLiteral("color: %1;").arg(Theme::textMuted().name()));
    }
    else
    {
        m_changeHintLabel->setText(
            tr("The canvas and artwork bounds are unchanged."));
        m_changeHintLabel->setStyleSheet(
            QStringLiteral("color: %1;").arg(Theme::textMuted().name()));
    }

    m_buttons->button(QDialogButtonBox::Ok)
        ->setEnabled(targetSize != m_currentSize || !offset.isNull());
}

QSize CanvasSizeDialog::requestedSize() const
{
    if (m_relativeCheck && m_relativeCheck->isChecked())
    {
        return QSize(m_currentSize.width() + m_widthSpin->value(),
            m_currentSize.height() + m_heightSpin->value());
    }
    return QSize(m_widthSpin->value(), m_heightSpin->value());
}

QPoint CanvasSizeDialog::offsetForAnchor(int anchorId, const QSize &size) const
{
    const int row = anchorId / anchorColumnCount;
    const int column = anchorId % anchorColumnCount;
    return QPoint(alignedOffset(size.width() - m_currentSize.width(), column),
        alignedOffset(size.height() - m_currentSize.height(), row));
}

}
