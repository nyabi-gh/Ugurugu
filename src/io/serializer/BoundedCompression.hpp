// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QByteArray>
#include <QtTypes>

#include <optional>

namespace ugurugu::serializer_detail
{

std::optional<QByteArray> uncompressQtPayload(
    const QByteArray &compressed, quint64 expectedBytes);

}
