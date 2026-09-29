// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QDateTime>
#include <QJsonObject>
#include <QString>
#include <QUuid>

#include <optional>

namespace ugurugu
{

class DocumentController;

class RecoveryStore final
{
public:
    enum class MetadataStatus
    {
        Missing,
        Valid,
        Invalid
    };

    struct Metadata
    {
        QUuid sessionId;
        QString sourcePath;
        quint64 revision = 0;
        QDateTime timestampUtc;
    };

    struct Snapshot
    {
        Document document;
        std::optional<Metadata> metadata;
        MetadataStatus metadataStatus = MetadataStatus::Missing;
    };

    static QString filePath();
    static bool isRecoveryPath(const QString &candidatePath);
    static bool ensureParentDirectory(QString *error = nullptr);
    static bool isValidMetadata(const Metadata &metadata);
    static QJsonObject metadataRootFields(const Metadata &metadata);
    static bool save(const Document &document,
        const Metadata &metadata,
        QString *error = nullptr);
    static bool save(DocumentController &controller,
        const Metadata &metadata,
        QString *error = nullptr);
    static std::optional<Snapshot> load(QString *error = nullptr);
    static bool discard(QString *error = nullptr);
    static QString preserve(QString *error = nullptr);

    // Moves an unreadable recovery aside so it can be inspected or recovered
    // manually. On failure, the original file is deliberately left intact.
    static QString quarantine(QString *error = nullptr);
};

}
