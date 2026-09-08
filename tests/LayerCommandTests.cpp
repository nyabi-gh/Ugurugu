// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/DocumentTestHelpers.hpp"
#include "support/DocumentTestSuites.hpp"

namespace ugurugu
{

class LayerCommandTests final : public QObject
{
    Q_OBJECT

private slots:
    void changesLayerBlendModeUndoably()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;

        controller.setLayerBlendMode(layerId, LayerBlendMode::Multiply);
        QCOMPARE(controller.document().layer(layerId)->blendMode,
            LayerBlendMode::Multiply);
        QCOMPARE(controller.undoStack()->count(), 1);

        controller.setLayerBlendMode(layerId, static_cast<LayerBlendMode>(100));
        QCOMPARE(controller.undoStack()->count(), 1);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layer(layerId)->blendMode,
            LayerBlendMode::Normal);
        controller.undoStack()->redo();
        QCOMPARE(controller.document().layer(layerId)->blendMode,
            LayerBlendMode::Multiply);
    }

    void changesReferenceLayerUndoably()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;

        controller.setLayerReference(layerId, true);
        QVERIFY(controller.document().layer(layerId)->reference);
        QCOMPARE(controller.undoStack()->count(), 1);

        controller.undoStack()->undo();
        QVERIFY(!controller.document().layer(layerId)->reference);
        controller.undoStack()->redo();
        QVERIFY(controller.document().layer(layerId)->reference);

        controller.addLayerGroup(layerId);
        const QUuid groupId =
            controller.document().layer(layerId)->parentGroupId;
        const int count = controller.undoStack()->count();
        controller.setLayerReference(groupId, true);
        QCOMPARE(controller.undoStack()->count(), count);
        QVERIFY(!controller.document().layer(groupId)->reference);
    }

    // A group is not a layer as far as the numbering goes. Counting every row
    // meant Layer 1, Layer 2 and Group 1 were followed by Layer 4.
    void numbersNewLayersPastThePaintLayersOnly()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        QCOMPARE(controller.document().layers.size(), 1);

        controller.addLayer();
        QCOMPARE(controller.document().layers.last().name,
            QStringLiteral("Layer 2"));

        controller.addLayerGroup(controller.document().activeLayerId);
        QCOMPARE(controller.document().layers.size(), 3);

        controller.addLayer();
        const auto named = [&controller](const QString &name)
        {
            return std::any_of(controller.document().layers.cbegin(),
                controller.document().layers.cend(),
                [&name](const Layer &layer)
                {
                    return layer.name == name;
                });
        };
        QVERIFY(named(QStringLiteral("Layer 3")));
        QVERIFY(!named(QStringLiteral("Layer 4")));
    }

    void managesLayerGroupsAndClippingUndoably()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid firstId = controller.document().activeLayerId;

        controller.addLayerGroup(firstId);
        QCOMPARE(controller.document().layers.size(), 2);
        const Layer *first = controller.document().layer(firstId);
        QVERIFY(first);
        QVERIFY(!first->parentGroupId.isNull());
        const QUuid groupId = first->parentGroupId;
        const Layer *group = controller.document().layer(groupId);
        QVERIFY(group);
        QCOMPARE(group->kind, LayerKind::Group);
        QVERIFY(group->strokes.isEmpty());
        QVERIFY(controller.document().isLayerDescendantOf(firstId, groupId));
        QCOMPARE(controller.document().layerDepth(firstId), 1);

        controller.addLayer();
        const QUuid secondId = controller.document().activeLayerId;
        QCOMPARE(controller.document().layer(secondId)->parentGroupId, groupId);
        controller.setLayerClipToBelow(secondId, true);
        QVERIFY(controller.document().layer(secondId)->clipToLayerBelow);
        controller.setLayerParentGroup(secondId, {});
        QVERIFY(controller.document().layer(secondId)->parentGroupId.isNull());

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layer(secondId)->parentGroupId, groupId);
        controller.undoStack()->undo();
        QVERIFY(!controller.document().layer(secondId)->clipToLayerBelow);
        controller.undoStack()->redo();
        QVERIFY(controller.document().layer(secondId)->clipToLayerBelow);
    }

    void editsStrokePropertiesUndoably()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;
        Stroke paint;
        paint.color = QColor(20, 40, 60);
        paint.width = 7.0;
        paint.brush.wobbleScale = 0.5;
        paint.points = {{QPointF(12.0, 14.0), 1.0}, {QPointF(50.0, 52.0), 1.0}};
        controller.addStroke(layerId, paint);
        controller.undoStack()->setClean();

        QVERIFY(controller.updateStrokeAttributes(
            layerId, {paint.id}, QColor(90, 110, 130), 15.0));
        const Stroke &edited =
            controller.document().layer(layerId)->strokes.first();
        QCOMPARE(edited.color, QColor(90, 110, 130));
        QCOMPARE(edited.width, 15.0);
        QCOMPARE(edited.brush.wobbleScale, paint.brush.wobbleScale);

        controller.undoStack()->undo();
        const Stroke &restored =
            controller.document().layer(layerId)->strokes.first();
        QCOMPARE(restored.color, paint.color);
        QCOMPARE(restored.width, paint.width);
        QCOMPARE(restored.brush.wobbleScale, paint.brush.wobbleScale);
        controller.undoStack()->redo();
        QCOMPARE(controller.document()
                     .layer(layerId)
                     ->strokes.first()
                     .brush.wobbleScale,
            paint.brush.wobbleScale);
    }

    void roundTripsLayerHierarchySchema()
    {
        Document document = Document::createDefault(QSize(64, 64));
        Layer group;
        group.name = QStringLiteral("Group");
        group.kind = LayerKind::Group;
        group.opacity = 0.7;
        group.initialCanvasSize = document.size;
        document.layers.first().parentGroupId = group.id;
        document.layers.first().clipToLayerBelow = true;
        document.layers.first().reference = true;
        document.layers.append(group);

        const QByteArray json = DocumentSerializer::toJson(document);
        QVERIFY(!json.isEmpty());
        const QJsonObject root = QJsonDocument::fromJson(json).object();
        QCOMPARE(root.value(QStringLiteral("schemaVersion")).toInt(), 13);
        QString error;
        const std::optional<Document> decoded =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY2(decoded.has_value(), qPrintable(error));
        QCOMPARE(decoded->layers.last().kind, LayerKind::Group);
        QCOMPARE(decoded->layers.first().parentGroupId, group.id);
        QVERIFY(decoded->layers.first().clipToLayerBelow);
        QVERIFY(decoded->layers.first().reference);

        QJsonObject legacyRoot = root;
        legacyRoot.insert(QStringLiteral("schemaVersion"), 8);
        QJsonArray legacyLayers =
            legacyRoot.value(QStringLiteral("layers")).toArray();
        for (int index = 0; index < legacyLayers.size(); ++index)
        {
            QJsonObject legacyLayer = legacyLayers[index].toObject();
            legacyLayer.remove(QStringLiteral("reference"));
            legacyLayers[index] = legacyLayer;
        }
        legacyRoot.insert(QStringLiteral("layers"), legacyLayers);
        const std::optional<Document> legacy = DocumentSerializer::fromJson(
            QJsonDocument(legacyRoot).toJson(QJsonDocument::Compact), &error);
        QVERIFY2(legacy.has_value(), qPrintable(error));
        QVERIFY(!legacy->layers.first().reference);

        Document cyclic = *decoded;
        cyclic.layers.last().parentGroupId = cyclic.layers.last().id;
        QVERIFY(DocumentSerializer::toJson(cyclic).isEmpty());

        Document invalidReference = *decoded;
        invalidReference.layers.last().reference = true;
        QVERIFY(DocumentSerializer::toJson(invalidReference).isEmpty());
    }

    void analyzesLayerHierarchyMetricsAndFailures()
    {
        const Document document = documentWithNestedPaintAtDepth(3);
        const LayerHierarchyAnalysis hierarchy =
            analyzeLayerHierarchy(document);
        QVERIFY(hierarchy.isValid());
        QCOMPARE(hierarchy.issue(), LayerHierarchyIssue::None);
        QCOMPARE(hierarchy.maximumDepth(), 3);
        QCOMPARE(hierarchy.depths(), QVector<int>({3, 0, 1, 2}));
        QCOMPARE(hierarchy.subtreeHeights(), QVector<int>({0, 3, 2, 1}));
        QCOMPARE(hierarchy.depth(document.activeLayerId), 3);
        QCOMPARE(hierarchy.subtreeHeight(document.layers[1].id), 3);
        QVERIFY(hierarchy.isDescendantOf(
            document.activeLayerId, document.layers[1].id));
        QVERIFY(!hierarchy.isDescendantOf(
            document.layers[1].id, document.activeLayerId));

        Document missingParent = document;
        missingParent.layers.first().parentGroupId = QUuid::createUuid();
        QCOMPARE(analyzeLayerHierarchy(missingParent).issue(),
            LayerHierarchyIssue::MissingParent);

        Document nonGroupParent = document;
        nonGroupParent.layers[1].parentGroupId =
            nonGroupParent.layers.first().id;
        QCOMPARE(analyzeLayerHierarchy(nonGroupParent).issue(),
            LayerHierarchyIssue::ParentNotGroup);

        Document selfParent = document;
        selfParent.layers[1].parentGroupId = selfParent.layers[1].id;
        QCOMPARE(analyzeLayerHierarchy(selfParent).issue(),
            LayerHierarchyIssue::SelfParent);

        Document cyclic = document;
        cyclic.layers[1].parentGroupId = cyclic.layers.last().id;
        const LayerHierarchyAnalysis cyclicHierarchy =
            analyzeLayerHierarchy(cyclic);
        QCOMPARE(cyclicHierarchy.issue(), LayerHierarchyIssue::Cycle);
        QCOMPARE(cyclic.layers[cyclicHierarchy.issueLayerIndex()].kind,
            LayerKind::Group);

        Document duplicate = document;
        duplicate.layers.last().id = duplicate.layers[1].id;
        QCOMPARE(analyzeLayerHierarchy(duplicate).issue(),
            LayerHierarchyIssue::DuplicateLayerId);
    }

    void preservesLegacyOverDepthHierarchyThroughSerialization()
    {
        for (const int depth : {DocumentLimits::maximumLayerDepth,
                 DocumentLimits::maximumLayerDepth + 1})
        {
            const Document source = documentWithNestedPaintAtDepth(depth);
            const LayerHierarchyAnalysis sourceHierarchy =
                analyzeLayerHierarchy(source);
            QVERIFY(sourceHierarchy.isValid());
            QCOMPARE(sourceHierarchy.maximumDepth(), depth);

            const QByteArray json = DocumentSerializer::toJson(source);
            QVERIFY(!json.isEmpty());
            QString error;
            const std::optional<Document> decoded =
                DocumentSerializer::fromJson(json, &error);
            QVERIFY2(decoded.has_value(), qPrintable(error));
            const LayerHierarchyAnalysis decodedHierarchy =
                analyzeLayerHierarchy(*decoded);
            QVERIFY(decodedHierarchy.isValid());
            QCOMPARE(decodedHierarchy.maximumDepth(), depth);
            QCOMPARE(DocumentSerializer::toJson(*decoded), json);

            QTemporaryDir directory;
            QVERIFY(directory.isValid());
            const QString path =
                directory.filePath(QStringLiteral("depth-%1.ugu").arg(depth));
            QVERIFY2(DocumentSerializer::save(path, *decoded, &error),
                qPrintable(error));
            const std::optional<Document> loaded =
                DocumentSerializer::load(path, &error);
            QVERIFY2(loaded.has_value(), qPrintable(error));
            QCOMPARE(DocumentSerializer::toJson(*loaded), json);
        }

        const Document overDepth = documentWithNestedPaintAtDepth(
            DocumentLimits::maximumLayerDepth + 1);
        QJsonObject schemaEight =
            QJsonDocument::fromJson(DocumentSerializer::toJson(overDepth))
                .object();
        schemaEight.insert(QStringLiteral("schemaVersion"), 8);
        QJsonArray layers =
            schemaEight.value(QStringLiteral("layers")).toArray();
        for (int index = 0; index < layers.size(); ++index)
        {
            QJsonObject layer = layers[index].toObject();
            layer.remove(QStringLiteral("reference"));
            layers[index] = layer;
        }
        schemaEight.insert(QStringLiteral("layers"), layers);
        QString error;
        const std::optional<Document> decoded = DocumentSerializer::fromJson(
            QJsonDocument(schemaEight).toJson(QJsonDocument::Compact), &error);
        QVERIFY2(decoded.has_value(), qPrintable(error));
        QCOMPARE(analyzeLayerHierarchy(*decoded).maximumDepth(),
            DocumentLimits::maximumLayerDepth + 1);
        QVERIFY(!DocumentSerializer::toJson(*decoded).isEmpty());
    }

    void validatesHierarchyOnPreparedMetadataReuse()
    {
        DocumentSerializer::SerializationCache cache;
        QString error;
        std::optional<DocumentSerializer::PreparedDocument> base =
            DocumentSerializer::prepare(
                documentWithGroupAtDepth(2), cache, &error);
        QVERIFY2(base.has_value(), qPrintable(error));

        Document cyclic = base->document();
        const QUuid rootGroup = layerIdAtDepth(cyclic, 0, LayerKind::Group);
        const QUuid deepestGroup = layerIdAtDepth(cyclic, 2, LayerKind::Group);
        QVERIFY(!rootGroup.isNull());
        QVERIFY(!deepestGroup.isNull());
        cyclic.layer(rootGroup)->parentGroupId = deepestGroup;
        cache.resetStats();
        error.clear();
        const auto rejected = DocumentSerializer::prepare(std::move(cyclic),
            cache,
            &*base,
            DocumentLimits::maximumProjectBytes,
            &error);
        QVERIFY(!rejected.has_value());
        QVERIFY(!error.isEmpty());
        QCOMPARE(cache.stats().fullDocumentPreparations, 0ULL);

        base = DocumentSerializer::prepare(
            documentWithGroupAtDepth(DocumentLimits::maximumLayerDepth),
            cache,
            &error);
        QVERIFY2(base.has_value(), qPrintable(error));
        Document overDepth = base->document();
        const QUuid paintId = overDepth.activeLayerId;
        const QUuid depthLimitGroup = layerIdAtDepth(
            overDepth, DocumentLimits::maximumLayerDepth, LayerKind::Group);
        overDepth.layer(paintId)->parentGroupId = depthLimitGroup;
        cache.resetStats();
        error.clear();
        const auto preserved = DocumentSerializer::prepare(std::move(overDepth),
            cache,
            &*base,
            DocumentLimits::maximumProjectBytes,
            &error);
        QVERIFY2(preserved.has_value(), qPrintable(error));
        QCOMPARE(analyzeLayerHierarchy(preserved->document()).maximumDepth(),
            DocumentLimits::maximumLayerDepth + 1);
        QCOMPARE(cache.stats().fullDocumentPreparations, 0ULL);
    }

    void rejectsLayerDepthGrowthWithoutChangingControllerState()
    {
        DocumentController controller;
        QVERIFY(controller.loadDocument(
            documentWithGroupAtDepth(DocumentLimits::maximumLayerDepth)));
        const QUuid deepestGroup = layerIdAtDepth(controller.document(),
            DocumentLimits::maximumLayerDepth,
            LayerKind::Group);
        const QUuid rootPaint = controller.document().activeLayerId;
        QVERIFY(!deepestGroup.isNull());
        verifyRejectedHierarchyMutation(controller,
            [&controller, deepestGroup]
            {
                controller.addLayer(deepestGroup);
            });
        verifyRejectedHierarchyMutation(controller,
            [&controller, rootPaint, deepestGroup]
            {
                controller.setLayerParentGroup(rootPaint, deepestGroup);
            });

        QVERIFY(controller.loadDocument(
            documentWithNestedPaintAtDepth(DocumentLimits::maximumLayerDepth)));
        const QUuid deepestPaint = controller.document().activeLayerId;
        verifyRejectedHierarchyMutation(controller,
            [&controller, deepestPaint]
            {
                controller.addLayerGroup(deepestPaint);
            });

        Document subtree =
            documentWithNestedPaintAtDepth(DocumentLimits::maximumLayerDepth);
        const QUuid subtreeRoot = layerIdAtDepth(subtree, 0, LayerKind::Group);
        Layer targetGroup;
        targetGroup.name = QStringLiteral("Target group");
        targetGroup.kind = LayerKind::Group;
        targetGroup.initialCanvasSize = subtree.size;
        const QUuid targetGroupId = targetGroup.id;
        subtree.layers.append(std::move(targetGroup));
        QVERIFY(controller.loadDocument(std::move(subtree)));
        verifyRejectedHierarchyMutation(controller,
            [&controller, subtreeRoot]
            {
                controller.addLayerGroup(subtreeRoot);
            });
        verifyRejectedHierarchyMutation(controller,
            [&controller, subtreeRoot, targetGroupId]
            {
                controller.setLayerParentGroup(subtreeRoot, targetGroupId);
            });
    }

    void permitsOnlyNonGrowingLegacyHierarchyMutations()
    {
        DocumentController controller;
        QVERIFY(controller.loadDocument(documentWithNestedPaintAtDepth(
            DocumentLimits::maximumLayerDepth + 1)));
        const QUuid legacyPaint = controller.document().activeLayerId;
        QCOMPARE(analyzeLayerHierarchy(controller.document()).maximumDepth(),
            DocumentLimits::maximumLayerDepth + 1);

        verifyRejectedHierarchyMutation(controller,
            [&controller, legacyPaint]
            {
                controller.addLayerGroup(legacyPaint);
            });

        const int originalLayerCount =
            static_cast<int>(controller.document().layers.size());
        controller.addLayer();
        QCOMPARE(controller.document().layers.size(), originalLayerCount + 1);
        QCOMPARE(analyzeLayerHierarchy(controller.document()).maximumDepth(),
            DocumentLimits::maximumLayerDepth + 1);
        QVERIFY(controller.isModified());
        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.size(), originalLayerCount);

        controller.duplicateLayer(legacyPaint);
        QCOMPARE(controller.document().layers.size(), originalLayerCount + 1);
        QCOMPARE(analyzeLayerHierarchy(controller.document()).maximumDepth(),
            DocumentLimits::maximumLayerDepth + 1);
        controller.undoStack()->undo();

        controller.setLayerParentGroup(legacyPaint, {});
        QCOMPARE(analyzeLayerHierarchy(controller.document()).maximumDepth(),
            DocumentLimits::maximumLayerDepth);
        controller.undoStack()->undo();
        QCOMPARE(analyzeLayerHierarchy(controller.document()).maximumDepth(),
            DocumentLimits::maximumLayerDepth + 1);
    }

    void acceptsMaximumWidthShallowHierarchy()
    {
        Document wide = Document::createDefault(QSize(64, 64));
        while (wide.layers.size() < DocumentLimits::maximumLayers)
        {
            Layer layer;
            layer.name = QStringLiteral("Layer %1").arg(wide.layers.size() + 1);
            layer.initialCanvasSize = wide.size;
            wide.layers.append(std::move(layer));
        }
        const LayerHierarchyAnalysis hierarchy = analyzeLayerHierarchy(wide);
        QVERIFY(hierarchy.isValid());
        QCOMPARE(hierarchy.maximumDepth(), 0);
        QVERIFY(!DocumentSerializer::toJson(wide).isEmpty());

        DocumentController controller;
        QVERIFY(controller.loadDocument(std::move(wide)));
        QCOMPARE(
            controller.document().layers.size(), DocumentLimits::maximumLayers);
    }

    void undoActionsCannotBypassHistoryPreflight()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;
        controller.renameLayer(layerId, QStringLiteral("Renamed"));

        QVERIFY(controller.undoStack()->canUndo());
        QVERIFY(!controller.undoStack()->canRedo());

        DocumentControllerTestAccess::failHistoryPrepareAfter(controller, 0);
        controller.undoStack()->undo();
        QCOMPARE(controller.undoStack()->index(), 1);
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Renamed"));
        QCOMPARE(
            controller.undoStack()->storageStats().retainedPreparedDocuments,
            qsizetype(0));

        controller.undoStack()->undo();
        QCOMPARE(controller.undoStack()->index(), 0);
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Layer 1"));
        QVERIFY(controller.undoStack()->canRedo());
        controller.undoStack()->redo();
        QCOMPARE(controller.undoStack()->index(), 1);
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Renamed"));
    }

    void keepsCleanRevisionAcrossScalarMergeBoundary()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        controller.setWobbleAmount(2.0);
        controller.markSaved();
        QVERIFY(!controller.isModified());
        QVERIFY(controller.undoStack()->isClean());

        controller.setWobbleAmount(3.0);
        QVERIFY(controller.isModified());
        QCOMPARE(controller.undoStack()->count(), 2);
        controller.undoStack()->undo();
        QCOMPARE(controller.document().wobbleAmount, 2.0);
        QVERIFY(!controller.isModified());
        QVERIFY(controller.undoStack()->isClean());
        controller.undoStack()->redo();
        QCOMPARE(controller.document().wobbleAmount, 3.0);
        QVERIFY(controller.isModified());
    }

    void marksRecoveredDocumentsAsModified()
    {
        DocumentController controller;
        Document recovered = Document::createDefault(QSize(480, 320));
        recovered.wobbleAmount = 4.0;

        controller.loadRecoveredDocument(recovered);

        QCOMPARE(controller.document().size, recovered.size);
        QCOMPARE(controller.document().wobbleAmount, recovered.wobbleAmount);
        QVERIFY(controller.isModified());
        controller.markSaved();
        QVERIFY(!controller.isModified());
    }

    void undoesLayerAdditionAndRemoval()
    {
        DocumentController controller;
        controller.newDocument(QSize(256, 256));
        const QUuid originalId = controller.document().activeLayerId;

        controller.addLayer();
        QCOMPARE(controller.document().layers.size(), 2);
        const QUuid addedId = controller.document().activeLayerId;
        QVERIFY(addedId != originalId);
        QCOMPARE(controller.document().layers.constLast().id, addedId);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.size(), 1);
        QCOMPARE(controller.document().activeLayerId, originalId);
        QVERIFY(controller.document().layer(addedId) == nullptr);

        controller.undoStack()->redo();
        QCOMPARE(controller.document().layers.size(), 2);
        QCOMPARE(controller.document().activeLayerId, addedId);

        controller.removeLayer(addedId);
        QCOMPARE(controller.document().layers.size(), 1);
        QCOMPARE(controller.document().activeLayerId, originalId);
        QVERIFY(controller.document().layer(addedId) == nullptr);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.size(), 2);
        QCOMPARE(controller.document().activeLayerId, addedId);
        QCOMPARE(controller.document().layers.constLast().id, addedId);
    }

    void supportsDocumentsWithoutLayers()
    {
        DocumentController controller;
        controller.newDocument(QSize(256, 256));
        const Layer originalLayer = controller.document().layers.first();

        controller.removeLayer(originalLayer.id);
        QVERIFY(controller.document().layers.isEmpty());
        QVERIFY(controller.document().activeLayerId.isNull());
        QVERIFY(controller.isModified());

        QString error;
        const QByteArray serialized =
            DocumentSerializer::toJson(controller.document());
        QVERIFY(!serialized.isEmpty());
        const QJsonObject root = QJsonDocument::fromJson(serialized).object();
        QVERIFY(root.value(QStringLiteral("activeLayerId")).isNull());
        const std::optional<Document> loaded =
            DocumentSerializer::fromJson(serialized, &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        QVERIFY(loaded->layers.isEmpty());
        QVERIFY(loaded->activeLayerId.isNull());

        DocumentController loadedController;
        loadedController.loadDocument(*loaded);
        QVERIFY(loadedController.document().layers.isEmpty());
        QVERIFY(loadedController.document().activeLayerId.isNull());

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.size(), 1);
        QCOMPARE(controller.document().layers.first().id, originalLayer.id);
        QCOMPARE(controller.document().activeLayerId, originalLayer.id);

        controller.undoStack()->redo();
        QVERIFY(controller.document().layers.isEmpty());
        QVERIFY(controller.document().activeLayerId.isNull());

        controller.addLayer();
        QCOMPARE(controller.document().layers.size(), 1);
        const QUuid addedId = controller.document().activeLayerId;
        QVERIFY(!addedId.isNull());
        QCOMPARE(controller.document().layers.first().name,
            QStringLiteral("Layer 1"));

        controller.undoStack()->undo();
        QVERIFY(controller.document().layers.isEmpty());
        QVERIFY(controller.document().activeLayerId.isNull());

        controller.undoStack()->redo();
        QCOMPARE(controller.document().layers.size(), 1);
        QCOMPARE(controller.document().activeLayerId, addedId);
    }

    void activatesAdjacentLayerWhenRemovingFirstLayer()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 64));
        controller.addLayer();
        controller.addLayer();
        QCOMPARE(controller.document().layers.size(), 3);
        const QUuid first = controller.document().layers[0].id;
        const QUuid second = controller.document().layers[1].id;
        const QUuid third = controller.document().layers[2].id;

        controller.setActiveLayer(first);
        controller.removeLayer(first);
        QCOMPARE(controller.document().layers.size(), 2);
        QCOMPARE(controller.document().activeLayerId, second);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.size(), 3);
        QCOMPARE(controller.document().activeLayerId, first);

        controller.setActiveLayer(third);
        controller.removeLayer(second);
        QCOMPARE(controller.document().layers.size(), 2);
        QCOMPARE(controller.document().activeLayerId, third);
    }

    void mergesSafePaintLayerDownWithoutChangingPixels()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 64));
        const QUuid lowerId = controller.document().activeLayerId;
        Stroke lowerStroke;
        lowerStroke.color = QColor(220, 40, 30);
        lowerStroke.width = 8.0;
        lowerStroke.points = {
            {QPointF(8.0, 10.0), 1.0}, {QPointF(52.0, 10.0), 1.0}};
        controller.addStroke(lowerId, lowerStroke);
        controller.addLayer();
        const QUuid upperId = controller.document().activeLayerId;
        Stroke upperStroke;
        upperStroke.color = QColor(30, 80, 220);
        upperStroke.width = 8.0;
        upperStroke.points = {
            {QPointF(8.0, 40.0), 1.0}, {QPointF(52.0, 40.0), 1.0}};
        controller.addStroke(upperId, upperStroke);
        controller.undoStack()->setClean();
        const QImage before = RenderEngine::render(controller.document(), 0);

        QCOMPARE(controller.mergeLayerDownStatus(upperId),
            DocumentController::MergeLayerDownStatus::Available);
        QVERIFY(controller.mergeLayerDown(upperId));
        QCOMPARE(controller.document().layers.size(), 1);
        QCOMPARE(controller.document().activeLayerId, lowerId);
        // Two strokes plus the composite-boundary pair sealing the merged
        // layer's section.
        QCOMPARE(controller.document().layers.first().strokes.size(), 4);
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.size(), 2);
        QCOMPARE(controller.document().activeLayerId, upperId);
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);
    }

    void keepsTheUpperLayersOnlyArtworkWhenMergingDown()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 64));
        controller.addLayer();
        const QUuid upperId = controller.document().activeLayerId;
        Stroke upperStroke;
        upperStroke.color = QColor(30, 80, 220);
        upperStroke.width = 12.0;
        upperStroke.points = {
            {QPointF(8.0, 32.0), 1.0}, {QPointF(56.0, 32.0), 1.0}};
        controller.addStroke(upperId, upperStroke);
        const QImage before = RenderEngine::render(controller.document(), 0);
        const QImage laterBefore =
            RenderEngine::render(controller.document(), 11);

        QVERIFY(controller.mergeLayerDown(upperId));
        QCOMPARE(controller.document().layers.size(), 1);
        // One stroke plus the composite-boundary pair sealing the merged
        // layer's section.
        QCOMPARE(controller.document().layers.first().strokes.size(), 3);
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);
        QCOMPARE(RenderEngine::render(controller.document(), 11), laterBefore);
    }

    void overridesWobblePerLayerUndoablyAndAcrossASaveCycle()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;
        Stroke stroke;
        stroke.width = 6.0;
        stroke.points = {
            {QPointF(10.0, 48.0), 1.0}, {QPointF(86.0, 48.0), 1.0}};
        controller.addStroke(layerId, stroke);
        controller.undoStack()->setClean();
        QVERIFY(!controller.document().layer(layerId)->wobbleAmount);

        MotionSettings still;
        still.poseCount = 1;
        controller.setLayerWobbleOverride(layerId, 0.0, still);
        const Layer *held = controller.document().layer(layerId);
        QVERIFY(held->wobbleAmount.has_value());
        QCOMPARE(*held->wobbleAmount, 0.0);
        QCOMPARE(effectiveWobbleAmount(controller.document(), *held), 0.0);
        // A layer pinned to zero must be identical on every frame while the
        // document itself still wobbles.
        QCOMPARE(RenderEngine::render(controller.document(), 3),
            RenderEngine::render(controller.document(), 11));

        const QByteArray json =
            DocumentSerializer::toJson(controller.document());
        QVERIFY(!json.isEmpty());
        QString error;
        const std::optional<Document> loaded =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        const Layer *reloaded = loaded->layer(layerId);
        QVERIFY(reloaded);
        QCOMPARE(reloaded->wobbleAmount, held->wobbleAmount);
        QCOMPARE(reloaded->motion, held->motion);

        controller.undoStack()->undo();
        QVERIFY(!controller.document().layer(layerId)->wobbleAmount);
        QVERIFY(!controller.document().layer(layerId)->motion);
        controller.undoStack()->redo();
        QCOMPARE(*controller.document().layer(layerId)->wobbleAmount, 0.0);

        // Half an override is meaningless, so it is refused outright.
        const int count = controller.undoStack()->count();
        controller.setLayerWobbleOverride(layerId, 4.0, std::nullopt);
        QCOMPARE(controller.undoStack()->count(), count);
        QCOMPARE(*controller.document().layer(layerId)->wobbleAmount, 0.0);
    }

    void refusesToMergeLayersWithDifferentEffectiveWobble()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid lowerId = controller.document().activeLayerId;
        Stroke lowerStroke;
        lowerStroke.points = {
            {QPointF(12.0, 24.0), 1.0}, {QPointF(84.0, 24.0), 1.0}};
        controller.addStroke(lowerId, lowerStroke);
        controller.addLayer();
        const QUuid upperId = controller.document().activeLayerId;
        Stroke upperStroke;
        upperStroke.points = {
            {QPointF(12.0, 72.0), 1.0}, {QPointF(84.0, 72.0), 1.0}};
        controller.addStroke(upperId, upperStroke);

        MotionSettings still = controller.document().motion;
        still.poseCount = 1;
        controller.setLayerWobbleOverride(upperId, 0.0, still);
        controller.undoStack()->setClean();
        const QImage before = RenderEngine::render(controller.document(), 7);

        QCOMPARE(controller.mergeLayerDownStatus(upperId),
            DocumentController::MergeLayerDownStatus::UnsupportedProperties);
        QVERIFY(!controller.mergeLayerDown(upperId));
        QCOMPARE(controller.document().layers.size(), 2);
        QCOMPARE(RenderEngine::render(controller.document(), 7), before);
        QVERIFY(controller.undoStack()->isClean());
    }

    void keepsLayerMotionValidWhenAnimationFramesShrink()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;
        MotionSettings motion = controller.document().motion;
        motion.style = MotionStyle::Smooth;
        motion.poseCount = 20;
        controller.setLayerWobbleOverride(layerId, 3.0, motion);
        controller.undoStack()->setClean();
        const int historyCountBefore = controller.undoStack()->count();

        controller.setAnimationFrames(8);
        QCOMPARE(controller.document().animationFrames, 8);
        QVERIFY(controller.document().layer(layerId)->motion.has_value());
        QCOMPARE(controller.document().layer(layerId)->motion->poseCount, 8);
        QCOMPARE(controller.undoStack()->count(), historyCountBefore + 1);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().animationFrames, 30);
        QCOMPARE(controller.document().layer(layerId)->motion->poseCount, 20);
    }

    // The one merge a composite boundary cannot express: an eraser drawn
    // after a merge acted on the flattened artwork, and sealing it into its
    // own section would give that reach up and change the picture.
    void refusesToMergeWhenATailEraserAlreadyReachedMergedArtwork()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 64));
        const QUuid lowerId = controller.document().activeLayerId;
        Stroke lowerStroke;
        lowerStroke.color = QColor(220, 40, 30);
        lowerStroke.width = 24.0;
        lowerStroke.points = {
            {QPointF(8.0, 32.0), 1.0}, {QPointF(56.0, 32.0), 1.0}};
        controller.addStroke(lowerId, lowerStroke);
        controller.addLayer();
        const QUuid middleId = controller.document().activeLayerId;
        Stroke middleStroke;
        middleStroke.color = QColor(30, 60, 200);
        middleStroke.width = 24.0;
        middleStroke.points = {
            {QPointF(8.0, 32.0), 1.0}, {QPointF(56.0, 32.0), 1.0}};
        controller.addStroke(middleId, middleStroke);
        QVERIFY(controller.mergeLayerDown(middleId));
        const QUuid mergedId = controller.document().activeLayerId;

        Stroke tailErase;
        tailErase.mode = StrokeMode::Erase;
        tailErase.width = 24.0;
        tailErase.points = {
            {QPointF(24.0, 32.0), 1.0}, {QPointF(40.0, 32.0), 1.0}};
        QCOMPARE(controller.addStroke(mergedId, tailErase),
            DocumentController::AddStrokeResult::Added);

        controller.addLayer();
        const QUuid upperId = controller.document().activeLayerId;
        Stroke upperStroke;
        upperStroke.color = QColor(40, 180, 90);
        upperStroke.width = 8.0;
        upperStroke.points = {
            {QPointF(8.0, 8.0), 1.0}, {QPointF(56.0, 8.0), 1.0}};
        controller.addStroke(upperId, upperStroke);
        controller.undoStack()->setClean();
        const QImage before = RenderEngine::render(controller.document(), 0);

        QCOMPARE(controller.mergeLayerDownStatus(upperId),
            DocumentController::MergeLayerDownStatus::UnsupportedStrokes);
        QVERIFY(!controller.mergeLayerDown(upperId));
        QCOMPARE(controller.document().layers.size(), 2);
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);
        QVERIFY(controller.undoStack()->isClean());
    }

    // Merging must not ask the user to choose between keeping a layer's
    // eraser and merging it. Appending the strokes lets the eraser reach the
    // artwork below, so the merged layer has to keep compositing the two
    // sides the way the two layers did: identically, in every frame.
    void mergesALayerWhoseEraserOverlapsTheArtworkBelow()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 64));
        const QUuid lowerId = controller.document().activeLayerId;
        Stroke lowerStroke;
        lowerStroke.color = QColor(220, 40, 30);
        lowerStroke.width = 24.0;
        lowerStroke.points = {
            {QPointF(8.0, 32.0), 1.0}, {QPointF(56.0, 32.0), 1.0}};
        controller.addStroke(lowerId, lowerStroke);
        controller.addLayer();
        const QUuid upperId = controller.document().activeLayerId;
        Stroke upperStroke;
        upperStroke.color = QColor(30, 60, 200);
        upperStroke.width = 24.0;
        upperStroke.points = {
            {QPointF(8.0, 32.0), 1.0}, {QPointF(56.0, 32.0), 1.0}};
        controller.addStroke(upperId, upperStroke);
        Stroke upperErase;
        upperErase.mode = StrokeMode::Erase;
        upperErase.width = 24.0;
        upperErase.points = {
            {QPointF(24.0, 32.0), 1.0}, {QPointF(40.0, 32.0), 1.0}};
        controller.addStroke(upperId, upperErase);

        const int frames = controller.document().animationFrames;
        QVector<QImage> before;
        for (int frame = 0; frame < frames; ++frame)
        {
            before.append(RenderEngine::render(controller.document(), frame));
        }

        QCOMPARE(controller.mergeLayerDownStatus(upperId),
            DocumentController::MergeLayerDownStatus::Available);
        QVERIFY(controller.mergeLayerDown(upperId));
        QCOMPARE(controller.document().layers.size(), 1);

        for (int frame = 0; frame < frames; ++frame)
        {
            QVERIFY2(RenderEngine::render(controller.document(), frame)
                         == before.at(frame),
                qPrintable(
                    QStringLiteral("merging changed frame %1").arg(frame)));
        }

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.size(), 2);
        QCOMPARE(RenderEngine::render(controller.document(), 0), before.at(0));
    }

    // Scoping an eraser to what it covered before the merge must not make the
    // merged layer behave like two layers afterwards: a new eraser reaches
    // everything already on it.
    void letsANewEraserOnAMergedLayerReachWhatItWasMergedWith()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 64));
        controller.setWobbleAmount(0.0);
        const QUuid lowerId = controller.document().activeLayerId;
        Stroke lowerStroke;
        lowerStroke.color = QColor(220, 40, 30);
        lowerStroke.width = 24.0;
        lowerStroke.points = {
            {QPointF(8.0, 32.0), 1.0}, {QPointF(56.0, 32.0), 1.0}};
        controller.addStroke(lowerId, lowerStroke);
        controller.addLayer();
        const QUuid upperId = controller.document().activeLayerId;
        Stroke upperErase;
        upperErase.mode = StrokeMode::Erase;
        upperErase.width = 24.0;
        upperErase.points = {
            {QPointF(24.0, 32.0), 1.0}, {QPointF(40.0, 32.0), 1.0}};
        controller.addStroke(upperId, upperErase);

        QCOMPARE(controller.mergeLayerDownStatus(upperId),
            DocumentController::MergeLayerDownStatus::Available);
        QVERIFY(controller.mergeLayerDown(upperId));
        const QUuid mergedId = controller.document().activeLayerId;
        QCOMPARE(
            RenderEngine::render(controller.document(), 0).pixelColor(32, 32),
            QColor(220, 40, 30));

        Stroke freshErase;
        freshErase.mode = StrokeMode::Erase;
        freshErase.width = 24.0;
        freshErase.points = {
            {QPointF(24.0, 32.0), 1.0}, {QPointF(40.0, 32.0), 1.0}};
        QCOMPARE(controller.addStroke(mergedId, freshErase),
            DocumentController::AddStrokeResult::Added);
        QCOMPARE(
            RenderEngine::render(controller.document(), 0).pixelColor(32, 32),
            controller.document().background);
    }

    void mergesALayerWhoseEraserOnlyTouchesItsOwnStrokes()
    {
        DocumentController controller;
        controller.newDocument(QSize(512, 512));
        const QUuid lowerId = controller.document().activeLayerId;
        Stroke lowerStroke;
        lowerStroke.color = QColor(220, 40, 30);
        lowerStroke.width = 8.0;
        lowerStroke.points = {
            {QPointF(24.0, 40.0), 1.0}, {QPointF(480.0, 40.0), 1.0}};
        controller.addStroke(lowerId, lowerStroke);
        controller.addLayer();
        const QUuid upperId = controller.document().activeLayerId;
        Stroke upperStroke;
        upperStroke.color = QColor(30, 80, 220);
        upperStroke.width = 8.0;
        upperStroke.points = {
            {QPointF(24.0, 460.0), 1.0}, {QPointF(480.0, 460.0), 1.0}};
        controller.addStroke(upperId, upperStroke);
        // Far clear of the lower layer's stroke, so flattening keeps pixels.
        Stroke upperErase;
        upperErase.mode = StrokeMode::Erase;
        upperErase.width = 6.0;
        upperErase.points = {
            {QPointF(240.0, 460.0), 1.0}, {QPointF(272.0, 460.0), 1.0}};
        controller.addStroke(upperId, upperErase);
        controller.undoStack()->setClean();
        const QImage before = RenderEngine::render(controller.document(), 0);

        QCOMPARE(controller.mergeLayerDownStatus(upperId),
            DocumentController::MergeLayerDownStatus::Available);
        QVERIFY(controller.mergeLayerDown(upperId));
        QCOMPARE(controller.document().layers.size(), 1);
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);
    }

    void insertsImagesAboveTheActiveLayerAndDeduplicatesAssets()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 80));
        const QUuid originalLayerId = controller.document().activeLayerId;
        controller.addLayerGroup(originalLayerId);
        const QUuid groupId =
            controller.document().layer(originalLayerId)->parentGroupId;
        controller.undoStack()->setClean();

        QImage image(QSize(20, 10), QImage::Format_RGBA8888);
        image.fill(QColor(30, 90, 210, 180));
        QCOMPARE(
            controller.insertImage(image, QStringLiteral("/tmp/My.paint.png")),
            DocumentController::InsertImageResult::Inserted);
        const QUuid firstImageLayerId = controller.document().activeLayerId;
        const Layer *firstImageLayer =
            controller.document().layer(firstImageLayerId);
        QVERIFY(firstImageLayer);
        QCOMPARE(firstImageLayer->name, QStringLiteral("My.paint"));
        QCOMPARE(firstImageLayer->parentGroupId, groupId);
        QCOMPARE(controller.document().layerIndex(firstImageLayerId),
            controller.document().layerIndex(originalLayerId) + 1);
        QCOMPARE(firstImageLayer->strokes.size(), 1);
        const Stroke &operation = firstImageLayer->strokes.first();
        QCOMPARE(operation.mode, StrokeMode::Image);
        QVERIFY(operation.points.isEmpty());
        QVERIFY(operation.imageOp.has_value());
        const QString firstAssetId = operation.imageOp->assetId;
        QCOMPARE(operation.imageOp->transform,
            QTransform(1.0, 0.0, 0.0, 1.0, 40.0, 35.0));
        QCOMPARE(controller.document().rasterAssets.size(), 1);

        QCOMPARE(controller.insertImage(
                     image.copy(), QStringLiteral("/tmp/copy.png")),
            DocumentController::InsertImageResult::Inserted);
        QCOMPARE(controller.document().rasterAssets.size(), 1);
        QCOMPARE(controller.document()
                     .layer(controller.document().activeLayerId)
                     ->strokes.first()
                     .imageOp->assetId,
            firstAssetId);
    }

    void preservesOriginalImageAcrossTransformSaveAndUndo()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 48));
        QImage image(QSize(8, 6), QImage::Format_RGBA8888);
        image.fill(Qt::transparent);
        image.setPixelColor(1, 1, QColor(240, 30, 20, 255));
        image.setPixelColor(6, 4, QColor(20, 180, 70, 200));
        QCOMPARE(controller.insertImage(image, QStringLiteral("image.png")),
            DocumentController::InsertImageResult::Inserted);
        const QUuid layerId = controller.document().activeLayerId;
        const QUuid strokeId =
            controller.document().layer(layerId)->strokes.first().id;
        const QString assetId = controller.document()
                                    .layer(layerId)
                                    ->strokes.first()
                                    .imageOp->assetId;
        const char *payload = controller.document()
                                  .rasterAssets.value(assetId)
                                  .compressedRgba.constData();
        const QImage inserted = RenderEngine::render(controller.document(), 0);
        QCOMPARE(RenderEngine::render(controller.document(), 1), inserted);

        const QTransform finalTransform(1.75, 0.25, -0.15, 1.4, 12.0, 9.0);
        QVERIFY(controller.setImageTransform(
            layerId, strokeId, finalTransform, SamplingMode::Smooth));
        QCOMPARE(controller.document()
                     .layer(layerId)
                     ->strokes.first()
                     .imageOp->transform,
            finalTransform);
        QCOMPARE(controller.document()
                     .rasterAssets.value(assetId)
                     .compressedRgba.constData(),
            payload);
        const QImage transformed =
            RenderEngine::render(controller.document(), 0);
        QVERIFY(transformed != inserted);
        QCOMPARE(RenderEngine::render(controller.document(), 7), transformed);

        QString error;
        const QByteArray serialized =
            DocumentSerializer::toJson(controller.document());
        QVERIFY(!serialized.isEmpty());
        const std::optional<Document> reloaded =
            DocumentSerializer::fromJson(serialized, &error);
        QVERIFY2(reloaded.has_value(), qPrintable(error));
        QCOMPARE(RenderEngine::render(*reloaded, 0), transformed);
        QCOMPARE(reloaded->layer(layerId)->strokes.first().imageOp->transform,
            finalTransform);

        controller.undoStack()->undo();
        QCOMPARE(RenderEngine::render(controller.document(), 0), inserted);
        controller.undoStack()->redo();
        QCOMPARE(RenderEngine::render(controller.document(), 0), transformed);

        controller.removeLayer(layerId);
        QVERIFY(controller.document().rasterAssets.isEmpty());
        controller.undoStack()->undo();
        QVERIFY(controller.document().rasterAssets.contains(assetId));
        QCOMPARE(RenderEngine::render(controller.document(), 0), transformed);
    }

    void rejectsInvalidImageAndLayerLimitAtomically()
    {
        DocumentController controller;
        controller.newDocument(QSize(32, 32));
        const DocumentTransitionSnapshot invalidBefore =
            documentTransitionSnapshot(controller);
        QCOMPARE(controller.insertImage({}, QStringLiteral("empty.png")),
            DocumentController::InsertImageResult::RejectedInvalidImage);
        compareDocumentTransitionSnapshot(controller, invalidBefore);

        Document full = Document::createDefault(QSize(32, 32));
        while (full.layers.size() < DocumentLimits::maximumLayers)
        {
            Layer layer;
            layer.name = QStringLiteral("Layer %1").arg(full.layers.size());
            layer.initialCanvasSize = full.size;
            full.layers.append(std::move(layer));
        }
        QString error;
        QVERIFY2(controller.loadDocument(std::move(full), &error),
            qPrintable(error));
        const DocumentTransitionSnapshot fullBefore =
            documentTransitionSnapshot(controller);
        QImage image(QSize(2, 2), QImage::Format_RGBA8888);
        image.fill(Qt::red);
        QCOMPARE(controller.insertImage(image, QStringLiteral("red.png")),
            DocumentController::InsertImageResult::RejectedLayerLimit);
        compareDocumentTransitionSnapshot(controller, fullBefore);
    }

    void rejectsUnsafeLayerMergeDown()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 64));
        const QUuid lowerId = controller.document().activeLayerId;
        controller.addLayer();
        const QUuid upperId = controller.document().activeLayerId;
        controller.setLayerReference(upperId, true);
        controller.undoStack()->setClean();

        QCOMPARE(controller.mergeLayerDownStatus(upperId),
            DocumentController::MergeLayerDownStatus::UnsupportedProperties);
        QVERIFY(!controller.mergeLayerDown(upperId));
        QCOMPARE(controller.document().layers.size(), 2);
        QCOMPARE(controller.document().activeLayerId, upperId);
        QVERIFY(controller.document().layer(lowerId));
        QVERIFY(controller.undoStack()->isClean());
    }
};

int runLayerCommandTests(int argc, char **argv)
{
    LayerCommandTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "LayerCommandTests.moc"
