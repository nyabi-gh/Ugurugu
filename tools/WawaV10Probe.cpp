// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/DocumentSerializer.hpp"
#include "io/WawaV10Importer.hpp"
#include "render/RenderEngine.hpp"

#include <QCoreApplication>
#include <QFile>
#include <QTextStream>

int main(int argc, char **argv)
{
    QCoreApplication application(argc, argv);
    if (application.arguments().size() != 2)
    {
        return 2;
    }
    QFile file(application.arguments()[1]);
    if (!file.open(QIODevice::ReadOnly))
    {
        return 3;
    }
    QString error;
    const std::optional<ugurugu::WawaImportResult> imported =
        ugurugu::WawaV10Importer::import(file.readAll(), &error);
    QTextStream output(stdout);
    if (!imported)
    {
        output << error << '\n';
        return 1;
    }
    const QByteArray serialized =
        ugurugu::DocumentSerializer::toJson(imported->document);
    const std::optional<ugurugu::Document> reloaded =
        ugurugu::DocumentSerializer::fromJson(serialized, &error);
    if (serialized.isEmpty() || !reloaded)
    {
        output << (error.isEmpty() ? QStringLiteral("Serialization failed")
                                   : error)
               << '\n';
        return 1;
    }
    for (int frame = 0; frame < imported->document.animationFrames; ++frame)
    {
        const QImage importedFrame =
            ugurugu::RenderEngine::render(imported->document, frame);
        const QImage reloadedFrame =
            ugurugu::RenderEngine::render(*reloaded, frame);
        if (importedFrame.isNull() || importedFrame != reloadedFrame)
        {
            output << "Render verification failed at frame " << frame << '\n';
            return 1;
        }
    }
    const ugurugu::WawaImportSummary &summary = imported->summary;
    output << "canvas=" << imported->document.size.width() << 'x'
           << imported->document.size.height() << '\n';
    output << "layers=" << summary.layers << '\n';
    output << "baseImages=" << summary.baseImages << '\n';
    output << "paintStrokes=" << summary.paintStrokes << '\n';
    output << "eraserStrokes=" << summary.eraserStrokes << '\n';
    output << "polygonFills=" << summary.polygonFills << '\n';
    output << "skippedOperations=" << summary.skippedOperations << '\n';
    output << "serializedBytes=" << serialized.size() << '\n';
    output << "verifiedFrames=" << imported->document.animationFrames << '\n';
    return 0;
}
