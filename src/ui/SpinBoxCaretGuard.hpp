// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

class QAbstractSpinBox;

namespace ugurugu
{

// Keeps the caret out of a spin box's suffix. A click on the empty space right
// of "640 px" lands the caret behind the suffix, where every digit typed turns
// the text into "640 px8", fails the spin box validator and is dropped without
// a trace. Qt's own guard does not help: it leaves that end position alone and
// pushes a click inside the suffix to it. Install this on every spin box that
// carries a suffix.
void installSuffixCaretGuard(QAbstractSpinBox *spinBox);

}
