// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/WebPWriter.hpp"

#include "io/AnimationExportPolicy.hpp"

#include <QFileDevice>
#include <QSaveFile>

#include <limits>
#include <memory>
#include <webp/encode.h>
#include <webp/mux.h>

namespace ugurugu
{

namespace
{

bool fail(QString *error, const QString &message)
{
    if (error)
    {
        *error = message;
    }
    return false;
}

struct EncoderDeleter
{
    void operator()(WebPAnimEncoder *encoder) const
    {
        WebPAnimEncoderDelete(encoder);
    }
};

using Encoder = std::unique_ptr<WebPAnimEncoder, EncoderDeleter>;

class Picture final
{
public:
    Picture()
    {
        m_valid = WebPPictureInit(&m_picture) != 0;
    }

    ~Picture()
    {
        WebPPictureFree(&m_picture);
    }

    Picture(const Picture &) = delete;
    Picture &operator=(const Picture &) = delete;

    bool isValid() const
    {
        return m_valid;
    }

    WebPPicture *get()
    {
        return &m_picture;
    }

private:
    WebPPicture m_picture{};
    bool m_valid = false;
};

class EncodedData final
{
public:
    EncodedData()
    {
        WebPDataInit(&m_data);
    }

    ~EncodedData()
    {
        WebPDataClear(&m_data);
    }

    EncodedData(const EncodedData &) = delete;
    EncodedData &operator=(const EncodedData &) = delete;

    WebPData *get()
    {
        return &m_data;
    }

    const WebPData &value() const
    {
        return m_data;
    }

private:
    WebPData m_data{};
};

QString encoderError(WebPAnimEncoder *encoder)
{
    const char *message = WebPAnimEncoderGetError(encoder);
    return message && *message != '\0'
               ? QString::fromUtf8(message)
               : WebPWriter::tr("The WebP encoder failed.");
}

}

bool WebPWriter::write(const QString &filePath,
    const QVector<QImage> &frames,
    const QVector<int> &durationsMilliseconds,
    QString *error,
    const std::function<bool()> &isCanceled)
{
    if (frames.isEmpty() || frames.size() != durationsMilliseconds.size())
    {
        return fail(error, tr("The animation frames or timings are invalid."));
    }
    const QSize frameSize = frames.first().size();
    if (frameSize.isEmpty()
        || !AnimationExportPolicy::fitsMemoryBudget(frameSize, frames.size()))
    {
        return fail(
            error, tr("The animation exceeds the export memory budget."));
    }

    int totalDuration = 0;
    for (qsizetype index = 0; index < frames.size(); ++index)
    {
        const int duration = durationsMilliseconds[index];
        if (frames[index].isNull() || frames[index].size() != frameSize
            || duration <= 0
            || duration > std::numeric_limits<int>::max() - totalDuration)
        {
            return fail(
                error, tr("The animation frames or timings are invalid."));
        }
        totalDuration += duration;
    }

    WebPAnimEncoderOptions options;
    if (WebPAnimEncoderOptionsInit(&options) == 0)
    {
        return fail(error, tr("The WebP encoder could not be initialized."));
    }
    options.anim_params.loop_count = 0;
    options.anim_params.bgcolor = 0;
    options.minimize_size = 1;

    Encoder encoder(
        WebPAnimEncoderNew(frameSize.width(), frameSize.height(), &options));
    if (!encoder)
    {
        return fail(error, tr("The WebP encoder could not be initialized."));
    }

    WebPConfig config;
    if (WebPConfigInit(&config) == 0
        || WebPConfigLosslessPreset(&config, 6) == 0
        || WebPValidateConfig(&config) == 0)
    {
        return fail(error, tr("The WebP encoder settings are invalid."));
    }
    config.alpha_quality = 100;

    int timestamp = 0;
    for (qsizetype index = 0; index < frames.size(); ++index)
    {
        if (isCanceled && isCanceled())
        {
            return false;
        }
        const QImage rgba =
            frames[index].convertToFormat(QImage::Format_RGBA8888);
        if (rgba.isNull()
            || rgba.bytesPerLine() > std::numeric_limits<int>::max())
        {
            return fail(error, tr("An animation frame could not be encoded."));
        }
        Picture picture;
        if (!picture.isValid())
        {
            return fail(
                error, tr("The WebP encoder could not be initialized."));
        }
        picture.get()->use_argb = 1;
        picture.get()->width = rgba.width();
        picture.get()->height = rgba.height();
        if (WebPPictureImportRGBA(picture.get(),
                rgba.constBits(),
                static_cast<int>(rgba.bytesPerLine()))
                == 0
            || WebPAnimEncoderAdd(
                   encoder.get(), picture.get(), timestamp, &config)
                   == 0)
        {
            return fail(error, encoderError(encoder.get()));
        }
        timestamp += durationsMilliseconds[index];
    }
    if ((isCanceled && isCanceled())
        || WebPAnimEncoderAdd(encoder.get(), nullptr, timestamp, nullptr) == 0)
    {
        return isCanceled && isCanceled()
                   ? false
                   : fail(error, encoderError(encoder.get()));
    }

    EncodedData encoded;
    if (WebPAnimEncoderAssemble(encoder.get(), encoded.get()) == 0)
    {
        return fail(error, encoderError(encoder.get()));
    }
    if (encoded.value().size
        > static_cast<size_t>(std::numeric_limits<qint64>::max()))
    {
        return fail(error, tr("The encoded WebP file is too large."));
    }
    if (isCanceled && isCanceled())
    {
        return false;
    }

    QSaveFile file(filePath);
    if (!file.open(QIODevice::WriteOnly))
    {
        return fail(error, file.errorString());
    }
    const qint64 byteCount = static_cast<qint64>(encoded.value().size);
    if (file.write(
            reinterpret_cast<const char *>(encoded.value().bytes), byteCount)
            != byteCount
        || (isCanceled && isCanceled()))
    {
        const QString message = file.error() == QFileDevice::NoError
                                    ? tr("The WebP export was canceled.")
                                    : file.errorString();
        file.cancelWriting();
        return fail(error, message);
    }
    if (!file.commit())
    {
        return fail(error, file.errorString());
    }
    return true;
}

}
