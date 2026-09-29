// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "app/MemoryBudget.hpp"
#include "document/Document.hpp"

#include <QCoreApplication>
#include <QJsonObject>

#include <memory>
#include <optional>

class QIODevice;

namespace ugurugu
{

class DocumentSerializerTestAccess;

class DocumentSerializer
{
    Q_DECLARE_TR_FUNCTIONS(ugurugu::DocumentSerializer)

public:
    // Process-local, thread-confined cache for compressed mask payloads.
    // PreparedDocument instances never pin its bounded LRU entries.
    class SerializationCache final
    {
    public:
        static constexpr qint64 maximumPayloadBytes =
            MemoryBudget::serializationCacheBytes;

        struct Stats
        {
            quint64 clipMaskContentHashes = 0;
            quint64 clipMaskCompressions = 0;
            quint64 binaryMaskContentHashes = 0;
            quint64 binaryMaskCompressions = 0;
            quint64 strokeSerializations = 0;
            quint64 payloadCacheHits = 0;
            quint64 payloadCacheMisses = 0;
            quint64 fullDocumentPreparations = 0;
            quint64 incrementalStrokeAppends = 0;
        };

        explicit SerializationCache(
            qint64 payloadCapacityBytes = maximumPayloadBytes);
        ~SerializationCache();
        SerializationCache(SerializationCache &&) noexcept;
        SerializationCache &operator=(SerializationCache &&) noexcept;

        SerializationCache(const SerializationCache &) = delete;
        SerializationCache &operator=(const SerializationCache &) = delete;

        void clear();
        void resetStats();
        Stats stats() const;
        qint64 payloadBytes() const;
        qint64 payloadCapacityBytes() const;

    private:
        struct Impl;
        std::unique_ptr<Impl> m_impl;

        friend class DocumentSerializer;
    };

    // Immutable normalized document paired with an exact Compact JSON
    // byte-size plan. Only prepared documents should enter editable state.
    class PreparedDocument final
    {
    public:
        PreparedDocument();
        ~PreparedDocument();
        PreparedDocument(const PreparedDocument &);
        PreparedDocument &operator=(const PreparedDocument &);
        PreparedDocument(PreparedDocument &&) noexcept;
        PreparedDocument &operator=(PreparedDocument &&) noexcept;

        bool isValid() const;
        const Document &document() const;
        qint64 compactSize() const;
        qsizetype totalStrokeCount() const;
        qsizetype totalPointCount() const;

    private:
        struct Impl;
        std::shared_ptr<const Impl> m_impl;

        explicit PreparedDocument(std::shared_ptr<const Impl> impl);

        friend class DocumentSerializer;
    };

    enum class AppendStrokeStatus
    {
        Appended,
        NotApplicable,
        Invalid,
        StrokeLimit,
        PointLimit,
        MaskLimit,
        TooLarge
    };

    struct AppendStrokeResult
    {
        AppendStrokeStatus status = AppendStrokeStatus::NotApplicable;
        PreparedDocument prepared;
    };

    // A capability issued only after matching payload backings against an
    // immutable PreparedDocument. History replay may use the retained
    // backings without copying them again; arbitrary caller-owned aliases can
    // never manufacture this capability.
    class ImmutableBackingLease final
    {
    public:
        ImmutableBackingLease();
        ~ImmutableBackingLease();
        ImmutableBackingLease(const ImmutableBackingLease &);
        ImmutableBackingLease &operator=(const ImmutableBackingLease &);
        ImmutableBackingLease(ImmutableBackingLease &&) noexcept;
        ImmutableBackingLease &operator=(ImmutableBackingLease &&) noexcept;

        bool isValid() const;

    private:
        struct Impl;
        std::shared_ptr<const Impl> m_impl;

        explicit ImmutableBackingLease(std::shared_ptr<const Impl> impl);

        friend class DocumentSerializer;
    };

    static std::optional<PreparedDocument> prepare(
        Document document, SerializationCache &cache, QString *error = nullptr);
    static std::optional<PreparedDocument> prepare(Document document,
        SerializationCache &cache,
        const PreparedDocument *base,
        qint64 maximumBytes,
        QString *error = nullptr);
    static std::optional<PreparedDocument> prepare(Document document,
        SerializationCache &cache,
        const PreparedDocument *base,
        const ImmutableBackingLease *trusted,
        qint64 maximumBytes,
        QString *error = nullptr);
    static AppendStrokeResult appendStroke(const PreparedDocument &base,
        const QUuid &layerId,
        const Stroke &stroke,
        SerializationCache &cache,
        qint64 maximumBytes);
    static ImmutableBackingLease retainImmutableBackings(
        const PreparedDocument &source, const QVector<Stroke> &strokes);
    static std::optional<PreparedDocument> rebindActiveLayer(
        const PreparedDocument &prepared, const QUuid &activeLayerId);

    static bool save(const QString &filePath,
        const Document &document,
        QString *error = nullptr);
    static bool save(const QString &filePath,
        const PreparedDocument &document,
        SerializationCache &cache,
        QString *error = nullptr);
    static std::optional<Document> load(
        const QString &filePath, QString *error = nullptr);
    static QByteArray toJson(const Document &document);
    static QByteArray toJson(
        const Document &document, const QJsonObject &additionalRootFields);
    static QByteArray toJson(
        const PreparedDocument &document, SerializationCache &cache);
    static QByteArray toJson(const PreparedDocument &document,
        SerializationCache &cache,
        const QJsonObject &additionalRootFields);
    static std::optional<Document> fromJson(
        const QByteArray &data, QString *error = nullptr);
    static std::optional<Document> fromJson(
        const QByteArray &data, QJsonObject *root, QString *error);

private:
    static std::optional<Document> loadFromDevice(
        QIODevice &device, qint64 maximumBytes, QString *error);

    friend class DocumentSerializerTestAccess;
};

}
