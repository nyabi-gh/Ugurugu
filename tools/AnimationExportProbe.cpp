// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Times 2.2.13's animated GIF or WebP export of a document without the
// window, as ExportWorker::writeAnimation does it (exact frames scaled to the
// size chosen, then GifWriter or WebPWriter), and prints the peak working
// set. The output's extension picks the format. Compared with `ugu-doc gif`
// and `ugu-doc webp` (docs/rust/decisions.md M5-8 and M5-9).
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

#include <algorithm>
#include <cmath>
#include <cstdio>
#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
// psapi.h needs windows.h before it.
#include <psapi.h>
#endif

namespace
{

// Not a number where the working set cannot be read.
double peakWorkingSetMib()
{
#ifdef _WIN32
    PROCESS_MEMORY_COUNTERS counters{};
    counters.cb = sizeof(counters);
    GetProcessMemoryInfo(GetCurrentProcess(), &counters, sizeof(counters));
    return static_cast<double>(counters.PeakWorkingSetSize) / (1024.0 * 1024.0);
#else
    return std::nan("");
#endif
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
    const ugurugu::Document &document = *loaded;
    bool percentValid = false;
    const int percent = QString::fromLocal8Bit(argv[3]).toInt(&percentValid);
    if (!percentValid || percent <= 0)
    {
        std::fprintf(stderr, "invalid percent %s\n", argv[3]);
        return 2;
    }
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
        static_cast<double>(drawing) / 1000.0,
        static_cast<double>(timer.elapsed()) / 1000.0,
        static_cast<double>(QFile(QString::fromLocal8Bit(argv[2])).size())
            / 1e6,
        peakWorkingSetMib());
    return 0;
}
