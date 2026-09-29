// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/RecoveryStore.hpp"

#include "document/DocumentController.hpp"
#include "document/DocumentLimits.hpp"
#include "io/DocumentSerializer.hpp"

#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonObject>
#include <QSaveFile>
#include <QStandardPaths>
#include <QUuid>

#include <filesystem>

namespace ugurugu
{

namespace
{

void setError(QString *error, const QString &message)
{
    if (error)
    {
        *error = message;
    }
}

Qt::CaseSensitivity filePathCaseSensitivity()
{
#if defined(Q_OS_WIN) || defined(Q_OS_MACOS)
    return Qt::CaseInsensitive;
#else
    return Qt::CaseSensitive;
#endif
}

QString resolvedPath(const QFileInfo &file)
{
    const QString canonicalFile = file.canonicalFilePath();
    if (!canonicalFile.isEmpty())
    {
        return QDir::cleanPath(canonicalFile);
    }
    const QFileInfo parent(file.absolutePath());
    QString resolvedParent = parent.canonicalFilePath();
    if (resolvedParent.isEmpty())
    {
        resolvedParent = parent.absoluteFilePath();
    }
    return QDir::cleanPath(QDir(resolvedParent).filePath(file.fileName()));
}

QString uniqueSiblingPath(const QString &source, const QString &label)
{
    const QFileInfo sourceInfo(source);
    const QString stamp = QDateTime::currentDateTimeUtc().toString(
        QStringLiteral("yyyyMMdd-HHmmss-zzz"));
    const QString identifier =
        QUuid::createUuid().toString(QUuid::WithoutBraces);
    return sourceInfo.dir().filePath(
        QStringLiteral("recovery-%1-%2-%3.ugu").arg(label, stamp, identifier));
}

bool validMetadata(const RecoveryStore::Metadata &metadata)
{
    return !metadata.sessionId.isNull() && metadata.sourcePath.size() <= 32768
           && metadata.revision > 0 && metadata.timestampUtc.isValid();
}

QJsonObject metadataToJson(const RecoveryStore::Metadata &metadata)
{
    QJsonObject object;
    object.insert(QStringLiteral("formatVersion"), 1);
    object.insert(QStringLiteral("sessionId"),
        metadata.sessionId.toString(QUuid::WithoutBraces));
    object.insert(QStringLiteral("sourcePath"), metadata.sourcePath);
    object.insert(
        QStringLiteral("revision"), QString::number(metadata.revision));
    object.insert(QStringLiteral("timestampUtc"),
        metadata.timestampUtc.toUTC().toString(Qt::ISODateWithMs));
    return object;
}

QJsonObject recoveryRootFields(const RecoveryStore::Metadata &metadata)
{
    QJsonObject fields;
    fields.insert(QStringLiteral("uguruguRecovery"), metadataToJson(metadata));
    return fields;
}

bool writeRecoveryData(const QByteArray &data, QString *error)
{
    if (data.isEmpty())
    {
        setError(
            error, QStringLiteral("Could not serialize the recovery project."));
        return false;
    }
    if (data.size() > DocumentLimits::maximumProjectBytes)
    {
        setError(error, QStringLiteral("The recovery project is too large."));
        return false;
    }
    QSaveFile file(RecoveryStore::filePath());
    if (!file.open(QIODevice::WriteOnly))
    {
        setError(error, file.errorString());
        return false;
    }
    if (file.write(data) != data.size())
    {
        setError(error, file.errorString());
        file.cancelWriting();
        return false;
    }
    if (!file.commit())
    {
        setError(error, file.errorString());
        return false;
    }
    return true;
}

std::optional<RecoveryStore::Metadata> metadataFromJson(const QJsonValue &value)
{
    if (!value.isObject())
    {
        return std::nullopt;
    }
    const QJsonObject object = value.toObject();
    const QJsonValue sourcePathValue =
        object.value(QStringLiteral("sourcePath"));
    if (object.value(QStringLiteral("formatVersion")).toInt(-1) != 1
        || !object.value(QStringLiteral("sessionId")).isString()
        || (!sourcePathValue.isString() && !sourcePathValue.isNull())
        || !object.value(QStringLiteral("revision")).isString()
        || !object.value(QStringLiteral("timestampUtc")).isString())
    {
        return std::nullopt;
    }
    bool revisionValid = false;
    const quint64 revision = object.value(QStringLiteral("revision"))
                                 .toString()
                                 .toULongLong(&revisionValid);
    const bool canonicalRevision =
        QString::number(revision)
        == object.value(QStringLiteral("revision")).toString();
    const QString sessionIdText =
        object.value(QStringLiteral("sessionId")).toString();
    const QString timestampText =
        object.value(QStringLiteral("timestampUtc")).toString();
    RecoveryStore::Metadata metadata;
    metadata.sessionId = QUuid(sessionIdText);
    metadata.sourcePath =
        sourcePathValue.isNull() ? QString() : sourcePathValue.toString();
    metadata.revision = revision;
    metadata.timestampUtc =
        QDateTime::fromString(timestampText, Qt::ISODateWithMs).toUTC();
    const bool canonicalSessionId =
        metadata.sessionId.toString(QUuid::WithoutBraces) == sessionIdText;
    const bool canonicalTimestamp =
        timestampText.endsWith(QLatin1Char('Z'))
        && metadata.timestampUtc.toString(Qt::ISODateWithMs) == timestampText;
    return revisionValid && canonicalRevision && canonicalSessionId
                   && canonicalTimestamp && validMetadata(metadata)
               ? std::optional<RecoveryStore::Metadata>(std::move(metadata))
               : std::nullopt;
}

}

bool RecoveryStore::isValidMetadata(const Metadata &metadata)
{
    return validMetadata(metadata);
}

QJsonObject RecoveryStore::metadataRootFields(const Metadata &metadata)
{
    return recoveryRootFields(metadata);
}

QString RecoveryStore::filePath()
{
    const QString configured = qEnvironmentVariable("UGURUGU_RECOVERY_PATH");
    if (!configured.isEmpty())
    {
        return QFileInfo(configured).absoluteFilePath();
    }
    return QDir(
        QStandardPaths::writableLocation(QStandardPaths::AppLocalDataLocation))
        .filePath(QStringLiteral("recovery.ugu"));
}

bool RecoveryStore::isRecoveryPath(const QString &candidatePath)
{
    if (candidatePath.isEmpty())
    {
        return false;
    }
    const QFileInfo recovery(filePath());
    const QFileInfo candidate(candidatePath);
    if (recovery.exists() && candidate.exists())
    {
        std::error_code error;
        if (std::filesystem::equivalent(recovery.filesystemAbsoluteFilePath(),
                candidate.filesystemAbsoluteFilePath(),
                error))
        {
            return true;
        }
    }
    return resolvedPath(recovery).compare(
               resolvedPath(candidate), filePathCaseSensitivity())
           == 0;
}

bool RecoveryStore::ensureParentDirectory(QString *error)
{
    const QString directory = QFileInfo(filePath()).absolutePath();
    if (QDir().mkpath(directory))
    {
        return true;
    }
    setError(error,
        QStringLiteral("Could not create recovery directory: %1")
            .arg(directory));
    return false;
}

bool RecoveryStore::save(
    const Document &document, const Metadata &metadata, QString *error)
{
    if (!validMetadata(metadata))
    {
        setError(error, QStringLiteral("Recovery metadata is invalid."));
        return false;
    }
    if (!ensureParentDirectory(error))
    {
        return false;
    }
    QByteArray recoveryData =
        DocumentSerializer::toJson(document, recoveryRootFields(metadata));
    if (recoveryData.isEmpty())
    {
        recoveryData = DocumentSerializer::toJson(document);
    }
    return writeRecoveryData(recoveryData, error);
}

bool RecoveryStore::save(
    DocumentController &controller, const Metadata &metadata, QString *error)
{
    if (!validMetadata(metadata))
    {
        setError(error, QStringLiteral("Recovery metadata is invalid."));
        return false;
    }
    if (!ensureParentDirectory(error))
    {
        return false;
    }
    QByteArray data =
        controller.serializeDocument(recoveryRootFields(metadata), error);
    if (data.isEmpty() && (!error || error->isEmpty()))
    {
        data = controller.serializeDocument({}, error);
    }
    if (data.isEmpty())
    {
        if (error && error->isEmpty())
        {
            setError(error,
                QStringLiteral("Could not serialize the recovery project."));
        }
        return false;
    }
    return writeRecoveryData(data, error);
}

std::optional<RecoveryStore::Snapshot> RecoveryStore::load(QString *error)
{
    QFile file(filePath());
    if (!file.open(QIODevice::ReadOnly))
    {
        setError(error, file.errorString());
        return std::nullopt;
    }
    if (file.size() < 0 || file.size() > DocumentLimits::maximumProjectBytes)
    {
        setError(error, QStringLiteral("The recovery project is too large."));
        return std::nullopt;
    }
    const QByteArray data = file.read(DocumentLimits::maximumProjectBytes + 1);
    if (data.size() > DocumentLimits::maximumProjectBytes || !file.atEnd())
    {
        setError(error, QStringLiteral("The recovery project is too large."));
        return std::nullopt;
    }
    QJsonObject root;
    std::optional<Document> document =
        DocumentSerializer::fromJson(data, &root, error);
    if (!document)
    {
        return std::nullopt;
    }
    std::optional<Metadata> metadata;
    MetadataStatus metadataStatus = MetadataStatus::Missing;
    const QJsonValue metadataValue =
        root.value(QStringLiteral("uguruguRecovery"));
    if (!metadataValue.isUndefined())
    {
        metadata = metadataFromJson(metadataValue);
        metadataStatus =
            metadata ? MetadataStatus::Valid : MetadataStatus::Invalid;
    }
    return Snapshot{std::move(*document), std::move(metadata), metadataStatus};
}

bool RecoveryStore::discard(QString *error)
{
    const QString path = filePath();
    if (!QFileInfo::exists(path) || QFile::remove(path))
    {
        return true;
    }
    setError(
        error, QStringLiteral("Could not remove recovery file: %1").arg(path));
    return false;
}

QString RecoveryStore::preserve(QString *error)
{
    const QString source = filePath();
    if (!QFileInfo::exists(source))
    {
        setError(error,
            QStringLiteral("Recovery file does not exist: %1").arg(source));
        return {};
    }
    QString destination =
        uniqueSiblingPath(source, QStringLiteral("preserved"));
    if (QFile::rename(source, destination))
    {
        return destination;
    }
    setError(error,
        QStringLiteral("Could not preserve recovery file as: %1")
            .arg(destination));
    return {};
}

QString RecoveryStore::quarantine(QString *error)
{
    const QString source = filePath();
    if (!QFileInfo::exists(source))
    {
        setError(error,
            QStringLiteral("Recovery file does not exist: %1").arg(source));
        return {};
    }

    const QString stamp = QDateTime::currentDateTimeUtc().toString(
        QStringLiteral("yyyyMMdd-HHmmss"));
    const QString base = source + QStringLiteral(".failed-") + stamp;
    QString destination = base;
    for (int suffix = 2; QFileInfo::exists(destination); ++suffix)
    {
        destination = base + QStringLiteral("-%1").arg(suffix);
    }
    if (QFile::rename(source, destination))
    {
        return destination;
    }

    setError(error,
        QStringLiteral("Could not preserve recovery file as: %1")
            .arg(destination));
    return {};
}

}
