// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Times 2.2.13's animated GIF or WebP export of a document without the
// window, as ExportWorker::writeAnimation does it (exact frames scaled to the
// size chosen, then GifWriter or WebPWriter), and prints the peak working
// set. The output's extension picks the format. Compared with `ugu-doc gif`
// and `ugu-doc webp` (docs/rust/m5-plan.md M5-8 and M5-9).
//
// Usage: ugurugu_animation_export_probe <document.ugu> <out.gif|out.webp>
//            <percent>

#include "io/AnimationExportPolicy.hpp"
#include "io/DocumentSerializer.hpp"
#include "io/GifWriter.hpp"
#include "io/WebPWriter.hpp"
#include "render/RenderEngine.hpp"

#include <QElapsedTimer>
#include <QFile>
#include <QImage>
#include <QString>

#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <algorithm>
#include <cstdio>
#include <windows.h>
// psapi.h needs windows.h before it.
#include <psapi.h>

namespace
{

double peakWorkingSetMib()
{
    PROCESS_MEMORY_COUNTERS counters{};
    counters.cb = sizeof(counters);
    GetProcessMemoryInfo(GetCurrentProcess(), &counters, sizeof(counters));
    return static_cast<double>(counters.PeakWorkingSetSize) / (1024.0 * 1024.0);
}

}

int main(int argc, char **argv)
{
    if (argc != 4)
    {
        std::fprintf(stderr,
            "usage: ugurugu_animation_export_probe <document.ugu> "
            "<out.gif|out.webp> <percent>\n");
        return 2;
    }
    QFile file(QString::fromLocal8Bit(argv[1]));
    if (!file.open(QIODevice::ReadOnly))
    {
        std::fprintf(stderr, "cannot read %s\n", argv[1]);
        return 1;
    }
    QString error;
    auto loaded = ugurugu::DocumentSerializer::fromJson(file.readAll(), &error);
    if (!loaded)
    {
        std::fprintf(stderr, "open failed: %s\n", qPrintable(error));
        return 1;
    }
    const ugurugu::Document document = *loaded;
    const int percent = std::atoi(argv[3]);
    const QSize outputSize =
        percent >= 100
            ? document.size
            : QSize(std::max(1, document.size.width() * percent / 100),
                  std::max(1, document.size.height() * percent / 100));
    const bool nativeSize = outputSize == document.size;
    const int frameCount = document.animationFrames;
    std::printf(
        "%s: %d frames of %dx%d at %d%% = %dx%d, fits 2.2.13's budget: %s\n",
        argv[1],
        frameCount,
        document.size.width(),
        document.size.height(),
        percent,
        outputSize.width(),
        outputSize.height(),
        ugurugu::AnimationExportPolicy::fitsMemoryBudget(document, outputSize)
            ? "yes"
            : "no");
    QElapsedTimer timer;
    timer.start();
    qint64 drawing = 0;
    const auto renderFrame = [&](int frame) -> QImage
    {
        QElapsedTimer frameTimer;
        frameTimer.start();
        QImage image =
            nativeSize
                ? ugurugu::RenderEngine::render(document, frame)
                : ugurugu::RenderEngine::renderScaled(document,
                      frame,
                      outputSize,
                      ugurugu::RenderEngine::ScaledRenderMode::NativeExact);
        drawing += frameTimer.elapsed();
        return image;
    };
    const QString path = QString::fromLocal8Bit(argv[2]);
    const bool written =
        path.endsWith(QStringLiteral(".webp"))
            ? ugurugu::WebPWriter::write(path,
                  frameCount,
                  renderFrame,
                  ugurugu::AnimationExportPolicy::frameDurations(
                      frameCount, document.framesPerSecond, 1000),
                  &error)
            : ugurugu::GifWriter::write(path,
                  frameCount,
                  renderFrame,
                  ugurugu::AnimationExportPolicy::frameDurations(
                      frameCount, document.framesPerSecond, 100),
                  &error);
    if (!written)
    {
        std::fprintf(stderr, "export failed: %s\n", qPrintable(error));
        return 1;
    }
    std::printf(
        "  drawing %.2fs, total %.2fs, %.1fMB, peak working set %.0f MiB\n",
        drawing / 1000.0,
        timer.elapsed() / 1000.0,
        QFile(QString::fromLocal8Bit(argv[2])).size() / 1e6,
        peakWorkingSetMib());
    return 0;
}
