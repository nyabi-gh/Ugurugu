// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include <QColor>
#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#ifdef Q_OS_WIN
#include <QGuiApplication>
#include <QLibraryInfo>
#endif
#include <QImage>
#include <QImageReader>
#include <QImageWriter>
#include <QTemporaryDir>

#include <cstdio>

namespace
{

int fail(const QString &message)
{
    std::fprintf(stderr, "%s\n", message.toLocal8Bit().constData());
    return 1;
}

}

int main(int argc, char **argv)
{
#ifdef Q_OS_WIN
    QGuiApplication application(argc, argv);
#else
    QCoreApplication application(argc, argv);
#endif
    if (application.arguments().size() != 2)
    {
        return fail(
            QStringLiteral("Usage: ugurugu_package_smoke <installed package>"));
    }

    const QDir bundle(application.arguments().at(1));
#ifdef Q_OS_WIN
    const QString pluginRoot = bundle.absolutePath();
    const QString jpegPlugin =
        bundle.filePath(QStringLiteral("imageformats/qjpeg.dll"));
    const QString platformPlugin =
        bundle.filePath(QStringLiteral("platforms/qwindows.dll"));
    const QString resourceRoot = bundle.absolutePath();
    const QString updaterLicenseFile =
        bundle.filePath(QStringLiteral("Velopack-LICENSE.txt"));
    const QString qtConfiguration = bundle.filePath(QStringLiteral("qt.conf"));
#else
    const QString pluginRoot =
        bundle.filePath(QStringLiteral("Contents/PlugIns"));
    const QString jpegPlugin =
        QDir(pluginRoot)
            .filePath(QStringLiteral("imageformats/libqjpeg.dylib"));
    const QString unexpectedOffscreenPlugin =
        QDir(pluginRoot)
            .filePath(QStringLiteral("platforms/libqoffscreen.dylib"));
    const QString resourceRoot =
        bundle.filePath(QStringLiteral("Contents/Resources"));
    const QString updaterLicenseFile =
        QDir(resourceRoot).filePath(QStringLiteral("Sparkle-LICENSE.txt"));
#endif
    const QString licenseFile =
        QDir(resourceRoot).filePath(QStringLiteral("LICENSE"));
    const QString readmeFile =
        QDir(resourceRoot).filePath(QStringLiteral("README.md"));
    const QString noticesFile =
        QDir(resourceRoot).filePath(QStringLiteral("THIRD_PARTY_NOTICES.md"));
    const QString fontLicenseFile =
        QDir(resourceRoot).filePath(QStringLiteral("Pretendard-OFL.txt"));
    const QString loggingLicenseFile =
        QDir(resourceRoot).filePath(QStringLiteral("spdlog-LICENSE.txt"));
    const QString compressionLicenseFile =
        QDir(resourceRoot).filePath(QStringLiteral("zlib-LICENSE.txt"));

    if (!QFileInfo(jpegPlugin).isFile())
    {
        return fail(QStringLiteral("The installed application does not "
                                   "contain the JPEG image plugin."));
    }
#ifdef Q_OS_WIN
    if (!QFileInfo(platformPlugin).isFile())
    {
        return fail(QStringLiteral(
            "The installed application does not contain qwindows.dll."));
    }
    if (!QFileInfo(qtConfiguration).isFile())
    {
        return fail(QStringLiteral(
            "The installed application does not contain qt.conf."));
    }
    const QString configuredPluginRoot =
        QDir::cleanPath(QFileInfo(QLibraryInfo::path(QLibraryInfo::PluginsPath))
                .absoluteFilePath());
    const QString expectedPluginRoot =
        QDir::cleanPath(QFileInfo(pluginRoot).absoluteFilePath());
    if (configuredPluginRoot.compare(expectedPluginRoot, Qt::CaseInsensitive)
        != 0)
    {
        return fail(QStringLiteral("qt.conf resolved the plugin path outside "
                                   "the installed application: %1")
                .arg(configuredPluginRoot));
    }
    for (const QString &libraryPath : QCoreApplication::libraryPaths())
    {
        const QString resolvedLibraryPath =
            QDir::cleanPath(QFileInfo(libraryPath).absoluteFilePath());
        if (resolvedLibraryPath.compare(expectedPluginRoot, Qt::CaseInsensitive)
            != 0)
        {
            return fail(QStringLiteral("Qt retained an external plugin search "
                                       "path: %1")
                    .arg(resolvedLibraryPath));
        }
    }
#else
    if (QFileInfo::exists(unexpectedOffscreenPlugin))
    {
        return fail(
            QStringLiteral("The production application unexpectedly contains "
                           "the offscreen platform plugin."));
    }
#endif
    if (!QFileInfo(licenseFile).isFile() || !QFileInfo(readmeFile).isFile()
        || !QFileInfo(noticesFile).isFile()
        || !QFileInfo(fontLicenseFile).isFile()
        || !QFileInfo(loggingLicenseFile).isFile()
        || !QFileInfo(compressionLicenseFile).isFile()
        || !QFileInfo(updaterLicenseFile).isFile())
    {
        return fail(QStringLiteral(
            "The installed application does not contain all required "
            "license and copyright notices."));
    }

    QCoreApplication::setLibraryPaths({pluginRoot});
    const QList<QByteArray> formats = QImageWriter::supportedImageFormats();
    if (!formats.contains(QByteArrayLiteral("jpeg"))
        && !formats.contains(QByteArrayLiteral("jpg")))
    {
        return fail(QStringLiteral(
            "The installed application plugin set does not support JPEG."));
    }

    QTemporaryDir outputDirectory;
    if (!outputDirectory.isValid())
    {
        return fail(
            QStringLiteral("Could not create a temporary output directory."));
    }

    QImage source(8, 8, QImage::Format_RGB32);
    source.fill(QColor(0x31, 0x82, 0xce));
    const QString outputPath =
        outputDirectory.filePath(QStringLiteral("package-smoke.jpg"));
    QImageWriter writer(outputPath, QByteArrayLiteral("JPEG"));
    writer.setQuality(90);
    if (!writer.write(source))
    {
        return fail(
            QStringLiteral("JPEG write failed: %1").arg(writer.errorString()));
    }
    if (QFileInfo(outputPath).size() <= 0)
    {
        return fail(QStringLiteral("JPEG output file is empty."));
    }

    QImageReader reader(outputPath, QByteArrayLiteral("JPEG"));
    const QImage decoded = reader.read();
    if (decoded.isNull())
    {
        return fail(QStringLiteral("JPEG read-back failed: %1")
                .arg(reader.errorString()));
    }
    if (decoded.size() != source.size())
    {
        return fail(QStringLiteral(
            "JPEG read-back dimensions do not match the source image."));
    }

    return 0;
}
