// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/LayerDock.hpp"

#include "app/WatchedFutureResult.hpp"
#include "document/DocumentController.hpp"
#include "document/DocumentLimits.hpp"
#include "document/LayerHierarchy.hpp"
#include "render/LayerThumbnailRenderer.hpp"
#include "ui/ElidingCheckBox.hpp"
#include "ui/Icons.hpp"
#include "ui/LayerItemDelegate.hpp"
#include "ui/LayerListWidget.hpp"
#include "ui/PaletteDockTitleBar.hpp"
#include "ui/ResponsiveGrid.hpp"

#include <QAbstractItemView>
#include <QCheckBox>
#include <QComboBox>
#include <QEvent>
#include <QFormLayout>
#include <QFutureWatcher>
#include <QHBoxLayout>
#include <QLabel>
#include <QListWidgetItem>
#include <QScopedValueRollback>
#include <QSignalBlocker>
#include <QSlider>
#include <QToolButton>
#include <QVBoxLayout>
#include <QVariant>
#include <QWidget>
#include <QtConcurrentRun>

#include <cmath>
#include <functional>
#include <optional>
#include <utility>

namespace ugurugu
{

namespace
{

constexpr int propertyComboContentsLength = 6;

// Enough for a drop-down arrow or the opacity readout at normal metrics.
// Below it the control clips, which only happens once the panel is squeezed
// to the width the dock declares.
constexpr int paletteControlMinimumWidth = 48;

// A control that reports a text-derived minimum pins the whole palette dock
// area open: a combo box reports its widest entry, which the user names, and a
// label reports the digits it reserves. Both grow with the platform's font
// metrics and the active translation. An explicit minimum width replaces that
// hint outright in the layout, so the panel stops following its own text; the
// policy change is what lets the layout shrink the control that far.
void pinPaletteControlWidth(QWidget *control)
{
    control->setSizePolicy(
        QSizePolicy::Preferred, control->sizePolicy().verticalPolicy());
    control->setMinimumWidth(paletteControlMinimumWidth);
}

void makePropertyComboShrinkable(QComboBox *combo)
{
    combo->setMinimumContentsLength(propertyComboContentsLength);
    pinPaletteControlWidth(combo);
}

QString blendModeName(LayerBlendMode mode)
{
    switch (mode)
    {
    case LayerBlendMode::Multiply:
        return LayerDock::tr("Multiply");
    case LayerBlendMode::Screen:
        return LayerDock::tr("Screen");
    case LayerBlendMode::Overlay:
        return LayerDock::tr("Overlay");
    case LayerBlendMode::Normal:
        break;
    }
    return LayerDock::tr("Normal");
}

// A layer is held still by overriding the drawing with a zero amount. Leaving
// the override unset means it follows the drawing instead.
bool holdsStill(const std::optional<qreal> &wobbleAmount)
{
    if (!wobbleAmount.has_value())
    {
        return false;
    }
    return wobbleAmount.value() <= 0.0;
}

QToolButton *makeLayerButton(
    QWidget *parent, IconGlyph glyph, const QString &name, const QString &label)
{
    auto *button = new QToolButton(parent);
    button->setObjectName(name);
    button->setIcon(Icons::icon(glyph));
    button->setIconSize(QSize(16, 16));
    button->setToolTip(label);
    button->setAccessibleName(label);
    return button;
}

}

LayerDock::LayerDock(DocumentController *controller, QWidget *parent)
    : QDockWidget(tr("Layers"), parent)
    , m_controller(controller)
{
    setObjectName(QStringLiteral("LayerDock"));
    setAllowedAreas(Qt::LeftDockWidgetArea | Qt::RightDockWidgetArea);
    setMinimumWidth(150);
    installCompactPaletteTitleBar(this);

    buildContent();
    connectControls();

    m_thumbnailTimer.setSingleShot(true);
    m_thumbnailTimer.setInterval(180);
    connect(&m_thumbnailTimer,
        &QTimer::timeout,
        this,
        &LayerDock::regenerateThumbnails);

    rebuild();
    scheduleAllThumbnails();
}

void LayerDock::buildContent()
{
    auto *content = new QWidget(this);
    content->setObjectName(QStringLiteral("LayerDockBody"));
    auto *layout = new QVBoxLayout(content);
    layout->setContentsMargins(0, 0, 0, 8);
    layout->setSpacing(8);

    m_layerList = new LayerListWidget(content);
    m_layerList->setAccessibleName(tr("Layers"));
    m_layerList->setEditTriggers(
        QAbstractItemView::DoubleClicked | QAbstractItemView::EditKeyPressed);
    m_layerList->setItemDelegate(new LayerItemDelegate(m_layerList));
    layout->addWidget(m_layerList, 1);

    auto *buttonGrid = new ResponsiveGrid(30, 7, 2, content);
    buttonGrid->setObjectName(QStringLiteral("layerButtonGrid"));
    buttonGrid->setContentsMargins(8, 0, 8, 0);

    m_addButton = makeLayerButton(content,
        IconGlyph::Add,
        QStringLiteral("layerAddButton"),
        tr("Add layer"));
    buttonGrid->addWidget(m_addButton);

    m_addGroupButton = makeLayerButton(content,
        IconGlyph::Add,
        QStringLiteral("layerAddGroupButton"),
        tr("Add group containing the selected layer"));
    m_addGroupButton->setText(QStringLiteral("G"));
    m_addGroupButton->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    buttonGrid->addWidget(m_addGroupButton);

    m_duplicateButton = makeLayerButton(content,
        IconGlyph::Duplicate,
        QStringLiteral("layerDuplicateButton"),
        tr("Duplicate layer"));
    buttonGrid->addWidget(m_duplicateButton);

    m_mergeDownButton = makeLayerButton(content,
        IconGlyph::MoveDown,
        QStringLiteral("layerMergeDownButton"),
        tr("Merge with layer below"));
    buttonGrid->addWidget(m_mergeDownButton);

    m_deleteButton = makeLayerButton(content,
        IconGlyph::Remove,
        QStringLiteral("layerDeleteButton"),
        tr("Delete layer"));
    buttonGrid->addWidget(m_deleteButton);

    m_moveUpButton = makeLayerButton(content,
        IconGlyph::MoveUp,
        QStringLiteral("layerMoveUpButton"),
        tr("Move layer up"));
    buttonGrid->addWidget(m_moveUpButton);

    m_moveDownButton = makeLayerButton(content,
        IconGlyph::MoveDown,
        QStringLiteral("layerMoveDownButton"),
        tr("Move layer down"));
    buttonGrid->addWidget(m_moveDownButton);

    layout->addWidget(buttonGrid);

    auto *properties = new QFormLayout;
    properties->setContentsMargins(10, 0, 10, 0);
    properties->setSpacing(6);
    properties->setRowWrapPolicy(QFormLayout::WrapLongRows);

    auto *blendModeLabel = new QLabel(tr("BLEND MODE"), content);
    blendModeLabel->setProperty("fieldLabel", true);
    m_blendModeCombo = new QComboBox(content);
    m_blendModeCombo->setObjectName(QStringLiteral("layerBlendModeCombo"));
    m_blendModeCombo->setAccessibleName(tr("Layer blend mode"));
    makePropertyComboShrinkable(m_blendModeCombo);
    for (LayerBlendMode mode : {LayerBlendMode::Normal,
             LayerBlendMode::Multiply,
             LayerBlendMode::Screen,
             LayerBlendMode::Overlay})
    {
        m_blendModeCombo->addItem(blendModeName(mode), static_cast<int>(mode));
    }
    blendModeLabel->setBuddy(m_blendModeCombo);
    properties->addRow(blendModeLabel, m_blendModeCombo);

    auto *groupLabel = new QLabel(tr("GROUP"), content);
    groupLabel->setProperty("fieldLabel", true);
    m_parentGroupCombo = new QComboBox(content);
    m_parentGroupCombo->setObjectName(QStringLiteral("layerParentGroupCombo"));
    m_parentGroupCombo->setAccessibleName(tr("Parent layer group"));
    makePropertyComboShrinkable(m_parentGroupCombo);
    groupLabel->setBuddy(m_parentGroupCombo);
    properties->addRow(groupLabel, m_parentGroupCombo);

    m_clipCheck = new ElidingCheckBox(tr("Clip to layer below"), content);
    m_clipCheck->setObjectName(QStringLiteral("layerClipCheck"));
    m_clipCheck->setToolTip(
        tr("Limit this layer to the opacity of the base layer below it"));
    properties->addRow(m_clipCheck);

    m_referenceCheck = new ElidingCheckBox(tr("Reference layer"), content);
    m_referenceCheck->setObjectName(QStringLiteral("layerReferenceCheck"));
    m_referenceCheck->setToolTip(
        tr("Use this layer when a selection tool references marked layers"));
    properties->addRow(m_referenceCheck);

    auto *opacityControls = new QWidget(content);
    auto *opacityLayout = new QHBoxLayout(opacityControls);
    opacityLayout->setContentsMargins(0, 0, 0, 0);
    opacityLayout->setSpacing(6);

    auto *opacityLabel = new QLabel(tr("OPACITY"), content);
    opacityLabel->setProperty("fieldLabel", true);
    m_opacitySlider = new QSlider(Qt::Horizontal, content);
    m_opacitySlider->setRange(0, 100);
    m_opacitySlider->setSingleStep(1);
    m_opacitySlider->setPageStep(10);
    m_opacitySlider->setTracking(true);
    opacityLabel->setBuddy(m_opacitySlider);
    opacityLayout->addWidget(m_opacitySlider, 1);

    m_opacityValue = new QLabel(content);
    pinPaletteControlWidth(m_opacityValue);
    m_opacityValue->setAlignment(Qt::AlignRight | Qt::AlignVCenter);
    opacityLayout->addWidget(m_opacityValue);

    properties->addRow(opacityLabel, opacityControls);
    layout->addLayout(properties);
    setWidget(content);
}

void LayerDock::connectControls()
{
    connect(m_layerList,
        &QListWidget::currentItemChanged,
        this,
        [this](QListWidgetItem *current)
        {
            if (m_syncing || !m_controller || !current)
            {
                updateControls();
                return;
            }
            const QUuid id = current->data(LayerItemRoles::LayerId).toUuid();
            m_selectedLayerId = id;
            const Layer *layer = m_controller->document().layer(id);
            if (layer && layer->kind == LayerKind::Paint)
            {
                m_controller->setActiveLayer(id);
            }
            updateControls();
        });

    connect(m_layerList,
        &QListWidget::itemChanged,
        this,
        [this](QListWidgetItem *item)
        {
            if (m_syncing || !m_controller || !item)
            {
                return;
            }
            const QUuid id = item->data(LayerItemRoles::LayerId).toUuid();
            const Layer *layer = m_controller->document().layer(id);
            if (!layer)
            {
                return;
            }
            const QString name = item->text().trimmed();
            if (!name.isEmpty() && name != layer->name)
            {
                const DocumentController::RenameLayerResult result =
                    m_controller->renameLayer(id, name);
                if (result != DocumentController::RenameLayerResult::Renamed)
                {
                    const Layer *currentLayer =
                        m_controller->document().layer(id);
                    if (currentLayer)
                    {
                        QSignalBlocker blocker(m_layerList);
                        item->setText(currentLayer->name);
                    }
                    else
                    {
                        rebuild();
                    }
                }
            }
            else if (item->text() != layer->name)
            {
                QSignalBlocker blocker(m_layerList);
                item->setText(layer->name);
            }
        });

    auto *delegate =
        qobject_cast<LayerItemDelegate *>(m_layerList->itemDelegate());
    const auto toggleVisibility = [this](const QModelIndex &index)
    {
        if (!m_controller)
        {
            return;
        }
        const QUuid id = index.data(LayerItemRoles::LayerId).toUuid();
        const bool visible = index.data(LayerItemRoles::Visible).toBool();
        if (!id.isNull())
        {
            m_controller->setLayerVisible(id, !visible);
        }
    };
    connect(delegate,
        &LayerItemDelegate::visibilityToggled,
        this,
        toggleVisibility);
    connect(m_layerList,
        &LayerListWidget::visibilityToggleRequested,
        this,
        toggleVisibility);
    connect(delegate,
        &LayerItemDelegate::wobbleToggled,
        this,
        [this](const QModelIndex &index)
        {
            if (!m_controller)
            {
                return;
            }
            const QUuid id = index.data(LayerItemRoles::LayerId).toUuid();
            const Layer *layer = m_controller->document().layer(id);
            if (!layer)
            {
                return;
            }
            // An override carries both halves or neither, so stopping a layer
            // pins the drawing's motion alongside the zero amount and
            // restarting it drops the pair.
            if (holdsStill(layer->wobbleAmount))
            {
                m_controller->setLayerWobbleOverride(id, {}, {});
                return;
            }
            m_controller->setLayerWobbleOverride(id,
                0.0,
                layer->motion.value_or(m_controller->document().motion));
        });

    connect(m_layerList,
        &LayerListWidget::dropRequested,
        this,
        &LayerDock::handleLayerDrop);

    connect(m_addButton,
        &QToolButton::clicked,
        this,
        [this]()
        {
            if (m_controller)
            {
                const Layer *selected =
                    m_controller->document().layer(selectedLayerId());
                m_controller->addLayer(
                    selected && selected->kind == LayerKind::Group
                        ? selected->id
                        : QUuid());
            }
        });
    connect(m_addGroupButton,
        &QToolButton::clicked,
        this,
        [this]()
        {
            if (m_controller)
            {
                m_controller->addLayerGroup(selectedLayerId());
            }
        });
    connect(m_duplicateButton,
        &QToolButton::clicked,
        this,
        [this]()
        {
            const QUuid id = selectedLayerId();
            if (m_controller && !id.isNull())
            {
                m_controller->duplicateLayer(id);
            }
        });
    connect(m_mergeDownButton,
        &QToolButton::clicked,
        this,
        [this]()
        {
            const QUuid id = selectedLayerId();
            if (m_controller && !id.isNull())
            {
                m_controller->mergeLayerDown(id);
            }
        });
    connect(m_deleteButton,
        &QToolButton::clicked,
        this,
        [this]()
        {
            const QUuid id = selectedLayerId();
            if (m_controller && !id.isNull())
            {
                m_controller->removeLayer(id);
            }
        });
    connect(m_moveUpButton,
        &QToolButton::clicked,
        this,
        [this]()
        {
            const QUuid id = selectedLayerId();
            if (m_controller && !id.isNull())
            {
                m_controller->moveLayer(id, 1);
            }
        });
    connect(m_moveDownButton,
        &QToolButton::clicked,
        this,
        [this]()
        {
            const QUuid id = selectedLayerId();
            if (m_controller && !id.isNull())
            {
                m_controller->moveLayer(id, -1);
            }
        });

    connect(m_opacitySlider,
        &QSlider::sliderPressed,
        this,
        [this]()
        {
            if (m_syncing)
            {
                return;
            }
            m_opacityDragging = true;
            m_opacityLayerId = selectedLayerId();
        });
    connect(m_opacitySlider,
        &QSlider::sliderReleased,
        this,
        [this]()
        {
            if (!m_opacityDragging)
            {
                return;
            }
            const QUuid id = m_opacityLayerId;
            m_opacityDragging = false;
            m_opacityLayerId = QUuid();
            commitOpacity(id, m_opacitySlider->value());
        });
    connect(m_opacitySlider,
        &QSlider::valueChanged,
        this,
        [this](int value)
        {
            m_opacityValue->setText(tr("%1%").arg(value));
            if (!m_syncing && !m_opacityDragging)
            {
                commitOpacity(selectedLayerId(), value);
            }
        });

    connect(m_blendModeCombo,
        &QComboBox::currentIndexChanged,
        this,
        [this](int index)
        {
            if (!m_controller || m_syncing || index < 0)
            {
                return;
            }
            const QUuid id = selectedLayerId();
            if (!id.isNull())
            {
                m_controller->setLayerBlendMode(id,
                    static_cast<LayerBlendMode>(
                        m_blendModeCombo->itemData(index).toInt()));
            }
        });

    connect(m_parentGroupCombo,
        &QComboBox::currentIndexChanged,
        this,
        [this](int index)
        {
            if (!m_controller || m_syncing || index < 0)
            {
                return;
            }
            const QUuid id = selectedLayerId();
            if (!id.isNull())
            {
                m_controller->setLayerParentGroup(
                    id, m_parentGroupCombo->itemData(index).toUuid());
            }
        });

    connect(m_clipCheck,
        &QCheckBox::toggled,
        this,
        [this](bool clipped)
        {
            if (!m_controller || m_syncing)
            {
                return;
            }
            const QUuid id = selectedLayerId();
            if (!id.isNull())
            {
                m_controller->setLayerClipToBelow(id, clipped);
            }
        });

    connect(m_referenceCheck,
        &QCheckBox::toggled,
        this,
        [this](bool reference)
        {
            if (!m_controller || m_syncing)
            {
                return;
            }
            const QUuid id = selectedLayerId();
            if (!id.isNull())
            {
                m_controller->setLayerReference(id, reference);
            }
        });

    if (m_controller)
    {
        connect(m_controller,
            &DocumentController::documentChanged,
            this,
            [this]()
            {
                rebuild();
            });
        connect(m_controller,
            &DocumentController::layerThumbnailChanged,
            this,
            &LayerDock::scheduleLayerThumbnail);
        connect(m_controller,
            &DocumentController::layerThumbnailsReset,
            this,
            &LayerDock::scheduleAllThumbnails);
        connect(m_controller,
            &DocumentController::activeLayerChanged,
            this,
            &LayerDock::syncActiveLayer);
        connect(m_controller,
            &QObject::destroyed,
            this,
            [this]()
            {
                m_controller = nullptr;
                rebuild();
            });
    }
}

void LayerDock::rebuild()
{
    QScopedValueRollback syncing(m_syncing, true);
    QSignalBlocker blocker(m_layerList);
    struct DisplayLayer
    {
        const Layer *layer = nullptr;
        int depth = 0;
    };
    QVector<DisplayLayer> displayLayers;
    if (m_controller)
    {
        const Document &document = m_controller->document();
        if (!document.layer(m_selectedLayerId))
        {
            m_selectedLayerId = document.activeLayerId;
        }
        std::function<void(const QUuid &, int)> collectChildren;
        collectChildren = [&](const QUuid &parentId, int depth)
        {
            for (int index = static_cast<int>(document.layers.size()) - 1;
                index >= 0;
                --index)
            {
                const Layer &layer = document.layers[index];
                if (layer.parentGroupId != parentId)
                {
                    continue;
                }
                displayLayers.append({&layer, depth});
                if (layer.kind == LayerKind::Group)
                {
                    collectChildren(layer.id, depth + 1);
                }
            }
        };
        collectChildren({}, 0);
    }

    QSet<QUuid> displayedIds;
    for (const DisplayLayer &display : std::as_const(displayLayers))
    {
        displayedIds.insert(display.layer->id);
    }
    for (int row = m_layerList->count() - 1; row >= 0; --row)
    {
        const QUuid id =
            m_layerList->item(row)->data(LayerItemRoles::LayerId).toUuid();
        if (!displayedIds.contains(id))
        {
            delete m_layerList->takeItem(row);
        }
    }

    QListWidgetItem *selectedItem = nullptr;
    for (int targetRow = 0; targetRow < displayLayers.size(); ++targetRow)
    {
        const DisplayLayer &display = displayLayers[targetRow];
        const Layer &layer = *display.layer;
        int existingRow = -1;
        for (int row = 0; row < m_layerList->count(); ++row)
        {
            if (m_layerList->item(row)->data(LayerItemRoles::LayerId).toUuid()
                == layer.id)
            {
                existingRow = row;
                break;
            }
        }
        QListWidgetItem *item = nullptr;
        if (existingRow < 0)
        {
            item = new QListWidgetItem;
            m_layerList->insertItem(targetRow, item);
        }
        else if (existingRow != targetRow)
        {
            item = m_layerList->takeItem(existingRow);
            m_layerList->insertItem(targetRow, item);
        }
        else
        {
            item = m_layerList->item(existingRow);
        }
        item->setText(layer.name);
        item->setData(LayerItemRoles::LayerId, QVariant::fromValue(layer.id));
        item->setData(LayerItemRoles::Visible, layer.visible);
        item->setData(Qt::AccessibleDescriptionRole,
            layer.visible ? tr("Layer is visible") : tr("Layer is hidden"));
        item->setData(LayerItemRoles::Thumbnail,
            QVariant::fromValue(m_thumbnails.value(layer.id)));
        item->setData(LayerItemRoles::Kind, static_cast<int>(layer.kind));
        item->setData(LayerItemRoles::Depth, display.depth);
        item->setData(LayerItemRoles::Clipped, layer.clipToLayerBelow);
        item->setData(LayerItemRoles::Reference, layer.reference);
        item->setData(LayerItemRoles::OpacityPercent,
            qRound(std::clamp(layer.opacity, 0.0, 1.0) * 100.0));
        item->setData(
            LayerItemRoles::BlendModeName, blendModeName(layer.blendMode));
        item->setData(
            LayerItemRoles::WobbleToggleable, layer.kind == LayerKind::Paint);
        item->setData(
            LayerItemRoles::WobbleStopped, holdsStill(layer.wobbleAmount));
        item->setFlags(
            item->flags() | Qt::ItemIsEditable | Qt::ItemIsDragEnabled);
        if (layer.id == m_selectedLayerId)
        {
            selectedItem = item;
        }
    }

    m_layerList->setCurrentItem(selectedItem);
    m_layerList->setEnabled(m_layerList->count() > 0);
    updateControls();
}

void LayerDock::syncActiveLayer(const QUuid &id)
{
    m_selectedLayerId = id;
    QScopedValueRollback syncing(m_syncing, true);
    QSignalBlocker blocker(m_layerList);
    QListWidgetItem *match = nullptr;

    for (int row = 0; row < m_layerList->count(); ++row)
    {
        QListWidgetItem *item = m_layerList->item(row);
        if (item->data(LayerItemRoles::LayerId).toUuid() == id)
        {
            match = item;
            break;
        }
    }

    m_layerList->setCurrentItem(match);
    updateControls();
}

void LayerDock::updateControls()
{
    const QUuid id = selectedLayerId();
    const Document *document =
        m_controller ? &m_controller->document() : nullptr;
    const Layer *layer = document ? document->layer(id) : nullptr;
    const bool hasLayer = layer != nullptr;
    const bool paintLayer = layer && layer->kind == LayerKind::Paint;
    const bool groupSelected = layer && layer->kind == LayerKind::Group;
    if (groupSelected != m_groupSelectionActive)
    {
        m_groupSelectionActive = groupSelected;
        emit groupSelectionChanged(groupSelected);
    }
    const bool hasCapacity =
        document && document->layers.size() < DocumentLimits::maximumLayers;
    const LayerHierarchyAnalysis hierarchy =
        document ? analyzeLayerHierarchy(*document) : LayerHierarchyAnalysis();
    const int maximumEditableDepth =
        hierarchy.isValid() ? std::max(hierarchy.maximumDepth(),
                                  DocumentLimits::maximumLayerDepth)
                            : -1;
    const bool canAddInsideSelectedGroup =
        !layer || layer->kind != LayerKind::Group
        || (hierarchy.isValid() && hierarchy.depth(layer->id) >= 0
            && hierarchy.depth(layer->id) + 1 <= maximumEditableDepth);
    const bool canWrapSelectedLayer =
        !layer
        || (hierarchy.isValid() && hierarchy.depth(layer->id) >= 0
            && hierarchy.subtreeHeight(layer->id) >= 0
            && hierarchy.depth(layer->id) + hierarchy.subtreeHeight(layer->id)
                       + 1
                   <= maximumEditableDepth);

    QVector<const Layer *> siblings;
    if (document && layer)
    {
        for (const Layer &candidate : document->layers)
        {
            if (candidate.parentGroupId == layer->parentGroupId)
            {
                siblings.append(&candidate);
            }
        }
    }
    const int siblingPosition =
        layer ? static_cast<int>(std::find_if(siblings.cbegin(),
                                     siblings.cend(),
                                     [layer](const Layer *candidate)
                                     {
                                         return candidate->id == layer->id;
                                     })
                                 - siblings.cbegin())
              : -1;

    m_addButton->setEnabled(
        m_controller && hasCapacity && canAddInsideSelectedGroup);
    m_addGroupButton->setEnabled(
        m_controller && hasCapacity && canWrapSelectedLayer);
    m_duplicateButton->setEnabled(paintLayer && hasCapacity);
    const DocumentController::MergeLayerDownStatus mergeStatus =
        m_controller ? m_controller->mergeLayerDownStatus(id)
                     : DocumentController::MergeLayerDownStatus::MissingLayer;
    m_mergeDownButton->setEnabled(
        mergeStatus == DocumentController::MergeLayerDownStatus::Available);
    QString mergeToolTip = tr("Merge with layer below");
    switch (mergeStatus)
    {
    case DocumentController::MergeLayerDownStatus::Available:
        break;
    case DocumentController::MergeLayerDownStatus::MissingLayer:
        mergeToolTip = tr("Select a paint layer to merge");
        break;
    case DocumentController::MergeLayerDownStatus::NoPaintLayerBelow:
        mergeToolTip = tr("No paint layer is directly below");
        break;
    case DocumentController::MergeLayerDownStatus::UnsupportedProperties:
        mergeToolTip = tr("Both layers must use matching safe properties");
        break;
    case DocumentController::MergeLayerDownStatus::UnsupportedStrokes:
        mergeToolTip =
            tr("This layer resizes the canvas or erased merged artwork, "
               "so merging would change the picture");
        break;
    case DocumentController::MergeLayerDownStatus::IncompatibleCanvasEpoch:
        mergeToolTip = tr("The layers use incompatible canvas histories");
        break;
    case DocumentController::MergeLayerDownStatus::StrokeLimit:
        mergeToolTip = tr("The merged layer would exceed the stroke limit");
        break;
    }
    m_mergeDownButton->setToolTip(mergeToolTip);
    m_mergeDownButton->setAccessibleName(mergeToolTip);
    m_deleteButton->setEnabled(hasLayer);
    m_moveUpButton->setEnabled(hasLayer && siblingPosition >= 0
                               && siblingPosition < siblings.size() - 1);
    m_moveDownButton->setEnabled(hasLayer && siblingPosition > 0);
    m_blendModeCombo->setEnabled(hasLayer);
    m_opacitySlider->setEnabled(hasLayer);
    m_clipCheck->setEnabled(paintLayer);
    m_referenceCheck->setEnabled(paintLayer);

    {
        const QSignalBlocker blocker(m_parentGroupCombo);
        m_parentGroupCombo->clear();
        m_parentGroupCombo->addItem(
            tr("No group"), QVariant::fromValue(QUuid()));
        if (document && layer)
        {
            for (const Layer &candidate : document->layers)
            {
                if (candidate.kind != LayerKind::Group
                    || candidate.id == layer->id
                    || hierarchy.isDescendantOf(candidate.id, layer->id)
                    || hierarchy.depth(candidate.id) < 0
                    || hierarchy.subtreeHeight(layer->id) < 0
                    || hierarchy.depth(candidate.id) + 1
                               + hierarchy.subtreeHeight(layer->id)
                           > maximumEditableDepth)
                {
                    continue;
                }
                m_parentGroupCombo->addItem(
                    candidate.name, QVariant::fromValue(candidate.id));
            }
        }
        m_parentGroupCombo->setCurrentIndex(
            layer ? m_parentGroupCombo->findData(
                        QVariant::fromValue(layer->parentGroupId))
                  : -1);
    }
    m_parentGroupCombo->setEnabled(hasLayer);
    {
        const QSignalBlocker blocker(m_clipCheck);
        m_clipCheck->setChecked(layer && layer->clipToLayerBelow);
    }
    {
        const QSignalBlocker blocker(m_referenceCheck);
        m_referenceCheck->setChecked(layer && layer->reference);
    }

    {
        const QSignalBlocker blocker(m_blendModeCombo);
        const int blendModeIndex =
            layer
                ? m_blendModeCombo->findData(static_cast<int>(layer->blendMode))
                : -1;
        m_blendModeCombo->setCurrentIndex(blendModeIndex);
    }

    if (!m_opacityDragging || m_opacityLayerId != id)
    {
        const int opacity =
            layer ? static_cast<int>(std::lround(layer->opacity * 100.0)) : 0;
        QSignalBlocker blocker(m_opacitySlider);
        m_opacitySlider->setValue(opacity);
        m_opacityValue->setText(tr("%1%").arg(opacity));
    }
}

void LayerDock::regenerateThumbnails()
{
    if (!m_controller)
    {
        m_thumbnails.clear();
        m_pendingThumbnails.clear();
        m_thumbnailRevisions.clear();
        m_regenerateAllThumbnails = false;
        return;
    }

    const Document &document = m_controller->document();
    QSet<QUuid> existing;
    for (const Layer &layer : document.layers)
    {
        existing.insert(layer.id);
        if (m_regenerateAllThumbnails)
        {
            queueThumbnail(layer.id);
        }
    }
    for (auto iterator = m_thumbnails.begin(); iterator != m_thumbnails.end();)
    {
        if (!existing.contains(iterator.key()))
        {
            m_thumbnailRevisions.remove(iterator.key());
            iterator = m_thumbnails.erase(iterator);
        }
        else
        {
            ++iterator;
        }
    }
    for (auto iterator = m_thumbnailRevisions.begin();
        iterator != m_thumbnailRevisions.end();)
    {
        if (!existing.contains(iterator.key()))
        {
            iterator = m_thumbnailRevisions.erase(iterator);
        }
        else
        {
            ++iterator;
        }
    }
    m_regenerateAllThumbnails = false;

    if (m_pendingThumbnails.isEmpty() || m_thumbnailRendering)
    {
        return;
    }

    const QUuid nextId = *m_pendingThumbnails.constBegin();
    m_pendingThumbnails.remove(nextId);
    const Layer *layer = document.layer(nextId);
    if (!layer)
    {
        m_thumbnailRevisions.remove(nextId);
        m_thumbnailTimer.start(0);
        return;
    }
    const quint64 revision = m_thumbnailRevisions.value(nextId);
    const Document snapshot = document;
    // Read on this thread: the worker has no widget to ask, and the answer has
    // to be the one the finished pixmap is tagged with.
    const qreal ratio = devicePixelRatioF();
    m_thumbnailRendering = true;
    // QObject parenting retains the watcher until its finished callback queues
    // deleteLater(); the analyzer does not model either Qt ownership mechanism.
    // NOLINTBEGIN(clang-analyzer-cplusplus.NewDeleteLeaks)
    auto *watcher = new QFutureWatcher<QImage>(this);
    connect(watcher,
        &QFutureWatcher<QImage>::finished,
        this,
        [this, watcher, nextId, revision, ratio]()
        {
            const QImage image = watchedFutureResult(*watcher);
            watcher->deleteLater();
            m_thumbnailRendering = false;
            if (m_controller && m_thumbnailRevisions.value(nextId) == revision
                && m_controller->document().layer(nextId) && !image.isNull())
            {
                QPixmap pixmap = QPixmap::fromImage(image);
                pixmap.setDevicePixelRatio(ratio);
                m_thumbnails.insert(nextId, std::move(pixmap));
                QScopedValueRollback syncing(m_syncing, true);
                QSignalBlocker blocker(m_layerList);
                for (int row = 0; row < m_layerList->count(); ++row)
                {
                    QListWidgetItem *item = m_layerList->item(row);
                    if (item->data(LayerItemRoles::LayerId).toUuid() != nextId)
                    {
                        continue;
                    }
                    item->setData(LayerItemRoles::Thumbnail,
                        QVariant::fromValue(m_thumbnails.value(nextId)));
                    break;
                }
                m_layerList->viewport()->update();
            }
            if (!m_pendingThumbnails.isEmpty() || m_regenerateAllThumbnails)
            {
                m_thumbnailTimer.start(0);
            }
        });
    watcher->setFuture(QtConcurrent::run(
        [snapshot, nextId, ratio]()
        {
            const Layer *snapshotLayer = snapshot.layer(nextId);
            return snapshotLayer ? LayerThumbnailRenderer::renderImage(
                                       snapshot, *snapshotLayer, ratio)
                                 : QImage();
        }));
}
// NOLINTEND(clang-analyzer-cplusplus.NewDeleteLeaks)

bool LayerDock::event(QEvent *event)
{
    // Thumbnails already on screen hold pixels for the scale of the display
    // they were rendered for. Following the window to another one means
    // rendering them again rather than letting Qt resample the old ones.
    if (event->type() == QEvent::DevicePixelRatioChange)
    {
        m_thumbnails.clear();
        scheduleAllThumbnails();
    }
    return QDockWidget::event(event);
}

void LayerDock::scheduleAllThumbnails()
{
    m_regenerateAllThumbnails = true;
    m_pendingThumbnails.clear();
    const quint64 revision = ++m_nextThumbnailRevision;
    if (m_controller)
    {
        for (const Layer &layer : m_controller->document().layers)
        {
            m_thumbnailRevisions.insert(layer.id, revision);
        }
    }
    m_thumbnailTimer.start(180);
}

void LayerDock::scheduleLayerThumbnail(const QUuid &id)
{
    if (!id.isNull() && !m_regenerateAllThumbnails)
    {
        queueThumbnail(id);
        if (m_controller)
        {
            const Document &document = m_controller->document();
            const Layer *layer = document.layer(id);
            while (layer && !layer->parentGroupId.isNull())
            {
                queueThumbnail(layer->parentGroupId);
                layer = document.layer(layer->parentGroupId);
            }
        }
    }
    m_thumbnailTimer.start(180);
}

void LayerDock::queueThumbnail(const QUuid &id)
{
    if (id.isNull())
    {
        return;
    }
    m_pendingThumbnails.insert(id);
    m_thumbnailRevisions.insert(id, ++m_nextThumbnailRevision);
}

void LayerDock::commitOpacity(const QUuid &id, int value)
{
    if (!m_controller || id.isNull())
    {
        return;
    }
    m_controller->setLayerOpacity(id, static_cast<qreal>(value) / 100.0);
}

void LayerDock::handleLayerDrop(
    int sourceRow, int targetRow, LayerListWidget::DropPlacement placement)
{
    if (!m_controller || sourceRow < 0 || sourceRow >= m_layerList->count())
    {
        return;
    }
    const QUuid id =
        m_layerList->item(sourceRow)->data(LayerItemRoles::LayerId).toUuid();
    const Document &document = m_controller->document();
    const Layer *source = document.layer(id);
    if (id.isNull() || !source)
    {
        return;
    }

    QUuid targetParentId;
    QUuid anchorId;
    bool insertBelowAnchor = false;
    bool intoGroup = false;
    if (targetRow >= 0 && targetRow < m_layerList->count())
    {
        const QUuid rowId = m_layerList->item(targetRow)
                                ->data(LayerItemRoles::LayerId)
                                .toUuid();
        const Layer *anchor = document.layer(rowId);
        if (!anchor || anchor->id == id)
        {
            return;
        }
        intoGroup = anchor->kind == LayerKind::Group
                    && placement != LayerListWidget::DropPlacement::AboveTarget;
        if (intoGroup)
        {
            targetParentId = anchor->id;
        }
        else
        {
            targetParentId = anchor->parentGroupId;
            anchorId = anchor->id;
            insertBelowAnchor =
                placement == LayerListWidget::DropPlacement::BelowTarget;
        }
    }
    else if (targetRow >= 0)
    {
        return;
    }

    const bool reparent = targetParentId != source->parentGroupId;
    if (reparent)
    {
        const LayerHierarchyAnalysis hierarchy =
            analyzeLayerHierarchy(document);
        if (!hierarchy.isValid())
        {
            return;
        }
        if (!targetParentId.isNull()
            && (targetParentId == id
                || hierarchy.isDescendantOf(targetParentId, id)))
        {
            return;
        }
        const int maximumEditableDepth = std::max(
            hierarchy.maximumDepth(), DocumentLimits::maximumLayerDepth);
        const int parentDepth =
            targetParentId.isNull() ? -1 : hierarchy.depth(targetParentId);
        if ((!targetParentId.isNull() && parentDepth < 0)
            || hierarchy.subtreeHeight(id) < 0
            || parentDepth + 1 + hierarchy.subtreeHeight(id)
                   > maximumEditableDepth)
        {
            return;
        }
    }

    QVector<QUuid> orderedSiblings;
    for (int row = 0; row < m_layerList->count(); ++row)
    {
        const QUuid rowId =
            m_layerList->item(row)->data(LayerItemRoles::LayerId).toUuid();
        const Layer *layer = document.layer(rowId);
        if (layer && layer->parentGroupId == targetParentId && rowId != id)
        {
            orderedSiblings.append(rowId);
        }
    }
    int insertPosition =
        intoGroup ? 0 : static_cast<int>(orderedSiblings.size());
    if (!anchorId.isNull())
    {
        const int anchorPosition =
            static_cast<int>(orderedSiblings.indexOf(anchorId));
        if (anchorPosition < 0)
        {
            return;
        }
        insertPosition =
            insertBelowAnchor ? anchorPosition + 1 : anchorPosition;
    }

    const auto siblingVectorPosition =
        [](const Document &state, const QUuid &parentId, const QUuid &layerId)
    {
        int currentPosition = 0;
        for (const Layer &candidate : state.layers)
        {
            if (candidate.parentGroupId != parentId)
            {
                continue;
            }
            if (candidate.id == layerId)
            {
                return currentPosition;
            }
            ++currentPosition;
        }
        return -1;
    };
    const int desiredVectorPosition =
        static_cast<int>(orderedSiblings.size()) - insertPosition;

    if (!reparent)
    {
        const int currentVectorPosition =
            siblingVectorPosition(document, targetParentId, id);
        const int offset = desiredVectorPosition - currentVectorPosition;
        if (currentVectorPosition >= 0 && offset != 0)
        {
            m_controller->moveLayer(id, offset);
        }
        return;
    }

    m_controller->undoStack()->beginMacro(tr("Move layer"));
    m_controller->setLayerParentGroup(id, targetParentId);
    const Document &updated = m_controller->document();
    const Layer *moved = updated.layer(id);
    if (moved && moved->parentGroupId == targetParentId)
    {
        const int currentVectorPosition =
            siblingVectorPosition(updated, targetParentId, id);
        const int offset = desiredVectorPosition - currentVectorPosition;
        if (currentVectorPosition >= 0 && offset != 0)
        {
            m_controller->moveLayer(id, offset);
        }
    }
    m_controller->undoStack()->endMacro();
}

QUuid LayerDock::selectedLayerId() const
{
    const QListWidgetItem *item = m_layerList->currentItem();
    return item ? item->data(LayerItemRoles::LayerId).toUuid() : QUuid();
}

}
