// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::io::{Cursor, Read};

use ugu_core::document::{Document, Group, Layer, LayerId, LayerKind};
use ugu_core::ops::{
    Affine, AssetId, Blend, MaskId, Motion, MotionStyle, Op, PaintLayer, Rgba8, Sampling, Section,
    StrokeId, Wobble,
};
use ugu_core::store::{Asset, Brush, BrushEngine, Mask, Point, Stroke};

use sha2::{Digest, Sha256};

use crate::format::*;
use crate::read::{ReadError, read};
use crate::write::{strokes_bytes, write};

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
        // Every field away from its default.
        brush: Brush {
            engine: BrushEngine::Spray,
            tip: ugu_core::store::TipShape::Square,
            opacity: 0.75,
            flow: 0.5,
            hardness: 0.5,
            spacing: 0.3,
            scatter: 1.2,
            particle_size: 0.2,
            density: 2.0,
            size_dynamics: 0.6,
            opacity_dynamics: 0.4,
            size_jitter: 0.7,
            animated_jitter: true,
            wobble_scale: 1.5,
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
        let mut stroke = stroke(id as f32, u64::MAX - u64::from(id));
        // Strokes of other engines, the pixel brush's included.
        stroke.brush.engine = [
            BrushEngine::Spray,
            BrushEngine::Airbrush,
            BrushEngine::Pixel,
            BrushEngine::Spray,
        ][id as usize];
        store.strokes.insert(StrokeId(id), stroke);
    }
    store.masks.insert(
        MaskId(0),
        Mask {
            bounds: [1, 2, 10, 2],
            bits: vec![0xff, 0xc0, 0x80, 0x40].into(),
        },
    );
    let png: Vec<u8> = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
    let asset = AssetId(Sha256::digest(&png).into());
    store.assets.insert(
        asset,
        Asset {
            size: [2, 2],
            png: png.into(),
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
            wobble: Some(Wobble {
                amount: 2.0,
                motion: Motion {
                    style: MotionStyle::Stepped,
                    poses: 5,
                    detail: 3,
                    linked: 0.25,
                    randomness: 0.75,
                    broken: true,
                    break_amount: 0.5,
                    break_range: 10.0,
                },
            }),
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
    let image = image_entry(&hex(&Sha256::digest([0x89, b'P', b'N', b'G', 1, 2, 3])));
    assert_eq!(names, [MANIFEST, DOCUMENT, STROKES, MASKS, image.as_str()]);
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
        strokes_bytes([(StrokeId(7), &points[..])].into_iter()),
        [
            b'U', b'G', b'S', 0, // magic
            1, 0, // version
            0, 0, // flags
            1, 0, 0, 0, // stroke count
            7, 0, 0, 0, // id
            1, 0, 0, 0, // point count
            0x00, 0x00, 0x80, 0x3f, // 1.0
            0x00, 0x00, 0x00, 0xc0, // -2.0
            0x00, 0x00, 0x00, 0x3f, // 0.5
        ]
    );
}

/// The sample as it reads back: stroke 2, which nothing used, is not saved.
fn sample_as_saved() -> Document {
    let mut document = sample();
    document.store.strokes.remove(&StrokeId(2));
    document
}

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..archive.len())
        .map(|index| {
            let mut entry = archive.by_index(index).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            (entry.name().to_owned(), data)
        })
        .collect()
}

fn archive(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        zip.start_file(name.as_str(), options).unwrap();
        std::io::Write::write_all(&mut zip, data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// The sample with one entry changed by `edit`.
fn with_entry(name: &str, edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut all = entries(&written(&sample()));
    let entry = all.iter_mut().find(|(entry, _)| entry == name).unwrap();
    edit(&mut entry.1);
    archive(&all)
}

fn with_json(name: &str, from: &str, to: &str) -> Vec<u8> {
    with_entry(name, |data| {
        let text = String::from_utf8(data.clone()).unwrap();
        assert!(text.contains(from), "{from} not in {text}");
        *data = text.replacen(from, to, 1).into_bytes();
    })
}

fn read_bytes(bytes: Vec<u8>) -> Result<(Document, [u8; 16]), ReadError> {
    read(Cursor::new(bytes))
}

#[test]
fn a_written_document_reads_back_the_same() {
    let (document, id) = read_bytes(written(&sample())).unwrap();
    assert_eq!(id, ID);
    assert_eq!(document, sample_as_saved());
    // And writing it again gives the same file.
    assert_eq!(written(&document), written(&sample()));
}

#[test]
fn files_that_are_not_documents_are_refused() {
    assert!(matches!(
        read_bytes(b"not a zip".to_vec()),
        Err(ReadError::NotAnArchive(_))
    ));
    let bytes = written(&sample());
    assert!(read_bytes(bytes[..bytes.len() / 2].to_vec()).is_err());
    let other = with_json(MANIFEST, "ugurugu-document", "something-else");
    assert!(matches!(read_bytes(other), Err(ReadError::Corrupt(_))));
}

#[test]
fn files_from_a_newer_version_are_refused_as_newer() {
    for (from, to) in [
        (r#""schema":1"#, r#""schema":2"#),
        (r#""render_revision":1"#, r#""render_revision":2"#),
        (r#""required":[]"#, r#""required":["layers-v2"]"#),
    ] {
        assert!(
            matches!(
                read_bytes(with_json(MANIFEST, from, to)),
                Err(ReadError::Newer(_))
            ),
            "{to}"
        );
    }
}

#[test]
fn missing_extra_and_unknown_entries_are_refused() {
    let mut all = entries(&written(&sample()));
    all.retain(|(name, _)| name != STROKES);
    assert!(matches!(
        read_bytes(archive(&all)),
        Err(ReadError::Corrupt(_))
    ));
    let mut all = entries(&written(&sample()));
    all.push(("strokes/9.bin".to_owned(), Vec::new()));
    assert!(matches!(
        read_bytes(archive(&all)),
        Err(ReadError::Corrupt(_))
    ));
    // A stroke whose points are stored under another id.
    let renamed = with_json(DOCUMENT, r#""id":3,"color""#, r#""id":4,"color""#);
    assert!(matches!(read_bytes(renamed), Err(ReadError::Corrupt(_))));
    let mut all = entries(&written(&sample()));
    all.push(("strokes/9.bin".to_owned(), Vec::new()));
    assert!(matches!(
        read_bytes(archive(&all)),
        Err(ReadError::Corrupt(_))
    ));
    let mut all = entries(&written(&sample()));
    all.push(("../evil.txt".to_owned(), Vec::new()));
    assert!(matches!(
        read_bytes(archive(&all)),
        Err(ReadError::Corrupt(_))
    ));
}

#[test]
fn broken_binary_entries_are_refused() {
    // One point too few for its count.
    let short = with_entry(STROKES, |data| data.truncate(data.len() - 12));
    assert!(matches!(read_bytes(short), Err(ReadError::Corrupt(_))));
    // A point that is not a number.
    // The first point of the first stroke, after the header, id and count.
    let nan = with_entry(STROKES, |data| {
        data[20..24].copy_from_slice(&f32::NAN.to_le_bytes())
    });
    assert!(matches!(read_bytes(nan), Err(ReadError::Invalid(_))));
    // Mask bounds that do not match the bits that follow.
    // The width of the first mask, after the header, id, left and top.
    let mask = with_entry(MASKS, |data| {
        data[24..28].copy_from_slice(&5i32.to_le_bytes())
    });
    assert!(matches!(read_bytes(mask), Err(ReadError::Corrupt(_))));
    // An image whose bytes no longer match its name.
    let name = image_entry(&hex(&Sha256::digest([0x89, b'P', b'N', b'G', 1, 2, 3])));
    let image = with_entry(&name, |data| data[4] ^= 1);
    assert!(matches!(read_bytes(image), Err(ReadError::Corrupt(_))));
}

#[test]
fn unexpected_json_is_refused() {
    let unknown = with_json(DOCUMENT, r#""frames":30"#, r#""frames":30,"extra":1"#);
    assert!(matches!(read_bytes(unknown), Err(ReadError::Corrupt(_))));
    let both = with_json(
        DOCUMENT,
        r#""reference":false,"paint":{"#,
        r#""reference":false,"group":{"opacity":1.0,"blend":"normal","clip_to_below":false,"children":[]},"paint":{"#,
    );
    assert!(matches!(read_bytes(both), Err(ReadError::Corrupt(_))));
    let invalid = with_json(DOCUMENT, r#""frames":30"#, r#""frames":999"#);
    assert!(matches!(read_bytes(invalid), Err(ReadError::Invalid(_))));
}

#[test]
fn an_entry_that_inflates_past_its_limit_is_refused() {
    // A few kilobytes that inflate to more than the manifest limit.
    let bomb = with_entry(MANIFEST, |data| {
        data.resize(crate::read::limits::MANIFEST_BYTES as usize * 4, b' ');
    });
    assert!(bomb.len() < 64 * 1024);
    assert!(matches!(read_bytes(bomb), Err(ReadError::TooLarge(_))));
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

    /// Damaged files never crash the reader, and whatever it accepts is
    /// a valid document.
    #[test]
    fn damaged_files_are_refused_or_valid(flips in proptest::collection::vec((0usize..1_000_000, 1u8..=255), 1..8), cut in 0usize..1_000_000) {
        let mut bytes = written(&sample());
        for (at, mask) in flips {
            let at = at % bytes.len();
            bytes[at] ^= mask;
        }
        let keep = bytes.len() - cut % (bytes.len() / 4 + 1);
        bytes.truncate(keep);
        if let Ok((document, _)) = read_bytes(bytes) {
            proptest::prop_assert_eq!(document.validate(), Ok(()));
        }
    }
}

#[test]
fn files_of_earlier_versions_are_recognised() {
    use crate::read::Legacy;
    // How a 2.2.13 .ugu begins.
    let old_ugu = b"{\r\n    \"schemaVersion\": 1,\r\n    \"algorithmVersion\": 1,".to_vec();
    assert!(matches!(
        read_bytes(old_ugu),
        Err(ReadError::Legacy(Legacy::Json))
    ));
    let bom = b"\xef\xbb\xbf  {\"name\": \"preset\"}".to_vec();
    assert!(matches!(
        read_bytes(bom),
        Err(ReadError::Legacy(Legacy::Json))
    ));
    let mut wawa = b"\x04WAWA".to_vec();
    wawa.extend_from_slice(&10i32.to_le_bytes());
    assert!(matches!(
        read_bytes(wawa),
        Err(ReadError::Legacy(Legacy::Wawa))
    ));
    assert!(matches!(
        read_bytes(b"GIF89a".to_vec()),
        Err(ReadError::NotAnArchive(_))
    ));
}
