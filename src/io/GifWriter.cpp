// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/GifWriter.hpp"

#include "io/AnimationExportPolicy.hpp"

#include <QByteArray>
#include <QHash>
#include <QIODevice>
#include <QSaveFile>

#include <algorithm>
#include <limits>
#include <utility>

namespace ugurugu
{
namespace
{

struct HistogramBucket
{
    quint64 count = 0;
    quint64 red = 0;
    quint64 green = 0;
    quint64 blue = 0;
};

struct HistogramEntry
{
    int key = 0;
    quint64 count = 0;
    quint64 red = 0;
    quint64 green = 0;
    quint64 blue = 0;

    int averageRed() const
    {
        return static_cast<int>(red / count);
    }

    int averageGreen() const
    {
        return static_cast<int>(green / count);
    }

    int averageBlue() const
    {
        return static_cast<int>(blue / count);
    }
};

struct ColorBox
{
    QVector<int> entries;
    quint64 population = 0;
    int redMinimum = 0;
    int redMaximum = 0;
    int greenMinimum = 0;
    int greenMaximum = 0;
    int blueMinimum = 0;
    int blueMaximum = 0;
};

class BitWriter
{
public:
    void write(int code, int width)
    {
        buffer_ |= static_cast<quint32>(code) << bitCount_;
        bitCount_ += width;

        while (bitCount_ >= 8)
        {
            bytes_.append(static_cast<char>(buffer_ & 0xffU));
            buffer_ >>= 8;
            bitCount_ -= 8;
        }
    }

    QByteArray finish()
    {
        if (bitCount_ > 0)
        {
            bytes_.append(static_cast<char>(buffer_ & 0xffU));
            buffer_ = 0;
            bitCount_ = 0;
        }

        return bytes_;
    }

private:
    QByteArray bytes_;
    quint32 buffer_ = 0;
    int bitCount_ = 0;
};

bool fail(QString *error, const QString &message)
{
    if (error != nullptr)
    {
        *error = message;
    }
    return false;
}

void appendByte(QByteArray &output, int value)
{
    output.append(static_cast<char>(value & 0xff));
}

void appendWord(QByteArray &output, int value)
{
    appendByte(output, value);
    appendByte(output, value >> 8);
}

// Buckets the histogram at RGB555, so median cut works over at most 32768
// distinct colors instead of the full 24-bit space. Five bits per channel is
// finer than the 256-entry table the format can carry.
int colorKey(QRgb color)
{
    return ((qRed(color) >> 3) << 10) | ((qGreen(color) >> 3) << 5)
           | (qBlue(color) >> 3);
}

void updateBox(ColorBox &box, const QVector<HistogramEntry> &entries)
{
    const HistogramEntry &first = entries.at(box.entries.first());
    box.population = 0;
    box.redMinimum = first.averageRed();
    box.redMaximum = box.redMinimum;
    box.greenMinimum = first.averageGreen();
    box.greenMaximum = box.greenMinimum;
    box.blueMinimum = first.averageBlue();
    box.blueMaximum = box.blueMinimum;

    for (int index : box.entries)
    {
        const HistogramEntry &entry = entries.at(index);
        const int red = entry.averageRed();
        const int green = entry.averageGreen();
        const int blue = entry.averageBlue();
        box.population += entry.count;
        box.redMinimum = std::min(box.redMinimum, red);
        box.redMaximum = std::max(box.redMaximum, red);
        box.greenMinimum = std::min(box.greenMinimum, green);
        box.greenMaximum = std::max(box.greenMaximum, green);
        box.blueMinimum = std::min(box.blueMinimum, blue);
        box.blueMaximum = std::max(box.blueMaximum, blue);
    }
}

int splitAxis(const ColorBox &box)
{
    const int redRange = box.redMaximum - box.redMinimum;
    const int greenRange = box.greenMaximum - box.greenMinimum;
    const int blueRange = box.blueMaximum - box.blueMinimum;

    if (redRange >= greenRange && redRange >= blueRange)
    {
        return 0;
    }
    if (greenRange >= blueRange)
    {
        return 1;
    }
    return 2;
}

int component(const HistogramEntry &entry, int axis)
{
    if (axis == 0)
    {
        return entry.averageRed();
    }
    if (axis == 1)
    {
        return entry.averageGreen();
    }
    return entry.averageBlue();
}

QVector<QRgb> buildPalette(
    const QVector<HistogramEntry> &entries, bool hasTransparency)
{
    // A GIF color table holds at most 256 entries, and index 0 is reserved for
    // the transparent color whenever the animation needs one.
    const int maximumOpaqueColors = hasTransparency ? 255 : 256;
    QVector<ColorBox> boxes;

    if (!entries.isEmpty() && maximumOpaqueColors > 0)
    {
        ColorBox initial;
        initial.entries.reserve(entries.size());
        for (int index = 0; index < entries.size(); ++index)
        {
            initial.entries.append(index);
        }
        updateBox(initial, entries);
        boxes.append(std::move(initial));
    }

    while (boxes.size() < maximumOpaqueColors)
    {
        int selected = -1;
        long double selectedScore = -1.0L;

        for (int index = 0; index < boxes.size(); ++index)
        {
            const ColorBox &box = boxes.at(index);
            if (box.entries.size() < 2)
            {
                continue;
            }

            const int range = std::max({box.redMaximum - box.redMinimum,
                box.greenMaximum - box.greenMinimum,
                box.blueMaximum - box.blueMinimum});
            const long double score =
                static_cast<long double>(range + 1)
                * static_cast<long double>(box.population);

            if (score > selectedScore)
            {
                selected = index;
                selectedScore = score;
            }
        }

        if (selected < 0)
        {
            break;
        }

        ColorBox source = std::move(boxes[selected]);
        const int axis = splitAxis(source);
        std::sort(source.entries.begin(),
            source.entries.end(),
            [&entries, axis](int left, int right)
            {
                const int leftComponent = component(entries.at(left), axis);
                const int rightComponent = component(entries.at(right), axis);
                if (leftComponent != rightComponent)
                {
                    return leftComponent < rightComponent;
                }
                return entries.at(left).key < entries.at(right).key;
            });

        quint64 cumulative = 0;
        int division = 1;
        for (int index = 0; index < source.entries.size() - 1; ++index)
        {
            cumulative += entries.at(source.entries.at(index)).count;
            division = index + 1;
            if (cumulative >= (source.population + 1) / 2)
            {
                break;
            }
        }

        ColorBox left;
        ColorBox right;
        left.entries = source.entries.mid(0, division);
        right.entries = source.entries.mid(division);

        if (left.entries.isEmpty() || right.entries.isEmpty())
        {
            break;
        }

        updateBox(left, entries);
        updateBox(right, entries);
        boxes[selected] = std::move(left);
        boxes.append(std::move(right));
    }

    QVector<QRgb> palette;
    palette.reserve(boxes.size() + (hasTransparency ? 1 : 0));

    if (hasTransparency)
    {
        palette.append(qRgb(0, 0, 0));
    }

    for (const ColorBox &box : std::as_const(boxes))
    {
        quint64 population = 0;
        quint64 red = 0;
        quint64 green = 0;
        quint64 blue = 0;

        for (int index : box.entries)
        {
            const HistogramEntry &entry = entries.at(index);
            population += entry.count;
            red += entry.red;
            green += entry.green;
            blue += entry.blue;
        }

        if (population == 0)
        {
            continue;
        }

        palette.append(qRgb(static_cast<int>(red / population),
            static_cast<int>(green / population),
            static_cast<int>(blue / population)));
    }

    if (palette.isEmpty())
    {
        palette.append(qRgb(0, 0, 0));
    }

    return palette;
}

QVector<quint8> buildColorMap(const QVector<HistogramEntry> &entries,
    const QVector<QRgb> &palette,
    bool hasTransparency)
{
    QVector<quint8> colorMap(32768, 0);
    const int firstOpaque = hasTransparency ? 1 : 0;

    for (const HistogramEntry &entry : entries)
    {
        const int red = entry.averageRed();
        const int green = entry.averageGreen();
        const int blue = entry.averageBlue();
        int bestIndex = firstOpaque;
        int bestDistance = std::numeric_limits<int>::max();

        for (int paletteIndex = firstOpaque; paletteIndex < palette.size();
            ++paletteIndex)
        {
            const QRgb paletteColor = palette.at(paletteIndex);
            const int redDifference = red - qRed(paletteColor);
            const int greenDifference = green - qGreen(paletteColor);
            const int blueDifference = blue - qBlue(paletteColor);
            const int distance = redDifference * redDifference
                                 + greenDifference * greenDifference
                                 + blueDifference * blueDifference;

            if (distance < bestDistance)
            {
                bestDistance = distance;
                bestIndex = paletteIndex;
            }
        }

        colorMap[entry.key] = static_cast<quint8>(bestIndex);
    }

    return colorMap;
}

QByteArray frameIndices(
    const QImage &image, const QVector<quint8> &colorMap, bool hasTransparency)
{
    const qsizetype pixelCount = static_cast<qsizetype>(image.width())
                                 * static_cast<qsizetype>(image.height());
    QByteArray indices;
    indices.resize(pixelCount);
    qsizetype destination = 0;

    for (int y = 0; y < image.height(); ++y)
    {
        const auto *row =
            reinterpret_cast<const QRgb *>(image.constScanLine(y));
        for (int x = 0; x < image.width(); ++x)
        {
            // GIF transparency is all or nothing, so partial alpha has to
            // collapse to a threshold rather than blend.
            const QRgb color = row[x];
            const quint8 paletteIndex = hasTransparency && qAlpha(color) < 128
                                            ? 0
                                            : colorMap.at(colorKey(color));
            indices[destination++] = static_cast<char>(paletteIndex);
        }
    }

    return indices;
}

// GIF's LZW variant: the two codes just above the index range are reserved for
// clear and end-of-stream, codes widen as the dictionary grows, and 4096 is the
// largest code the format allows before the dictionary must be reset.
QByteArray compressLzw(const QByteArray &indices, int minimumCodeSize)
{
    const int clearCode = 1 << minimumCodeSize;
    const int endCode = clearCode + 1;
    int nextCode = endCode + 1;
    int codeSize = minimumCodeSize + 1;
    int emittedSinceClear = 0;
    QHash<quint32, int> dictionary;
    dictionary.reserve(4096);
    BitWriter writer;
    writer.write(clearCode, codeSize);

    if (indices.isEmpty())
    {
        writer.write(endCode, codeSize);
        return writer.finish();
    }

    int prefix = static_cast<quint8>(indices.at(0));

    for (qsizetype index = 1; index < indices.size(); ++index)
    {
        const int suffix = static_cast<quint8>(indices.at(index));
        const quint32 key =
            (static_cast<quint32>(prefix) << 8) | static_cast<quint32>(suffix);
        const auto found = dictionary.constFind(key);

        if (found != dictionary.cend())
        {
            prefix = found.value();
            continue;
        }

        writer.write(prefix, codeSize);
        ++emittedSinceClear;

        if (nextCode < 4096)
        {
            dictionary.insert(key, nextCode);
            ++nextCode;
            if (codeSize < 12 && nextCode > (1 << codeSize))
            {
                ++codeSize;
            }
        }
        else
        {
            writer.write(clearCode, codeSize);
            dictionary.clear();
            nextCode = endCode + 1;
            codeSize = minimumCodeSize + 1;
            emittedSinceClear = 0;
        }

        prefix = suffix;
    }

    writer.write(prefix, codeSize);
    if (emittedSinceClear > 0 && codeSize < 12 && nextCode == (1 << codeSize))
    {
        ++codeSize;
    }
    writer.write(endCode, codeSize);
    return writer.finish();
}

void appendSubBlocks(QByteArray &output, const QByteArray &data)
{
    qsizetype offset = 0;

    while (offset < data.size())
    {
        const qsizetype blockSize =
            std::min<qsizetype>(255, data.size() - offset);
        appendByte(output, static_cast<int>(blockSize));
        output.append(data.constData() + offset, blockSize);
        offset += blockSize;
    }

    appendByte(output, 0);
}

}

bool GifWriter::write(const QString &path,
    const QVector<QImage> &frames,
    int delayCentiseconds,
    QString *error,
    const std::function<bool()> &isCanceled)
{
    return write(path,
        frames,
        QVector<int>(frames.size(), delayCentiseconds),
        error,
        isCanceled);
}

bool GifWriter::write(const QString &path,
    const QVector<QImage> &frames,
    const QVector<int> &delaysCentiseconds,
    QString *error,
    const std::function<bool()> &isCanceled)
{
    if (error != nullptr)
    {
        error->clear();
    }

    if (path.isEmpty())
    {
        return fail(error, GifWriter::tr("The output path is empty."));
    }
    if (frames.isEmpty())
    {
        return fail(error, GifWriter::tr("At least one frame is required."));
    }
    if (delaysCentiseconds.size() != frames.size())
    {
        return fail(
            error, GifWriter::tr("Each frame must have one delay value."));
    }
    for (int delayCentiseconds : delaysCentiseconds)
    {
        if (delayCentiseconds < 0 || delayCentiseconds > 65535)
        {
            return fail(error,
                GifWriter::tr(
                    "Frame delays must be between 0 and 65535 centiseconds."));
        }
    }

    const int width = frames.first().width();
    const int height = frames.first().height();
    if (frames.first().isNull() || width <= 0 || height <= 0)
    {
        return fail(
            error, GifWriter::tr("Frames must contain valid image data."));
    }
    if (width > 65535 || height > 65535)
    {
        return fail(
            error, GifWriter::tr("GIF dimensions cannot exceed 65535 pixels."));
    }

    const qsizetype pixelCount =
        static_cast<qsizetype>(width) * static_cast<qsizetype>(height);
    if (pixelCount <= 0 || pixelCount > std::numeric_limits<int>::max())
    {
        return fail(
            error, GifWriter::tr("The frame dimensions are too large."));
    }
    if (!AnimationExportPolicy::fitsMemoryBudget(
            frames.first().size(), frames.size()))
    {
        return fail(error,
            GifWriter::tr("The animation is too large to encode safely."));
    }

    QVector<QImage> normalizedFrames;
    normalizedFrames.reserve(frames.size());

    for (const QImage &frame : frames)
    {
        if (isCanceled && isCanceled())
        {
            return false;
        }
        if (frame.isNull())
        {
            return fail(
                error, GifWriter::tr("Frames must contain valid image data."));
        }
        if (frame.width() != width || frame.height() != height)
        {
            return fail(error,
                GifWriter::tr("All frames must have the same dimensions."));
        }
        QImage normalized = frame.convertToFormat(QImage::Format_ARGB32);
        if (normalized.isNull())
        {
            return fail(error,
                GifWriter::tr(
                    "A frame could not be converted to the GIF pixel format."));
        }
        normalizedFrames.append(std::move(normalized));
    }

    QVector<HistogramBucket> histogram(32768);
    bool hasTransparency = false;

    for (const QImage &frame : std::as_const(normalizedFrames))
    {
        if (isCanceled && isCanceled())
        {
            return false;
        }
        for (int y = 0; y < height; ++y)
        {
            const auto *row =
                reinterpret_cast<const QRgb *>(frame.constScanLine(y));
            for (int x = 0; x < width; ++x)
            {
                const QRgb color = row[x];
                if (qAlpha(color) < 128)
                {
                    hasTransparency = true;
                    continue;
                }

                HistogramBucket &bucket = histogram[colorKey(color)];
                ++bucket.count;
                bucket.red += static_cast<quint64>(qRed(color));
                bucket.green += static_cast<quint64>(qGreen(color));
                bucket.blue += static_cast<quint64>(qBlue(color));
            }
        }
    }

    QVector<HistogramEntry> entries;
    entries.reserve(32768);

    for (int key = 0; key < histogram.size(); ++key)
    {
        const HistogramBucket &bucket = histogram.at(key);
        if (bucket.count == 0)
        {
            continue;
        }

        entries.append(HistogramEntry{
            key, bucket.count, bucket.red, bucket.green, bucket.blue});
    }

    const QVector<QRgb> palette = buildPalette(entries, hasTransparency);
    const QVector<quint8> colorMap =
        buildColorMap(entries, palette, hasTransparency);
    // The global color table must be a power of two long, and the screen
    // descriptor stores its size as log2(entries) - 1.
    int tableSize = 2;
    int tableBits = 1;

    while (tableSize < palette.size())
    {
        tableSize <<= 1;
        ++tableBits;
    }

    QByteArray output;
    output.reserve(13 + tableSize * 3 + frames.size() * 32);
    output.append("GIF89a", 6);
    appendWord(output, width);
    appendWord(output, height);
    // Packed field: global color table present, 8-bit color resolution, and
    // the table size in the low three bits.
    appendByte(output, 0x80 | 0x70 | (tableBits - 1));
    appendByte(output, 0);
    appendByte(output, 0);

    for (int index = 0; index < tableSize; ++index)
    {
        const QRgb color =
            index < palette.size() ? palette.at(index) : qRgb(0, 0, 0);
        appendByte(output, qRed(color));
        appendByte(output, qGreen(color));
        appendByte(output, qBlue(color));
    }

    appendByte(output, 0x21);
    appendByte(output, 0xff);
    appendByte(output, 0x0b);
    output.append("NETSCAPE2.0", 11);
    appendByte(output, 0x03);
    appendByte(output, 0x01);
    appendWord(output, 0);
    appendByte(output, 0);

    const int minimumCodeSize = std::max(2, tableBits);

    for (int frameIndex = 0; frameIndex < normalizedFrames.size(); ++frameIndex)
    {
        if (isCanceled && isCanceled())
        {
            return false;
        }
        const QImage &frame = normalizedFrames.at(frameIndex);
        appendByte(output, 0x21);
        appendByte(output, 0xf9);
        appendByte(output, 0x04);
        appendByte(output, 0x08 | (hasTransparency ? 0x01 : 0x00));
        appendWord(output, delaysCentiseconds.at(frameIndex));
        appendByte(output, 0);
        appendByte(output, 0);
        appendByte(output, 0x2c);
        appendWord(output, 0);
        appendWord(output, 0);
        appendWord(output, width);
        appendWord(output, height);
        appendByte(output, 0);

        const QByteArray indices =
            frameIndices(frame, colorMap, hasTransparency);
        const QByteArray compressed = compressLzw(indices, minimumCodeSize);
        appendByte(output, minimumCodeSize);
        appendSubBlocks(output, compressed);
    }

    appendByte(output, 0x3b);

    if (isCanceled && isCanceled())
    {
        return false;
    }

    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly))
    {
        return fail(error, file.errorString());
    }

    if (file.write(output) != output.size())
    {
        const QString message = file.errorString();
        file.cancelWriting();
        return fail(error, message);
    }

    if (isCanceled && isCanceled())
    {
        file.cancelWriting();
        return false;
    }

    if (!file.commit())
    {
        return fail(error, file.errorString());
    }

    return true;
}

}
