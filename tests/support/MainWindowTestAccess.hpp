// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "document/DocumentController.hpp"
#include "ui/MainWindow.hpp"

#include <QApplication>
#include <QLabel>
#include <QString>

#include <utility>

namespace ugurugu
{

class MainWindowTestAccess final
{
public:
    static void failNextDocumentReplacementPreparation(MainWindow &window)
    {
        window.m_controller.m_failNextDocumentReplacementPreparationForTesting =
            true;
    }

    static DocumentController &controller(MainWindow &window)
    {
        return window.m_controller;
    }

    static bool saveToFile(MainWindow &window, const QString &filePath)
    {
        return window.saveToFile(filePath);
    }

    static void setCurrentFilePath(MainWindow &window, const QString &filePath)
    {
        window.m_currentFilePath = filePath;
    }

    static bool clearAutosave(MainWindow &window)
    {
        return window.clearAutosave();
    }

    static void writeModifiedAutosave(MainWindow &window)
    {
        window.m_controller.addLayer();
        window.m_autosavePending = true;
        window.writeAutosave();
        flushAutosave(window);
    }

    static bool flushAutosave(MainWindow &window)
    {
        const bool idle = window.m_recoveryWriter.waitForIdle(15000);
        QApplication::processEvents();
        return idle;
    }

    static void requestAutosave(MainWindow &window)
    {
        window.m_autosavePending = true;
        window.writeAutosave();
    }

    // Unlike requestAutosave, leaves the pending state to whatever the
    // document signals armed, so a test can cover that wiring.
    static void writeAutosave(MainWindow &window)
    {
        window.writeAutosave();
    }

    static void setAutosaveWriterSuspended(MainWindow &window, bool suspended)
    {
        window.m_recoveryWriter.setSuspendedForTesting(suspended);
    }

    static bool autosavePending(const MainWindow &window)
    {
        return window.m_autosavePending;
    }

    static quint64 submittedRecoveryRevision(const MainWindow &window)
    {
        return window.m_submittedRecoveryRevision;
    }

    static void deliverAutosaveCompletion(MainWindow &window,
        bool success,
        quint64 revision,
        const QString &error)
    {
        window.handleAutosaveWritten(success, revision, error);
    }

    // Empty when the window is not telling the artist that recovery is broken.
    // isHidden rather than isVisible: the label is a child of a window a test
    // never has to show.
    static QString recoveryFailureText(const MainWindow &window)
    {
        return window.m_recoveryFailureLabel
                       && !window.m_recoveryFailureLabel->isHidden()
                   ? window.m_recoveryFailureLabel->text()
                   : QString();
    }

    static bool loadDocument(MainWindow &window, Document document)
    {
        return window.m_controller.loadDocument(std::move(document));
    }

    static void exportImage(MainWindow &window)
    {
        window.exportImage();
    }
};

}
