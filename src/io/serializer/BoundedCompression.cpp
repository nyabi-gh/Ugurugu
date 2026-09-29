// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/serializer/BoundedCompression.hpp"

#include <QtEndian>

#include <limits>
#include <zlib.h>

namespace ugurugu::serializer_detail
{

std::optional<QByteArray> uncompressQtPayload(
    const QByteArray &compressed, quint64 expectedBytes)
{
    if (compressed.size() < 4)
    {
        return std::nullopt;
    }
    const qsizetype compressedDataBytes = compressed.size() - 4;
    if (expectedBytes == 0
        || expectedBytes > std::numeric_limits<quint32>::max()
        || expectedBytes > std::numeric_limits<uInt>::max()
        || expectedBytes
               > static_cast<quint64>(std::numeric_limits<qsizetype>::max())
        || static_cast<quint64>(compressedDataBytes)
               > std::numeric_limits<uInt>::max())
    {
        return std::nullopt;
    }

    const auto *prefix =
        reinterpret_cast<const uchar *>(compressed.constData());
    if (static_cast<quint64>(qFromBigEndian<quint32>(prefix)) != expectedBytes)
    {
        return std::nullopt;
    }

    QByteArray output;
    output.resize(static_cast<qsizetype>(expectedBytes));
    z_stream stream{};
    stream.next_in = reinterpret_cast<Bytef *>(
        const_cast<char *>(compressed.constData() + 4));
    stream.avail_in = static_cast<uInt>(compressedDataBytes);
    stream.next_out = reinterpret_cast<Bytef *>(output.data());
    stream.avail_out = static_cast<uInt>(expectedBytes);
    if (inflateInit(&stream) != Z_OK)
    {
        return std::nullopt;
    }

    const int result = inflate(&stream, Z_FINISH);
    const bool valid =
        result == Z_STREAM_END
        && static_cast<quint64>(stream.total_out) == expectedBytes
        && stream.total_in == static_cast<uLong>(compressedDataBytes);
    inflateEnd(&stream);
    if (!valid)
    {
        return std::nullopt;
    }
    return output;
}

}
