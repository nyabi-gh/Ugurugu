// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "app/BackgroundWork.hpp"
#include "app/RecoveryStore.hpp"
#include "app/RecoveryWriter.hpp"
#include "app/WatchedFutureResult.hpp"
#include "brush/BrushPreset.hpp"
#include "brush/EraserPreset.hpp"
#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"
#include "document/SelectionVisibility.hpp"
#include "io/DocumentSerializer.hpp"
#include "io/ExportWorker.hpp"
#include "render/LayerThumbnailRenderer.hpp"
#include "render/PreviewRenderPolicy.hpp"
#include "render/RenderEngine.hpp"
#include "support/CanvasWidgetTestAccess.hpp"
#include "support/MainWindowTestAccess.hpp"
#include "ui/AboutDialog.hpp"
#include "ui/BrushPopoverPanel.hpp"
#include "ui/BrushPresetButton.hpp"
#include "ui/BrushSizeRow.hpp"
#include "ui/CanvasSizeDialog.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/ColorDock.hpp"
#include "ui/ColorHistoryDock.hpp"
#include "ui/ColorHistoryGrid.hpp"
#include "ui/ColorPairSwatch.hpp"
#include "ui/ColorWheel.hpp"
#include "ui/EraserPopoverPanel.hpp"
#include "ui/EraserPresetButton.hpp"
#include "ui/FrameScrubber.hpp"
#include "ui/HelpDialog.hpp"
#include "ui/ImageSizeDialog.hpp"
#include "ui/LassoPopoverPanel.hpp"
#include "ui/LayerDock.hpp"
#include "ui/LayerItemDelegate.hpp"
#include "ui/LayerListWidget.hpp"
#include "ui/MainWindow.hpp"
#include "ui/SelectionActionBar.hpp"
#include "ui/SelectionShapeButton.hpp"
#include "ui/SettingsDialog.hpp"
#include "ui/ShortcutBinding.hpp"
#include "ui/SpinBoxCaretGuard.hpp"
#include "ui/StrokePropertiesDialog.hpp"
#include "ui/Theme.hpp"
#include "ui/TimelineBar.hpp"
#include "ui/ToolDock.hpp"
#include "ui/WandPopoverPanel.hpp"
#include "ui/WandReferenceButton.hpp"
#include "ui/WobbleDock.hpp"
#include "ui/WobblePopoverPanel.hpp"
#include "ui/WobblePreview.hpp"
#include "ui/WwpPresetCodec.hpp"

#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDateTime>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDir>
#include <QDoubleSpinBox>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QFocusEvent>
#include <QFont>
#include <QHideEvent>
#include <QInputDialog>
#include <QKeySequenceEdit>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMenu>
#include <QPaintEvent>
#include <QPixmap>
#include <QPointingDevice>
#include <QPushButton>
#include <QSettings>
#include <QShowEvent>
#include <QSlider>
#include <QSpinBox>
#include <QTabWidget>
#include <QTabletEvent>
#include <QTemporaryDir>
#include <QTextBrowser>
#include <QTimer>
#include <QToolButton>
#include <QVariant>
#include <QtTest>

#include <algorithm>
#include <limits>
#include <utility>

namespace ugurugu
{

class PaintRegionTracker final : public QObject
{
public:
    void reset()
    {
        m_eventCount = 0;
        m_largestArea = 0;
    }

    int eventCount() const
    {
        return m_eventCount;
    }

    qint64 largestArea() const
    {
        return m_largestArea;
    }

protected:
    bool eventFilter(QObject *watched, QEvent *event) override
    {
        if (event->type() == QEvent::Paint)
        {
            const auto *paintEvent = static_cast<QPaintEvent *>(event);
            qint64 area = 0;
            for (const QRect &rect : paintEvent->region())
            {
                area += static_cast<qint64>(rect.width()) * rect.height();
            }
            m_largestArea = std::max(m_largestArea, area);
            ++m_eventCount;
        }
        return QObject::eventFilter(watched, event);
    }

private:
    int m_eventCount = 0;
    qint64 m_largestArea = 0;
};

class SettingValueGuard final
{
public:
    explicit SettingValueGuard(QString key)
        : m_key(std::move(key))
        , m_existed(m_settings.contains(m_key))
        , m_value(m_settings.value(m_key))
    {
    }

    ~SettingValueGuard()
    {
        if (m_existed)
        {
            m_settings.setValue(m_key, m_value);
        }
        else
        {
            m_settings.remove(m_key);
        }
    }

private:
    QSettings m_settings;
    QString m_key;
    bool m_existed = false;
    QVariant m_value;
};

class EnvironmentVariableGuard final
{
public:
    explicit EnvironmentVariableGuard(QByteArray name)
        : m_name(std::move(name))
        , m_existed(qEnvironmentVariableIsSet(m_name.constData()))
        , m_value(qgetenv(m_name.constData()))
    {
    }

    ~EnvironmentVariableGuard()
    {
        if (m_existed)
        {
            qputenv(m_name.constData(), m_value);
        }
        else
        {
            qunsetenv(m_name.constData());
        }
    }

private:
    QByteArray m_name;
    bool m_existed = false;
    QByteArray m_value;
};

class ApplicationFontGuard final
{
public:
    explicit ApplicationFontGuard(int pointSizeScale)
        : m_font(QApplication::font())
    {
        QFont scaled = m_font;
        scaled.setPointSize(m_font.pointSize() * pointSizeScale);
        QApplication::setFont(scaled);
    }

    ~ApplicationFontGuard()
    {
        QApplication::setFont(m_font);
    }

private:
    QFont m_font;
};

class ApplicationVersionGuard final
{
public:
    ApplicationVersionGuard()
        : m_version(QApplication::applicationVersion())
    {
    }

    ~ApplicationVersionGuard()
    {
        QApplication::setApplicationVersion(m_version);
    }

private:
    QString m_version;
};

inline void scheduleDialogButtonClick(
    QObject *context, const QString &objectName, bool *clicked)
{
    QTimer::singleShot(0,
        context,
        [objectName, clicked]()
        {
            QDialog *dialog =
                qobject_cast<QDialog *>(QApplication::activeModalWidget());
            if (!dialog)
            {
                return;
            }
            QPushButton *button = dialog->findChild<QPushButton *>(objectName);
            if (!button)
            {
                dialog->reject();
                return;
            }
            *clicked = true;
            button->click();
        });
    QTimer::singleShot(1000,
        context,
        []()
        {
            QDialog *dialog =
                qobject_cast<QDialog *>(QApplication::activeModalWidget());
            if (dialog)
            {
                dialog->reject();
            }
        });
}

inline void scheduleDialogButtonClickAndAcceptNext(QObject *context,
    const QString &objectName,
    bool *clicked,
    bool *followupAccepted)
{
    QTimer::singleShot(0,
        context,
        [context, objectName, clicked, followupAccepted]()
        {
            QDialog *dialog =
                qobject_cast<QDialog *>(QApplication::activeModalWidget());
            if (!dialog)
            {
                return;
            }
            QPushButton *button = dialog->findChild<QPushButton *>(objectName);
            if (!button)
            {
                dialog->reject();
                return;
            }
            QTimer::singleShot(0,
                context,
                [followupAccepted]()
                {
                    QDialog *followup = qobject_cast<QDialog *>(
                        QApplication::activeModalWidget());
                    if (!followup)
                    {
                        return;
                    }
                    *followupAccepted = true;
                    followup->accept();
                });
            *clicked = true;
            button->click();
        });
    QTimer::singleShot(1000,
        context,
        []()
        {
            QDialog *dialog =
                qobject_cast<QDialog *>(QApplication::activeModalWidget());
            if (dialog)
            {
                dialog->reject();
            }
        });
}

inline Document nestedLayerDocument(
    int depth, const QSize &size = QSize(100, 100))
{
    Document document = Document::createDefault(size);
    Layer paint = document.layers.takeFirst();
    document.layers.clear();
    QUuid parentGroupId;
    for (int currentDepth = 0; currentDepth < depth; ++currentDepth)
    {
        Layer group;
        group.name = QStringLiteral("Group %1").arg(currentDepth + 1);
        group.kind = LayerKind::Group;
        group.parentGroupId = parentGroupId;
        group.initialCanvasSize = document.size;
        parentGroupId = group.id;
        document.layers.append(std::move(group));
    }
    paint.parentGroupId = parentGroupId;
    document.activeLayerId = paint.id;
    document.layers.append(std::move(paint));
    return document;
}

}
