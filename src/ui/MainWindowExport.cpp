// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/AnimationExportPolicy.hpp"
#include "io/RenderExportPolicy.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/GifExportDialog.hpp"
#include "ui/MainWindow.hpp"

#include <QFileDialog>
#include <QFileInfo>
#include <QMessageBox>
#include <QProgressDialog>
#include <QStatusBar>

#include <spdlog/spdlog.h>

#include <algorithm>

namespace ugurugu
{

void MainWindow::exportGif()
{
    exportAnimation(ExportWorker::Kind::Gif);
}

void MainWindow::exportWebP()
{
    exportAnimation(ExportWorker::Kind::WebP);
}

void MainWindow::exportAnimation(ExportWorker::Kind kind)
{
    if (m_exportWorker.isBusy())
    {
        return;
    }
    const Document document = m_canvas->documentWithPendingSelectionTransform();
    if (document.size.width() <= 0 || document.size.height() <= 0
        || document.animationFrames <= 0)
    {
        return;
    }
    // The smallest offered scale is the only thing that can still be over
    // budget; anything above that is the user's choice inside the dialog.
    if (!AnimationExportPolicy::fitsMemoryBudget(document,
            QSize(std::max(1, document.size.width() / 4),
                std::max(1, document.size.height() / 4))))
    {
        const long double mebibytes =
            AnimationExportPolicy::estimatedWorkingBytes(document)
            / (1024.0L * 1024.0L);
        QMessageBox::warning(this,
            tr("Animation is too large"),
            tr("This animation would need about %1 MiB of working memory even "
               "when "
               "scaled down. Reduce the frame count before exporting.")
                .arg(static_cast<double>(mebibytes), 0, 'f', 0));
        return;
    }

    const bool webP = kind == ExportWorker::Kind::WebP;
    const QString exportTitle =
        webP ? tr("Export animated WebP") : tr("Export animated GIF");
    GifExportDialog optionsDialog(document, exportTitle, this);
    if (optionsDialog.exec() != QDialog::Accepted)
    {
        return;
    }
    const GifExportDialog::Result options = optionsDialog.currentResult();

    const QString extension =
        webP ? QStringLiteral("webp") : QStringLiteral("gif");
    const QString filter =
        webP ? tr("WebP images (*.webp)") : tr("GIF images (*.gif)");
    const QString selected = QFileDialog::getSaveFileName(
        this, exportTitle, saveDialogStartPath(extension), filter);
    if (selected.isEmpty())
    {
        return;
    }
    const QString filePath = normalizedPath(selected, extension);
    m_canvas->releaseTransientRenderCaches();
    m_controller.releaseTransientCaches();
    const ExportWorker::AnimationOptions workerOptions{
        options.outputSize, options.preserveTransparency};
    const bool started =
        webP ? m_exportWorker.startWebP(document, filePath, workerOptions)
             : m_exportWorker.startGif(document, filePath, workerOptions);
    if (started)
    {
        beginExportProgress(kind, filePath, document.animationFrames);
    }
}

void MainWindow::exportImage()
{
    if (m_exportWorker.isBusy())
    {
        return;
    }
    const int frame = m_canvas->currentFrame();
    Document exportDocument =
        m_canvas->displayDocumentWithPendingSelectionTransform();
    const RenderExportMemoryEstimate memoryEstimate =
        RenderExportPolicy::staticImage(exportDocument);
    if (!RenderExportPolicy::staticImageFitsMemoryBudget(exportDocument))
    {
        spdlog::error("Static image export exceeds the working memory budget");
        const long double mebibytes =
            memoryEstimate.workingBytes / (1024.0L * 1024.0L);
        QMessageBox dialog(QMessageBox::Warning,
            tr("Image is too large"),
            tr("This image would need about %1 MiB of working memory. Reduce "
               "the canvas size or layer group nesting before exporting.")
                .arg(static_cast<double>(mebibytes), 0, 'f', 0),
            QMessageBox::Ok,
            this);
        dialog.setObjectName(QStringLiteral("staticExportMemoryWarning"));
        dialog.exec();
        return;
    }
    const QString pngFilter = tr("PNG images (*.png)");
    const QString jpegFilter = tr("JPEG images (*.jpg *.jpeg)");
    QString selectedFilter = pngFilter;
    const QString selected = QFileDialog::getSaveFileName(this,
        tr("Export current frame"),
        saveDialogStartPath(QStringLiteral("png")),
        pngFilter + QStringLiteral(";;") + jpegFilter,
        &selectedFilter);
    if (selected.isEmpty())
    {
        return;
    }
    const QString suffix = QFileInfo(selected).suffix().toLower();
    const bool jpeg = suffix == QStringLiteral("jpg")
                      || suffix == QStringLiteral("jpeg")
                      || (suffix.isEmpty() && selectedFilter == jpegFilter);
    const QString filePath =
        suffix == QStringLiteral("jpeg")
            ? selected
            : normalizedPath(selected,
                  jpeg ? QStringLiteral("jpg") : QStringLiteral("png"));
    m_canvas->releaseTransientRenderCaches();
    m_controller.releaseTransientCaches();
    if (m_exportWorker.startImage(
            std::move(exportDocument), frame, filePath, jpeg))
    {
        beginExportProgress(ExportWorker::Kind::Image, filePath, 0);
    }
}

void MainWindow::beginExportProgress(
    ExportWorker::Kind kind, const QString &filePath, int maximum)
{
    if (m_exportProgress)
    {
        m_exportProgress->deleteLater();
    }
    m_exportProgress = new QProgressDialog(kind != ExportWorker::Kind::Image
                                               ? tr("Rendering animation…")
                                               : tr("Rendering image…"),
        tr("Cancel"),
        0,
        maximum,
        this);
    m_exportProgress->setObjectName(QStringLiteral("exportProgressDialog"));
    m_exportProgress->setWindowTitle(tr("Export"));
    m_exportProgress->setWindowModality(Qt::WindowModal);
    m_exportProgress->setMinimumDuration(0);
    m_exportProgress->setAutoClose(false);
    m_exportProgress->setAutoReset(false);
    m_exportProgress->setProperty("exportFilePath", filePath);
    QProgressDialog *progress = m_exportProgress;
    connect(m_exportProgress,
        &QProgressDialog::canceled,
        &m_exportWorker,
        &ExportWorker::cancel);
    connect(m_exportProgress,
        &QObject::destroyed,
        this,
        [this, progress]()
        {
            if (m_exportProgress == progress)
            {
                m_exportProgress = nullptr;
            }
        });
    m_exportProgress->show();
    updateExportActions();
}

void MainWindow::handleExportFinished(ExportWorker::Kind kind,
    bool success,
    bool canceled,
    const QString &filePath,
    const QString &error)
{
    if (m_exportProgress)
    {
        m_exportProgress->deleteLater();
        m_exportProgress = nullptr;
    }
    updateExportActions();
    if (canceled)
    {
        statusBar()->showMessage(tr("Export canceled"), 3000);
        spdlog::info("Export canceled for {}", filePath.toUtf8().constData());
        return;
    }
    if (!success)
    {
        spdlog::error("Failed to export {}: {}",
            filePath.toUtf8().constData(),
            error.toUtf8().constData());
        QMessageBox::critical(this,
            tr("Export failed"),
            error.isEmpty()
                ? tr("Could not export the file.")
                : tr("Could not export the file.\n\n%1").arg(error));
        return;
    }
    statusBar()->showMessage(tr("Exported %1").arg(filePath), 5000);
    if (kind == ExportWorker::Kind::Gif)
    {
        spdlog::info("Exported GIF {}", filePath.toUtf8().constData());
    }
    else if (kind == ExportWorker::Kind::WebP)
    {
        spdlog::info("Exported WebP {}", filePath.toUtf8().constData());
    }
    else
    {
        spdlog::info("Exported image {}", filePath.toUtf8().constData());
    }
}

void MainWindow::updateExportActions()
{
    const bool idle = !m_exportWorker.isBusy();
    if (auto *exportGifAction =
            findChild<QAction *>(QStringLiteral("exportGifAction")))
    {
        exportGifAction->setEnabled(
            idle && m_canvas->isWobbleAnimationEnabled());
    }
    if (auto *exportWebPAction =
            findChild<QAction *>(QStringLiteral("exportWebPAction")))
    {
        exportWebPAction->setEnabled(
            idle && m_canvas->isWobbleAnimationEnabled());
    }
    if (auto *exportImageAction =
            findChild<QAction *>(QStringLiteral("exportPngAction")))
    {
        exportImageAction->setEnabled(idle);
    }
}
}
