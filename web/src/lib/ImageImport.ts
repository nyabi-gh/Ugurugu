// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import type { MemoryProfile } from "./MemoryPolicy";

// Read dimensions before asking a browser codec to allocate the full raster.
// Only the advertised PNG/JPEG/WebP formats enter this path; SVG is excluded.
export function imageDimensions(bytes: Uint8Array): {
    width: number;
    height: number;
} {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const ascii = (start: number, length: number) =>
        String.fromCharCode(...bytes.subarray(start, start + length));
    if (
        bytes.length >= 24 &&
        view.getUint32(0) === 0x89504e47 &&
        view.getUint32(4) === 0x0d0a1a0a &&
        ascii(12, 4) === "IHDR"
    ) {
        return { width: view.getUint32(16), height: view.getUint32(20) };
    }
    if (bytes.length >= 4 && view.getUint16(0) === 0xffd8) {
        let offset = 2;
        while (offset + 4 <= bytes.length) {
            if (bytes[offset++] !== 0xff) break;
            while (bytes[offset] === 0xff) offset++;
            const marker = bytes[offset++]!;
            if (marker === 0xd9 || marker === 0xda) break;
            if (marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) continue;
            if (offset + 2 > bytes.length) break;
            const length = view.getUint16(offset);
            if (length < 2 || offset + length > bytes.length) break;
            if (
                [
                    0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb,
                    0xcd, 0xce, 0xcf,
                ].includes(marker) &&
                length >= 8
            ) {
                return {
                    width: view.getUint16(offset + 5),
                    height: view.getUint16(offset + 3),
                };
            }
            offset += length;
        }
    }
    if (
        bytes.length >= 30 &&
        ascii(0, 4) === "RIFF" &&
        ascii(8, 4) === "WEBP"
    ) {
        const type = ascii(12, 4);
        const uint24 = (at: number) =>
            bytes[at]! + bytes[at + 1]! * 256 + bytes[at + 2]! * 65536;
        if (type === "VP8X")
            return { width: uint24(24) + 1, height: uint24(27) + 1 };
        if (type === "VP8 " && ascii(23, 3) === "\x9d\x01\x2a") {
            return {
                width: view.getUint16(26, true) & 0x3fff,
                height: view.getUint16(28, true) & 0x3fff,
            };
        }
        if (type === "VP8L" && bytes[20] === 0x2f) {
            const bits = view.getUint32(21, true);
            return {
                width: (bits & 0x3fff) + 1,
                height: ((bits >>> 14) & 0x3fff) + 1,
            };
        }
    }
    throw new Error("Choose a valid PNG, JPEG, or WebP image.");
}

export async function decodeImage(file: File, profile: MemoryProfile) {
    const maximumBytes = (profile.name === "mobile" ? 8 : 16) * 1024 * 1024;
    const maximumEdge = profile.name === "mobile" ? 2048 : 4096;
    if (!file.size || file.size > maximumBytes) {
        throw new Error(
            `Choose an image smaller than ${maximumBytes / 1024 / 1024} MiB.`,
        );
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    const dimensions = imageDimensions(bytes);
    const validateSize = (width: number, height: number) => {
        if (!width || !height || width > maximumEdge || height > maximumEdge) {
            throw new Error(
                `Images can be up to ${maximumEdge} × ${maximumEdge} pixels on this device.`,
            );
        }
    };
    validateSize(dimensions.width, dimensions.height);
    const bitmap = await createImageBitmap(new Blob([bytes]));
    try {
        validateSize(bitmap.width, bitmap.height);
        if (
            bitmap.width * bitmap.height !==
            dimensions.width * dimensions.height
        ) {
            throw new Error(
                "The decoded image size does not match its header.",
            );
        }
        const canvas = document.createElement("canvas");
        canvas.width = bitmap.width;
        canvas.height = bitmap.height;
        const context = canvas.getContext("2d", { willReadFrequently: true });
        if (!context) throw new Error("The image could not be decoded.");
        context.drawImage(bitmap, 0, 0);
        const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height)
            .data.buffer;
        canvas.width = canvas.height = 1;
        return { pixels, width: bitmap.width, height: bitmap.height };
    } finally {
        bitmap.close();
    }
}
