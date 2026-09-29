// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/ReleaseNotes.hpp"

#include <QtTest>

namespace ugurugu
{

namespace
{

QString sampleNotes()
{
    return QStringLiteral("<!-- lang:ko -->\n"
                          "## Fix\n"
                          "- 한국어 노트\n"
                          "<!-- lang:en -->\n"
                          "## Fix\n"
                          "- English notes\n"
                          "<!-- lang:ja -->\n"
                          "## Fix\n"
                          "- 日本語ノート\n");
}

}

class ReleaseNotesTests final : public QObject
{
    Q_OBJECT

private slots:
    void returnsWholeTextWithoutMarkers()
    {
        const QString plain = QStringLiteral("## Fix\n- Single language\n");
        QCOMPARE(localizedReleaseNotes(plain, QStringLiteral("ko")),
            plain.trimmed());
    }

    void picksSectionForLanguage()
    {
        QCOMPARE(localizedReleaseNotes(sampleNotes(), QStringLiteral("ko")),
            QStringLiteral("## Fix\n- 한국어 노트"));
        QCOMPARE(localizedReleaseNotes(sampleNotes(), QStringLiteral("ja")),
            QStringLiteral("## Fix\n- 日本語ノート"));
    }

    void normalizesRegionalLanguageCodes()
    {
        QCOMPARE(localizedReleaseNotes(sampleNotes(), QStringLiteral("ko_KR")),
            QStringLiteral("## Fix\n- 한국어 노트"));
        QCOMPARE(localizedReleaseNotes(sampleNotes(), QStringLiteral("ja-JP")),
            QStringLiteral("## Fix\n- 日本語ノート"));
    }

    void fallsBackToEnglishForUnknownLanguage()
    {
        QCOMPARE(localizedReleaseNotes(sampleNotes(), QStringLiteral("fr")),
            QStringLiteral("## Fix\n- English notes"));
    }

    void fallsBackToFirstSectionWithoutEnglish()
    {
        const QString notes = QStringLiteral("<!-- lang:ko -->\n"
                                             "한국어만\n"
                                             "<!-- lang:ja -->\n"
                                             "日本語のみ\n");
        QCOMPARE(localizedReleaseNotes(notes, QStringLiteral("fr")),
            QStringLiteral("한국어만"));
    }
};

int runReleaseNotesTests(int argc, char **argv)
{
    ReleaseNotesTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "ReleaseNotesTests.moc"
