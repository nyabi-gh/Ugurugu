// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/DocumentTestHelpers.hpp"
#include "support/DocumentTestSuites.hpp"

namespace ugurugu
{

class DocumentLifecycleTests final : public QObject
{
    Q_OBJECT

private slots:
    void isolatesRecoveryFromUserData()
    {
        const QByteArray configured = qgetenv("UGURUGU_RECOVERY_PATH");
        QVERIFY(!configured.isEmpty());
        QCOMPARE(RecoveryStore::filePath(),
            QFileInfo(QString::fromUtf8(configured)).absoluteFilePath());
    }

    void identifiesRecoveryPathAliases()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QByteArray variable = QByteArrayLiteral("UGURUGU_RECOVERY_PATH");
        const bool existed = qEnvironmentVariableIsSet(variable.constData());
        const QByteArray previous = qgetenv(variable.constData());
        [[maybe_unused]] const auto restoreEnvironment = qScopeGuard(
            [&]()
            {
                if (existed)
                {
                    qputenv(variable.constData(), previous);
                }
                else
                {
                    qunsetenv(variable.constData());
                }
            });
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        QVERIFY(qputenv(variable.constData(), recoveryPath.toUtf8()));
        QFile recoveryFile(recoveryPath);
        QVERIFY(recoveryFile.open(QIODevice::WriteOnly));
        QCOMPARE(recoveryFile.write(QByteArrayLiteral("recovery")), 8);
        recoveryFile.close();

        QVERIFY(RecoveryStore::isRecoveryPath(recoveryPath));
        QVERIFY(RecoveryStore::isRecoveryPath(
            directory.filePath(QStringLiteral("./recovery.ugu"))));
#if defined(Q_OS_WIN) || defined(Q_OS_MACOS)
        QVERIFY(RecoveryStore::isRecoveryPath(recoveryPath.toUpper()));
#endif
#ifndef Q_OS_WIN
        const QString linkedPath =
            directory.filePath(QStringLiteral("linked-recovery.ugu"));
        QVERIFY(QFile::link(recoveryPath, linkedPath));
        QVERIFY(RecoveryStore::isRecoveryPath(linkedPath));
#endif
        QVERIFY(!RecoveryStore::isRecoveryPath(
            directory.filePath(QStringLiteral("other.ugu"))));
    }

    void preservesRecoveryAsSeparateProject()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QByteArray variable = QByteArrayLiteral("UGURUGU_RECOVERY_PATH");
        const bool previouslySet =
            qEnvironmentVariableIsSet(variable.constData());
        const QByteArray previous = qgetenv(variable.constData());
        [[maybe_unused]] const auto restoreEnvironment = qScopeGuard(
            [&]()
            {
                if (previouslySet)
                {
                    qputenv(variable.constData(), previous);
                }
                else
                {
                    qunsetenv(variable.constData());
                }
            });
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        qputenv(variable.constData(), recoveryPath.toUtf8());

        Document document = Document::createDefault(QSize(80, 60));
        document.wobbleAmount = 2.5;
        QString error;
        QVERIFY2(DocumentSerializer::save(recoveryPath, document, &error),
            qPrintable(error));

        const QString preserved = RecoveryStore::preserve(&error);
        QVERIFY2(!preserved.isEmpty(), qPrintable(error));
        QVERIFY(!QFileInfo::exists(recoveryPath));
        QVERIFY(QFileInfo::exists(preserved));
        QCOMPARE(QFileInfo(preserved).suffix(), QStringLiteral("ugu"));
        QVERIFY(QFileInfo(preserved).fileName().startsWith(
            QStringLiteral("recovery-preserved-")));
        const std::optional<Document> restored =
            DocumentSerializer::load(preserved, &error);
        QVERIFY2(restored.has_value(), qPrintable(error));
        QCOMPARE(DocumentSerializer::toJson(*restored),
            DocumentSerializer::toJson(document));
    }

    void quarantinesUnreadableRecoveryWithoutDeletingIt()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QByteArray variable = QByteArrayLiteral("UGURUGU_RECOVERY_PATH");
        const bool existed = qEnvironmentVariableIsSet(variable.constData());
        const QByteArray previous = qgetenv(variable.constData());
        [[maybe_unused]] const auto restoreEnvironment = qScopeGuard(
            [&]()
            {
                if (existed)
                {
                    qputenv(variable.constData(), previous);
                }
                else
                {
                    qunsetenv(variable.constData());
                }
            });

        const QString source =
            directory.filePath(QStringLiteral("recovery.ugu"));
        QVERIFY(qputenv(variable.constData(), source.toUtf8()));
        QFile file(source);
        QVERIFY(file.open(QIODevice::WriteOnly));
        const QByteArray contents("not a project");
        QCOMPARE(file.write(contents), contents.size());
        file.close();

        QString error;
        const QString preserved = RecoveryStore::quarantine(&error);
        QVERIFY2(!preserved.isEmpty(), qPrintable(error));
        QVERIFY(!QFileInfo::exists(source));
        QVERIFY(QFileInfo::exists(preserved));
        QFile preservedFile(preserved);
        QVERIFY(preservedFile.open(QIODevice::ReadOnly));
        QCOMPARE(preservedFile.readAll(), contents);
    }

    void roundTripsAtomicRecoveryMetadata()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QByteArray variable = QByteArrayLiteral("UGURUGU_RECOVERY_PATH");
        const bool existed = qEnvironmentVariableIsSet(variable.constData());
        const QByteArray previous = qgetenv(variable.constData());
        [[maybe_unused]] const auto restoreEnvironment = qScopeGuard(
            [&]()
            {
                if (existed)
                {
                    qputenv(variable.constData(), previous);
                }
                else
                {
                    qunsetenv(variable.constData());
                }
            });
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        QVERIFY(qputenv(variable.constData(), recoveryPath.toUtf8()));

        Document document = Document::createDefault(QSize(91, 73));
        document.layers.first().name = QStringLiteral("복구 レイヤー");
        const RecoveryStore::Metadata metadata{
            QUuid(QStringLiteral("12345678-1234-4abc-8def-1234567890ab")),
            QStringLiteral(R"(\\server\공유\制作\原本.ugu)"),
            quint64(9007199254740993ULL),
            QDateTime(
                QDate(2026, 8, 2), QTime(12, 34, 56, 789), QTimeZone::UTC)};
        QString error;
        QVERIFY2(
            RecoveryStore::save(document, metadata, &error), qPrintable(error));

        const std::optional<RecoveryStore::Snapshot> recovered =
            RecoveryStore::load(&error);
        QVERIFY2(recovered.has_value(), qPrintable(error));
        QCOMPARE(
            recovered->metadataStatus, RecoveryStore::MetadataStatus::Valid);
        QVERIFY(recovered->metadata.has_value());
        QCOMPARE(recovered->metadata->sessionId, metadata.sessionId);
        QCOMPARE(recovered->metadata->sourcePath, metadata.sourcePath);
        QCOMPARE(recovered->metadata->revision, metadata.revision);
        QCOMPARE(recovered->metadata->timestampUtc, metadata.timestampUtc);
        QCOMPARE(DocumentSerializer::toJson(recovered->document),
            DocumentSerializer::toJson(document));

        const std::optional<Document> openedNormally =
            DocumentSerializer::load(recoveryPath, &error);
        QVERIFY2(openedNormally.has_value(), qPrintable(error));
        QCOMPARE(DocumentSerializer::toJson(*openedNormally),
            DocumentSerializer::toJson(document));

        QFile recoveryFile(recoveryPath);
        QVERIFY(recoveryFile.open(QIODevice::ReadOnly));
        const QByteArray recoveryBytes = recoveryFile.readAll();
        const QJsonObject recoveryRoot =
            QJsonDocument::fromJson(recoveryBytes).object();
        QVERIFY(
            recoveryRoot.value(QStringLiteral("uguruguRecovery")).isObject());
        recoveryFile.close();

        const QString normalPath =
            directory.filePath(QStringLiteral("normal.ugu"));
        QVERIFY2(DocumentSerializer::save(normalPath, *openedNormally, &error),
            qPrintable(error));
        QFile normalFile(normalPath);
        QVERIFY(normalFile.open(QIODevice::ReadOnly));
        const QJsonObject normalRoot =
            QJsonDocument::fromJson(normalFile.readAll()).object();
        QVERIFY(
            normalRoot.value(QStringLiteral("uguruguRecovery")).isUndefined());

        const QString preservedPath = RecoveryStore::preserve(&error);
        QVERIFY2(!preservedPath.isEmpty(), qPrintable(error));
        QFile preservedFile(preservedPath);
        QVERIFY(preservedFile.open(QIODevice::ReadOnly));
        const QByteArray preservedBytes = preservedFile.readAll();
        const QJsonObject preservedRoot =
            QJsonDocument::fromJson(preservedBytes).object();
        QVERIFY(
            preservedRoot.value(QStringLiteral("uguruguRecovery")).isObject());
        QCOMPARE(preservedBytes, recoveryBytes);
    }

    void recoversDocumentWithInvalidRecoveryMetadata()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QByteArray variable = QByteArrayLiteral("UGURUGU_RECOVERY_PATH");
        const bool existed = qEnvironmentVariableIsSet(variable.constData());
        const QByteArray previous = qgetenv(variable.constData());
        [[maybe_unused]] const auto restoreEnvironment = qScopeGuard(
            [&]()
            {
                if (existed)
                {
                    qputenv(variable.constData(), previous);
                }
                else
                {
                    qunsetenv(variable.constData());
                }
            });
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        QVERIFY(qputenv(variable.constData(), recoveryPath.toUtf8()));

        Document document = Document::createDefault(QSize(83, 67));
        const RecoveryStore::Metadata metadata{QUuid::createUuid(),
            QStringLiteral("/tmp/source.ugu"),
            7,
            QDateTime::currentDateTimeUtc()};
        QString error;
        QVERIFY2(
            RecoveryStore::save(document, metadata, &error), qPrintable(error));
        QFile recoveryFile(recoveryPath);
        QVERIFY(recoveryFile.open(QIODevice::ReadOnly));
        QJsonDocument json = QJsonDocument::fromJson(recoveryFile.readAll());
        recoveryFile.close();
        QJsonObject root = json.object();
        QJsonObject invalidMetadata =
            root.value(QStringLiteral("uguruguRecovery")).toObject();
        invalidMetadata.insert(QStringLiteral("formatVersion"), 99);
        root.insert(QStringLiteral("uguruguRecovery"), invalidMetadata);
        json.setObject(root);
        QSaveFile invalidFile(recoveryPath);
        QVERIFY(invalidFile.open(QIODevice::WriteOnly));
        const QByteArray invalidData = json.toJson(QJsonDocument::Compact);
        QCOMPARE(invalidFile.write(invalidData), invalidData.size());
        QVERIFY(invalidFile.commit());

        const std::optional<RecoveryStore::Snapshot> recovered =
            RecoveryStore::load(&error);
        QVERIFY2(recovered.has_value(), qPrintable(error));
        QCOMPARE(
            recovered->metadataStatus, RecoveryStore::MetadataStatus::Invalid);
        QVERIFY(!recovered->metadata.has_value());
        QCOMPARE(DocumentSerializer::toJson(recovered->document),
            DocumentSerializer::toJson(document));
        QVERIFY(QFileInfo::exists(recoveryPath));
    }

    void preservesExistingRecoveryWhenSerializationFails()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QByteArray variable = QByteArrayLiteral("UGURUGU_RECOVERY_PATH");
        const bool existed = qEnvironmentVariableIsSet(variable.constData());
        const QByteArray previous = qgetenv(variable.constData());
        [[maybe_unused]] const auto restoreEnvironment = qScopeGuard(
            [&]()
            {
                if (existed)
                {
                    qputenv(variable.constData(), previous);
                }
                else
                {
                    qunsetenv(variable.constData());
                }
            });
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        QVERIFY(qputenv(variable.constData(), recoveryPath.toUtf8()));

        const RecoveryStore::Metadata metadata{
            QUuid::createUuid(), QString(), 1, QDateTime::currentDateTimeUtc()};
        QString error;
        QVERIFY2(RecoveryStore::save(
                     Document::createDefault(QSize(79, 61)), metadata, &error),
            qPrintable(error));
        QFile beforeFile(recoveryPath);
        QVERIFY(beforeFile.open(QIODevice::ReadOnly));
        const QByteArray before = beforeFile.readAll();
        beforeFile.close();

        Document invalid = Document::createDefault(QSize(79, 61));
        invalid.size = QSize();
        QVERIFY(!RecoveryStore::save(invalid, metadata, &error));
        QVERIFY(!error.isEmpty());
        QFile afterFile(recoveryPath);
        QVERIFY(afterFile.open(QIODevice::ReadOnly));
        QCOMPARE(afterFile.readAll(), before);
    }

    void createsDefaultDocument()
    {
        const QSize size(640, 360);
        const Document document = Document::createDefault(size);

        QCOMPARE(document.size, size);
        QCOMPARE(document.background, QColor(Qt::white));
        QCOMPARE(document.animationFrames, 30);
        QCOMPARE(document.framesPerSecond, 25.0);
        QCOMPARE(document.wobbleAmount, 1.6);
        QCOMPARE(document.layers.size(), 1);
        QCOMPARE(document.layers.first().name, QStringLiteral("Layer 1"));
        QVERIFY(document.layers.first().visible);
        QCOMPARE(document.layers.first().opacity, 1.0);
        QCOMPARE(document.layers.first().blendMode, LayerBlendMode::Normal);
        QCOMPARE(document.layers.first().kind, LayerKind::Paint);
        QVERIFY(document.layers.first().parentGroupId.isNull());
        QVERIFY(!document.layers.first().clipToLayerBelow);
        QVERIFY(!document.layers.first().reference);
        QVERIFY(document.layers.first().strokes.isEmpty());
        QCOMPARE(document.activeLayerId, document.layers.first().id);
        QCOMPARE(document.layerIndex(document.activeLayerId), 0);
        QVERIFY(document.layer(document.activeLayerId) != nullptr);
        QVERIFY(document.layer(QUuid::createUuid()) == nullptr);

        const Document localized =
            Document::createDefault(size, QStringLiteral("초기 레이어"));
        QCOMPARE(localized.layers.first().name, QStringLiteral("초기 레이어"));
    }

    void preservesStateWhenNewDocumentPreparationFails()
    {
        DocumentController controller;
        prepareDocumentTransitionFailureState(controller);

        verifyDocumentTransitionPreparationFailure(controller,
            [&controller](QString *error)
            {
                return controller.newDocument(QSize(640, 480), error);
            });
    }

    void preservesStateWhenLoadedDocumentPreparationFails()
    {
        DocumentController controller;
        prepareDocumentTransitionFailureState(controller);
        Document replacement = Document::createDefault(QSize(640, 480));
        replacement.wobbleAmount = 4.0;

        verifyDocumentTransitionPreparationFailure(controller,
            [&controller, &replacement](QString *error)
            {
                return controller.loadDocument(std::move(replacement), error);
            });
    }

    void preservesStateWhenRecoveredDocumentPreparationFails()
    {
        DocumentController controller;
        prepareDocumentTransitionFailureState(controller);
        Document replacement = Document::createDefault(QSize(640, 480));
        replacement.wobbleAmount = 4.0;

        verifyDocumentTransitionPreparationFailure(controller,
            [&controller, &replacement](QString *error)
            {
                return controller.loadRecoveredDocument(
                    std::move(replacement), error);
            });
    }

    void preservesOpenHistoryWhenDocumentReplacementIsRejected()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;
        controller.undoStack()->beginMacro(QStringLiteral("Open macro"));
        controller.renameLayer(layerId, QStringLiteral("Macro layer"));
        const DocumentTransitionSnapshot before =
            documentTransitionSnapshot(controller);

        QString serializationError;
        QVERIFY(
            controller.serializeDocument({}, &serializationError).isEmpty());
        QVERIFY(!serializationError.isEmpty());
        compareDocumentTransitionSnapshot(controller, before);

        QString error;
        QVERIFY(!controller.loadDocument(
            Document::createDefault(QSize(640, 480)), &error));
        QVERIFY(!error.isEmpty());
        compareDocumentTransitionSnapshot(controller, before);

        controller.undoStack()->endMacro();
        QCOMPARE(controller.undoStack()->count(), 1);
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Macro layer"));
        QVERIFY(controller.isModified());
    }

    void installsReplacementStateBeforeRejectingNestedReplacement()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QSize replacementSize(640, 480);
        constexpr qreal replacementWobble = 4.0;
        Document replacement = Document::createDefault(replacementSize);
        replacement.wobbleAmount = replacementWobble;
        bool stateInstalledBeforeSignal = false;
        bool nestedResult = true;
        QString nestedError;
        QObject::connect(&controller,
            &DocumentController::documentReplaced,
            &controller,
            [&]()
            {
                stateInstalledBeforeSignal =
                    controller.document().size == replacementSize
                    && qFuzzyCompare(
                        controller.document().wobbleAmount, replacementWobble)
                    && controller.undoStack()->count() == 0
                    && controller.undoStack()->index() == 0
                    && controller.undoStack()->isClean()
                    && DocumentControllerTestAccess::historyNode(controller)
                           == 0
                    && DocumentControllerTestAccess::nextHistoryNode(controller)
                           == 0
                    && DocumentControllerTestAccess::contentRevision(controller)
                           == 0
                    && DocumentControllerTestAccess::savedContentRevision(
                           controller)
                           == 0
                    && DocumentControllerTestAccess::nextContentRevision(
                           controller)
                           == 0;
                nestedResult = controller.loadRecoveredDocument(
                    Document::createDefault(QSize(128, 128)), &nestedError);
            });

        QString error;
        QVERIFY(controller.loadDocument(std::move(replacement), &error));
        QVERIFY(error.isEmpty());
        QVERIFY(stateInstalledBeforeSignal);
        QVERIFY(!nestedResult);
        QVERIFY(!nestedError.isEmpty());
        QCOMPARE(controller.document().size, replacementSize);
        QCOMPARE(controller.document().wobbleAmount, replacementWobble);
        QVERIFY(!controller.isModified());
    }
};

int runDocumentLifecycleTests(int argc, char **argv)
{
    DocumentLifecycleTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "DocumentLifecycleTests.moc"
