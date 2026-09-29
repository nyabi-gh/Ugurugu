// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Prints the same per-frame pixel digests and serialization digest as
// tools/wasm_engine_smoke.mjs so native and WebAssembly renders of one
// document can be diffed line by line.

#include "io/DocumentSerializer.hpp"
#include "render/RenderEngine.hpp"

#include <QByteArray>
#include <QCryptographicHash>
#include <QFile>
#include <QImage>
#include <QString>

#include <cstdio>

namespace
{

QByteArray frameDigest(const QImage &frame)
{
    QCryptographicHash hash(QCryptographicHash::Sha256);
    const int rowBytes = frame.width() * 4;
    for (int row = 0; row < frame.height(); ++row)
    {
        hash.addData(QByteArrayView(
            reinterpret_cast<const char *>(frame.constScanLine(row)),
            rowBytes));
    }
    return hash.result().toHex();
}

}

int main(int argc, char **argv)
{
    if (argc != 2)
    {
        std::fprintf(stderr, "usage: %s <document.ugu>\n", argv[0]);
        return 2;
    }

    QFile file(QString::fromLocal8Bit(argv[1]));
    if (!file.open(QIODevice::ReadOnly))
    {
        std::fprintf(stderr, "cannot read %s\n", argv[1]);
        return 1;
    }
    const QByteArray bytes = file.readAll();

    QString error;
    const auto document = ugurugu::DocumentSerializer::fromJson(bytes, &error);
    if (!document)
    {
        std::fprintf(stderr, "open failed: %s\n", qPrintable(error));
        return 1;
    }

    std::printf("fixture: %lld bytes\n", static_cast<long long>(bytes.size()));
    std::printf("document: %dx%d, %d frames, %lld layers\n",
        document->size.width(),
        document->size.height(),
        document->animationFrames,
        static_cast<long long>(document->layers.size()));

    const int frameCount = document->animationFrames;
    const int sampledFrames[] = {0, frameCount / 2, frameCount - 1};
    for (const int frame : sampledFrames)
    {
        const QImage rendered = ugurugu::RenderEngine::render(*document, frame);
        std::printf("frame %d: %dx%d sha256=%s\n",
            frame,
            rendered.width(),
            rendered.height(),
            frameDigest(rendered).constData());
    }

    const QByteArray serialized =
        ugurugu::DocumentSerializer::toJson(*document);
    const QByteArray serializedDigest =
        QCryptographicHash::hash(serialized, QCryptographicHash::Sha256)
            .toHex();
    std::printf("serialized: %lld bytes sha256=%s\n",
        static_cast<long long>(serialized.size()),
        serializedDigest.constData());
    return 0;
}
