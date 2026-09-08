// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

#include "document/DocumentLimits.hpp"
#include "render/LayerThumbnailRenderer.hpp"
#include "wasm/BridgeDocument.hpp"

#include <QJsonDocument>
#include <QJsonObject>
#include <QString>

#include <algorithm>
#include <cmath>

using namespace ugurugu::wasm;

extern "C"
{
    EMSCRIPTEN_KEEPALIVE const char *ugu_layer_name(
        BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        if (layer == nullptr)
        {
            return "";
        }
        handle->scratchText = layer->name.toUtf8();
        return handle->scratchText.constData();
    }

    // The layer's stable identity. Indexes shift under every add, remove and
    // move, so a shell that queues those operations has to name the layer it
    // meant rather than the row it saw.
    EMSCRIPTEN_KEEPALIVE const char *ugu_layer_id(
        BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        if (layer == nullptr)
        {
            return "";
        }
        handle->scratchLayerId =
            layer->id.toString(QUuid::WithoutBraces).toUtf8();
        return handle->scratchLayerId.constData();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_layer_kind(
        const BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        return layer != nullptr && layer->kind == ugurugu::LayerKind::Group ? 1
                                                                            : 0;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_layer_visible(
        const BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        return layer != nullptr && layer->visible ? 1 : 0;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_layer_reference(
        const BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        return layer != nullptr && layer->reference ? 1 : 0;
    }

    EMSCRIPTEN_KEEPALIVE double ugu_layer_opacity(
        const BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        return layer != nullptr ? layer->opacity : 1.0;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_layer_is_active(
        const BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        return layer != nullptr
                       && layer->id
                              == handle->controller->document().activeLayerId
                   ? 1
                   : 0;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_layer_depth(
        const BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        if (layer == nullptr)
        {
            return 0;
        }
        return handle->controller->document().layerDepth(layer->id);
    }

    EMSCRIPTEN_KEEPALIVE void ugu_layer_activate(
        BridgeDocument *handle, int index)
    {
        if (const ugurugu::Layer *layer = layerAtIndex(handle, index))
        {
            handle->controller->setActiveLayer(layer->id);
            invalidateSplit(handle);
        }
    }

    EMSCRIPTEN_KEEPALIVE void ugu_layer_set_visible(
        BridgeDocument *handle, int index, int visible)
    {
        if (const ugurugu::Layer *layer = layerAtIndex(handle, index))
        {
            handle->controller->setLayerVisible(layer->id, visible != 0);
            invalidateSplit(handle);
        }
    }

    // Marks a paint layer as a flood-fill reference, the source the wand and
    // the bucket read when their reference option is "marked layers".
    EMSCRIPTEN_KEEPALIVE void ugu_layer_set_reference(
        BridgeDocument *handle, int index, int reference)
    {
        if (const ugurugu::Layer *layer = layerAtIndex(handle, index))
        {
            handle->controller->setLayerReference(layer->id, reference != 0);
        }
    }

    EMSCRIPTEN_KEEPALIVE void ugu_layer_set_opacity(
        BridgeDocument *handle, int index, double opacity)
    {
        if (const ugurugu::Layer *layer = layerAtIndex(handle, index))
        {
            handle->controller->setLayerOpacity(layer->id, opacity);
            invalidateSplit(handle);
        }
    }

    EMSCRIPTEN_KEEPALIVE void ugu_layer_add(BridgeDocument *handle)
    {
        const ugurugu::Document &document = handle->controller->document();
        QUuid parentGroupId;
        if (const ugurugu::Layer *active =
                document.layer(document.activeLayerId))
        {
            parentGroupId = active->parentGroupId;
        }
        handle->controller->addLayer(parentGroupId);
        invalidateSplit(handle);
    }

    EMSCRIPTEN_KEEPALIVE void ugu_layer_remove(
        BridgeDocument *handle, int index)
    {
        if (const ugurugu::Layer *layer = layerAtIndex(handle, index))
        {
            handle->controller->removeLayer(layer->id);
            invalidateSplit(handle);
        }
    }

    EMSCRIPTEN_KEEPALIVE void ugu_layer_rename(
        BridgeDocument *handle, int index, const char *name)
    {
        if (const ugurugu::Layer *layer = layerAtIndex(handle, index))
        {
            handle->controller->renameLayer(layer->id, QString::fromUtf8(name));
            invalidateSplit(handle);
        }
    }

    EMSCRIPTEN_KEEPALIVE void ugu_layer_move(
        BridgeDocument *handle, int index, int offset)
    {
        if (const ugurugu::Layer *layer = layerAtIndex(handle, index))
        {
            handle->controller->moveLayer(layer->id, offset);
            invalidateSplit(handle);
        }
    }

    // Wraps the layer at index in a new group, or makes an empty one when the
    // index names no layer, the two forms LayerDock offers.
    EMSCRIPTEN_KEEPALIVE void ugu_layer_add_group(
        BridgeDocument *handle, int childIndex)
    {
        const ugurugu::Layer *child = layerAtIndex(handle, childIndex);
        handle->controller->addLayerGroup(
            child != nullptr ? child->id : QUuid());
        invalidateSplit(handle);
    }

    EMSCRIPTEN_KEEPALIVE void ugu_layer_duplicate(
        BridgeDocument *handle, int index)
    {
        if (const ugurugu::Layer *layer = layerAtIndex(handle, index))
        {
            handle->controller->duplicateLayer(layer->id);
            invalidateSplit(handle);
        }
    }

    EMSCRIPTEN_KEEPALIVE void ugu_layer_clear(BridgeDocument *handle, int index)
    {
        if (const ugurugu::Layer *layer = layerAtIndex(handle, index))
        {
            handle->controller->clearLayer(layer->id);
            invalidateSplit(handle);
        }
    }

    // Mirrors MergeLayerDownStatus so the shell can disable the action and say
    // why instead of offering a merge that will be refused.
    EMSCRIPTEN_KEEPALIVE int ugu_layer_merge_down_status(
        BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        if (layer == nullptr)
        {
            return static_cast<int>(ugurugu::DocumentController::
                    MergeLayerDownStatus::MissingLayer);
        }
        return static_cast<int>(
            handle->controller->mergeLayerDownStatus(layer->id));
    }

    EMSCRIPTEN_KEEPALIVE int ugu_layer_merge_down(
        BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        if (layer == nullptr)
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("there is no layer at that index"));
            return 0;
        }
        if (!handle->controller->mergeLayerDown(layer->id))
        {
            setError(StatusLayerNotDrawable,
                QByteArrayLiteral("this layer cannot be merged down"));
            return 0;
        }
        invalidateSplit(handle);
        clearError();
        return 1;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_layer_blend_mode(
        const BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        return layer != nullptr ? static_cast<int>(layer->blendMode) : 0;
    }

    EMSCRIPTEN_KEEPALIVE void ugu_layer_set_blend_mode(
        BridgeDocument *handle, int index, int mode)
    {
        const auto blendMode = static_cast<ugurugu::LayerBlendMode>(mode);
        if (!ugurugu::isValidLayerBlendMode(blendMode))
        {
            setError(
                StatusInvalidArgument, QByteArrayLiteral("unknown blend mode"));
            return;
        }
        if (const ugurugu::Layer *layer = layerAtIndex(handle, index))
        {
            handle->controller->setLayerBlendMode(layer->id, blendMode);
            invalidateSplit(handle);
            clearError();
        }
    }

    EMSCRIPTEN_KEEPALIVE int ugu_layer_clip_to_below(
        const BridgeDocument *handle, int index)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        return layer != nullptr && layer->clipToLayerBelow ? 1 : 0;
    }

    EMSCRIPTEN_KEEPALIVE void ugu_layer_set_clip_to_below(
        BridgeDocument *handle, int index, int clipped)
    {
        if (const ugurugu::Layer *layer = layerAtIndex(handle, index))
        {
            handle->controller->setLayerClipToBelow(layer->id, clipped != 0);
            invalidateSplit(handle);
        }
    }

    // A group index of -1 moves the layer out to the top level.
    EMSCRIPTEN_KEEPALIVE void ugu_layer_set_parent_group(
        BridgeDocument *handle, int index, int groupIndex)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        if (layer == nullptr)
        {
            return;
        }
        QUuid groupId;
        if (groupIndex >= 0)
        {
            const ugurugu::Layer *group = layerAtIndex(handle, groupIndex);
            if (group == nullptr || group->kind != ugurugu::LayerKind::Group)
            {
                setError(StatusInvalidArgument,
                    QByteArrayLiteral("that row is not a group"));
                return;
            }
            groupId = group->id;
        }
        handle->controller->setLayerParentGroup(layer->id, groupId);
        invalidateSplit(handle);
        clearError();
    }

    // Empty text means that this layer inherits the whole drawing settings.
    EMSCRIPTEN_KEEPALIVE const char *ugu_layer_wobble(
        BridgeDocument *handle, int index)
    {
        const auto *layer = layerAtIndex(handle, index);
        if (!layer || !layer->wobbleAmount || !layer->motion)
        {
            return "";
        }
        const auto &motion = *layer->motion;
        handle->scratchText =
            QJsonDocument(QJsonObject{{"amount", *layer->wobbleAmount},
                              {"style", static_cast<int>(motion.style)},
                              {"poseCount", motion.poseCount},
                              {"detail", motion.detail},
                              {"linked", motion.linked},
                              {"randomness", motion.randomness},
                              {"brokenLine", motion.brokenLine},
                              {"breakAmount", motion.breakAmount},
                              {"breakRange", motion.breakRange}})
                .toJson(QJsonDocument::Compact);
        return handle->scratchText.constData();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_layer_set_wobble(BridgeDocument *handle,
        int index,
        int enabled,
        double amount,
        int style,
        int poseCount,
        int detail,
        double linked,
        double randomness,
        int brokenLine,
        double breakAmount,
        double breakRange)
    {
        const auto *layer = layerAtIndex(handle, index);
        if (!layer || layer->kind != ugurugu::LayerKind::Paint)
        {
            setError(StatusLayerNotDrawable,
                QByteArrayLiteral("choose a paint layer for layer wobble"));
            return 0;
        }
        auto motion = motionFromValues(style,
            poseCount,
            detail,
            linked,
            randomness,
            brokenLine,
            breakAmount,
            breakRange);
        if (enabled && (!motion || !std::isfinite(amount)))
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("the wobble settings are out of range"));
            return 0;
        }
        const QUuid id = layer->id;
        std::optional<qreal> normalizedAmount;
        if (enabled)
        {
            normalizedAmount = std::clamp(amount,
                ugurugu::DocumentLimits::minimumWobbleAmount,
                ugurugu::DocumentLimits::maximumWobbleAmount);
            if (motion->style != ugurugu::MotionStyle::Classic)
            {
                motion->poseCount = std::min(motion->poseCount,
                    handle->controller->document().animationFrames);
            }
        }
        else
        {
            motion.reset();
        }
        if (layer->wobbleAmount != normalizedAmount || layer->motion != motion)
        {
            // One released web control is one edit, including the explicit
            // Follow action. Do not coalesce it with the preceding override.
            auto *history = handle->controller->undoStack();
            history->beginMacro(QStringLiteral("Change layer wobble"));
            handle->controller->setLayerWobbleOverride(
                id, normalizedAmount, motion);
            history->endMacro();
        }
        const auto *updated = handle->controller->document().layer(id);
        if (!updated || updated->wobbleAmount != normalizedAmount
            || updated->motion != motion)
        {
            setError(StatusDocumentInvalid,
                QByteArrayLiteral("the layer settings could not be committed"));
            return 0;
        }
        invalidateSplit(handle);
        clearError();
        return 1;
    }

    // Renders the layer subtree at index as a static thumbnail (frame 0,
    // wobble off), the same picture the desktop layer dock shows. The buffer
    // is premultiplied BGRA like ugu_render_frame and stays valid until the
    // next thumbnail render or close on the same handle.
    EMSCRIPTEN_KEEPALIVE const std::uint8_t *ugu_layer_thumbnail(
        BridgeDocument *handle, int index, double devicePixelRatio)
    {
        const ugurugu::Layer *layer = layerAtIndex(handle, index);
        if (layer == nullptr)
        {
            return nullptr;
        }
        handle->thumbnail = ugurugu::LayerThumbnailRenderer::renderImage(
            handle->controller->document(), *layer, devicePixelRatio);
        if (handle->thumbnail.isNull())
        {
            return nullptr;
        }
        return handle->thumbnail.constBits();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_thumbnail_width(const BridgeDocument *handle)
    {
        return handle->thumbnail.width();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_thumbnail_height(const BridgeDocument *handle)
    {
        return handle->thumbnail.height();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_thumbnail_bytes_per_line(
        const BridgeDocument *handle)
    {
        return static_cast<int>(handle->thumbnail.bytesPerLine());
    }
}
