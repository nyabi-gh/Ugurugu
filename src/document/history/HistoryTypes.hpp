// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QtGlobal>

namespace ugurugu
{
namespace history
{

struct StorageStats
{
    qsizetype retainedLayers = 0;
    qsizetype retainedStrokes = 0;
    qsizetype retainedPreparedDocuments = 0;
    qsizetype entryCount = 0;
    qsizetype stagedPreparedDocuments = 0;
    qsizetype peakTransientPreparedDocuments = 0;
    qsizetype macroPreparedDocuments = 0;
    qint64 retainedBytes = 0;
    bool residentBudgetSoftExceeded = false;
};

// Adjacent commands collapse into one undo entry only when they carry the
// same merge id, so every mergeable gesture needs an id distinct from all
// others. Reusing an id across unrelated properties would let two different
// edits merge into a single entry that cannot be undone separately.
constexpr int wobbleAmountMergeId = 1;
constexpr int animationFramesMergeId = 2;
constexpr int framesPerSecondMergeId = 3;
constexpr int layerOpacityMergeId = 4;
constexpr int motionStyleMergeId = 5;
constexpr int motionPoseCountMergeId = 6;
constexpr int motionDetailMergeId = 7;
constexpr int motionLinkedMergeId = 8;
constexpr int motionRandomnessMergeId = 9;
constexpr int brokenLineMergeId = 10;
constexpr int breakAmountMergeId = 11;
constexpr int breakRangeMergeId = 12;
constexpr int layerWobbleMergeId = 13;

}

}
