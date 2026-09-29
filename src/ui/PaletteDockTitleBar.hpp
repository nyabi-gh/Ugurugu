// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

class QDockWidget;

namespace ugurugu
{

void installCompactPaletteTitleBar(QDockWidget *dock);
bool isPaletteDockCollapsed(const QDockWidget *dock);
void setPaletteDockCollapsed(QDockWidget *dock, bool collapsed);

}
