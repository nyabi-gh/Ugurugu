// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/UiTestHelpers.hpp"
#include "support/UiTestSuites.hpp"

#include <QMessageBox>

namespace ugurugu
{

class UiSessionTests final : public QObject
{
    Q_OBJECT

private slots:
    void init()
    {
        QSettings settings;
        settings.remove(QStringLiteral("drawingTools"));
        settings.remove(QStringLiteral("brush/recentColors"));
        settings.remove(QStringLiteral("brush/colorHistory"));
        settings.remove(QStringLiteral("canvas/strokeStabilization"));
        settings.sync();
    }

    void cleanup()
    {
        QSettings settings;
        settings.remove(QStringLiteral("drawingTools"));
        settings.remove(QStringLiteral("brush/recentColors"));
        settings.remove(QStringLiteral("brush/colorHistory"));
        settings.remove(QStringLiteral("canvas/strokeStabilization"));
        settings.sync();
    }

    void prioritizesRecoveryOverRequestedStartupFile()
    {
        const QString recoveryKey = QStringLiteral("recovery/sourcePath");
        SettingValueGuard recoveryValueGuard(recoveryKey);
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        const QString requestedPath =
            directory.filePath(QStringLiteral("requested.ugu"));
        const QString recoverySourcePath =
            directory.filePath(QStringLiteral("original-project.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(173, 109));
        recoveryDocument.layers.first().name = QStringLiteral("Recovered");
        Document requestedDocument = Document::createDefault(QSize(257, 131));
        requestedDocument.layers.first().name = QStringLiteral("Requested");
        QString error;
        QVERIFY2(
            DocumentSerializer::save(recoveryPath, recoveryDocument, &error),
            qPrintable(error));
        QVERIFY2(
            DocumentSerializer::save(requestedPath, requestedDocument, &error),
            qPrintable(error));

        QSettings settings;
        settings.setValue(recoveryKey, recoverySourcePath);
        settings.sync();

        bool recoverClicked = false;
        MainWindow window;
        scheduleDialogButtonClick(
            &window, QStringLiteral("startupRecoverButton"), &recoverClicked);

        const MainWindow::StartupResult result =
            window.initializeSession(requestedPath);

        QVERIFY(recoverClicked);
        QCOMPARE(result, MainWindow::StartupResult::Recovered);
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);
        QCOMPARE(DocumentSerializer::toJson(
                     canvas->documentWithPendingSelectionTransform()),
            DocumentSerializer::toJson(recoveryDocument));
        QVERIFY(window.windowFilePath().isEmpty());
        QVERIFY(!settings.contains(recoveryKey));
        QVERIFY(QFileInfo::exists(recoveryPath));

        const std::optional<Document> retainedRecovery =
            DocumentSerializer::load(recoveryPath, &error);
        QVERIFY2(retainedRecovery.has_value(), qPrintable(error));
        QCOMPARE(DocumentSerializer::toJson(*retainedRecovery),
            DocumentSerializer::toJson(recoveryDocument));
    }

    void rejectsReservedRecoveryProjectPaths()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        MainWindow window;
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);
        const QByteArray documentBefore = DocumentSerializer::toJson(
            canvas->documentWithPendingSelectionTransform());

        Document sentinel = Document::createDefault(QSize(167, 103));
        sentinel.layers.first().name = QStringLiteral("Recovery sentinel");
        QString error;
        QVERIFY2(DocumentSerializer::save(recoveryPath, sentinel, &error),
            qPrintable(error));
        QFile recoveryFile(recoveryPath);
        QVERIFY(recoveryFile.open(QIODevice::ReadOnly));
        const QByteArray recoveryBytes = recoveryFile.readAll();
        recoveryFile.close();

        const auto dismissWarning = [&window](bool *shown)
        {
            QTimer::singleShot(0,
                &window,
                [shown]()
                {
                    QDialog *dialog = qobject_cast<QDialog *>(
                        QApplication::activeModalWidget());
                    if (!dialog)
                    {
                        return;
                    }
                    *shown = true;
                    dialog->accept();
                });
        };

        bool openWarningShown = false;
        dismissWarning(&openWarningShown);
        QVERIFY(!window.openFile(recoveryPath));
        QVERIFY(openWarningShown);
        QCOMPARE(DocumentSerializer::toJson(
                     canvas->documentWithPendingSelectionTransform()),
            documentBefore);
        QFile afterOpen(recoveryPath);
        QVERIFY(afterOpen.open(QIODevice::ReadOnly));
        QCOMPARE(afterOpen.readAll(), recoveryBytes);
        afterOpen.close();

        bool saveWarningShown = false;
        dismissWarning(&saveWarningShown);
        QVERIFY(!MainWindowTestAccess::saveToFile(window, recoveryPath));
        QVERIFY(saveWarningShown);
        QFile afterSave(recoveryPath);
        QVERIFY(afterSave.open(QIODevice::ReadOnly));
        QCOMPARE(afterSave.readAll(), recoveryBytes);
        afterSave.close();

        MainWindowTestAccess::setCurrentFilePath(window, recoveryPath);
        MainWindowTestAccess::writeModifiedAutosave(window);
        QVERIFY(MainWindowTestAccess::clearAutosave(window));
        QFile afterAutosave(recoveryPath);
        QVERIFY(afterAutosave.open(QIODevice::ReadOnly));
        QCOMPARE(afterAutosave.readAll(), recoveryBytes);
    }

    void capturesAutosaveSnapshotWhileEditingContinues()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());

        MainWindow window;
        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        controller.addLayer();
        const qsizetype layersAtSubmit = controller.document().layers.size();

        MainWindowTestAccess::requestAutosave(window);
        controller.addLayer();
        QVERIFY(MainWindowTestAccess::flushAutosave(window));

        QString error;
        std::optional<RecoveryStore::Snapshot> snapshot =
            RecoveryStore::load(&error);
        QVERIFY2(snapshot.has_value(), qPrintable(error));
        QCOMPARE(snapshot->document.layers.size(), layersAtSubmit);

        MainWindowTestAccess::requestAutosave(window);
        QVERIFY(MainWindowTestAccess::flushAutosave(window));
        snapshot = RecoveryStore::load(&error);
        QVERIFY2(snapshot.has_value(), qPrintable(error));
        QCOMPARE(snapshot->document.layers.size(), layersAtSubmit + 1);
        QVERIFY(snapshot->metadata.has_value());
        QVERIFY(snapshot->metadata->revision >= 2);
    }

    void ignoresStaleAutosaveCompletionForNewerPendingWork()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());

        MainWindow window;
        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        controller.addLayer();

        MainWindowTestAccess::setAutosaveWriterSuspended(window, true);
        MainWindowTestAccess::requestAutosave(window);
        const quint64 firstRevision =
            MainWindowTestAccess::submittedRecoveryRevision(window);

        controller.addLayer();
        MainWindowTestAccess::requestAutosave(window);
        const quint64 secondRevision =
            MainWindowTestAccess::submittedRecoveryRevision(window);
        QVERIFY(secondRevision > firstRevision);
        QVERIFY(MainWindowTestAccess::autosavePending(window));

        // The first write completes only after the second submission, the way
        // an in-flight write races a newer one. Its success must not release
        // the pending state the second write still owes.
        MainWindowTestAccess::deliverAutosaveCompletion(
            window, true, firstRevision, {});
        QVERIFY2(MainWindowTestAccess::autosavePending(window),
            "a stale autosave completion released the newer pending state");

        MainWindowTestAccess::deliverAutosaveCompletion(
            window, true, secondRevision, {});
        QVERIFY(!MainWindowTestAccess::autosavePending(window));

        MainWindowTestAccess::setAutosaveWriterSuspended(window, false);
        QVERIFY(MainWindowTestAccess::flushAutosave(window));
    }

    void documentEditsArmTheAutosaveThroughTheDocumentSignal()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        MainWindow window;
        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        // An unmodified document has nothing to autosave, so the pending state
        // is settled from a first edit rather than from a fresh window.
        controller.addLayer();
        MainWindowTestAccess::requestAutosave(window);
        QVERIFY(MainWindowTestAccess::flushAutosave(window));
        QVERIFY(!MainWindowTestAccess::autosavePending(window));

        // The arming itself is the contract under test, so nothing here may
        // set the pending state on the window's behalf.
        controller.addLayer();
        QVERIFY2(MainWindowTestAccess::autosavePending(window),
            "an edit did not arm the autosave through documentChanged");

        const int layerCount =
            static_cast<int>(controller.document().layers.size());
        MainWindowTestAccess::writeAutosave(window);
        QVERIFY(MainWindowTestAccess::flushAutosave(window));
        QVERIFY(!MainWindowTestAccess::autosavePending(window));
        QString error;
        const std::optional<RecoveryStore::Snapshot> snapshot =
            RecoveryStore::load(&error);
        QVERIFY2(snapshot.has_value(), qPrintable(error));
        QCOMPARE(
            static_cast<int>(snapshot->document.layers.size()), layerCount);
    }

    void aFailedRecoveryWriteIsShownToTheArtist()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());

        MainWindow window;
        QVERIFY(MainWindowTestAccess::recoveryFailureText(window).isEmpty());

        // A failed snapshot only reached the log. Nothing in the window said
        // the work had stopped being protected, so the artist kept drawing
        // against an autosave that was not happening.
        MainWindowTestAccess::deliverAutosaveCompletion(
            window, false, 3, QStringLiteral("Disk is full."));
        QVERIFY(!MainWindowTestAccess::recoveryFailureText(window).isEmpty());
        QMessageBox *notice = window.findChild<QMessageBox *>(
            QStringLiteral("recoveryFailureNotice"));
        QVERIFY(notice);
        notice->close();

        MainWindowTestAccess::deliverAutosaveCompletion(
            window, true, 4, QString());
        QVERIFY(MainWindowTestAccess::recoveryFailureText(window).isEmpty());
    }

    void recoveryWriteReportsFailureWhenTheWriteThrows()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        qputenv("UGURUGU_RECOVERY_PATH",
            directory.filePath(QStringLiteral("recovery.ugu")).toUtf8());

        RecoveryWriter writer;
        QSignalSpy finished(&writer, &RecoveryWriter::writeFinished);
        RecoveryStore::Metadata metadata;
        metadata.sessionId = QUuid::createUuid();
        metadata.revision = 7;
        metadata.timestampUtc = QDateTime::currentDateTimeUtc();

        writer.throwFromNextWriteForTesting();
        writer.submitWrite(Document::createDefault(QSize(32, 32)), metadata);
        QTRY_COMPARE_WITH_TIMEOUT(finished.size(), 1, 10000);
        QVERIFY2(!finished.at(0).at(0).toBool(),
            "a throwing recovery write reported success");
        QCOMPARE(finished.at(0).at(1).toULongLong(), metadata.revision);
        QVERIFY(!finished.at(0).at(2).toString().isEmpty());

        // A worker left busy would hang the destructor's wait, and the pending
        // snapshot would never be retried.
        QVERIFY(writer.waitForIdle(5000));
        metadata.revision = 8;
        writer.submitWrite(Document::createDefault(QSize(32, 32)), metadata);
        QTRY_COMPARE_WITH_TIMEOUT(finished.size(), 2, 10000);
        QVERIFY2(finished.at(1).at(0).toBool(),
            qPrintable(finished.at(1).at(2).toString()));
    }

    void dropsQueuedAutosaveAfterDiscard()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        MainWindow window;
        DocumentController &controller =
            MainWindowTestAccess::controller(window);
        controller.addLayer();

        MainWindowTestAccess::setAutosaveWriterSuspended(window, true);
        MainWindowTestAccess::requestAutosave(window);
        QVERIFY(MainWindowTestAccess::clearAutosave(window));
        MainWindowTestAccess::setAutosaveWriterSuspended(window, false);
        QVERIFY(MainWindowTestAccess::flushAutosave(window));

        QVERIFY(!QFileInfo::exists(recoveryPath));
    }

    void flushesPendingAutosaveOnWindowTeardown()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        {
            MainWindow window;
            DocumentController &controller =
                MainWindowTestAccess::controller(window);
            controller.addLayer();
            MainWindowTestAccess::setAutosaveWriterSuspended(window, true);
            MainWindowTestAccess::requestAutosave(window);
        }

        QVERIFY(QFileInfo::exists(recoveryPath));
        QString error;
        const std::optional<RecoveryStore::Snapshot> snapshot =
            RecoveryStore::load(&error);
        QVERIFY2(snapshot.has_value(), qPrintable(error));
        QCOMPARE(snapshot->document.layers.size(), 2);
    }

    void treatsRequestedRecoveryPathAsRecoveryOnly()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(171, 105));
        QString error;
        QVERIFY2(
            DocumentSerializer::save(recoveryPath, recoveryDocument, &error),
            qPrintable(error));
        QFile recoveryFile(recoveryPath);
        QVERIFY(recoveryFile.open(QIODevice::ReadOnly));
        const QByteArray recoveryBytes = recoveryFile.readAll();
        recoveryFile.close();

        bool dialogInspected = false;
        bool recoveryOnly = false;
        MainWindow window;
        QTimer::singleShot(0,
            &window,
            [&dialogInspected, &recoveryOnly]()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (!dialog)
                {
                    return;
                }
                dialogInspected = true;
                recoveryOnly = !dialog->findChild<QPushButton *>(
                    QStringLiteral("startupPreserveRecoveryButton"));
                QPushButton *cancelButton = dialog->findChild<QPushButton *>(
                    QStringLiteral("startupCancelButton"));
                if (cancelButton)
                {
                    cancelButton->click();
                }
                else
                {
                    dialog->reject();
                }
            });

        QCOMPARE(window.initializeSession(recoveryPath),
            MainWindow::StartupResult::Canceled);
        QVERIFY(dialogInspected);
        QVERIFY(recoveryOnly);
        QFile unchangedRecovery(recoveryPath);
        QVERIFY(unchangedRecovery.open(QIODevice::ReadOnly));
        QCOMPARE(unchangedRecovery.readAll(), recoveryBytes);
    }

    void cancelingRecoveredSaveAsPreservesSourceAndRecovery()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        const QString sourcePath =
            directory.filePath(QStringLiteral("source.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(179, 107));
        recoveryDocument.layers.first().name = QStringLiteral("Recovered");
        const RecoveryStore::Metadata metadata{QUuid::createUuid(),
            sourcePath,
            4,
            QDateTime::currentDateTimeUtc()};
        QString error;
        QVERIFY2(RecoveryStore::save(recoveryDocument, metadata, &error),
            qPrintable(error));

        Document newerSource = Document::createDefault(QSize(251, 149));
        newerSource.layers.first().name = QStringLiteral("Newer source");
        QVERIFY2(DocumentSerializer::save(sourcePath, newerSource, &error),
            qPrintable(error));

        QFile recoveryFile(recoveryPath);
        QVERIFY(recoveryFile.open(QIODevice::ReadOnly));
        const QByteArray recoveryBytes = recoveryFile.readAll();
        recoveryFile.close();
        QFile sourceFile(sourcePath);
        QVERIFY(sourceFile.open(QIODevice::ReadOnly));
        const QByteArray sourceBytes = sourceFile.readAll();
        sourceFile.close();

        bool recoverClicked = false;
        MainWindow window;
        scheduleDialogButtonClick(
            &window, QStringLiteral("startupRecoverButton"), &recoverClicked);
        QCOMPARE(
            window.initializeSession(), MainWindow::StartupResult::Recovered);
        QVERIFY(recoverClicked);
        QVERIFY(window.windowFilePath().isEmpty());

        QAction *saveAction =
            window.findChild<QAction *>(QStringLiteral("saveAction"));
        QVERIFY(saveAction);
        bool saveDialogRejected = false;
        QTimer::singleShot(0,
            &window,
            [&saveDialogRejected]()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (!dialog)
                {
                    return;
                }
                saveDialogRejected = true;
                dialog->reject();
            });
        QTimer::singleShot(1000,
            &window,
            []()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (dialog)
                {
                    dialog->reject();
                }
            });
        saveAction->trigger();

        QVERIFY(saveDialogRejected);
        QVERIFY(window.windowFilePath().isEmpty());
        QFile unchangedSource(sourcePath);
        QVERIFY(unchangedSource.open(QIODevice::ReadOnly));
        QCOMPARE(unchangedSource.readAll(), sourceBytes);
        QFile unchangedRecovery(recoveryPath);
        QVERIFY(unchangedRecovery.open(QIODevice::ReadOnly));
        QCOMPARE(unchangedRecovery.readAll(), recoveryBytes);
    }

    void preservesRecoveryBeforeOpeningRequestedStartupFile()
    {
        const QString recoveryKey = QStringLiteral("recovery/sourcePath");
        SettingValueGuard recoveryValueGuard(recoveryKey);
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        const QString requestedPath =
            directory.filePath(QStringLiteral("requested.ugu"));
        const QString recoverySourcePath =
            directory.filePath(QStringLiteral("original-project.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(181, 113));
        recoveryDocument.layers.first().name = QStringLiteral("Recovered");
        Document requestedDocument = Document::createDefault(QSize(263, 137));
        requestedDocument.layers.first().name = QStringLiteral("Requested");
        QString error;
        QVERIFY2(
            DocumentSerializer::save(recoveryPath, recoveryDocument, &error),
            qPrintable(error));
        QVERIFY2(
            DocumentSerializer::save(requestedPath, requestedDocument, &error),
            qPrintable(error));

        QSettings settings;
        settings.setValue(recoveryKey, recoverySourcePath);
        settings.sync();

        bool preserveClicked = false;
        MainWindow window;
        scheduleDialogButtonClick(&window,
            QStringLiteral("startupPreserveRecoveryButton"),
            &preserveClicked);

        const MainWindow::StartupResult result =
            window.initializeSession(requestedPath);

        QVERIFY(preserveClicked);
        QCOMPARE(result, MainWindow::StartupResult::OpenedRequestedFile);
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);
        QCOMPARE(DocumentSerializer::toJson(
                     canvas->documentWithPendingSelectionTransform()),
            DocumentSerializer::toJson(requestedDocument));
        QCOMPARE(window.windowFilePath(),
            QFileInfo(requestedPath).absoluteFilePath());
        QVERIFY(!QFileInfo::exists(recoveryPath));

        const QStringList preservedFiles =
            QDir(directory.path())
                .entryList(
                    {QStringLiteral("recovery-preserved-*.ugu")}, QDir::Files);
        QCOMPARE(preservedFiles.size(), 1);
        const QString preservedPath =
            directory.filePath(preservedFiles.first());
        const std::optional<Document> preservedRecovery =
            DocumentSerializer::load(preservedPath, &error);
        QVERIFY2(preservedRecovery.has_value(), qPrintable(error));
        QCOMPARE(DocumentSerializer::toJson(*preservedRecovery),
            DocumentSerializer::toJson(recoveryDocument));
        QVERIFY(!settings.contains(recoveryKey));

        QToolButton *addLayerButton =
            window.findChild<QToolButton *>(QStringLiteral("layerAddButton"));
        QVERIFY(addLayerButton);
        addLayerButton->click();
        QTRY_VERIFY(window.isWindowModified());
        QEvent deactivate(QEvent::ApplicationDeactivate);
        QApplication::sendEvent(qApp, &deactivate);
        QVERIFY(MainWindowTestAccess::flushAutosave(window));

        QVERIFY(QFileInfo::exists(recoveryPath));
        QVERIFY(QFileInfo::exists(preservedPath));
        const std::optional<RecoveryStore::Snapshot> currentRecovery =
            RecoveryStore::load(&error);
        QVERIFY2(currentRecovery.has_value(), qPrintable(error));
        QCOMPARE(currentRecovery->metadataStatus,
            RecoveryStore::MetadataStatus::Valid);
        QVERIFY(currentRecovery->metadata.has_value());
        QCOMPARE(currentRecovery->metadata->sourcePath,
            QFileInfo(requestedPath).absoluteFilePath());
        QCOMPARE(currentRecovery->metadata->revision, quint64(1));
        QVERIFY(!currentRecovery->metadata->sessionId.isNull());
        QVERIFY(currentRecovery->metadata->timestampUtc.isValid());
        QCOMPARE(DocumentSerializer::toJson(currentRecovery->document),
            DocumentSerializer::toJson(
                canvas->documentWithPendingSelectionTransform()));
        QVERIFY(!settings.contains(recoveryKey));
    }

    void keepsRecoveryWhenStartupIsCanceled()
    {
        const QString recoveryKey = QStringLiteral("recovery/sourcePath");
        SettingValueGuard recoveryValueGuard(recoveryKey);
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        const QString requestedPath =
            directory.filePath(QStringLiteral("requested.ugu"));
        const QString recoverySourcePath =
            directory.filePath(QStringLiteral("original-project.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(191, 127));
        recoveryDocument.layers.first().name = QStringLiteral("Recovered");
        Document requestedDocument = Document::createDefault(QSize(269, 139));
        requestedDocument.layers.first().name = QStringLiteral("Requested");
        QString error;
        QVERIFY2(
            DocumentSerializer::save(recoveryPath, recoveryDocument, &error),
            qPrintable(error));
        QVERIFY2(
            DocumentSerializer::save(requestedPath, requestedDocument, &error),
            qPrintable(error));

        QSettings settings;
        settings.setValue(recoveryKey, recoverySourcePath);
        settings.sync();

        bool cancelClicked = false;
        MainWindow window;
        scheduleDialogButtonClick(
            &window, QStringLiteral("startupCancelButton"), &cancelClicked);

        const MainWindow::StartupResult result =
            window.initializeSession(requestedPath);

        QVERIFY(cancelClicked);
        QCOMPARE(result, MainWindow::StartupResult::Canceled);
        QVERIFY(QFileInfo::exists(recoveryPath));
        QCOMPARE(settings.value(recoveryKey).toString(), recoverySourcePath);
        QVERIFY(window.windowFilePath().isEmpty());

        const std::optional<Document> retainedRecovery =
            DocumentSerializer::load(recoveryPath, &error);
        QVERIFY2(retainedRecovery.has_value(), qPrintable(error));
        QCOMPARE(DocumentSerializer::toJson(*retainedRecovery),
            DocumentSerializer::toJson(recoveryDocument));
    }

    void discardsRecoveryBeforeOpeningRequestedStartupFile()
    {
        const QString recoveryKey = QStringLiteral("recovery/sourcePath");
        SettingValueGuard recoveryValueGuard(recoveryKey);
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        const QString requestedPath =
            directory.filePath(QStringLiteral("requested.ugu"));
        const QString recoverySourcePath =
            directory.filePath(QStringLiteral("original-project.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(193, 131));
        recoveryDocument.layers.first().name = QStringLiteral("Recovered");
        Document requestedDocument = Document::createDefault(QSize(271, 151));
        requestedDocument.layers.first().name = QStringLiteral("Requested");
        QString error;
        QVERIFY2(
            DocumentSerializer::save(recoveryPath, recoveryDocument, &error),
            qPrintable(error));
        QVERIFY2(
            DocumentSerializer::save(requestedPath, requestedDocument, &error),
            qPrintable(error));

        QSettings settings;
        settings.setValue(recoveryKey, recoverySourcePath);
        settings.sync();

        bool discardClicked = false;
        MainWindow window;
        scheduleDialogButtonClick(&window,
            QStringLiteral("startupDiscardRecoveryButton"),
            &discardClicked);

        const MainWindow::StartupResult result =
            window.initializeSession(requestedPath);

        QVERIFY(discardClicked);
        QCOMPARE(result, MainWindow::StartupResult::OpenedRequestedFile);
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);
        QCOMPARE(DocumentSerializer::toJson(
                     canvas->documentWithPendingSelectionTransform()),
            DocumentSerializer::toJson(requestedDocument));
        QCOMPARE(window.windowFilePath(),
            QFileInfo(requestedPath).absoluteFilePath());
        QVERIFY(!QFileInfo::exists(recoveryPath));
        QVERIFY(!settings.contains(recoveryKey));
        const QStringList preservedFiles =
            QDir(directory.path())
                .entryList(
                    {QStringLiteral("recovery-preserved-*.ugu")}, QDir::Files);
        QVERIFY(preservedFiles.isEmpty());
    }

    void discardsRecoveryWithoutARequestedStartupFile()
    {
        const QString recoveryKey = QStringLiteral("recovery/sourcePath");
        SettingValueGuard recoveryValueGuard(recoveryKey);
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        const QString recoverySourcePath =
            directory.filePath(QStringLiteral("original-project.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(199, 137));
        QString error;
        QVERIFY2(
            DocumentSerializer::save(recoveryPath, recoveryDocument, &error),
            qPrintable(error));
        QSettings settings;
        settings.setValue(recoveryKey, recoverySourcePath);
        settings.sync();

        bool discardClicked = false;
        MainWindow window;
        scheduleDialogButtonClick(&window,
            QStringLiteral("startupDiscardRecoveryButton"),
            &discardClicked);

        const MainWindow::StartupResult result = window.initializeSession();

        QVERIFY(discardClicked);
        QCOMPARE(result, MainWindow::StartupResult::Ready);
        QVERIFY(!QFileInfo::exists(recoveryPath));
        QVERIFY(!settings.contains(recoveryKey));
        QVERIFY(window.windowFilePath().isEmpty());
    }

    void keepsRecoveryWhenRecoveryTransitionFails()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(217, 159));
        recoveryDocument.layers.first().name = QStringLiteral("Recovered");
        QString error;
        QVERIFY2(
            DocumentSerializer::save(recoveryPath, recoveryDocument, &error),
            qPrintable(error));
        QFile recoveryFile(recoveryPath);
        QVERIFY(recoveryFile.open(QIODevice::ReadOnly));
        const QByteArray recoveryBytes = recoveryFile.readAll();
        recoveryFile.close();

        bool recoverClicked = false;
        bool failureAccepted = false;
        MainWindow window;
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);
        const QByteArray documentBefore = DocumentSerializer::toJson(
            canvas->documentWithPendingSelectionTransform());
        MainWindowTestAccess::failNextDocumentReplacementPreparation(window);
        scheduleDialogButtonClickAndAcceptNext(&window,
            QStringLiteral("startupRecoverButton"),
            &recoverClicked,
            &failureAccepted);

        QCOMPARE(window.initializeSession(), MainWindow::StartupResult::Failed);
        QVERIFY(recoverClicked);
        QVERIFY(failureAccepted);
        QVERIFY(window.windowFilePath().isEmpty());
        QCOMPARE(DocumentSerializer::toJson(
                     canvas->documentWithPendingSelectionTransform()),
            documentBefore);
        QFile unchangedRecovery(recoveryPath);
        QVERIFY(unchangedRecovery.open(QIODevice::ReadOnly));
        QCOMPARE(unchangedRecovery.readAll(), recoveryBytes);
    }

    void preservesArchivedRecoveryWhenRequestedTransitionFails()
    {
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        const QString requestedPath =
            directory.filePath(QStringLiteral("requested.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(219, 161));
        recoveryDocument.layers.first().name = QStringLiteral("Recovered");
        Document requestedDocument = Document::createDefault(QSize(281, 173));
        requestedDocument.layers.first().name = QStringLiteral("Requested");
        QString error;
        QVERIFY2(
            DocumentSerializer::save(recoveryPath, recoveryDocument, &error),
            qPrintable(error));
        QVERIFY2(
            DocumentSerializer::save(requestedPath, requestedDocument, &error),
            qPrintable(error));
        QFile recoveryFile(recoveryPath);
        QVERIFY(recoveryFile.open(QIODevice::ReadOnly));
        const QByteArray recoveryBytes = recoveryFile.readAll();
        recoveryFile.close();

        bool preserveClicked = false;
        bool failureAccepted = false;
        MainWindow window;
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);
        const QByteArray documentBefore = DocumentSerializer::toJson(
            canvas->documentWithPendingSelectionTransform());
        MainWindowTestAccess::failNextDocumentReplacementPreparation(window);
        scheduleDialogButtonClickAndAcceptNext(&window,
            QStringLiteral("startupPreserveRecoveryButton"),
            &preserveClicked,
            &failureAccepted);

        QCOMPARE(window.initializeSession(requestedPath),
            MainWindow::StartupResult::Failed);
        QVERIFY(preserveClicked);
        QVERIFY(failureAccepted);
        QVERIFY(window.windowFilePath().isEmpty());
        QCOMPARE(DocumentSerializer::toJson(
                     canvas->documentWithPendingSelectionTransform()),
            documentBefore);
        QVERIFY(!QFileInfo::exists(recoveryPath));
        const QStringList preservedFiles =
            QDir(directory.path())
                .entryList(
                    {QStringLiteral("recovery-preserved-*.ugu")}, QDir::Files);
        QCOMPARE(preservedFiles.size(), 1);
        QFile preservedFile(directory.filePath(preservedFiles.first()));
        QVERIFY(preservedFile.open(QIODevice::ReadOnly));
        QCOMPARE(preservedFile.readAll(), recoveryBytes);
    }

    void keepsRecoveryWhenRequestedStartupTransitionFails()
    {
        const QString recoveryKey = QStringLiteral("recovery/sourcePath");
        SettingValueGuard recoveryValueGuard(recoveryKey);
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        const QString requestedPath =
            directory.filePath(QStringLiteral("requested.ugu"));
        const QString recoverySourcePath =
            directory.filePath(QStringLiteral("original-project.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(223, 163));
        Document requestedDocument = Document::createDefault(QSize(277, 167));
        QString error;
        QVERIFY2(
            DocumentSerializer::save(recoveryPath, recoveryDocument, &error),
            qPrintable(error));
        QVERIFY2(
            DocumentSerializer::save(requestedPath, requestedDocument, &error),
            qPrintable(error));
        QSettings settings;
        settings.setValue(recoveryKey, recoverySourcePath);
        settings.sync();

        bool discardClicked = false;
        bool failureDismissed = false;
        MainWindow window;
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);
        const QByteArray documentBefore = DocumentSerializer::toJson(
            canvas->documentWithPendingSelectionTransform());
        MainWindowTestAccess::failNextDocumentReplacementPreparation(window);
        QTimer::singleShot(0,
            &window,
            [&window, &discardClicked, &failureDismissed]()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (!dialog)
                {
                    return;
                }
                QPushButton *discardButton = dialog->findChild<QPushButton *>(
                    QStringLiteral("startupDiscardRecoveryButton"));
                if (!discardButton)
                {
                    dialog->reject();
                    return;
                }
                QTimer::singleShot(0,
                    &window,
                    [&failureDismissed]()
                    {
                        QDialog *failureDialog = qobject_cast<QDialog *>(
                            QApplication::activeModalWidget());
                        if (!failureDialog)
                        {
                            return;
                        }
                        failureDismissed = true;
                        failureDialog->accept();
                    });
                discardClicked = true;
                discardButton->click();
            });
        QTimer::singleShot(1000,
            &window,
            []()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (dialog)
                {
                    dialog->reject();
                }
            });

        const MainWindow::StartupResult result =
            window.initializeSession(requestedPath);

        QVERIFY(discardClicked);
        QVERIFY(failureDismissed);
        QCOMPARE(result, MainWindow::StartupResult::Failed);
        QVERIFY(QFileInfo::exists(recoveryPath));
        QCOMPARE(settings.value(recoveryKey).toString(), recoverySourcePath);
        QVERIFY(window.windowFilePath().isEmpty());
        QCOMPARE(DocumentSerializer::toJson(
                     canvas->documentWithPendingSelectionTransform()),
            documentBefore);
        const std::optional<Document> retainedRecovery =
            DocumentSerializer::load(recoveryPath, &error);
        QVERIFY2(retainedRecovery.has_value(), qPrintable(error));
        QCOMPARE(DocumentSerializer::toJson(*retainedRecovery),
            DocumentSerializer::toJson(recoveryDocument));
    }

    void keepsUnresolvedRecoveryWhenWindowCloses()
    {
        const QString recoveryKey = QStringLiteral("recovery/sourcePath");
        SettingValueGuard recoveryValueGuard(recoveryKey);
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        const QString recoverySourcePath =
            directory.filePath(QStringLiteral("original-project.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(211, 157));
        QString error;
        QVERIFY2(
            DocumentSerializer::save(recoveryPath, recoveryDocument, &error),
            qPrintable(error));
        QSettings settings;
        settings.setValue(recoveryKey, recoverySourcePath);
        settings.sync();

        MainWindow window;
        QVERIFY(window.close());

        QVERIFY(QFileInfo::exists(recoveryPath));
        QCOMPARE(settings.value(recoveryKey).toString(), recoverySourcePath);
        const std::optional<Document> retainedRecovery =
            DocumentSerializer::load(recoveryPath, &error);
        QVERIFY2(retainedRecovery.has_value(), qPrintable(error));
        QCOMPARE(DocumentSerializer::toJson(*retainedRecovery),
            DocumentSerializer::toJson(recoveryDocument));
    }

    void keepsRecoveryWhenRequestedStartupFileIsInvalid()
    {
        const QString recoveryKey = QStringLiteral("recovery/sourcePath");
        SettingValueGuard recoveryValueGuard(recoveryKey);
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        const QString recoveryPath =
            directory.filePath(QStringLiteral("recovery.ugu"));
        const QString requestedPath =
            directory.filePath(QStringLiteral("invalid-request.ugu"));
        const QString recoverySourcePath =
            directory.filePath(QStringLiteral("original-project.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        Document recoveryDocument = Document::createDefault(QSize(197, 149));
        recoveryDocument.layers.first().name = QStringLiteral("Recovered");
        QString error;
        QVERIFY2(
            DocumentSerializer::save(recoveryPath, recoveryDocument, &error),
            qPrintable(error));
        QFile invalidRequestedFile(requestedPath);
        QVERIFY(invalidRequestedFile.open(QIODevice::WriteOnly));
        const QByteArray invalidData = QByteArrayLiteral("invalid project");
        QCOMPARE(invalidRequestedFile.write(invalidData), invalidData.size());
        invalidRequestedFile.close();

        QSettings settings;
        settings.setValue(recoveryKey, recoverySourcePath);
        settings.sync();

        bool discardClicked = false;
        bool failureDismissed = false;
        MainWindow window;
        QTimer::singleShot(0,
            &window,
            [&window, &discardClicked, &failureDismissed]()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (!dialog)
                {
                    return;
                }
                QPushButton *discardButton = dialog->findChild<QPushButton *>(
                    QStringLiteral("startupDiscardRecoveryButton"));
                if (!discardButton)
                {
                    dialog->reject();
                    return;
                }
                QTimer::singleShot(0,
                    &window,
                    [&failureDismissed]()
                    {
                        QDialog *failureDialog = qobject_cast<QDialog *>(
                            QApplication::activeModalWidget());
                        if (!failureDialog)
                        {
                            return;
                        }
                        failureDismissed = true;
                        failureDialog->accept();
                    });
                discardClicked = true;
                discardButton->click();
            });
        QTimer::singleShot(1000,
            &window,
            []()
            {
                QDialog *dialog =
                    qobject_cast<QDialog *>(QApplication::activeModalWidget());
                if (dialog)
                {
                    dialog->reject();
                }
            });

        const MainWindow::StartupResult result =
            window.initializeSession(requestedPath);

        QVERIFY(discardClicked);
        QVERIFY(failureDismissed);
        QCOMPARE(result, MainWindow::StartupResult::Failed);
        QVERIFY(QFileInfo::exists(recoveryPath));
        QCOMPARE(settings.value(recoveryKey).toString(), recoverySourcePath);
        QVERIFY(window.windowFilePath().isEmpty());

        const std::optional<Document> retainedRecovery =
            DocumentSerializer::load(recoveryPath, &error);
        QVERIFY2(retainedRecovery.has_value(), qPrintable(error));
        QCOMPARE(DocumentSerializer::toJson(*retainedRecovery),
            DocumentSerializer::toJson(recoveryDocument));
    }

    void autosavesModifiedWork()
    {
        const QString recoveryKey = QStringLiteral("recovery/sourcePath");
        SettingValueGuard recoveryGuard(recoveryKey);
        EnvironmentVariableGuard environmentGuard(
            QByteArrayLiteral("UGURUGU_RECOVERY_PATH"));
        QTemporaryDir recoveryDirectory;
        QVERIFY(recoveryDirectory.isValid());
        const QString recoveryPath =
            recoveryDirectory.filePath(QStringLiteral("recovery.ugu"));
        qputenv("UGURUGU_RECOVERY_PATH", recoveryPath.toUtf8());

        MainWindow window;
        window.resize(1000, 680);
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));
        CanvasWidget *canvas = window.findChild<CanvasWidget *>();
        QVERIFY(canvas);

        const QPoint center = canvas->rect().center();
        QTest::mousePress(
            canvas, Qt::LeftButton, Qt::NoModifier, center - QPoint(40, 0));
        QTest::mouseMove(canvas, center + QPoint(40, 0), 5);
        QTest::mouseRelease(
            canvas, Qt::LeftButton, Qt::NoModifier, center + QPoint(40, 0));
        QTRY_VERIFY(window.isWindowModified());

        QEvent deactivate(QEvent::ApplicationDeactivate);
        QApplication::sendEvent(qApp, &deactivate);
        QVERIFY(MainWindowTestAccess::flushAutosave(window));
        QVERIFY(QFileInfo::exists(recoveryPath));

        QString error;
        const std::optional<RecoveryStore::Snapshot> recovered =
            RecoveryStore::load(&error);
        QVERIFY2(recovered.has_value(), qPrintable(error));
        QCOMPARE(
            recovered->metadataStatus, RecoveryStore::MetadataStatus::Valid);
        QVERIFY(recovered->metadata.has_value());
        QVERIFY(recovered->metadata->sourcePath.isEmpty());
        QCOMPARE(recovered->metadata->revision, quint64(1));
        QCOMPARE(recovered->document.layers.first().strokes.size(), 1);
        QFile::remove(recoveryPath);
    }
};

int runUiSessionTests(int argc, char **argv)
{
    UiSessionTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "UiSessionTests.moc"
