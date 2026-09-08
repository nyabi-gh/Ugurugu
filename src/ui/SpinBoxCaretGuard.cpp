// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/SpinBoxCaretGuard.hpp"

#include <QAbstractSpinBox>
#include <QLineEdit>

namespace ugurugu
{

void installSuffixCaretGuard(QAbstractSpinBox *spinBox)
{
    if (spinBox == nullptr)
    {
        return;
    }
    auto *edit = spinBox->findChild<QLineEdit *>();
    if (edit == nullptr)
    {
        return;
    }
    QObject::connect(edit,
        &QLineEdit::cursorPositionChanged,
        edit,
        [spinBox, edit](int, int position)
        {
            // QAbstractSpinBox has no suffix() of its own, but QSpinBox and
            // QDoubleSpinBox both expose one as a property, so a single guard
            // serves either. A special value text carries no suffix at all,
            // and clamping into it would move the caret for no reason.
            const QString suffix = spinBox->property("suffix").toString();
            const QString text = edit->text();
            if (suffix.isEmpty() || !text.endsWith(suffix))
            {
                return;
            }
            // Dragging a selection across the suffix is the user reading, not
            // typing, and Qt already restores a sane caret when it ends.
            if (edit->hasSelectedText())
            {
                return;
            }
            const qsizetype limit = text.size() - suffix.size();
            if (position > limit)
            {
                edit->setCursorPosition(static_cast<int>(limit));
            }
        });
}

}
