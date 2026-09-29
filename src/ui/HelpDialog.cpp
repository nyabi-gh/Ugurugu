// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/HelpDialog.hpp"

#include <QDialogButtonBox>
#include <QTextBrowser>
#include <QVBoxLayout>

namespace ugurugu
{

HelpDialog::HelpDialog(QWidget *parent)
    : QDialog(parent)
{
    setObjectName(QStringLiteral("helpDialog"));
    setWindowTitle(tr("Ugurugu Help"));
    setAttribute(Qt::WA_DeleteOnClose);
    setModal(false);

    auto *layout = new QVBoxLayout(this);
    auto *browser = new QTextBrowser(this);
    browser->setObjectName(QStringLiteral("helpBrowser"));
    browser->setOpenExternalLinks(true);
    browser->setHtml(tr(
        "<h1>Ugurugu</h1>"
        "<p>Draw a stroke, then play the timeline to see it wobble.</p>"
        "<h2>Drawing</h2>"
        "<ul><li>Choose Brush, Eraser, Area Select, Auto Select, or Paint "
        "Bucket from the tool rail.</li>"
        "<li>Open Wobble settings to choose Classic, Smooth, or Stepped "
        "motion and adjust line detail.</li>"
        "<li>Use the timeline for frame count, playback speed, and the "
        "current frame.</li></ul>"
        "<h2>Selection and layers</h2>"
        "<ul><li>Area Select can select content or paint a filled lasso "
        "shape.</li>"
        "<li>Merge Down is available only when the two layers can be "
        "combined without changing their appearance.</li></ul>"
        "<h2>Files and export</h2>"
        "<ul><li>Projects use .ugu, and older .wagle or .wobble projects still "
        "open. Native version 10 .wawa files open "
        "as new unsaved projects.</li>"
        "<li>Insert image keeps the original pixels for later "
        "non-destructive transforms.</li>"
        "<li>Export the current frame as PNG or JPEG, or the animation as "
        "GIF or WebP.</li></ul>"
        "<p>Press F1 to open this help again.</p>"));
    layout->addWidget(browser, 1);

    auto *buttons =
        new QDialogButtonBox(QDialogButtonBox::Close, Qt::Horizontal, this);
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::close);
    layout->addWidget(buttons);
    resize(620, 520);
}

}
