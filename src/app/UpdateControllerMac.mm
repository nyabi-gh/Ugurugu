// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/UpdateController.hpp"

#include <QTimer>

#import <Sparkle/Sparkle.h>

namespace ugurugu
{

class UpdateController::Impl
{
public:
    void start()
    {
        if (controller)
        {
            return;
        }
        controller =
            [[SPUStandardUpdaterController alloc] initWithStartingUpdater:YES
                                                          updaterDelegate:nil
                                                       userDriverDelegate:nil];
    }

    SPUStandardUpdaterController *controller = nil;
};

void UpdateController::initialize()
{
}

UpdateController::UpdateController(QWidget *, QObject *parent)
    : QObject(parent)
    , m_impl(std::make_unique<Impl>())
{
    QTimer::singleShot(0,
        this,
        [this]()
        {
            // Sparkle applies SUEnableAutomaticChecks and
            // SUScheduledCheckInterval. Do not force another request on every
            // application launch.
            m_impl->start();
        });
}

UpdateController::~UpdateController() = default;

void UpdateController::checkForUpdates()
{
    m_impl->start();
    [m_impl->controller checkForUpdates:nil];
}

}
