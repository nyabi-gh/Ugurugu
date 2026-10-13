// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The fixtures are written by 2.2.13's `ugurugu_fixture_generator`
//! (tools/FixtureGenerator.cpp); their frames are compared with 2.2.13's
//! by `ugu-doc compare`.

use super::*;

const MOTION: &[u8] = include_bytes!("../../../../tests/fixtures/ugu13/motion.ugu");
const FILL: &[u8] = include_bytes!("../../../../tests/fixtures/ugu13/fill.ugu");
const EDIT: &[u8] = include_bytes!("../../../../tests/fixtures/ugu13/edit.ugu");
const DEPTH: &[u8] = include_bytes!("../../../../tests/fixtures/ugu13/depth.ugu");
const PROCEDURAL: &[u8] = include_bytes!("../../../../tests/fixtures/ugu13/procedural.ugu");
const REFRAME_IN_SECTION: &[u8] =
    include_bytes!("../../../../tests/fixtures/ugu13/reframe-in-section.ugu");

fn paint<'a>(document: &'a Document, name: &str) -> &'a PaintLayer {
    fn find<'a>(layers: &'a [Layer], name: &str) -> Option<&'a PaintLayer> {
        layers.iter().find_map(|layer| match &layer.kind {
            LayerKind::Paint(paint) if layer.name == name => Some(paint),
            LayerKind::Group(group) => find(&group.children, name),
            _ => None,
        })
    }
    find(&document.layers, name).expect("layer")
}

fn edited(bytes: &[u8], change: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut root: serde_json::Value = serde_json::from_slice(bytes).expect("json");
    change(&mut root);
    serde_json::to_vec(&root).expect("json")
}

#[test]
fn every_motion_style_and_layer_wobble_comes_across() {
    let imported = import(MOTION).expect("imports");
    let document = &imported.document;
    assert_eq!(document.canvas, [480, 360]);
    assert_eq!(document.frames, 12);
    assert_eq!(document.wobble, Wobble::classic(2.4));
    assert_eq!(paint(document, "Presets").wobble, None);
    let smooth = paint(document, "Smooth").wobble.expect("own wobble");
    assert_eq!(smooth.amount, 3.0);
    assert_eq!(smooth.motion.style, MotionStyle::Smooth);
    assert_eq!(smooth.motion.poses, 5);
    assert_eq!(
        paint(document, "Stepped").wobble.expect("own").motion.style,
        MotionStyle::Stepped
    );
    assert!(paint(document, "Broken").wobble.expect("own").motion.broken);
    assert_eq!(paint(document, "Still").wobble, Some(Wobble::classic(0.0)));
    let flags: Vec<_> = document
        .layers
        .iter()
        .map(|layer| (layer.name.as_str(), layer.visible, layer.reference))
        .collect();
    assert!(flags.contains(&("Reference", true, true)));
    assert!(flags.contains(&("Hidden", false, false)));
    // The first layer, active when saved.
    assert_eq!(imported.active, Some(document.layers[0].id));
    let brushes: Vec<_> = document
        .store
        .strokes
        .values()
        .map(|stroke| (stroke.brush.engine, stroke.brush.tip))
        .collect();
    for brush in [
        (BrushEngine::Spray, TipShape::Round),
        (BrushEngine::Airbrush, TipShape::Round),
        (BrushEngine::Line, TipShape::Square),
    ] {
        assert!(brushes.contains(&brush), "{brush:?}");
    }
}

#[test]
fn fills_and_clipped_strokes_keep_their_masks() {
    let document = import(FILL).expect("imports").document;
    let fills: Vec<_> = paint(&document, "Fills")
        .ops
        .iter()
        .map(|op| match op {
            Op::Fill {
                antialias, clip, ..
            } => (*antialias, clip.is_some()),
            other => panic!("{other:?}"),
        })
        .collect();
    // The schema-5 fill mask always spreads by a pixel, as in 2.2.13.
    assert_eq!(
        fills,
        [(false, false), (true, false), (true, true), (true, false)]
    );
    let clipped = &paint(&document, "Clipped").ops;
    assert_eq!(clipped.len(), 8);
    assert!(clipped.iter().all(|op| matches!(
        op,
        Op::Paint { clip: Some(_), .. } | Op::Erase { clip: Some(_), .. }
    )));
    // A rectangle alone is a mask of that rectangle within the canvas.
    let Op::Paint {
        clip: Some(rectangle),
        ..
    } = clipped[1]
    else {
        panic!("{:?}", clipped[1]);
    };
    let mask = &document.store.masks[&rectangle];
    assert_eq!(mask.bounds, [107, 40, 150, 140]);
}

#[test]
fn merged_layers_become_sections_and_edits_keep_their_order() {
    let document = import(EDIT).expect("imports").document;
    assert_eq!(document.canvas, [300, 225]);
    let base = paint(&document, "Base");
    let kinds: Vec<&str> = base
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Paint { .. } | Op::Erase { .. } => None,
            Op::TransformSelection {
                keep_source: true, ..
            } => Some("copy"),
            Op::TransformSelection { .. } => Some("move"),
            Op::ClearSelection { .. } => Some("clear"),
            Op::Crop { .. } => Some("crop"),
            Op::Resample { .. } => Some("resample"),
            Op::Isolated(section) => {
                assert_eq!(section.opacity, 1.0);
                assert_eq!(section.wobble, base.wobble);
                Some("section")
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "copy", "section", "section", "move", "clear", "crop", "resample"
        ]
    );
    assert_eq!(base.initial_size, [360, 270]);
    assert!(matches!(
        paint(&document, "Image").ops[..2],
        [
            Op::PlaceImage {
                sampling: Sampling::Smooth,
                ..
            },
            Op::PlaceImage {
                sampling: Sampling::Nearest,
                ..
            }
        ]
    ));
    let LayerKind::Group(group) = &document.layers[2].kind else {
        panic!("{:?}", document.layers[2].name);
    };
    assert_eq!(group.blend, Blend::Overlay);
    let blends: Vec<_> = group
        .children
        .iter()
        .map(|layer| (layer.blend(), layer.clip_to_below()))
        .collect();
    assert_eq!(
        blends,
        [
            (Blend::Multiply, false),
            (Blend::Screen, true),
            (Blend::Normal, true),
            (Blend::Overlay, false)
        ]
    );
}

#[test]
fn groups_open_as_deep_as_2_2_13_makes_them() {
    let document = import(DEPTH).expect("imports").document;
    let mut depth = 1;
    let mut layers = &document.layers;
    while let Some(Layer {
        kind: LayerKind::Group(group),
        ..
    }) = layers.last()
    {
        depth += 1;
        layers = &group.children;
    }
    assert_eq!(depth, 9);
}

#[test]
fn what_3_0_cannot_show_alike_is_refused() {
    assert!(matches!(
        import(PROCEDURAL),
        Err(LegacyError::Refused(Refusal::ProceduralFill))
    ));
    assert!(matches!(
        import(REFRAME_IN_SECTION),
        Err(LegacyError::Refused(Refusal::CanvasChangeInMerge))
    ));
    let named = edited(FILL, |root| {
        root["canvas"]["background"] = "white".into();
    });
    assert!(matches!(
        import(&named),
        Err(LegacyError::Refused(Refusal::Color(text))) if text == "white"
    ));
}

#[test]
fn other_versions_and_damage_are_told_apart() {
    for (schema, older) in [(12, true), (1, true), (14, false)] {
        let other = edited(FILL, |root| root["schemaVersion"] = schema.into());
        match import(&other) {
            Err(LegacyError::Older(found)) => assert!(older && found == schema),
            Err(LegacyError::Newer(found)) => assert!(!older && found == schema),
            other => panic!("{other:?}"),
        }
    }
    let damaged_cases: [&dyn Fn(&mut serde_json::Value); 6] = [
        &|root| root["binaryMasks"][0]["data"] = "AAAA*".into(),
        &|root| root["binaryMasks"][0]["data"] = "AAAJxA==".into(),
        &|root| root["layers"][0]["blendMode"] = "dodge".into(),
        &|root| root["layers"][1]["parentGroupId"] = root["layers"][0]["id"].clone(),
        &|root| root["layers"][1]["strokes"][0]["mode"] = "smudge".into(),
        &|root| root["layers"][2]["strokes"][0]["seed"] = "-1".into(),
    ];
    for change in damaged_cases {
        let file = edited(FILL, change);
        assert!(
            matches!(import(&file), Err(LegacyError::Damaged(_))),
            "{:?}",
            import(&file)
        );
    }
    assert!(matches!(
        import(b"{\"schemaVersion\": 13"),
        Err(LegacyError::Damaged(_))
    ));
}

#[test]
fn colours_are_read_as_2_2_13_writes_them() {
    assert_eq!(color("#80102030").ok(), Some([0x10, 0x20, 0x30, 0x80]));
    assert_eq!(color("#102030").ok(), Some([0x10, 0x20, 0x30, 255]));
    for text in ["#1020304", "102030", "#10203g", "#ffé0000"] {
        assert!(color(text).is_err(), "{text}");
    }
}

#[test]
fn base64_is_decoded_strictly() {
    assert_eq!(decode_base64("").as_deref(), Some(&b""[..]));
    assert_eq!(decode_base64("TWFu").as_deref(), Some(&b"Man"[..]));
    assert_eq!(decode_base64("TWE=").as_deref(), Some(&b"Ma"[..]));
    assert_eq!(decode_base64("TQ==").as_deref(), Some(&b"M"[..]));
    for bad in ["TQ=", "T===", "TQ==TWFu", "TW-u"] {
        assert_eq!(decode_base64(bad), None, "{bad}");
    }
}

#[test]
fn a_fill_mask_used_twice_is_stored_once() {
    let masks = |bytes: &[u8]| import(bytes).expect("imports").document.store.masks.len();
    let twice = edited(FILL, |root| {
        let layers = root["layers"].as_array_mut().expect("layers");
        let fills = layers
            .iter_mut()
            .find(|layer| layer["name"] == "Fills")
            .expect("fills");
        let strokes = fills["strokes"].as_array_mut().expect("strokes");
        let mut copy = strokes
            .iter()
            .find(|stroke| stroke.get("fillMaskId").is_some())
            .expect("a schema-5 fill")
            .clone();
        copy["id"] = "copy".into();
        strokes.push(copy);
    });
    assert_eq!(masks(&twice), masks(FILL));
}

#[test]
fn an_import_stops_at_a_limit_before_reading_the_rest() {
    let bytes = edited(FILL, |root| {
        let layers = root["layers"].as_array_mut().expect("layers");
        let lines = layers
            .iter_mut()
            .find(|layer| layer["name"] == "Lines")
            .expect("lines");
        let strokes = lines["strokes"].as_array_mut().expect("strokes");
        let mut one = strokes[0].clone();
        one["points"] = serde_json::json!([[10.0, 10.0, 1.0]]);
        strokes.clear();
        for index in 0..=ugu_core::document::limits::OPERATIONS {
            let mut stroke = one.clone();
            stroke["id"] = index.to_string().into();
            strokes.push(stroke);
        }
        let mut broken = one.clone();
        broken["mode"] = "nonsense".into();
        strokes.push(broken);
    });
    assert!(matches!(
        import(&bytes),
        Err(LegacyError::Invalid(DocumentError::TooManyOperations(_)))
    ));
}
