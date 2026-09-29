// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QUuid>
#include <QWidget>

#include <functional>

namespace ugurugu
{

class DocumentController;
struct MotionSettings;
class WobblePreview;

class WobblePopoverPanel final : public QWidget
{
    Q_OBJECT

public:
    explicit WobblePopoverPanel(
        DocumentController *controller, QWidget *parent = nullptr);

    // A null id edits the whole drawing. A paint layer id edits that layer's
    // own wobble, which is what lets a background hold still while the lines
    // above it keep moving.
    void setScopeLayer(const QUuid &layerId);
    QUuid scopeLayer() const;

private:
    // Applies an edit to the scoped layer and returns true. Returns false when
    // the scope is the document, so callers fall through to the document-wide
    // setters that carry their own undo merge ids.
    bool editScopedLayer(
        const std::function<void(qreal &, MotionSettings &)> &mutate);

    DocumentController *m_controller = nullptr;
    WobblePreview *m_preview = nullptr;
    QUuid m_scopeLayer;
    std::function<void()> m_sync;
};

}
