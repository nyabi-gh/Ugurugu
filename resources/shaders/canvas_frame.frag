// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#version 440

layout(location = 0) in vec2 v_texCoord;
layout(location = 1) in vec2 v_widgetPos;

layout(location = 0) out vec4 fragColor;

layout(std140, binding = 0) uniform buf
{
    mat4 mvp;
    vec2 checkerOrigin;
    float checkerSize;
    float frameValid;
} u;

layout(binding = 1) uniform sampler2D frameTexture;

void main()
{
    vec2 cell = floor((v_widgetPos - u.checkerOrigin) / u.checkerSize);
    float dark = mod(cell.x + cell.y, 2.0);
    vec3 checker = mix(vec3(238.0 / 255.0), vec3(210.0 / 255.0), dark);
    vec4 frame = u.frameValid > 0.5
        ? texture(frameTexture, v_texCoord)
        : vec4(0.0);
    fragColor = vec4(frame.rgb + checker * (1.0 - frame.a), 1.0);
}
