// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/AboutDialog.hpp"

#include <QApplication>
#include <QDialogButtonBox>
#include <QTextBrowser>
#include <QVBoxLayout>

namespace ugurugu
{

AboutDialog::AboutDialog(QWidget *parent)
    : QDialog(parent)
{
    setObjectName(QStringLiteral("aboutDialog"));
    setWindowTitle(tr("About Ugurugu"));
    setAttribute(Qt::WA_DeleteOnClose);
    setModal(false);

    auto *layout = new QVBoxLayout(this);
    auto *browser = new QTextBrowser(this);
    browser->setObjectName(QStringLiteral("aboutBrowser"));
    browser->setOpenExternalLinks(true);
    // The license text itself is too long to read in a dialog, so the notice
    // points at the copy that release packages install beside the program.
    browser->setHtml(
        tr("<h1>Ugurugu %1</h1>"
           "<p>A drawing app where your pictures wiggle and move.</p>"
           "<p>Copyright (C) 2026 Nyabi (nyabi-gh)</p>"
           "<p>This program is free software: you can redistribute it and/or "
           "modify it under the terms of the GNU General Public License as "
           "published by the Free Software Foundation, either version 3 of "
           "the License, or (at your option) any later version.</p>"
           "<p>This program is distributed in the hope that it will be "
           "useful, but WITHOUT ANY WARRANTY; without even the implied "
           "warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. "
           "See the <a href=\"https://www.gnu.org/licenses/gpl-3.0.html\">GNU "
           "General Public License</a> for more details. The full license "
           "text is installed with Ugurugu as LICENSE.</p>"
           "<p>Qt, the bundled font and the other included libraries carry "
           "their own copyright holders and terms, listed in "
           "THIRD_PARTY_NOTICES.md next to the application.</p>"
           "<p><a href=\"https://github.com/nyabi-gh/Ugurugu\">"
           "github.com/nyabi-gh/Ugurugu</a></p>")
            .arg(QApplication::applicationVersion()));
    layout->addWidget(browser, 1);

    auto *buttons =
        new QDialogButtonBox(QDialogButtonBox::Close, Qt::Horizontal, this);
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::close);
    layout->addWidget(buttons);
    resize(520, 460);
}

}
