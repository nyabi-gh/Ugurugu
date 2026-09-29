// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/SettingsDialog.hpp"

#include "ui/ShortcutBinding.hpp"
#include "ui/Theme.hpp"

#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QColorDialog>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QDir>
#include <QFileDialog>
#include <QFormLayout>
#include <QFrame>
#include <QHBoxLayout>
#include <QKeySequenceEdit>
#include <QLabel>
#include <QLatin1StringView>
#include <QLineEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QScrollArea>
#include <QSettings>
#include <QSignalBlocker>
#include <QStandardPaths>
#include <QTabWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <utility>

namespace ugurugu
{

namespace
{

constexpr QLatin1StringView animateWhileDrawingKey(
    "canvas/animateWhileDrawing");
constexpr QLatin1StringView wobbleAnimationKey("canvas/wobbleAnimation");
constexpr QLatin1StringView defaultSaveFolderKey("files/defaultSaveFolder");
constexpr QLatin1StringView uiLanguageKey("appearance/language");

QString systemDefaultSaveFolder()
{
    const QString documents =
        QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation);
    if (!documents.isEmpty() && QDir(documents).exists())
    {
        return QDir(documents).absolutePath();
    }
    const QString home =
        QStandardPaths::writableLocation(QStandardPaths::HomeLocation);
    return home.isEmpty() ? QDir::currentPath() : QDir(home).absolutePath();
}

QString actionLabel(const QAction *action)
{
    QString label = action->property("shortcutLabel").toString();
    if (label.isEmpty())
    {
        label = action->text();
    }
    label.remove(QLatin1Char('&'));
    return label;
}

QKeySequence defaultShortcut(const QAction *action)
{
    return ShortcutBinding::defaultPrimary(action);
}

}

bool SettingsDialog::wobbleAnimationEnabled()
{
    return QSettings().value(wobbleAnimationKey, true).toBool();
}

bool SettingsDialog::animateWhileDrawing()
{
    return QSettings().value(animateWhileDrawingKey, false).toBool();
}

QString SettingsDialog::defaultSaveFolder()
{
    const QString configured =
        QSettings().value(defaultSaveFolderKey).toString();
    return !configured.isEmpty() && QDir(configured).exists()
               ? QDir(configured).absolutePath()
               : systemDefaultSaveFolder();
}

QString SettingsDialog::uiLanguage()
{
    QString language =
        QSettings().value(uiLanguageKey, QStringLiteral("system")).toString();
    if (language == QStringLiteral("en") || language == QStringLiteral("ko")
        || language == QStringLiteral("ja"))
    {
        return language;
    }
    return QStringLiteral("system");
}

SettingsDialog::SettingsDialog(
    QWidget *parent, const QList<QAction *> &shortcutActions)
    : QDialog(parent)
    , m_shortcutActions(shortcutActions)
{
    setWindowTitle(tr("Settings"));
    setModal(true);
    ShortcutBinding::resolveAliasConflicts(m_shortcutActions);

    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(14, 14, 14, 12);
    layout->setSpacing(10);

    auto *tabs = new QTabWidget(this);
    layout->addWidget(tabs);

    auto *generalTab = new QWidget(tabs);
    auto *generalLayout = new QFormLayout(generalTab);
    generalLayout->setContentsMargins(14, 14, 14, 14);
    generalLayout->setHorizontalSpacing(18);
    generalLayout->setVerticalSpacing(10);

    m_languageCombo = new QComboBox(generalTab);
    m_languageCombo->setObjectName(QStringLiteral("languageCombo"));
    m_languageCombo->addItem(tr("System default"), QStringLiteral("system"));
    m_languageCombo->addItem(QStringLiteral("English"), QStringLiteral("en"));
    m_languageCombo->addItem(QStringLiteral("한국어"), QStringLiteral("ko"));
    m_languageCombo->addItem(QStringLiteral("日本語"), QStringLiteral("ja"));
    const int languageIndex = m_languageCombo->findData(uiLanguage());
    m_languageCombo->setCurrentIndex(std::max(0, languageIndex));
    generalLayout->addRow(tr("Language"), m_languageCombo);

    m_themeColorButton = new QPushButton(generalTab);
    m_themeColorButton->setObjectName(QStringLiteral("themeColorButton"));
    refreshThemeColorButton();
    connect(m_themeColorButton,
        &QPushButton::clicked,
        this,
        &SettingsDialog::chooseThemeColor);
    generalLayout->addRow(tr("Theme color"), m_themeColorButton);

    auto *restartLabel = new QLabel(
        tr("Restart Ugurugu to apply language changes."), generalTab);
    restartLabel->setWordWrap(true);
    generalLayout->addRow(QString(), restartLabel);
    tabs->addTab(generalTab, tr("General"));

    auto *drawingTab = new QWidget(tabs);
    auto *drawingLayout = new QVBoxLayout(drawingTab);
    drawingLayout->setContentsMargins(14, 14, 14, 14);
    drawingLayout->setSpacing(8);

    m_wobbleAnimation = new QCheckBox(tr("Wobble animation"), drawingTab);
    m_wobbleAnimation->setObjectName(QStringLiteral("wobbleAnimationCheck"));
    m_wobbleAnimation->setChecked(wobbleAnimationEnabled());
    drawingLayout->addWidget(m_wobbleAnimation);

    m_drawingOptionsLabel =
        new QLabel(tr("Wobble preview while drawing a stroke"), drawingTab);
    drawingLayout->addWidget(m_drawingOptionsLabel);

    m_pauseWhileDrawing = new QRadioButton(
        tr("Pause the wobble until the stroke is finished"), drawingTab);
    drawingLayout->addWidget(m_pauseWhileDrawing);

    m_keepWobbling =
        new QRadioButton(tr("Keep wobbling while drawing"), drawingTab);
    drawingLayout->addWidget(m_keepWobbling);

    updateDrawingOptionsEnabled();
    connect(m_wobbleAnimation,
        &QCheckBox::toggled,
        this,
        [this](bool enabled)
        {
            QSettings().setValue(wobbleAnimationKey, enabled);
            emit wobbleAnimationEnabledChanged(enabled);
            updateDrawingOptionsEnabled();
        });

    drawingLayout->addStretch(1);
    tabs->addTab(drawingTab, tr("Drawing"));

    auto *filesTab = new QWidget(tabs);
    auto *filesLayout = new QVBoxLayout(filesTab);
    filesLayout->setContentsMargins(14, 14, 14, 14);
    filesLayout->setSpacing(10);

    auto *folderLabel = new QLabel(tr("Default save folder"), filesTab);
    filesLayout->addWidget(folderLabel);
    auto *folderDescription = new QLabel(
        tr("New projects and exports start in this folder."), filesTab);
    folderDescription->setWordWrap(true);
    filesLayout->addWidget(folderDescription);

    auto *folderRow = new QHBoxLayout;
    folderRow->setSpacing(8);
    m_defaultSaveFolderEdit = new QLineEdit(defaultSaveFolder(), filesTab);
    m_defaultSaveFolderEdit->setObjectName(
        QStringLiteral("defaultSaveFolderEdit"));
    m_defaultSaveFolderEdit->setReadOnly(true);
    folderRow->addWidget(m_defaultSaveFolderEdit, 1);
    auto *chooseFolderButton = new QPushButton(tr("Choose…"), filesTab);
    chooseFolderButton->setObjectName(
        QStringLiteral("chooseDefaultSaveFolderButton"));
    connect(chooseFolderButton,
        &QPushButton::clicked,
        this,
        &SettingsDialog::chooseDefaultSaveFolder);
    folderRow->addWidget(chooseFolderButton);
    filesLayout->addLayout(folderRow);

    auto *systemDefaultButton =
        new QPushButton(tr("Use system default"), filesTab);
    systemDefaultButton->setObjectName(
        QStringLiteral("systemDefaultSaveFolderButton"));
    connect(systemDefaultButton,
        &QPushButton::clicked,
        this,
        &SettingsDialog::resetDefaultSaveFolder);
    filesLayout->addWidget(systemDefaultButton, 0, Qt::AlignLeft);
    filesLayout->addStretch(1);
    tabs->addTab(filesTab, tr("Files"));

    auto *shortcutsTab = new QWidget(tabs);
    auto *shortcutsLayout = new QVBoxLayout(shortcutsTab);
    shortcutsLayout->setContentsMargins(14, 14, 14, 14);
    shortcutsLayout->setSpacing(10);

    auto *shortcutInstructions = new QLabel(
        tr("Click a shortcut field, then press the new key combination."),
        shortcutsTab);
    shortcutInstructions->setWordWrap(true);
    shortcutsLayout->addWidget(shortcutInstructions);

    auto *shortcutScroll = new QScrollArea(shortcutsTab);
    shortcutScroll->setWidgetResizable(true);
    shortcutScroll->setFrameShape(QFrame::NoFrame);
    auto *shortcutFormWidget = new QWidget(shortcutScroll);
    auto *shortcutForm = new QFormLayout(shortcutFormWidget);
    shortcutForm->setContentsMargins(0, 0, 8, 0);
    shortcutForm->setHorizontalSpacing(18);
    shortcutForm->setVerticalSpacing(8);

    for (QAction *action : std::as_const(m_shortcutActions))
    {
        if (!action || action->objectName().isEmpty())
        {
            continue;
        }
        auto *editor = new QKeySequenceEdit(
            ShortcutBinding::primary(action), shortcutFormWidget);
        editor->setObjectName(
            action->objectName() + QStringLiteral("ShortcutEdit"));
        editor->setClearButtonEnabled(true);
        editor->setMaximumSequenceLength(1);
        shortcutForm->addRow(actionLabel(action), editor);
        m_shortcutEditors.insert(action, editor);
        connect(editor,
            &QKeySequenceEdit::keySequenceChanged,
            this,
            [this, action](const QKeySequence &shortcut)
            {
                setShortcut(action, shortcut);
            });
    }

    shortcutScroll->setWidget(shortcutFormWidget);
    shortcutsLayout->addWidget(shortcutScroll, 1);

    m_shortcutMessage = new QLabel(shortcutsTab);
    m_shortcutMessage->setWordWrap(true);
    m_shortcutMessage->setMinimumHeight(
        m_shortcutMessage->fontMetrics().lineSpacing());
    shortcutsLayout->addWidget(m_shortcutMessage);
    tabs->addTab(shortcutsTab, tr("Shortcuts"));

    auto *aboutTab = new QWidget(tabs);
    aboutTab->setObjectName(QStringLiteral("aboutTab"));
    auto *aboutLayout = new QVBoxLayout(aboutTab);
    aboutLayout->setContentsMargins(14, 14, 14, 14);
    aboutLayout->setSpacing(8);

    auto *applicationNameLabel =
        new QLabel(QStringLiteral("Ugurugu"), aboutTab);
    QFont applicationNameFont = applicationNameLabel->font();
    applicationNameFont.setBold(true);
    applicationNameFont.setPointSize(applicationNameFont.pointSize() + 3);
    applicationNameLabel->setFont(applicationNameFont);
    aboutLayout->addWidget(applicationNameLabel);

    auto *versionLabel = new QLabel(
        tr("Version %1").arg(QApplication::applicationVersion()), aboutTab);
    versionLabel->setObjectName(QStringLiteral("applicationVersionLabel"));
    versionLabel->setTextInteractionFlags(Qt::TextSelectableByMouse);
    aboutLayout->addWidget(versionLabel);

    auto *developmentCreditLabel =
        new QLabel(tr("Development support by seuppi"), aboutTab);
    developmentCreditLabel->setObjectName(
        QStringLiteral("developmentCreditLabel"));
    developmentCreditLabel->setTextInteractionFlags(Qt::TextSelectableByMouse);
    aboutLayout->addWidget(developmentCreditLabel);

    auto *iconCreditLabel =
        new QLabel(tr("App icon artwork by seuppi"), aboutTab);
    iconCreditLabel->setObjectName(QStringLiteral("iconCreditLabel"));
    iconCreditLabel->setTextInteractionFlags(Qt::TextSelectableByMouse);
    aboutLayout->addWidget(iconCreditLabel);
    aboutLayout->addStretch(1);
    tabs->addTab(aboutTab, tr("About"));

    auto *buttons = new QDialogButtonBox(
        QDialogButtonBox::RestoreDefaults | QDialogButtonBox::Close, this);
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    connect(buttons->button(QDialogButtonBox::RestoreDefaults),
        &QPushButton::clicked,
        this,
        &SettingsDialog::restoreDefaults);
    layout->addWidget(buttons);

    if (animateWhileDrawing())
    {
        m_keepWobbling->setChecked(true);
    }
    else
    {
        m_pauseWhileDrawing->setChecked(true);
    }

    connect(m_keepWobbling,
        &QRadioButton::toggled,
        this,
        [this](bool keepWobbling)
        {
            QSettings settings;
            settings.setValue(animateWhileDrawingKey, keepWobbling);
            emit animateWhileDrawingChanged(keepWobbling);
        });
    connect(m_languageCombo,
        &QComboBox::currentIndexChanged,
        this,
        [this](int index)
        {
            QSettings settings;
            const QString language =
                m_languageCombo->itemData(index).toString();
            if (language == QStringLiteral("system"))
            {
                settings.remove(uiLanguageKey);
            }
            else
            {
                settings.setValue(uiLanguageKey, language);
            }
        });

    resize(520, 520);
}

void SettingsDialog::setShortcut(QAction *action, const QKeySequence &shortcut)
{
    if (!action)
    {
        return;
    }

    for (QAction *other : std::as_const(m_shortcutActions))
    {
        if (other && other != action && !shortcut.isEmpty()
            && ShortcutBinding::hasShortcut(other, shortcut))
        {
            m_shortcutMessage->setText(
                tr("This shortcut is already assigned to %1.")
                    .arg(actionLabel(other)));
            if (QKeySequenceEdit *editor = m_shortcutEditors.value(action))
            {
                const QSignalBlocker blocker(editor);
                editor->setKeySequence(ShortcutBinding::primary(action));
            }
            return;
        }
    }

    m_shortcutMessage->clear();
    ShortcutBinding::setPrimary(action, shortcut);
    ShortcutBinding::resolveAliasConflicts(m_shortcutActions);
}

void SettingsDialog::chooseDefaultSaveFolder()
{
    const QString selected = QFileDialog::getExistingDirectory(this,
        tr("Choose default save folder"),
        defaultSaveFolder(),
        QFileDialog::ShowDirsOnly);
    if (selected.isEmpty())
    {
        return;
    }
    const QString folder = QDir(selected).absolutePath();
    QSettings().setValue(defaultSaveFolderKey, folder);
    m_defaultSaveFolderEdit->setText(folder);
}

void SettingsDialog::resetDefaultSaveFolder()
{
    QSettings().remove(defaultSaveFolderKey);
    m_defaultSaveFolderEdit->setText(defaultSaveFolder());
}

void SettingsDialog::chooseThemeColor()
{
    const QColor color =
        QColorDialog::getColor(Theme::accent(), this, tr("Theme color"));
    if (!color.isValid())
    {
        return;
    }
    Theme::setAccent(*qApp, color);
    refreshThemeColorButton();
}

void SettingsDialog::refreshThemeColorButton()
{
    const QColor color = Theme::accent();
    m_themeColorButton->setText(color.name(QColor::HexRgb));
    m_themeColorButton->setStyleSheet(
        QStringLiteral("background: %1; color: %2;")
            .arg(color.name(QColor::HexRgb),
                Theme::accentText().name(QColor::HexRgb)));
}

void SettingsDialog::updateDrawingOptionsEnabled()
{
    const bool enabled = m_wobbleAnimation->isChecked();
    m_drawingOptionsLabel->setEnabled(enabled);
    m_pauseWhileDrawing->setEnabled(enabled);
    m_keepWobbling->setEnabled(enabled);
}

void SettingsDialog::restoreDefaults()
{
    QSettings settings;
    for (QAction *action : std::as_const(m_shortcutActions))
    {
        if (!action)
        {
            continue;
        }
        ShortcutBinding::restoreDefault(action);
        if (QKeySequenceEdit *editor = m_shortcutEditors.value(action))
        {
            const QSignalBlocker blocker(editor);
            editor->setKeySequence(defaultShortcut(action));
        }
    }
    ShortcutBinding::resolveAliasConflicts(m_shortcutActions);
    {
        const QSignalBlocker pauseBlocker(m_pauseWhileDrawing);
        const QSignalBlocker wobbleBlocker(m_keepWobbling);
        m_pauseWhileDrawing->setChecked(true);
        m_keepWobbling->setChecked(false);
    }
    settings.remove(animateWhileDrawingKey);
    emit animateWhileDrawingChanged(false);
    {
        const QSignalBlocker wobbleAnimationBlocker(m_wobbleAnimation);
        m_wobbleAnimation->setChecked(true);
    }
    settings.remove(wobbleAnimationKey);
    emit wobbleAnimationEnabledChanged(true);
    updateDrawingOptionsEnabled();
    {
        const QSignalBlocker languageBlocker(m_languageCombo);
        m_languageCombo->setCurrentIndex(
            m_languageCombo->findData(QStringLiteral("system")));
    }
    settings.remove(uiLanguageKey);
    Theme::setAccent(*qApp, Theme::defaultAccent());
    refreshThemeColorButton();
    resetDefaultSaveFolder();
    m_shortcutMessage->clear();
}

}
