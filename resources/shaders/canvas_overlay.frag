// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#version 440

layout(location = 0) in vec2 v_texCoord;

layout(location = 0) out vec4 fragColor;

layout(binding = 1) uniform sampler2D overlayTexture;

void main()
{
    fragColor = texture(overlayTexture, v_texCoord);
}
