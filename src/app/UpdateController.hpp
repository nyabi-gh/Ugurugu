// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QObject>

#include <memory>

class QWidget;

namespace ugurugu
{

class UpdateController final : public QObject
{
    Q_OBJECT

public:
    static void initialize();

    explicit UpdateController(QWidget *window, QObject *parent = nullptr);
    ~UpdateController() override;

public slots:
    void checkForUpdates();

private:
    class Impl;
    std::unique_ptr<Impl> m_impl;
};

}
