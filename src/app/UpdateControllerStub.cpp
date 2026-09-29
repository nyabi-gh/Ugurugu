// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/UpdateController.hpp"

namespace ugurugu
{

class UpdateController::Impl
{
};

void UpdateController::initialize()
{
}

UpdateController::UpdateController(QWidget *, QObject *parent)
    : QObject(parent)
    , m_impl(std::make_unique<Impl>())
{
}

UpdateController::~UpdateController() = default;

void UpdateController::checkForUpdates()
{
}

}
