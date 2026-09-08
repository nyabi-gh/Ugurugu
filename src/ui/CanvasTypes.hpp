// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

namespace ugurugu
{

enum class CanvasTool
{
    Brush,
    Eraser,
    Lasso,
    Wand,
    Bucket,
    Text,
    Eyedropper
};

enum class CanvasWandReference
{
    ActiveLayer,
    ReferenceLayers,
    AllVisibleLayers
};

enum class CanvasFillComparison
{
    AlphaBoundary,
    Color
};

enum class CanvasSelectionShape
{
    Freehand,
    Rectangle,
    Ellipse
};

enum class CanvasLassoMode
{
    Select,
    Paint
};

enum class CanvasSelectionCombine
{
    Replace,
    Add,
    Subtract
};

}
