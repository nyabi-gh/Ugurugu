// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#version 440

layout(location = 0) in vec2 position;
layout(location = 1) in vec2 texCoord;

layout(location = 0) out vec2 v_texCoord;
layout(location = 1) out vec2 v_widgetPos;

layout(std140, binding = 0) uniform buf
{
    mat4 mvp;
    vec2 checkerOrigin;
    float checkerSize;
    float frameValid;
} u;

void main()
{
    v_texCoord = texCoord;
    v_widgetPos = position;
    gl_Position = u.mvp * vec4(position, 0.0, 1.0);
}
