// SPDX-License-Identifier: GPL-3.0-or-later
// Synthetic UI review probe; no product source changes.

#include "ui/CanvasWidget.hpp"
#include "ui/ColorWheel.hpp"
#include "ui/MainWindow.hpp"
#include "ui/Theme.hpp"

#include <QAction>
#include <QApplication>
#include <QFontDatabase>
#include <QPainter>
#include <QSettings>
#include <QTemporaryDir>
#include <QTest>
#include <QTranslator>

#include <iostream>
namespace ugurugu
{
class MainWindowTestAccess
{
public:
    static DocumentController &controller(MainWindow &w)
    {
        return w.m_controller;
    }
    static void wobble(MainWindow &w, bool v)
    {
        w.applyWobbleAnimationEnabled(v);
    }
    static void syncExport(MainWindow &w)
    {
        w.updateExportActions();
    }
    static bool unsaved(MainWindow &w)
    {
        return w.hasUnsavedWork();
    }
    static bool save(MainWindow &w, const QString &p)
    {
        return w.saveToFile(p);
    }
};
class CanvasWidgetTestAccess
{
public:
    static void place(CanvasWidget &c)
    {
        c.beginTextInteraction(QPointF(80, 80));
    }
    static void select(CanvasWidget &c, const QUuid &layer)
    {
        QImage mask(QSize(256, 256), QImage::Format_Grayscale8);
        mask.fill(0);
        QPainter p(&mask);
        p.fillRect(QRect(60, 60, 60, 60), Qt::white);
        p.end();
        c.restoreSelectionState({{}, layer, mask});
    }
    static QPointF center(CanvasWidget &c)
    {
        return c.displayedSelectionBounds().center();
    }
    static void move(CanvasWidget &c)
    {
        c.beginSelectionMove(QPointF(90, 90));
        c.continueSelectionMove(QPointF(110, 100));
        c.commitSelectionMove();
    }
};
}
int main(int argc, char **argv)
{
    QTemporaryDir isolated;
    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(
        QSettings::IniFormat, QSettings::UserScope, isolated.path());
    QSettings::setPath(
        QSettings::IniFormat, QSettings::SystemScope, isolated.path());
    qputenv(
        "UGURUGU_RECOVERY_PATH", isolated.filePath("recovery.ugu").toUtf8());
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QApplication a(argc, argv);
    a.setApplicationName("UguruguNativeReview");
    a.setOrganizationName("UguruguReview");
    QTranslator translation;
    if (translation.load("/Users/nyabi/Documents/Code/Ugurugu/out/build/"
                         "macos-release/ugurugu_ko.qm"))
        a.installTranslator(&translation);
    ugurugu::Theme::apply(a);
    int fontId = QFontDatabase::addApplicationFont(
        "/Users/nyabi/Documents/Code/Ugurugu/resources/fonts/"
        "PretendardJP-Medium.otf");
    if (fontId >= 0)
    {
        QFont f = a.font();
        f.setFamilies(QFontDatabase::applicationFontFamilies(fontId));
        f.setWeight(QFont::Medium);
        a.setFont(f);
    }
    ugurugu::MainWindow w;
    w.initializeSession();
    auto *c = w.findChild<ugurugu::CanvasWidget *>();
    c->setAnimating(false);
    auto *gif = w.findChild<QAction *>("exportGifAction");
    auto *webp = w.findChild<QAction *>("exportWebPAction");
    std::cout << "initial gif=" << gif->isEnabled()
              << " webp=" << webp->isEnabled() << "\n";
    ugurugu::MainWindowTestAccess::wobble(w, false);
    std::cout << "wobble_off gif=" << gif->isEnabled()
              << " webp=" << webp->isEnabled() << "\n";
    ugurugu::MainWindowTestAccess::syncExport(w);
    ugurugu::MainWindowTestAccess::wobble(w, true);
    std::cout << "post_export_sync_then_on gif=" << gif->isEnabled()
              << " webp=" << webp->isEnabled() << "\n";
    w.resize(1440, 900);
    w.show();
    QTest::qWait(200);
    w.grab().save("/private/tmp/ugurugu-native-ui-review/native-1440x900.png");
    std::cout << "capture_large=" << w.width() << "x" << w.height() << "\n";
    w.resize(1024, 768);
    QTest::qWait(150);
    w.grab().save("/private/tmp/ugurugu-native-ui-review/native-1024x768.png");
    std::cout << "capture_small=" << w.width() << "x" << w.height() << "\n";
    c->setTool(ugurugu::CanvasTool::Text);
    c->setTextContent("Review text draft");
    ugurugu::CanvasWidgetTestAccess::place(*c);
    std::cout << "text_draft placement=" << c->hasTextPlacement()
              << " unsaved=" << ugurugu::MainWindowTestAccess::unsaved(w)
              << "\n";
    ugurugu::MainWindowTestAccess::save(w, isolated.filePath("text-draft.ugu"));
    std::cout << "after_save placement=" << c->hasTextPlacement()
              << " unsaved=" << ugurugu::MainWindowTestAccess::unsaved(w)
              << " strokes="
              << ugurugu::MainWindowTestAccess::controller(w)
                     .document()
                     .layers.first()
                     .strokes.size()
              << "\n";
    ugurugu::MainWindowTestAccess::controller(w).newDocument(QSize(256, 256));
    std::cout << "after_new_document placement=" << c->hasTextPlacement()
              << " content=" << c->textContent().toStdString()
              << " unsaved=" << ugurugu::MainWindowTestAccess::unsaved(w)
              << "\n";
    bool applied = c->applyTextPlacement();
    std::cout << "apply_old_draft_to_new applied=" << applied << " strokes="
              << ugurugu::MainWindowTestAccess::controller(w)
                     .document()
                     .layers.first()
                     .strokes.size()
              << "\n";
    auto *wheel = w.findChild<ugurugu::ColorWheel *>();
    QColor before = wheel->color();
    QTest::keyClick(wheel, Qt::Key_Right);
    std::cout << "color_wheel tab_focus="
              << bool(wheel->focusPolicy() & Qt::TabFocus)
              << " right_changes_color=" << (before != wheel->color())
              << " accessible_name_empty=" << wheel->accessibleName().isEmpty()
              << "\n";
    c->setTool(ugurugu::CanvasTool::Lasso);
    ugurugu::Document d = ugurugu::Document::createDefault(QSize(256, 256));
    ugurugu::Stroke s;
    s.points.append({QPointF(80, 80), 1.0});
    s.points.append({QPointF(100, 100), 1.0});
    d.layers.first().strokes.append(s);
    ugurugu::MainWindowTestAccess::controller(w).loadDocument(d);
    ugurugu::CanvasWidgetTestAccess::select(*c, d.activeLayerId);
    QTest::qWait(150);
    ugurugu::CanvasWidgetTestAccess::move(*c);
    QPointF centerBefore = ugurugu::CanvasWidgetTestAccess::center(*c);
    bool rotated = c->rotateSelection(90);
    QPointF centerAfter = ugurugu::CanvasWidgetTestAccess::center(*c);
    std::cout << "move_then_rotate transformable="
              << c->hasTransformableSelection() << " rotated=" << rotated
              << " center_before=" << centerBefore.x() << ","
              << centerBefore.y() << " center_after=" << centerAfter.x() << ","
              << centerAfter.y() << "\n";
    w.hide();
}
