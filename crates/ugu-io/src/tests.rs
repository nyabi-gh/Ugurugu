// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::io::{Cursor, Read};

use ugu_core::document::{Document, Group, Layer, LayerId, LayerKind};
use ugu_core::ops::{
    Affine, AssetId, Blend, MaskId, Op, PaintLayer, Rgba8, Sampling, Section, StrokeId, Wobble,
};
use ugu_core::store::{Asset, Brush, BrushEngine, Mask, Point, Stroke};

use crate::format::*;
use crate::write::{stroke_bytes, write};

const ID: [u8; 16] = [0xab; 16];

pub(crate) fn stroke(x: f32, seed: u64) -> Stroke {
    Stroke {
        points: vec![
            Point {
                x,
                y: 2.5,
                pressure: 0.25,
            },
            Point {
                x: x + 1.0,
                y: 3.0,
                pressure: 1.0,
            },
        ]
        .into(),
        color: Rgba8([10, 20, 30, 255]),
        width: 4.5,
        brush: Brush {
            engine: BrushEngine::Spray,
            opacity: 0.75,
            hardness: 0.5,
            antialias: true,
        },
        seed,
    }
}

/// A document using every kind of layer, operation and stored data, plus a
/// stroke no operation refers to.
pub(crate) fn sample() -> Document {
    let mut document = Document::new([64, 48]);
    let store = &mut document.store;
    for id in 0..4 {
        store
            .strokes
            .insert(StrokeId(id), stroke(id as f32, u64::MAX - u64::from(id)));
    }
    store.masks.insert(
        MaskId(0),
        Mask {
            bounds: [1, 2, 10, 2],
            bits: vec![0xff, 0xc0, 0x80, 0x40].into(),
        },
    );
    let asset = AssetId([7; 32]);
    store.assets.insert(
        asset,
        Asset {
            size: [2, 2],
            png: vec![0x89, b'P', b'N', b'G', 1, 2, 3].into(),
        },
    );
    let LayerKind::Paint(base) = &mut document.layers[0].kind else {
        unreachable!()
    };
    base.ops = vec![
        Op::Paint {
            stroke: StrokeId(0),
            clip: Some(MaskId(0)),
        },
        Op::Fill {
            coverage: MaskId(0),
            color: Rgba8([1, 2, 3, 4]),
            antialias: true,
            clip: None,
        },
        Op::PlaceImage {
            asset,
            transform: Affine::translation(3.0, -1.5),
            sampling: Sampling::Smooth,
        },
        Op::TransformSelection {
            mask: MaskId(0),
            transform: Affine([0.5, 0.0, 1.0, 0.0, 0.5, 2.0]),
            sampling: Sampling::Nearest,
            keep_source: true,
        },
        Op::ClearSelection { mask: MaskId(0) },
        Op::Isolated(Box::new(Section {
            ops: vec![Op::Erase {
                stroke: StrokeId(1),
                clip: None,
            }],
            opacity: 0.5,
            wobble: Some(Wobble { amount: 2.0 }),
        })),
        Op::Crop {
            offset: [-2, 3],
            size: [70, 50],
        },
        Op::Resample {
            size: [64, 48],
            sampling: Sampling::Smooth,
        },
    ];
    base.initial_size = [64, 48];
    document.layers.push(Layer {
        id: LayerId(2),
        name: "그룹".to_owned(),
        visible: false,
        reference: true,
        kind: LayerKind::Group(Group {
            opacity: 0.25,
            blend: Blend::Overlay,
            clip_to_below: true,
            children: vec![Layer {
                id: LayerId(3),
                name: "Inner".to_owned(),
                visible: true,
                reference: false,
                kind: LayerKind::Paint(PaintLayer {
                    ops: vec![Op::Paint {
                        stroke: StrokeId(3),
                        clip: None,
                    }],
                    opacity: 0.9,
                    blend: Blend::Multiply,
                    clip_to_below: true,
                    wobble: None,
                    initial_size: [64, 48],
                }),
            }],
        }),
    });
    // Stroke 2 is kept for undo but used by no operation.
    assert_eq!(document.validate(), Ok(()));
    document
}

fn written(document: &Document) -> Vec<u8> {
    write(document, ID, Cursor::new(Vec::new()))
        .unwrap()
        .into_inner()
}

#[test]
fn the_same_document_writes_the_same_bytes() {
    let document = sample();
    assert_eq!(written(&document), written(&document.clone()));
}

#[test]
fn entries_come_in_a_fixed_order_with_only_used_data() {
    let bytes = written(&sample());
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let names: Vec<_> = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_owned())
        .collect();
    let image = image_entry(&hex(&[7; 32]));
    assert_eq!(
        names,
        [
            MANIFEST,
            DOCUMENT,
            "strokes/0.bin",
            "strokes/1.bin",
            "strokes/3.bin",
            "masks/0.bin",
            image.as_str()
        ]
    );
    assert_eq!(
        archive.by_name(&image).unwrap().compression(),
        zip::CompressionMethod::Stored
    );
    assert_eq!(
        archive.by_name(DOCUMENT).unwrap().compression(),
        zip::CompressionMethod::Deflated
    );

    let mut manifest = String::new();
    archive
        .by_name(MANIFEST)
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    assert_eq!(
        manifest,
        r#"{"format":"ugurugu-document","schema":1,"render_revision":1,"document_id":"abababababababababababababababab","required":[]}"#
    );
    let mut document = String::new();
    archive
        .by_name(DOCUMENT)
        .unwrap()
        .read_to_string(&mut document)
        .unwrap();
    assert!(
        document.contains(r#""seed":"ffffffffffffffff""#),
        "{document}"
    );
    assert!(
        !document.contains(r#""id":2,"color""#),
        "unused stroke 2 was written"
    );
}

#[test]
fn stroke_points_have_an_explicit_little_endian_layout() {
    let points = [Point {
        x: 1.0,
        y: -2.0,
        pressure: 0.5,
    }];
    assert_eq!(
        stroke_bytes(&points),
        [
            b'U', b'G', b'S', 0, // magic
            1, 0, // version
            0, 0, // flags
            1, 0, 0, 0, // count
            0x00, 0x00, 0x80, 0x3f, // 1.0
            0x00, 0x00, 0x00, 0xc0, // -2.0
            0x00, 0x00, 0x00, 0x3f, // 0.5
        ]
    );
}
