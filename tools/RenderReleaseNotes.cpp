// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include <QCoreApplication>
#include <QFile>
#include <QRegularExpression>
#include <QSaveFile>
#include <QString>
#include <QStringList>

#include <cstdio>

namespace
{

int fail(const QString &message)
{
    std::fprintf(stderr, "%s\n", message.toLocal8Bit().constData());
    return 1;
}

QString escaped(QString text)
{
    text.replace(QLatin1Char('&'), QStringLiteral("&amp;"));
    text.replace(QLatin1Char('<'), QStringLiteral("&lt;"));
    text.replace(QLatin1Char('>'), QStringLiteral("&gt;"));
    return text;
}

QString withInlineMarkup(const QString &text)
{
    static const QRegularExpression codePattern(QStringLiteral("`([^`]+)`"));
    static const QRegularExpression boldPattern(
        QStringLiteral("\\*\\*([^*]+)\\*\\*"));
    static const QRegularExpression emphasisPattern(
        QStringLiteral("\\*([^*]+)\\*"));
    static const QRegularExpression linkPattern(
        QStringLiteral("\\[([^\\]]+)\\]\\((https?://[^)\\s]+)\\)"));

    QString result = escaped(text);
    result.replace(codePattern, QStringLiteral("<code>\\1</code>"));
    result.replace(boldPattern, QStringLiteral("<strong>\\1</strong>"));
    result.replace(emphasisPattern, QStringLiteral("<em>\\1</em>"));
    result.replace(linkPattern, QStringLiteral("<a href=\"\\2\">\\1</a>"));
    return result;
}

QString bodyFromMarkdown(const QString &markdown)
{
    QStringList blocks;
    QStringList paragraph;
    QStringList listItems;
    QString listItem;

    const auto flushParagraph = [&]()
    {
        if (!paragraph.isEmpty())
        {
            blocks.append(QStringLiteral("<p>%1</p>")
                    .arg(withInlineMarkup(paragraph.join(QLatin1Char(' ')))));
            paragraph.clear();
        }
    };
    const auto flushListItem = [&]()
    {
        if (!listItem.isEmpty())
        {
            listItems.append(
                QStringLiteral("<li>%1</li>").arg(withInlineMarkup(listItem)));
            listItem.clear();
        }
    };
    const auto flushList = [&]()
    {
        flushListItem();
        if (!listItems.isEmpty())
        {
            blocks.append(
                QStringLiteral("<ul>%1</ul>").arg(listItems.join(QString())));
            listItems.clear();
        }
    };

    const QStringList lines = markdown.split(QLatin1Char('\n'));
    for (const QString &line : lines)
    {
        const QString trimmed = line.trimmed();
        if (trimmed.isEmpty())
        {
            flushParagraph();
            continue;
        }
        if (trimmed.startsWith(QStringLiteral("### ")))
        {
            flushParagraph();
            flushList();
            blocks.append(QStringLiteral("<h3>%1</h3>")
                    .arg(withInlineMarkup(trimmed.mid(4))));
            continue;
        }
        if (trimmed.startsWith(QStringLiteral("## ")))
        {
            flushParagraph();
            flushList();
            blocks.append(QStringLiteral("<h2>%1</h2>")
                    .arg(withInlineMarkup(trimmed.mid(3))));
            continue;
        }
        if (trimmed.startsWith(QStringLiteral("# ")))
        {
            flushParagraph();
            flushList();
            blocks.append(QStringLiteral("<h1>%1</h1>")
                    .arg(withInlineMarkup(trimmed.mid(2))));
            continue;
        }
        if (trimmed.startsWith(QStringLiteral("- ")))
        {
            flushParagraph();
            flushListItem();
            listItem = trimmed.mid(2);
            continue;
        }
        if (!listItem.isEmpty() && line.startsWith(QLatin1Char(' ')))
        {
            listItem += QLatin1Char(' ') + trimmed;
            continue;
        }
        flushList();
        paragraph.append(trimmed);
    }
    flushParagraph();
    flushList();
    return blocks.join(QLatin1Char('\n'));
}

QString documentFromBody(const QString &body)
{
    return QStringLiteral(R"(<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
:root { color-scheme: light dark; }
* { margin: 0; padding: 0; box-sizing: border-box; }
body {
  font-family: "Pretendard JP Variable", "Pretendard JP",
    "Pretendard Variable", Pretendard, -apple-system,
    BlinkMacSystemFont, "Apple SD Gothic Neo",
    "Hiragino Kaku Gothic ProN", "Segoe UI", sans-serif;
  font-size: 13px;
  font-weight: 500;
  line-height: 1.7;
  color: #35323d;
  background: #ffffff;
  padding: 18px 22px 26px;
  -webkit-text-size-adjust: 100%;
}
h1 {
  font-size: 16px;
  font-weight: 700;
  margin: 20px 0 8px;
}
h2 {
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.09em;
  text-transform: uppercase;
  color: #8a6a1f;
  margin: 20px 0 7px;
  padding-bottom: 5px;
  border-bottom: 1px solid rgba(138, 106, 31, 0.25);
}
h3 {
  font-size: 13px;
  font-weight: 700;
  margin: 16px 0 5px;
}
h1:first-child, h2:first-child, h3:first-child { margin-top: 0; }
p { margin: 6px 0; }
ul { list-style: none; margin: 2px 0 10px; }
li {
  position: relative;
  padding-left: 17px;
  margin: 5px 0;
}
li::before {
  content: "";
  position: absolute;
  left: 2px;
  top: 0.66em;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #ffc94a;
  border: 1px solid rgba(0, 0, 0, 0.12);
}
code {
  font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  font-size: 0.92em;
  background: rgba(127, 127, 127, 0.14);
  border-radius: 4px;
  padding: 1px 5px;
}
a { color: #8a6a1f; }
strong { font-weight: 700; }
@media (prefers-color-scheme: dark) {
  body { color: #e8e8ea; background: #202226; }
  h2 {
    color: #ffc94a;
    border-bottom-color: rgba(255, 201, 74, 0.22);
  }
  li::before { border-color: rgba(0, 0, 0, 0.35); }
  a { color: #ffc94a; }
}
</style>
</head>
<body>
%1
</body>
</html>
)")
        .arg(body);
}

}

int main(int argc, char **argv)
{
    QCoreApplication application(argc, argv);
    if (application.arguments().size() != 3)
    {
        return fail(QStringLiteral("Usage: ugurugu_render_release_notes "
                                   "<input.md> <output.html>"));
    }

    QFile input(application.arguments().at(1));
    if (!input.open(QIODevice::ReadOnly))
    {
        return fail(QStringLiteral("Could not open %1: %2")
                .arg(input.fileName(), input.errorString()));
    }

    const QString html =
        documentFromBody(bodyFromMarkdown(QString::fromUtf8(input.readAll())));

    QSaveFile output(application.arguments().at(2));
    if (!output.open(QIODevice::WriteOnly))
    {
        return fail(QStringLiteral("Could not open %1: %2")
                .arg(output.fileName(), output.errorString()));
    }
    const QByteArray bytes = html.toUtf8();
    if (output.write(bytes) != bytes.size())
    {
        return fail(QStringLiteral("Could not write %1: %2")
                .arg(output.fileName(), output.errorString()));
    }
    if (!output.commit())
    {
        return fail(QStringLiteral("Could not commit %1: %2")
                .arg(output.fileName(), output.errorString()));
    }

    return 0;
}
