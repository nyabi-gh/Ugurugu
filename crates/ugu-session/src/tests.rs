// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use super::*;
use ugu_core::ops::{Affine, MaskId, Op, Rgba8, Sampling};
use ugu_core::restyle::Style;
use ugu_core::selection::Combine;
use ugu_core::store::BrushEngine;

fn at(x: f64, y: f64, time: f64) -> InputPoint {
    InputPoint {
        position: [x, y],
        pressure: None,
        time,
    }
}

fn session() -> Session {
    Session::new(Document::new([200, 100]), false)
}

fn draw(session: &mut Session, tools: &Tools, from: f64, to: f64) -> Outcome {
    session.begin_stroke(tools, at(from, 50.0, 0.0)).unwrap();
    let mut x = from;
    let mut time = 0.0;
    while x + 2.0 < to {
        x += 2.0;
        time += 8.0;
        session.extend_stroke(at(x, 50.0, time));
    }
    session.end_stroke(at(to, 50.0, time + 8.0)).unwrap()
}

fn ops(session: &Session, layer: LayerId) -> Vec<Op> {
    match &session.document().layer(layer).unwrap().kind {
        LayerKind::Paint(paint) => paint.ops.clone(),
        LayerKind::Group(_) => unreachable!(),
    }
}

#[test]
fn a_stroke_is_one_undo_step() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    assert!(!session.is_dirty() || session.undo_label().is_none());
    assert!(matches!(
        draw(&mut session, &tools, 10.0, 60.0),
        Outcome::Committed(_)
    ));
    assert_eq!(ops(&session, layer).len(), 1);
    assert_eq!(session.undo_label(), Some("Draw"));
    assert!(session.undo().unwrap());
    assert!(ops(&session, layer).is_empty());
    assert!(session.document().store.strokes.is_empty());
    assert!(session.redo().unwrap());
    assert_eq!(ops(&session, layer).len(), 1);
}

#[test]
fn a_cancelled_stroke_leaves_nothing() {
    let mut session = session();
    let tools = Tools::default();
    let revision = session.revision();
    session.begin_stroke(&tools, at(10.0, 10.0, 0.0)).unwrap();
    session.extend_stroke(at(30.0, 10.0, 8.0));
    session.cancel_stroke();
    assert!(session.live().is_none());
    assert_eq!(session.revision(), revision);
    assert_eq!(
        session.end_stroke(at(40.0, 10.0, 16.0)).unwrap(),
        Outcome::NoChange
    );
}

#[test]
fn without_smoothing_the_stroke_keeps_the_input_and_ends_where_the_pen_lifted() {
    let mut session = session();
    let tools = Tools::default();
    draw(&mut session, &tools, 10.0, 61.0);
    let stroke = session.document().store.strokes.values().next().unwrap();
    let xs: Vec<f32> = stroke.points.iter().map(|point| point.x).collect();
    assert_eq!(xs[0], 10.0);
    assert_eq!(*xs.last().unwrap(), 61.0);
    assert!(xs.windows(2).all(|pair| pair[1] > pair[0]));
    // A mouse has no pressure and draws at full pressure.
    assert!(stroke.points.iter().all(|point| point.pressure == 1.0));
}

#[test]
fn smoothing_lags_behind_but_the_stroke_still_ends_at_the_lift() {
    let mut session = session();
    let mut tools = Tools::default();
    tools.plain.pen.stabilizer = 1.0;
    session.begin_stroke(&tools, at(0.0, 50.0, 0.0)).unwrap();
    for step in 1..=40 {
        let jitter = if step % 2 == 0 { 3.0 } else { -3.0 };
        session.extend_stroke(at(
            f64::from(step) * 4.0,
            50.0 + jitter,
            f64::from(step) * 8.0,
        ));
    }
    let live = session.live().unwrap();
    assert!(live.points.iter().all(|point| (point.y - 50.0).abs() < 2.0));
    session.end_stroke(at(160.0, 53.0, 330.0)).unwrap();
    let stroke = session.document().store.strokes.values().next().unwrap();
    let last = stroke.points.last().unwrap();
    assert_eq!([last.x, last.y], [160.0, 53.0]);
}

#[test]
fn pen_pressure_is_kept_and_carried_to_the_lift() {
    let mut session = session();
    let tools = Tools::default();
    let pressed = |x: f64, pressure: f32, time: f64| InputPoint {
        position: [x, 20.0],
        pressure: Some(pressure),
        time,
    };
    session
        .begin_stroke(&tools, pressed(10.0, 0.0, 0.0))
        .unwrap();
    session.extend_stroke(pressed(20.0, 0.4, 8.0));
    session.extend_stroke(pressed(30.0, 0.7, 16.0));
    session.end_stroke(pressed(40.0, 0.0, 24.0)).unwrap();
    let stroke = session.document().store.strokes.values().next().unwrap();
    let pressures: Vec<f32> = stroke.points.iter().map(|point| point.pressure).collect();
    // The first touch never reaches zero width; the lift carries 0.7.
    assert_eq!(pressures, [0.05, 0.4, 0.7, 0.7]);
}

#[test]
fn with_pressure_off_a_stroke_is_at_full_pressure_from_its_start() {
    let mut session = session();
    let mut tools = Tools::default();
    let pressed = |x: f64, pressure: f32, time: f64| InputPoint {
        position: [x, 20.0],
        pressure: Some(pressure),
        time,
    };
    let pressures = |session: &Session| -> Vec<Vec<f32>> {
        let mut all: Vec<Vec<f32>> = session
            .document()
            .store
            .strokes
            .values()
            .map(|stroke| stroke.points.iter().map(|point| point.pressure).collect())
            .collect();
        all.sort_by(|a, b| a[0].total_cmp(&b[0]));
        all
    };
    tools.uses_pressure = false;
    session
        .begin_stroke(&tools, pressed(10.0, 0.2, 0.0))
        .unwrap();
    // Turning it back on mid-stroke changes the next stroke, as in 2.2.13.
    tools.uses_pressure = true;
    session.extend_stroke(pressed(20.0, 0.4, 8.0));
    session.end_stroke(pressed(30.0, 0.0, 16.0)).unwrap();
    assert_eq!(pressures(&session), [[1.0, 1.0, 1.0]]);
    session
        .begin_stroke(&tools, pressed(10.0, 0.3, 24.0))
        .unwrap();
    session.end_stroke(pressed(30.0, 0.0, 32.0)).unwrap();
    assert_eq!(pressures(&session)[0], [0.3, 0.3]);
}

#[test]
fn a_hidden_or_missing_layer_refuses_to_draw() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    session
        .update_layer(layer, "Hide layer", |layer| layer.visible = false)
        .unwrap();
    assert_eq!(
        session.begin_stroke(&tools, at(1.0, 1.0, 0.0)),
        Err(StrokeRefused::HiddenLayer)
    );
    session.select_layer(LayerId(99)).unwrap();
    assert_eq!(session.current_layer(), layer);
}

#[test]
fn a_layer_in_a_hidden_group_refuses_to_draw() {
    let mut document = Document::new([64, 64]);
    let inner = document.layers.remove(0);
    document.layers.push(ugu_core::document::Layer {
        id: LayerId(10),
        name: "Group".to_owned(),
        visible: false,
        reference: false,
        kind: LayerKind::Group(ugu_core::document::Group {
            opacity: 1.0,
            blend: ugu_core::ops::Blend::Normal,
            clip_to_below: false,
            children: vec![inner],
            cels: None,
            keys: Default::default(),
        }),
    });
    // The layer inside the group is current from the start.
    let mut session = Session::new(document, true);
    let tools = Tools::default();
    assert_eq!(session.current_layer(), LayerId(1));
    assert_eq!(
        session.begin_stroke(&tools, at(1.0, 1.0, 0.0)),
        Err(StrokeRefused::HiddenLayer)
    );
    session
        .update_layer(LayerId(10), "Show group", |layer| layer.visible = true)
        .unwrap();
    assert_eq!(session.begin_stroke(&tools, at(1.0, 1.0, 0.0)), Ok(()));
}

#[test]
fn the_eraser_commits_an_erase_operation() {
    let mut session = session();
    let mut tools = Tools::default();
    draw(&mut session, &tools, 10.0, 60.0);
    set_tool(&mut session, &mut tools, Tool::Eraser).unwrap();
    draw(&mut session, &tools, 20.0, 30.0);
    let layer = session.current_layer();
    assert!(matches!(ops(&session, layer)[1], Op::Erase { .. }));
    assert_eq!(session.undo_label(), Some("Erase"));
}

#[test]
fn each_preset_keeps_its_width_and_stabilizer_and_draws_with_its_brush() {
    let find = |id| ugu_core::brush::find(id).unwrap();
    let mut session = session();
    let mut tools = Tools::default();
    tools.plain.pen.width = 11.0;
    tools.plain.pen.antialias = true;
    tools.choose_preset(session.pixel_mode(), Tool::Pen, find("soft-airbrush"));
    assert_eq!(tools.plain.pen.width, find("soft-airbrush").size);
    tools.plain.pen.stabilizer = 0.4;
    draw(&mut session, &tools, 10.0, 60.0);
    let brush = |session: &Session| {
        let layer = session.current_layer();
        let Some(&Op::Paint { stroke, .. } | &Op::Erase { stroke, .. }) =
            ops(session, layer).last()
        else {
            unreachable!("a stroke was drawn");
        };
        session.document().store.strokes[&stroke].brush
    };
    // The brush tool's antialiasing goes with every brush preset.
    let airbrush = Brush {
        antialias: true,
        ..find("soft-airbrush").brush
    };
    assert_eq!(brush(&session), airbrush);
    tools.choose_preset(session.pixel_mode(), Tool::Pen, find("ink-pen"));
    assert_eq!(
        (tools.plain.pen.width, tools.plain.pen.stabilizer),
        (11.0, 0.0)
    );
    tools.choose_preset(session.pixel_mode(), Tool::Pen, find("soft-airbrush"));
    assert_eq!(
        (tools.plain.pen.width, tools.plain.pen.stabilizer),
        (find("soft-airbrush").size, 0.4)
    );
    // An eraser takes its preset's.
    set_tool(&mut session, &mut tools, Tool::Eraser).unwrap();
    tools.choose_preset(session.pixel_mode(), Tool::Eraser, find("kneaded-eraser"));
    draw(&mut session, &tools, 20.0, 30.0);
    assert_eq!(brush(&session), find("kneaded-eraser").brush);
}

#[test]
fn strokes_get_different_seeds() {
    let mut session = session();
    let tools = Tools::default();
    draw(&mut session, &tools, 10.0, 60.0);
    draw(&mut session, &tools, 10.0, 60.0);
    let mut seeds: Vec<u64> = session
        .document()
        .store
        .strokes
        .values()
        .map(|stroke| stroke.seed)
        .collect();
    seeds.dedup();
    assert_eq!(seeds.len(), 2);
}

#[test]
fn layer_commands_keep_a_valid_current_layer() {
    let mut session = session();
    let first = session.current_layer();
    session.add_layer().unwrap();
    let second = session.current_layer();
    assert_ne!(first, second);
    assert_eq!(session.document().layers[1].id, second);

    assert!(matches!(
        session.move_layer(-1).unwrap(),
        Outcome::Committed(_)
    ));
    assert_eq!(session.document().layers[0].id, second);
    assert_eq!(session.move_layer(-1).unwrap(), Outcome::NoChange);

    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(session.document().layers.len(), 1);
    assert_eq!(session.current_layer(), first);

    session.redo().unwrap();
    session.select_layer(second).unwrap();
    session.remove_layer().unwrap();
    assert_eq!(session.current_layer(), first);
    assert_eq!(session.remove_layer().unwrap(), Outcome::NoChange);
}

#[test]
fn merging_down_makes_the_lower_layer_current() {
    let mut session = session();
    let tools = Tools::default();
    let lower = session.current_layer();
    draw(&mut session, &tools, 10.0, 60.0);
    session.add_layer().unwrap();
    draw(&mut session, &tools, 10.0, 60.0);
    session.merge_down().unwrap();
    assert_eq!(session.current_layer(), lower);
    assert_eq!(session.document().layers.len(), 1);
    assert_eq!(session.undo_label(), Some("Merge down"));
}

#[test]
fn a_duplicate_draws_what_its_layer_does_right_above_it() {
    let mut session = session();
    let tools = Tools::default();
    let first = session.current_layer();
    draw(&mut session, &tools, 10.0, 60.0);
    session.add_layer().unwrap();
    let source = session.current_layer();
    draw(&mut session, &tools, 20.0, 80.0);
    session
        .update_layer(source, "Layer opacity", |layer| {
            if let LayerKind::Paint(paint) = &mut layer.kind {
                paint.opacity = 0.5;
            }
        })
        .unwrap();
    session.select_layer(first).unwrap();
    session.select_layer(source).unwrap();

    session
        .duplicate_layer(|name| format!("{name} copy"))
        .unwrap();
    let copy = session.current_layer();
    let layers = &session.document().layers;
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[2].id, copy);
    assert_eq!(layers[2].name, format!("{} copy", layers[1].name));
    assert_eq!(layers[2].kind, layers[1].kind);
    assert_eq!(ops(&session, copy), ops(&session, source));
    assert_eq!(session.undo_label(), Some("Duplicate layer"));

    // Drawing on the copy leaves the original as it was.
    draw(&mut session, &tools, 30.0, 90.0);
    assert_eq!(ops(&session, copy).len(), ops(&session, source).len() + 1);

    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(session.document().layers.len(), 2);
    assert_eq!(session.current_layer(), source);
}

#[test]
fn a_group_is_not_duplicated() {
    let mut session = session();
    session.add_group().unwrap();
    let group = session.document().layers[0].id;
    session.select_layer(group).unwrap();
    assert!(!session.can_duplicate_layer());
    assert_eq!(
        session.duplicate_layer(|name| name.to_owned()).unwrap(),
        Outcome::NoChange
    );
    assert_eq!(session.document().layers.len(), 1);
}

#[test]
fn a_duplicates_name_is_cut_to_the_limit() {
    let mut session = session();
    let source = session.current_layer();
    session
        .update_layer(source, "Rename layer", |layer| {
            layer.name = "가".repeat(limits::LAYER_NAME_CHARS);
        })
        .unwrap();
    session
        .duplicate_layer(|name| format!("{name} copy"))
        .unwrap();
    let copy = session.document().layer(session.current_layer()).unwrap();
    assert_eq!(copy.name, "가".repeat(limits::LAYER_NAME_CHARS));
}

#[test]
fn an_unchanged_property_is_no_edit() {
    let mut session = session();
    let layer = session.current_layer();
    let revision = session.revision();
    assert_eq!(
        session
            .update_layer(layer, "Opacity", |layer| layer.visible = true)
            .unwrap(),
        Outcome::NoChange
    );
    assert_eq!(session.revision(), revision);
    let wobble = session.document().wobble;
    assert_eq!(
        session.set_animation(30, 25.0, wobble).unwrap(),
        Outcome::NoChange
    );
    assert!(matches!(
        session.set_animation(12, 12.0, wobble).unwrap(),
        Outcome::Committed(_)
    ));
}

#[test]
fn saving_marks_the_state_not_whatever_came_after() {
    let mut session = session();
    let tools = Tools::default();
    draw(&mut session, &tools, 10.0, 60.0);
    let saved = session.state();
    draw(&mut session, &tools, 10.0, 60.0);
    session.mark_saved(saved);
    assert!(session.is_dirty());
    session.undo().unwrap();
    assert!(!session.is_dirty());
}

/// Pen-up on a document like fixture ④ (20,000 short strokes, at the
/// operation limit less one): `cargo test --release -p ugu-session
/// pen_up_cost -- --ignored --nocapture`.
#[test]
#[ignore = "measurement"]
fn pen_up_cost_on_a_large_document() {
    let mut document = Document::new([2048, 2048]);
    let LayerKind::Paint(paint) = &mut document.layers[0].kind else {
        unreachable!()
    };
    for index in 0..19_900u32 {
        let id = ugu_core::ops::StrokeId(index);
        let points: Vec<Point> = (0..6)
            .map(|step| Point {
                x: (index % 2000) as f32 + step as f32,
                y: (index / 10) as f32,
                pressure: 1.0,
            })
            .collect();
        document.store.strokes.insert(
            id,
            Stroke {
                points: Arc::from(points),
                color: Rgba8([0, 0, 0, 255]),
                width: 4.0,
                brush: Brush {
                    engine: BrushEngine::Line,
                    opacity: 1.0,
                    hardness: 1.0,
                    antialias: true,
                    size_dynamics: 0.8,
                    wobble_scale: 1.0,
                    ..Brush::default()
                },
                seed: u64::from(index),
            },
        );
        paint.ops.push(Op::Paint {
            stroke: id,
            clip: None,
        });
    }
    let mut session = Session::new(document, true);
    let tools = Tools::default();
    let mut times = Vec::new();
    for round in 0..100 {
        let y = f64::from(round) * 10.0;
        session.begin_stroke(&tools, at(10.0, y, 0.0)).unwrap();
        for step in 1..200 {
            session.extend_stroke(at(10.0 + f64::from(step) * 3.0, y, f64::from(step) * 4.0));
        }
        let started = std::time::Instant::now();
        session.end_stroke(at(620.0, y, 900.0)).unwrap();
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "pen-up: n={} p50={:.3} ms p95={:.3} ms max={:.3} ms",
        times.len(),
        times[times.len() / 2],
        times[times.len() * 95 / 100],
        times[times.len() - 1]
    );
}

/// Undoes one step and checks it gives `before`, then redoes it.
fn one_step(session: &mut Session, before: &Document) {
    let after = session.document().clone();
    session.undo().unwrap();
    assert_eq!(session.document(), before);
    session.redo().unwrap();
    assert_eq!(session.document(), &after);
}

#[test]
fn group_commands_are_one_undo_step_each() {
    let mut session = session();
    let tools = Tools::default();
    let bottom = session.current_layer();
    session.add_layer().unwrap();
    let middle = session.current_layer();
    session.add_layer().unwrap();
    let top = session.current_layer();

    session.select_layer(middle).unwrap();
    let before = session.document().clone();
    session.add_group().unwrap();
    assert_eq!(session.undo_label(), Some("Add layer group"));
    one_step(&mut session, &before);
    let group = session.document().layers[1].id;
    assert_eq!(session.document().layers[1].name, "Group 1");
    assert_eq!(session.document().position(middle), Some((Some(group), 0)));
    assert_eq!(session.current_layer(), middle);
    // Alone in its group, it has nowhere to move.
    assert_eq!(session.move_layer(1).unwrap(), Outcome::NoChange);

    session.select_layer(top).unwrap();
    let before = session.document().clone();
    session.move_to_group(Some(group)).unwrap();
    one_step(&mut session, &before);
    assert_eq!(session.document().position(top), Some((Some(group), 1)));
    let before = session.document().clone();
    session.move_layer(-1).unwrap();
    one_step(&mut session, &before);
    assert_eq!(session.document().position(top), Some((Some(group), 0)));
    let before = session.document().clone();
    session.move_to_group(None).unwrap();
    one_step(&mut session, &before);
    assert_eq!(session.document().position(top), Some((None, 2)));

    session.select_layer(group).unwrap();
    assert_eq!(
        session.begin_stroke(&tools, at(10.0, 10.0, 0.0)),
        Err(StrokeRefused::NoLayer)
    );
    let before = session.document().clone();
    session.ungroup().unwrap();
    assert_eq!(session.current_layer(), middle);
    one_step(&mut session, &before);
    let order: Vec<LayerId> = session
        .document()
        .layers
        .iter()
        .map(|layer| layer.id)
        .collect();
    assert_eq!(order, [bottom, middle, top]);
    session.undo().unwrap();
    assert_eq!(
        session.document().layer(group).map(|layer| layer.id),
        Some(group)
    );
}

#[test]
fn layers_in_a_group_merge_and_move_among_their_siblings() {
    let mut session = session();
    let tools = Tools::default();
    session.add_group().unwrap();
    let lower = session.current_layer();
    draw(&mut session, &tools, 10.0, 60.0);
    session.add_layer().unwrap();
    let upper = session.current_layer();
    draw(&mut session, &tools, 20.0, 70.0);
    let (group, index) = session.document().position(upper).unwrap();
    assert_eq!(index, 1);
    assert!(group.is_some());
    assert_eq!(session.move_layer(1).unwrap(), Outcome::NoChange);
    session.merge_down().unwrap();
    assert_eq!(session.current_layer(), lower);
    assert_eq!(session.document().position(lower), Some((group, 0)));
}

#[test]
fn the_last_paint_layer_is_not_removed_even_with_its_group() {
    let mut session = session();
    let _tools = Tools::default();
    let only = session.current_layer();
    session.add_group().unwrap();
    let group = session.document().layers[0].id;
    session.select_layer(group).unwrap();
    assert_eq!(session.remove_layer().unwrap(), Outcome::NoChange);
    session.select_layer(only).unwrap();
    assert_eq!(session.remove_layer().unwrap(), Outcome::NoChange);
    // Added beside the group, not in it.
    session.select_layer(group).unwrap();
    session.add_layer().unwrap();
    session.select_layer(group).unwrap();
    session.remove_layer().unwrap();
    assert_eq!(session.document().layers.len(), 1);
    assert!(matches!(
        session
            .document()
            .layer(session.current_layer())
            .map(|layer| &layer.kind),
        Some(LayerKind::Paint(_))
    ));
}

fn drag(
    session: &mut Session,
    kind: ShapeKind,
    from: [f64; 2],
    to: [f64; 2],
    how: Combine,
) -> bool {
    drag_with(session, &Tools::default(), kind, from, to, how)
}

fn drag_with(
    session: &mut Session,
    tools: &Tools,
    kind: ShapeKind,
    from: [f64; 2],
    to: [f64; 2],
    how: Combine,
) -> bool {
    session.begin_selection(kind, from, how);
    session.extend_selection([(from[0] + to[0]) / 2.0, from[1]]);
    session.end_selection(tools, to).unwrap()
}

/// Changes the tool as the window does: what is pending is applied first.
fn set_tool(session: &mut Session, tools: &mut Tools, tool: Tool) -> Result<(), EditError> {
    if tool != tools.tool {
        session.settle()?;
        tools.tool = tool;
    }
    Ok(())
}

/// The colour history after what the session used, as the window keeps it.
fn colors(session: &mut Session, tools: &mut Tools) -> Vec<Rgba8> {
    for color in session.take_used_colors() {
        tools.colors.record(color);
    }
    tools.colors.colors().to_vec()
}

fn selected(session: &Session) -> Option<[i32; 4]> {
    session.selection().map(|selection| selection.mask().bounds)
}

#[test]
fn shapes_replace_add_and_subtract_one_undo_step_each() {
    let mut session = session();
    assert!(drag(
        &mut session,
        ShapeKind::Rectangle,
        [10.0, 10.0],
        [30.0, 20.0],
        Combine::Replace
    ));
    assert_eq!(selected(&session), Some([10, 10, 20, 10]));
    assert_eq!(session.undo_label(), Some("Select area"));
    assert!(drag(
        &mut session,
        ShapeKind::Rectangle,
        [30.0, 10.0],
        [50.0, 20.0],
        Combine::Add
    ));
    assert_eq!(selected(&session), Some([10, 10, 40, 10]));
    assert_eq!(session.undo_label(), Some("Add to selection"));
    assert!(drag(
        &mut session,
        ShapeKind::Ellipse,
        [0.0, 0.0],
        [20.0, 30.0],
        Combine::Subtract
    ));
    assert_eq!(session.undo_label(), Some("Subtract from selection"));
    assert!(!session.selection().unwrap().mask().contains(12, 15));
    let revision = session.revision();
    session.undo().unwrap();
    assert_eq!(selected(&session), Some([10, 10, 40, 10]));
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(selected(&session), None);
    assert_eq!(session.revision(), revision);
}

#[test]
fn a_click_deselects_when_replacing_and_does_nothing_otherwise() {
    let mut session = session();
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [10.0, 10.0],
        [30.0, 20.0],
        Combine::Replace,
    );
    assert!(!drag(
        &mut session,
        ShapeKind::Rectangle,
        [5.0, 5.0],
        [5.0, 5.0],
        Combine::Add
    ));
    assert!(!drag(
        &mut session,
        ShapeKind::Freehand,
        [5.0, 5.0],
        [5.2, 5.0],
        Combine::Subtract
    ));
    assert!(selected(&session).is_some());
    assert!(drag(
        &mut session,
        ShapeKind::Ellipse,
        [5.0, 5.0],
        [5.0, 5.0],
        Combine::Replace
    ));
    assert_eq!(selected(&session), None);
    assert_eq!(session.undo_label(), Some("Deselect"));
    // With nothing selected, a click records nothing.
    assert!(!drag(
        &mut session,
        ShapeKind::Rectangle,
        [5.0, 5.0],
        [5.0, 5.0],
        Combine::Replace
    ));
}

#[test]
fn a_freehand_loop_takes_points_a_pixel_apart_and_stays_on_the_canvas() {
    let mut session = session();
    let tools = Tools::default();
    session.begin_selection(ShapeKind::Freehand, [-20.0, 10.0], Combine::Replace);
    assert!(!session.extend_selection([-19.0, 10.4]));
    for point in [[60.0, 10.0], [60.0, 300.0], [10.0, 60.0]] {
        assert!(session.extend_selection(point));
    }
    let lasso = session.lasso().unwrap();
    assert_eq!(lasso.points[0], [0.0, 10.0]);
    assert_eq!(lasso.points[2], [60.0, 100.0]);
    assert!(session.end_selection(&tools, [10.0, 60.0]).unwrap());
    assert!(session.lasso().is_none());
    assert!(session.selection().unwrap().mask().contains(30, 30));
}

#[test]
fn escape_drops_the_shape_being_dragged_before_the_selection() {
    let mut session = session();
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [10.0, 10.0],
        [30.0, 20.0],
        Combine::Replace,
    );
    session.begin_selection(ShapeKind::Rectangle, [0.0, 0.0], Combine::Replace);
    session.extend_selection([50.0, 50.0]);
    assert!(session.escape());
    assert!(session.lasso().is_none());
    assert_eq!(selected(&session), Some([10, 10, 20, 10]));
    assert_eq!(session.undo_label(), Some("Select area"));
    assert!(session.escape());
    assert_eq!(selected(&session), None);
    assert_eq!(session.undo_label(), Some("Deselect"));
    assert!(!session.escape());
}

#[test]
fn select_all_and_invert_cover_the_canvas() {
    let mut session = session();
    assert!(!session.invert_selection().unwrap());
    assert!(session.select_all().unwrap());
    assert_eq!(selected(&session), Some([0, 0, 200, 100]));
    assert!(session.invert_selection().unwrap());
    assert_eq!(selected(&session), None);
    assert_eq!(session.undo_label(), Some("Invert selection"));
    session.undo().unwrap();
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [0.0, 0.0],
        [100.0, 100.0],
        Combine::Replace,
    );
    session.invert_selection().unwrap();
    assert_eq!(selected(&session), Some([100, 0, 100, 100]));
}

#[test]
fn strokes_in_a_selection_are_cut_to_it_sharing_one_stored_mask() {
    let mut session = session();
    let mut tools = Tools::default();
    let layer = session.current_layer();
    draw(&mut session, &tools, 10.0, 60.0);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [20.0, 0.0],
        [40.0, 100.0],
        Combine::Replace,
    );
    draw(&mut session, &tools, 10.0, 60.0);
    set_tool(&mut session, &mut tools, Tool::Eraser).unwrap();
    draw(&mut session, &tools, 10.0, 60.0);
    let clips: Vec<_> = ops(&session, layer)
        .iter()
        .map(|op| match op {
            Op::Paint { clip, .. } | Op::Erase { clip, .. } => *clip,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(clips[0], None);
    assert!(clips[1].is_some());
    assert_eq!(clips[1], clips[2]);
    assert_eq!(session.document().store.masks.len(), 1);
    // Undoing the strokes takes the mask with them.
    session.undo().unwrap();
    session.undo().unwrap();
    assert!(session.document().store.masks.is_empty());
    // A selection of everything cuts nothing.
    session.select_all().unwrap();
    set_tool(&mut session, &mut tools, Tool::Pen).unwrap();
    draw(&mut session, &tools, 10.0, 60.0);
    assert!(matches!(
        ops(&session, layer).last(),
        Some(Op::Paint { clip: None, .. })
    ));
}

#[test]
fn the_selection_stays_across_layers_and_follows_canvas_changes() {
    let mut session = session();
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [20.0, 10.0],
        [40.0, 30.0],
        Combine::Replace,
    );
    session.add_layer().unwrap();
    assert_eq!(selected(&session), Some([20, 10, 20, 20]));
    session.crop_canvas([-10, 5], [150, 80]).unwrap();
    assert_eq!(selected(&session), Some([10, 15, 20, 20]));
    session
        .resample_image([300, 160], Sampling::Smooth)
        .unwrap();
    assert_eq!(selected(&session), Some([20, 30, 40, 40]));
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(selected(&session), Some([20, 10, 20, 20]));
    // A crop that leaves it off the canvas drops it, and undo brings it back.
    session.crop_canvas([-100, 0], [50, 50]).unwrap();
    assert_eq!(selected(&session), None);
    session.undo().unwrap();
    assert_eq!(selected(&session), Some([20, 10, 20, 20]));
}

/// A 200×100 reference: transparent with an opaque box outline around
/// x 40..120, y 20..80, two pixels thick.
fn boxed() -> Vec<[u8; 4]> {
    let mut pixels = vec![[0; 4]; 200 * 100];
    for y in 20..80 {
        for x in 40..120 {
            if !(42..118).contains(&x) || !(22..78).contains(&y) {
                pixels[y * 200 + x] = [0, 0, 0, 255];
            }
        }
    }
    pixels
}

fn fills(session: &Session, layer: LayerId) -> Vec<(MaskId, Rgba8, bool, Option<MaskId>)> {
    ops(session, layer)
        .into_iter()
        .filter_map(|op| match op {
            Op::Fill {
                coverage,
                color,
                antialias,
                clip,
                ..
            } => Some((coverage, color, antialias, clip)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_wand_selects_the_area_clicked_and_combines_like_a_shape() {
    let mut session = session();
    let mut tools = Tools::default();
    let reference = boxed();
    let read = Some(reference.as_slice());
    assert_eq!(
        session.wand(&tools, [60.0, 50.0], Combine::Replace, read),
        Ok(true)
    );
    assert_eq!(selected(&session), Some([42, 22, 76, 56]));
    assert_eq!(session.undo_label(), Some("Select area"));
    // The outside, added, rings the box.
    assert_eq!(
        session.wand(&tools, [5.0, 5.0], Combine::Add, read),
        Ok(true)
    );
    assert_eq!(selected(&session), Some([0, 0, 200, 100]));
    assert_eq!(
        session.wand(&tools, [60.0, 50.0], Combine::Subtract, read),
        Ok(true)
    );
    assert!(!session.selection().unwrap().mask().contains(60, 50));
    // On a line nothing is found: replacing deselects and says why, adding
    // keeps the selection.
    assert_eq!(
        session.wand(&tools, [40.5, 50.0], Combine::Add, read),
        Err(FillError::NothingThere)
    );
    assert!(session.selection().is_some());
    assert_eq!(
        session.wand(&tools, [40.5, 50.0], Combine::Replace, read),
        Err(FillError::NothingThere)
    );
    assert!(session.selection().is_none());
    assert_eq!(
        session.wand(&tools, [60.0, 50.0], Combine::Replace, None),
        Err(FillError::NoReference)
    );
    // Off the canvas, as a click with the selection tool.
    session
        .wand(&tools, [60.0, 50.0], Combine::Replace, read)
        .unwrap();
    assert_eq!(
        session.wand(&tools, [-3.0, 50.0], Combine::Replace, read),
        Ok(true)
    );
    assert!(session.selection().is_none());
    // Colour comparison with a tolerance passes the line.
    tools.fill.by_colour = true;
    tools.fill.tolerance = 255;
    session
        .wand(&tools, [60.0, 50.0], Combine::Replace, read)
        .unwrap();
    assert_eq!(selected(&session), Some([0, 0, 200, 100]));
}

#[test]
fn the_bucket_fills_the_area_clicked_once_and_only_inside_the_selection() {
    let mut session = session();
    let mut tools = Tools::default();
    let layer = session.current_layer();
    let reference = boxed();
    let read = Some(reference.as_slice());
    tools.color = Rgba8([200, 30, 30, 255]);
    assert!(matches!(
        session.bucket(&tools, [60.0, 50.0], read),
        Ok(Outcome::Committed(_))
    ));
    let [(coverage, color, antialias, clip)] = fills(&session, layer)[..] else {
        panic!("one fill");
    };
    assert_eq!(
        (color, antialias, clip),
        (Rgba8([200, 30, 30, 255]), true, None)
    );
    // Behind the 2 pixel line around it, and under its inner half.
    assert!(matches!(
        ops(&session, layer)[..],
        [Op::Fill { behind: true, .. }]
    ));
    assert_eq!(
        session.document().store.masks[&coverage].bounds,
        [41, 21, 78, 58]
    );
    assert_eq!(session.undo_label(), Some("Fill"));
    assert!(session.undo().unwrap());
    assert!(fills(&session, layer).is_empty());
    assert!(session.document().store.masks.is_empty());
    // Matched by colour, it replaces what it covers and stops at the line.
    tools.fill.by_colour = true;
    session.bucket(&tools, [60.0, 50.0], read).unwrap();
    let [
        Op::Fill {
            coverage,
            behind: false,
            ..
        },
    ] = ops(&session, layer)[..]
    else {
        panic!("one fill over what is there");
    };
    assert_eq!(
        session.document().store.masks[&coverage].bounds,
        [42, 22, 76, 56]
    );
    session.undo().unwrap();
    tools.fill.by_colour = false;
    // With a selection, a click outside it fills nothing; inside, the fill
    // is cut to it.
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [50.0, 30.0],
        [90.0, 70.0],
        Combine::Replace,
    );
    assert_eq!(
        session.bucket(&tools, [10.0, 10.0], read),
        Err(FillError::OutsideSelection)
    );
    session.bucket(&tools, [60.0, 50.0], read).unwrap();
    let [(coverage, _, _, Some(clip))] = fills(&session, layer)[..] else {
        panic!("one fill cut to the selection");
    };
    assert_ne!(coverage, clip);
    assert_eq!(
        &session.document().store.masks[&clip],
        session.selection().unwrap().mask()
    );
    assert_eq!(
        session.bucket(&tools, [41.0, 50.0], read),
        Err(FillError::OutsideSelection)
    );
    session.deselect().unwrap();
    assert_eq!(
        session.bucket(&tools, [41.0, 50.0], read),
        Err(FillError::NothingThere)
    );
    assert_eq!(
        session.bucket(&tools, [300.0, 50.0], read),
        Ok(Outcome::NoChange)
    );
    session
        .update_layer(layer, "Hide layer", |layer| layer.visible = false)
        .unwrap();
    assert_eq!(
        session.bucket(&tools, [60.0, 50.0], read),
        Err(FillError::HiddenLayer)
    );
}

#[test]
fn filling_the_selection_or_a_painted_shape_is_one_fill_cut_to_the_selection() {
    let mut session = session();
    let mut tools = Tools::default();
    let layer = session.current_layer();
    assert_eq!(session.fill_selection(&tools), Err(FillError::NoSelection));
    drag(
        &mut session,
        ShapeKind::Ellipse,
        [20.0, 20.0],
        [80.0, 70.0],
        Combine::Replace,
    );
    session.fill_selection(&tools).unwrap();
    let [(coverage, _, _, Some(clip))] = fills(&session, layer)[..] else {
        panic!("one fill cut to the selection");
    };
    // The selection is stored once, for both.
    assert_eq!(coverage, clip);
    assert_eq!(session.document().store.masks.len(), 1);
    session.select_all().unwrap();
    session.fill_selection(&tools).unwrap();
    assert!(matches!(fills(&session, layer)[1], (_, _, _, None)));
    // Painting fills the shape and leaves the selection as it was.
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [10.0, 10.0],
        [50.0, 50.0],
        Combine::Replace,
    );
    let before = session.selection().cloned();
    tools.lasso_paints = true;
    assert!(drag_with(
        &mut session,
        &tools,
        ShapeKind::Rectangle,
        [30.0, 30.0],
        [90.0, 90.0],
        Combine::Replace
    ));
    assert_eq!(session.selection().cloned(), before);
    let (coverage, _, _, clip) = fills(&session, layer)[2];
    assert_eq!(
        session.document().store.masks[&coverage].bounds,
        [30, 30, 60, 60]
    );
    assert_eq!(
        session.document().store.masks[&clip.unwrap()],
        *before.unwrap().mask()
    );
    assert_eq!(session.undo_label(), Some("Fill"));
    // A click paints nothing.
    assert!(!drag_with(
        &mut session,
        &tools,
        ShapeKind::Rectangle,
        [30.0, 30.0],
        [30.2, 30.2],
        Combine::Replace
    ));
    assert_eq!(fills(&session, layer).len(), 3);
}

fn transforms(session: &Session, layer: LayerId) -> Vec<Op> {
    ops(session, layer)
        .into_iter()
        .filter(|op| {
            matches!(
                op,
                Op::TransformSelection { .. } | Op::ClearSelection { .. }
            )
        })
        .collect()
}

#[test]
fn a_pending_transform_is_applied_as_one_step_moving_the_selection_along() {
    let mut session = session();
    let layer = session.current_layer();
    assert_eq!(session.begin_transform(), Err(FillError::NoSelection));
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [10.0, 10.0],
        [30.0, 20.0],
        Combine::Replace,
    );
    let selection = session.selection().cloned().unwrap();
    assert_eq!(session.begin_transform(), Ok(true));
    assert_eq!(session.begin_transform(), Ok(false));
    assert!(session.is_dirty());
    // A flattening transform is refused; the pending one stays.
    assert!(!session.set_transform(Affine::scaling_about([0.0, 1.0], [0.0, 0.0])));
    assert!(!session.set_transform(Affine::translation(40_000.0, 0.0)));
    let moved = Affine::translation(15.0, 5.0).then(Affine::rotation_about(0.3, [35.0, 20.0]));
    assert!(session.set_transform(moved));
    assert!(
        ops(&session, layer).is_empty(),
        "nothing in the document before applying"
    );
    assert!(matches!(
        session.apply_transform(),
        Ok(Outcome::Committed(_))
    ));
    assert_eq!(session.pending(), None);
    let [
        Op::TransformSelection {
            mask,
            transform,
            sampling,
            keep_source,
        },
    ] = &transforms(&session, layer)[..]
    else {
        panic!("one transform");
    };
    assert_eq!(
        (*transform, *sampling, *keep_source),
        (moved, Sampling::Smooth, false)
    );
    assert_eq!(&session.document().store.masks[mask], selection.mask());
    assert_eq!(
        session.selection().map(|selection| selection.as_ref()),
        selection.transformed(moved).as_ref()
    );
    assert_eq!(session.undo_label(), Some("Transform selection"));
    assert!(session.undo().unwrap());
    assert!(ops(&session, layer).is_empty());
    assert_eq!(session.selection(), Some(&selection));
    // An unmoved selection applies nothing.
    session.begin_transform().unwrap();
    assert_eq!(session.apply_transform(), Ok(Outcome::NoChange));
}

#[test]
fn undo_and_escape_cancel_a_pending_transform_and_other_edits_apply_it_first() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    draw(&mut session, &tools, 10.0, 60.0);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [5.0, 40.0],
        [70.0, 60.0],
        Combine::Replace,
    );
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(50.0, 0.0));
    let label = session.undo_label().map(str::to_owned);
    assert!(session.undo().unwrap());
    assert_eq!(session.pending(), None);
    assert_eq!(
        session.undo_label().map(str::to_owned),
        label,
        "undo only cancelled it"
    );
    assert_eq!(ops(&session, layer).len(), 1);

    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(50.0, 0.0));
    session.set_keep_source(true);
    draw(&mut session, &tools, 10.0, 30.0);
    let kinds: Vec<_> = ops(&session, layer)
        .iter()
        .map(|op| match op {
            Op::Paint { .. } => "paint",
            Op::TransformSelection {
                keep_source: true, ..
            } => "copy",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["paint", "copy", "paint"]);

    // Changing layer or frame applies it too.
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(0.0, 10.0));
    session.set_frame(3).unwrap();
    assert_eq!(session.pending(), None);
    assert_eq!(transforms(&session, layer).len(), 2);
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(0.0, 10.0));
    session.add_layer().unwrap();
    assert_eq!(transforms(&session, layer).len(), 3);
    assert!(!session.cancel_transform());
}

#[test]
fn deleting_clears_the_selected_part_after_a_pending_transform() {
    let mut session = session();
    let layer = session.current_layer();
    assert_eq!(session.delete_selected(), Err(FillError::NoSelection));
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [10.0, 10.0],
        [30.0, 20.0],
        Combine::Replace,
    );
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(40.0, 0.0));
    session.delete_selected().unwrap();
    let [Op::TransformSelection { .. }, Op::ClearSelection { mask }] =
        &transforms(&session, layer)[..]
    else {
        panic!("the move, then the clear");
    };
    assert_eq!(
        session.document().store.masks[mask].bounds,
        [50, 10, 20, 10]
    );
    assert_eq!(session.undo_label(), Some("Delete"));
}

/// A session with a stroke and a selection moved by a pending transform.
fn moved_selection() -> Session {
    let mut session = session();
    draw(&mut session, &Tools::default(), 10.0, 60.0);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [5.0, 5.0],
        [40.0, 30.0],
        Combine::Replace,
    );
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(100.0, 0.0));
    session
}

type Named<T> = (&'static str, T);

#[test]
fn layer_edits_apply_a_pending_transform_and_keep_it() {
    let edits: [Named<fn(&mut Session)>; 5] = [
        ("rename", |session| {
            let layer = session.current_layer();
            session
                .update_layer(layer, "Rename", |layer| layer.name = "Moved".into())
                .unwrap();
        }),
        ("move", |session| {
            session.add_layer().unwrap();
            session.move_layer(-1).unwrap();
        }),
        ("group", |session| {
            session.add_group().unwrap();
        }),
        ("move out of group", |session| {
            session.add_group().unwrap();
            session.ungroup().unwrap();
        }),
        ("delete another", |session| {
            let layer = session.current_layer();
            session.add_layer().unwrap();
            session.remove_layer().unwrap();
            session.select_layer(layer).unwrap();
        }),
    ];
    for (name, edit) in edits {
        let mut session = moved_selection();
        edit(&mut session);
        assert_eq!(session.pending(), None, "{name}");
        let held: usize = paint_layers(&session.document().layers)
            .into_iter()
            .map(|id| transforms(&session, id).len())
            .sum();
        assert_eq!(held, 1, "{name}: the transform is kept");
    }
}

fn paint_layers(layers: &[Layer]) -> Vec<LayerId> {
    layers
        .iter()
        .flat_map(|layer| match &layer.kind {
            LayerKind::Paint(_) => vec![layer.id],
            LayerKind::Group(group) => paint_layers(&group.children),
        })
        .collect()
}

#[test]
fn merging_down_keeps_a_pending_transform_of_the_upper_layer() {
    let mut session = session();
    session.add_layer().unwrap();
    draw(&mut session, &Tools::default(), 10.0, 60.0);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [5.0, 5.0],
        [40.0, 30.0],
        Combine::Replace,
    );
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(100.0, 0.0));
    session.merge_down().unwrap();
    let ops = ops(&session, session.current_layer());
    let Some(Op::Isolated(upper)) = ops.last() else {
        panic!("the upper layer as a section");
    };
    assert!(
        upper
            .ops
            .iter()
            .any(|op| matches!(op, Op::TransformSelection { .. }))
    );
}

#[test]
fn deleting_the_layer_of_a_pending_transform_leaves_the_session_usable() {
    let mut session = moved_selection();
    session.add_layer().unwrap();
    session
        .select_layer(session.document().layers[0].id)
        .unwrap();
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(100.0, 0.0));
    session.remove_layer().unwrap();
    assert_eq!(session.pending(), None);
    assert!(matches!(session.add_layer(), Ok(Outcome::Committed(_))));
}

#[test]
fn fills_and_selection_changes_use_the_moved_selection() {
    let mut session = moved_selection();
    let layer = session.current_layer();
    session.fill_selection(&Tools::default()).unwrap();
    let (coverage, ..) = *fills(&session, layer).last().unwrap();
    assert_eq!(
        session.document().store.masks[&coverage].bounds,
        [105, 5, 35, 25]
    );

    let mut session = moved_selection();
    session.invert_selection().unwrap();
    assert!(!session.selection().unwrap().mask().contains(110, 10));
    assert!(session.selection().unwrap().mask().contains(10, 10));

    let mut session = moved_selection();
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [150.0, 60.0],
        [160.0, 70.0],
        Combine::Add,
    );
    assert_eq!(selected(&session), Some([105, 5, 55, 65]));
}

#[test]
fn escape_cancels_a_pending_transform_before_deselecting_and_changing_tool_applies_it() {
    let mut session = session();
    let mut tools = Tools::default();
    let layer = session.current_layer();
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [10.0, 10.0],
        [30.0, 20.0],
        Combine::Replace,
    );
    assert_eq!(session.take_ended(), None);
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(5.0, 0.0));
    assert!(session.escape());
    assert_eq!(session.take_ended(), Some(Ended::Dropped));
    assert_eq!(session.take_ended(), None);
    assert!(session.selection().is_some(), "the selection stays");
    assert!(transforms(&session, layer).is_empty());

    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(5.0, 0.0));
    let same = tools.tool;
    set_tool(&mut session, &mut tools, same).unwrap();
    assert!(session.pending().is_some(), "the same tool keeps it");
    set_tool(&mut session, &mut tools, Tool::Fill).unwrap();
    assert_eq!(
        session.take_ended(),
        Some(Ended::Applied {
            revision: session.revision()
        })
    );
    assert_eq!(transforms(&session, layer).len(), 1);
    // Applied unmoved, it leaves the document as it was.
    session.begin_transform().unwrap();
    session.apply_transform().unwrap();
    assert_eq!(session.take_ended(), Some(Ended::Dropped));
}

#[test]
fn flipping_turns_the_selected_part_over_in_place_along_its_own_sides() {
    let mut session = session();
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [10.0, 10.0],
        [30.0, 20.0],
        Combine::Replace,
    );
    let corner = |session: &Session, point| session.pending().unwrap().transform.apply(point);
    session.flip(true).unwrap();
    assert_eq!(corner(&session, [10.0, 10.0]), [30.0, 10.0]);
    // Turned a quarter, a vertical flip follows the turned sides: the
    // selection's top edge, now on the right, goes to the left.
    let turned = Affine::rotation_about(std::f64::consts::FRAC_PI_2, [20.0, 15.0]);
    session.set_transform(turned);
    session.flip(false).unwrap();
    let [x, y] = turned.apply([20.0, 10.0]);
    assert!((x - 25.0).abs() < 1e-9 && (y - 15.0).abs() < 1e-9);
    let [x, y] = corner(&session, [20.0, 10.0]);
    assert!(
        (x - 15.0).abs() < 1e-9 && (y - 15.0).abs() < 1e-9,
        "{x} {y}"
    );
}

#[test]
fn a_paste_is_one_step_adding_a_selected_layer_ready_to_move() {
    let mut session = session();
    let tools = Tools::default();
    let source = session.current_layer();
    draw(&mut session, &tools, 10.0, 150.0);
    assert_eq!(session.copy().unwrap_err(), FillError::NoSelection);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [20.0, 30.0],
        [80.0, 70.0],
        Combine::Replace,
    );
    let selection = session.selection().cloned().unwrap();
    let clip = session.copy().unwrap();
    assert!(!session.is_dirty() || session.undo_label() != Some("Paste"));
    let label = session.undo_label().map(str::to_owned);
    session.paste(&clip).unwrap();
    let pasted = session.current_layer();
    assert_ne!(pasted, source);
    assert_eq!(session.document().position(pasted).unwrap().1, 1);
    assert_eq!(session.selection(), Some(&selection));
    let pending = session.pending().expect("ready to move");
    assert_eq!(pending.layer, pasted);
    assert_eq!(session.undo_label(), Some("Paste"));
    // Undo cancels the move, then takes the paste away.
    assert!(session.undo().unwrap());
    assert!(session.document().layer(pasted).is_some());
    assert!(session.undo().unwrap());
    assert!(session.document().layer(pasted).is_none());
    assert_eq!(session.undo_label().map(str::to_owned), label);
}

#[test]
fn a_placed_image_is_a_new_layer_selected_and_ready_to_move() {
    let mut session = session();
    let asset = ugu_core::store::Asset {
        size: [40, 20],
        png: std::sync::Arc::from(&b"png"[..]),
    };
    let id = ugu_core::ops::AssetId([5; 32]);
    session.place_image(id, asset).unwrap();
    let layer = session.current_layer();
    assert!(matches!(ops(&session, layer)[..], [Op::PlaceImage { .. }]));
    assert_eq!(session.selection().unwrap().mask().bounds, [80, 40, 40, 20]);
    assert_eq!(session.pending().unwrap().layer, layer);
    assert_eq!(session.undo_label(), Some("Place image"));
}

#[test]
fn a_canvas_change_applies_a_pending_transform_first_and_moves_its_selection() {
    for resample in [false, true] {
        let mut session = session();
        let tools = Tools::default();
        let layer = session.current_layer();
        draw(&mut session, &tools, 10.0, 60.0);
        drag(
            &mut session,
            ShapeKind::Rectangle,
            [10.0, 40.0],
            [30.0, 60.0],
            Combine::Replace,
        );
        session.begin_transform().unwrap();
        session.set_transform(Affine::translation(40.0, 0.0));
        let moved = session
            .selection()
            .unwrap()
            .transformed(Affine::translation(40.0, 0.0));
        let changed = if resample {
            session.resample_image([100, 50], Sampling::Nearest)
        } else {
            session.crop_canvas([-20, 10], [150, 100])
        };
        assert!(matches!(changed, Ok(Outcome::Committed(_))));
        assert_eq!(session.pending(), None);
        assert_eq!(
            session.document().canvas,
            if resample { [100, 50] } else { [150, 100] }
        );
        let kinds: Vec<_> = ops(&session, layer)
            .iter()
            .map(|op| match op {
                Op::Paint { .. } => "paint",
                Op::TransformSelection { .. } => "move",
                Op::Crop {
                    offset: [-20, 10],
                    size: [150, 100],
                } => "crop",
                Op::Resample {
                    size: [100, 50],
                    sampling: Sampling::Nearest,
                } => "resample",
                _ => "other",
            })
            .collect();
        let last = if resample { "resample" } else { "crop" };
        assert_eq!(kinds, ["paint", "move", last]);
        let expected = if resample {
            moved.as_ref().and_then(|moved| moved.resampled([100, 50]))
        } else {
            moved
                .as_ref()
                .and_then(|moved| moved.cropped([-20, 10], [150, 100]))
        };
        assert_eq!(
            session.selection().map(|selection| selection.as_ref()),
            expected.as_ref()
        );
        // Two steps: the canvas change, then the transform.
        assert!(session.undo().unwrap());
        assert_eq!(session.document().canvas, [200, 100]);
        assert_eq!(
            session.selection().map(|selection| selection.as_ref()),
            moved.as_ref()
        );
        assert_eq!(session.undo_label(), Some("Transform selection"));
        assert!(session.undo().unwrap());
        assert_eq!(selected(&session), Some([10, 40, 20, 20]));
        assert_eq!(ops(&session, layer).len(), 1);
        assert!(session.redo().unwrap() && session.redo().unwrap());
        assert_eq!(
            session.selection().map(|selection| selection.as_ref()),
            expected.as_ref()
        );
    }
}

#[test]
fn an_unchanged_canvas_is_no_step_and_a_refused_one_changes_nothing() {
    let mut session = session();
    let tools = Tools::default();
    draw(&mut session, &tools, 10.0, 60.0);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [10.0, 40.0],
        [30.0, 60.0],
        Combine::Replace,
    );
    let revision = session.revision();
    let label = session.undo_label().map(str::to_owned);
    assert_eq!(
        session.crop_canvas([0, 0], [200, 100]),
        Ok(Outcome::NoChange)
    );
    assert_eq!(
        session.resample_image([200, 100], Sampling::Smooth),
        Ok(Outcome::NoChange)
    );
    assert!(session.crop_canvas([0, 0], [0, 100]).is_err());
    assert!(
        session
            .resample_image([5000, 100], Sampling::Smooth)
            .is_err()
    );
    assert_eq!(session.revision(), revision);
    assert_eq!(session.undo_label().map(str::to_owned), label);
    assert_eq!(selected(&session), Some([10, 40, 20, 20]));
    // The same size with the artwork moved is a change.
    assert!(matches!(
        session.crop_canvas([-5, 0], [200, 100]),
        Ok(Outcome::Committed(_))
    ));
    assert_eq!(selected(&session), Some([5, 40, 20, 20]));
    assert!(session.is_dirty());
}

#[test]
fn a_canvas_change_ends_a_lasso_being_drawn() {
    let mut session = session();
    session.begin_selection(ShapeKind::Freehand, [10.0, 10.0], Combine::Replace);
    session.extend_selection([40.0, 10.0]);
    assert!(session.lasso().is_some());
    session.crop_canvas([0, 0], [120, 80]).unwrap();
    assert!(session.lasso().is_none());
    assert_eq!(session.selection(), None);
}

/// A 20 × 10 box with a hole, as laid-out text would give.
fn boxed_text() -> Arc<ugu_core::text::Outline> {
    Arc::new(ugu_core::text::Outline {
        contours: vec![
            vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]],
            vec![[5.0, 3.0], [5.0, 7.0], [15.0, 7.0], [15.0, 3.0]],
        ],
        size: [20.0, 10.0],
    })
}

#[test]
fn placed_text_is_applied_as_one_step_in_the_pen_colour() {
    let mut session = session();
    let mut tools = Tools::default();
    let layer = session.current_layer();
    set_tool(&mut session, &mut tools, Tool::Text).unwrap();
    session
        .place_text(&tools, [30.0, 40.0], boxed_text())
        .unwrap();
    assert!(session.is_dirty());
    assert!(ops(&session, layer).is_empty());
    // Moving keeps the seeds: the strokes move as they are.
    let before = session.text_drawing().unwrap();
    session
        .place_text(&tools, [35.0, 40.0], boxed_text())
        .unwrap();
    let after = session.text_drawing().unwrap();
    assert_eq!(after.strokes.len(), 2);
    for (a, b) in before.strokes.iter().zip(&after.strokes) {
        assert_eq!(a.seed, b.seed);
        assert_eq!(a.points[0].x + 5.0, b.points[0].x);
    }
    tools.color = Rgba8([10, 20, 30, 255]);
    session.use_tools(&tools);
    assert!(matches!(session.apply_text(), Ok(Outcome::Committed(_))));
    assert_eq!(session.placed_text(), None);
    assert_eq!(session.undo_label(), Some("Add text"));
    let strokes: Vec<_> = session.document().store.strokes.values().collect();
    assert_eq!(strokes.len(), 2);
    assert!(
        strokes
            .iter()
            .all(|stroke| stroke.color == Rgba8([10, 20, 30, 255]))
    );
    assert!(matches!(
        &ops(&session, layer)[..],
        [Op::Paint { clip: None, .. }, Op::Paint { clip: None, .. }]
    ));
    assert!(session.undo().unwrap());
    assert!(ops(&session, layer).is_empty());
    // Nothing to draw changes nothing.
    session
        .place_text(
            &tools,
            [0.0, 0.0],
            Arc::new(ugu_core::text::Outline::default()),
        )
        .unwrap();
    assert_eq!(session.apply_text(), Ok(Outcome::NoChange));
}

#[test]
fn letters_are_outlined_with_an_even_line_whatever_brush_is_chosen() {
    let mut session = session();
    let mut tools = Tools::default();
    tools.plain.pen.preset = ugu_core::brush::find("soft-airbrush").unwrap();
    tools.plain.pen.width = 9.0;
    tools.color = Rgba8([200, 10, 10, 255]);
    set_tool(&mut session, &mut tools, Tool::Text).unwrap();
    session
        .place_text(&tools, [30.0, 40.0], boxed_text())
        .unwrap();
    let monoline = ugu_core::brush::find("monoline").unwrap().brush;
    let drawing = session.text_drawing().unwrap();
    for stroke in &drawing.strokes {
        assert_eq!(
            ugu_core::store::Brush {
                antialias: monoline.antialias,
                ..stroke.brush
            },
            monoline
        );
        // The brush's colour and width still.
        assert_eq!(
            (stroke.color, stroke.width),
            (Rgba8([200, 10, 10, 255]), 9.0)
        );
    }
}

#[test]
fn filled_text_fills_under_its_outlines_and_keeps_inside_the_selection() {
    let mut session = session();
    let mut tools = Tools::default();
    let layer = session.current_layer();
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [0.0, 0.0],
        [40.0, 50.0],
        Combine::Replace,
    );
    tools.text.filled = true;
    session
        .place_text(&tools, [30.0, 40.0], boxed_text())
        .unwrap();
    let drawing = session.text_drawing().unwrap();
    assert_eq!(
        drawing.fill.as_ref().unwrap().mask().bounds,
        [30, 40, 20, 10]
    );
    session.apply_text().unwrap();
    let [
        Op::Fill {
            coverage,
            clip: Some(fill_clip),
            ..
        },
        Op::Paint {
            clip: Some(first), ..
        },
        Op::Paint {
            clip: Some(second), ..
        },
    ] = &ops(&session, layer)[..]
    else {
        panic!("a fill, then two outlines, cut to the selection");
    };
    assert!(fill_clip == first && first == second);
    assert_eq!(
        &session.document().store.masks[coverage],
        drawing.fill.unwrap().mask()
    );
}

#[test]
fn placed_text_is_dropped_by_undo_and_escape_and_applied_by_other_edits() {
    let mut session = session();
    let mut tools = Tools::default();
    let layer = session.current_layer();
    draw(&mut session, &tools, 10.0, 60.0);
    set_tool(&mut session, &mut tools, Tool::Text).unwrap();
    session
        .place_text(&tools, [30.0, 20.0], boxed_text())
        .unwrap();
    let label = session.undo_label().map(str::to_owned);
    assert!(session.undo().unwrap());
    assert_eq!(session.placed_text(), None);
    assert_eq!(session.undo_label().map(str::to_owned), label);
    session
        .place_text(&tools, [30.0, 20.0], boxed_text())
        .unwrap();
    assert!(session.escape());
    assert_eq!(session.placed_text(), None);
    assert_eq!(ops(&session, layer).len(), 1);

    // Changing tools or layers applies it.
    session
        .place_text(&tools, [30.0, 20.0], boxed_text())
        .unwrap();
    set_tool(&mut session, &mut tools, Tool::Pen).unwrap();
    assert_eq!(ops(&session, layer).len(), 3);
    set_tool(&mut session, &mut tools, Tool::Text).unwrap();
    session
        .place_text(&tools, [30.0, 20.0], boxed_text())
        .unwrap();
    session.add_layer().unwrap();
    assert_eq!(ops(&session, layer).len(), 5);
    assert_eq!(session.placed_text(), None);

    // A hidden layer takes no text.
    let hidden = session.current_layer();
    session
        .update_layer(hidden, "Hide", |layer| layer.visible = false)
        .unwrap();
    assert_eq!(
        session.place_text(&tools, [0.0, 0.0], boxed_text()),
        Err(FillError::HiddenLayer)
    );
}

#[test]
fn the_eyedropper_sets_the_pen_colour_without_an_edit() {
    let mut session = Session::new(Document::new([200, 100]), true);
    let mut tools = Tools::default();
    set_tool(&mut session, &mut tools, Tool::Eyedropper).unwrap();
    let (revision, label) = (session.revision(), session.undo_label().map(str::to_owned));
    // Half-transparent red, premultiplied.
    assert!(tools.pick_color([128, 0, 0, 128]));
    assert_eq!(tools.color, Rgba8([255, 0, 0, 128]));
    assert!(!tools.pick_color([128, 0, 0, 128]), "the same colour");
    assert!(!tools.pick_color([0, 0, 0, 0]), "a clear pixel");
    assert_eq!(tools.color, Rgba8([255, 0, 0, 128]));
    assert_eq!(session.revision(), revision);
    assert_eq!(session.undo_label().map(str::to_owned), label);
    assert!(!session.is_dirty());
}

#[test]
fn picking_leaves_a_pending_transform_and_placed_text_pending() {
    let mut session = session();
    let mut tools = Tools::default();
    draw(&mut session, &tools, 10.0, 60.0);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [5.0, 40.0],
        [70.0, 60.0],
        Combine::Replace,
    );
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(20.0, 0.0));
    tools.pick_color([0, 0, 255, 255]);
    assert!(session.pending().is_some());
    session.cancel_transform();
    set_tool(&mut session, &mut tools, Tool::Text).unwrap();
    session
        .place_text(&tools, [30.0, 40.0], boxed_text())
        .unwrap();
    tools.pick_color([0, 255, 0, 255]);
    assert!(session.placed_text().is_some());
}

#[test]
fn restyling_changes_what_the_selection_touches_in_one_step() {
    let mut session = session();
    let mut tools = Tools::default();
    let layer = session.current_layer();
    draw(&mut session, &tools, 10.0, 60.0);
    tools.plain.pen.width = 3.0;
    tools.color = Rgba8([0, 0, 255, 255]);
    draw(&mut session, &tools, 120.0, 180.0);
    let before = session.document().clone();
    assert_eq!(session.touched(), Err(FillError::NoSelection));
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [0.0, 0.0],
        [100.0, 100.0],
        Combine::Replace,
    );
    let touched = session.touched().unwrap();
    assert_eq!((touched.colored, touched.sized), (1, 1));
    assert_eq!(touched.color, Some(Rgba8([0, 0, 0, 255])));
    assert_eq!(touched.width, Some(6.0));

    let red = Rgba8([255, 0, 0, 255]);
    let style = Style {
        color: Some(red),
        width: None,
    };
    let selection = session.selection().cloned();
    assert!(matches!(
        session.restyle_selected(style).unwrap(),
        Outcome::Committed(_)
    ));
    assert_eq!(session.undo_label(), Some("Edit stroke properties"));
    assert_eq!(
        session.selection().cloned(),
        selection,
        "the selection stays"
    );
    let colors: Vec<_> = ops(&session, layer)
        .iter()
        .map(|op| match op {
            Op::Paint { stroke, .. } => session.document().store.strokes[stroke].color,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(colors, [red, Rgba8([0, 0, 255, 255])]);
    assert_eq!(
        session.restyle_selected(style).unwrap(),
        Outcome::NoChange,
        "already red"
    );
    assert!(session.undo().unwrap());
    assert_eq!(session.document(), &before);
    assert_eq!(session.selection().cloned(), selection);
}

#[test]
fn restyling_waits_for_a_pending_transform_or_placed_text() {
    let mut session = session();
    let mut tools = Tools::default();
    draw(&mut session, &tools, 10.0, 60.0);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [5.0, 40.0],
        [70.0, 60.0],
        Combine::Replace,
    );
    let style = Style {
        color: None,
        width: Some(12.0),
    };
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(20.0, 0.0));
    let revision = session.revision();
    assert_eq!(session.touched(), Err(FillError::Pending));
    assert_eq!(session.restyle_selected(style), Err(FillError::Pending));
    assert!(session.pending().is_some(), "not applied");
    assert_eq!(session.revision(), revision);
    session.apply_transform().unwrap();
    set_tool(&mut session, &mut tools, Tool::Text).unwrap();
    session
        .place_text(&tools, [30.0, 10.0], boxed_text())
        .unwrap();
    assert_eq!(session.restyle_selected(style), Err(FillError::Pending));
    assert!(session.cancel_text());
    assert!(matches!(
        session.restyle_selected(style).unwrap(),
        Outcome::Committed(_)
    ));
}

#[test]
fn restyling_refuses_a_hidden_layer() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    draw(&mut session, &tools, 10.0, 60.0);
    session.select_all().unwrap();
    session
        .update_layer(layer, "Hide", |layer| layer.visible = false)
        .unwrap();
    assert_eq!(session.touched(), Err(FillError::HiddenLayer));
}

#[test]
fn a_stroke_begun_while_a_transform_is_pending_is_cut_to_the_moved_selection() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    draw(&mut session, &tools, 10.0, 60.0);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [5.0, 40.0],
        [70.0, 60.0],
        Combine::Replace,
    );
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(100.0, 0.0));
    session.begin_stroke(&tools, at(120.0, 50.0, 0.0)).unwrap();
    assert!(session.pending().is_none(), "applied at the press");
    let moved = session.selection().cloned();
    assert_eq!(session.live().unwrap().clip, moved);
    session.end_stroke(at(160.0, 50.0, 8.0)).unwrap();
    let ops = ops(&session, layer);
    let Some(Op::Paint {
        clip: Some(clip), ..
    }) = ops.last()
    else {
        panic!("a clipped stroke");
    };
    assert_eq!(
        session.document().store.masks[clip].bounds,
        moved.unwrap().mask().bounds
    );
}

#[test]
fn the_colour_history_keeps_each_colour_once_newest_first_up_to_256() {
    let colour = |index: usize| Rgba8([index as u8, (index >> 8) as u8, 0, 255]);
    let mut history = ColorHistory::default();
    for index in 0..300 {
        history.record(colour(index));
    }
    assert_eq!(history.colors().len(), ColorHistory::CAPACITY);
    assert_eq!(history.colors()[0], colour(299));
    assert_eq!(history.colors()[255], colour(44));
    history.record(colour(100));
    assert_eq!(history.colors()[0], colour(100));
    assert_eq!(history.colors()[1], colour(299));
    assert_eq!(history.colors().len(), ColorHistory::CAPACITY);
    let kept = ColorHistory::from_colors([colour(1), colour(2), colour(1)]);
    assert_eq!(kept.colors(), [colour(1), colour(2)]);
    assert_eq!(
        ColorHistory::from_colors((0..300).map(colour))
            .colors()
            .len(),
        ColorHistory::CAPACITY
    );
    history.clear();
    assert!(history.colors().is_empty());
}

#[test]
fn strokes_fills_and_text_record_their_colour_and_erasing_does_not() {
    let mut session = session();
    let mut tools = Tools::default();
    let red = Rgba8([255, 0, 0, 255]);
    let blue = Rgba8([0, 0, 255, 128]);
    let green = Rgba8([0, 255, 0, 255]);
    tools.color = red;
    draw(&mut session, &tools, 10.0, 60.0);
    assert_eq!(colors(&mut session, &mut tools), [red]);
    set_tool(&mut session, &mut tools, Tool::Eraser).unwrap();
    draw(&mut session, &tools, 10.0, 30.0);
    assert_eq!(colors(&mut session, &mut tools), [red]);
    set_tool(&mut session, &mut tools, Tool::Fill).unwrap();
    tools.color = blue;
    assert!(matches!(
        session.bucket(&tools, [60.0, 50.0], Some(boxed().as_slice())),
        Ok(Outcome::Committed(_))
    ));
    assert_eq!(colors(&mut session, &mut tools), [blue, red]);
    set_tool(&mut session, &mut tools, Tool::Text).unwrap();
    tools.color = green;
    session
        .place_text(&tools, [30.0, 40.0], boxed_text())
        .unwrap();
    session.apply_text().unwrap();
    assert_eq!(colors(&mut session, &mut tools), [green, blue, red]);
    // Drawing red again moves it to the front; undo keeps the history.
    set_tool(&mut session, &mut tools, Tool::Pen).unwrap();
    tools.color = red;
    draw(&mut session, &tools, 10.0, 60.0);
    session.undo().unwrap();
    assert_eq!(colors(&mut session, &mut tools), [red, green, blue]);
}

#[test]
fn the_settled_document_is_what_applying_gives_and_the_session_stays() {
    let mut session = session();
    let mut tools = Tools::default();
    assert_eq!(session.settled_document(), Ok(None));
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [10.0, 10.0],
        [30.0, 20.0],
        Combine::Replace,
    );
    session.begin_transform().unwrap();
    assert!(session.set_transform(Affine::translation(15.0, 5.0)));
    let revision = session.revision();
    let settled = session.settled_document().unwrap().unwrap();
    assert!(session.pending().is_some());
    assert_eq!(session.revision(), revision);
    session.apply_transform().unwrap();
    assert_eq!(&settled, session.document());
    // Text clipped to the moved selection.
    set_tool(&mut session, &mut tools, Tool::Text).unwrap();
    session
        .place_text(&tools, [30.0, 10.0], boxed_text())
        .unwrap();
    let settled = session.settled_document().unwrap().unwrap();
    assert!(session.placed_text().is_some());
    assert_ne!(&settled, session.document());
    session.apply_text().unwrap();
    assert_eq!(&settled, session.document());
}

#[test]
fn a_pixel_stroke_is_whole_pixels_wide_and_never_antialiased() {
    let mut session = session();
    let mut tools = Tools::default();
    let layer = session.current_layer();
    tools.fill.antialias = true;
    tools.choose_preset(
        session.pixel_mode(),
        Tool::Pen,
        ugu_core::brush::find("pixel-pencil").unwrap(),
    );
    assert!(!tools.fill.antialias, "pixel art fills aliased");
    tools.plain.pen.width = 2.6;
    tools.plain.pen.antialias = true;
    draw(&mut session, &tools, 10.0, 60.0);
    let Op::Paint { stroke, .. } = ops(&session, layer)[0] else {
        panic!("a stroke");
    };
    let stroke = &session.document().store.strokes[&stroke];
    assert_eq!(stroke.brush.engine, BrushEngine::Pixel);
    assert_eq!(stroke.width, 3.0);
    assert!(!stroke.brush.antialias);
}

/// A document at the operation limit, so no further edit fits.
fn full_session() -> (Session, Document) {
    let mut document = Document::new([8, 8]);
    let LayerKind::Paint(paint) = &mut document.layers[0].kind else {
        unreachable!()
    };
    paint.ops = vec![
        Op::Crop {
            offset: [0, 0],
            size: [8, 8]
        };
        limits::OPERATIONS
    ];
    document.validate().unwrap();
    (Session::new(document.clone(), true), document)
}

#[test]
fn a_transform_the_document_cannot_take_stays_pending_and_unsaved() {
    let (mut session, document) = full_session();
    let mut tools = Tools::default();
    session.select_all().unwrap();
    session.begin_transform().unwrap();
    assert!(session.set_transform(Affine::translation(1.0, 0.0)));
    assert!(session.settled_document().is_err());
    assert!(session.apply_transform().is_err());
    assert!(session.pending().is_some());
    assert!(session.is_dirty());
    // What would apply it first is refused instead.
    assert!(set_tool(&mut session, &mut tools, Tool::Eraser).is_err());
    assert_eq!(tools.tool, Tool::Pen);
    assert!(session.set_frame(3).is_err());
    assert_eq!(session.frame(), 0);
    assert!(session.pending().is_some());
    assert_eq!(session.document(), &document);
    // Undo still drops it.
    assert_eq!(session.undo(), Ok(true));
    assert!(session.pending().is_none());
    assert!(!session.is_dirty());
}

#[test]
fn text_the_document_cannot_take_stays_placed_and_unsaved() {
    let (mut session, document) = full_session();
    let mut tools = Tools::default();
    set_tool(&mut session, &mut tools, Tool::Text).unwrap();
    session
        .place_text(&tools, [1.0, 1.0], boxed_text())
        .unwrap();
    assert!(session.apply_text().is_err());
    assert!(session.placed_text().is_some());
    assert!(session.is_dirty());
    assert!(set_tool(&mut session, &mut tools, Tool::Pen).is_err());
    assert!(session.placed_text().is_some());
    assert_eq!(session.document(), &document);
}

#[test]
fn pixel_mode_is_one_step_and_strokes_take_the_pixel_brushes() {
    let mut session = session();
    let mut tools = Tools::default();
    tools.fill.antialias = true;
    let plain = tools.plain.clone();
    session.set_pixel_mode(true).unwrap();
    assert!(session.pixel_mode());
    assert_eq!(session.sampling(Sampling::Smooth), Sampling::Nearest);
    assert_eq!(session.set_pixel_mode(true).unwrap(), Outcome::NoChange);
    session.begin_stroke(&tools, at(10.0, 10.0, 0.0)).unwrap();
    assert_eq!(
        session.live().unwrap().template.brush.engine,
        BrushEngine::Pixel
    );
    session.cancel_stroke();

    // A fill in pixel mode never blends its edge, whatever the setting.
    let layer = session.current_layer();
    session
        .bucket(&tools, [100.0, 50.0], Some(&boxed()))
        .unwrap();
    assert!(
        fills(&session, layer)
            .iter()
            .all(|(_, _, antialias, _)| !antialias)
    );

    // Pixel mode takes pixel presets alone.
    tools.choose_preset(true, Tool::Pen, &ugu_core::brush::BRUSHES[0]);
    assert!(is_pixel(tools.pixel.pen.preset));

    // Undone, the mode goes and the brushes out of it were never touched.
    session.undo().unwrap();
    session.undo().unwrap();
    assert!(!session.pixel_mode());
    assert_eq!(session.sampling(Sampling::Smooth), Sampling::Smooth);
    assert_eq!(tools.plain, plain);
    assert!(tools.fill.antialias);
    session.begin_stroke(&tools, at(10.0, 10.0, 0.0)).unwrap();
    assert_eq!(
        session.live().unwrap().template.brush.engine,
        BrushEngine::Line
    );
}

#[test]
fn documents_in_and_out_of_pixel_mode_keep_their_own_brushes() {
    let mut tools = Tools::default();
    let mut plain = session();
    let mut pixel = session();
    pixel.set_pixel_mode(true).unwrap();
    tools.choose_preset(plain.pixel_mode(), Tool::Pen, &ugu_core::brush::BRUSHES[3]);
    tools.plain.pen.width = 17.0;
    tools.pixel.pen.width = 4.0;
    for _ in 0..2 {
        let layer = pixel.current_layer();
        draw(&mut pixel, &tools, 10.0, 60.0);
        let Some(Op::Paint { stroke, .. }) = ops(&pixel, layer).last().cloned() else {
            panic!("a stroke");
        };
        let stroke = &pixel.document().store.strokes[&stroke];
        assert_eq!(stroke.brush.engine, BrushEngine::Pixel);
        assert_eq!(stroke.width, 4.0);

        let layer = plain.current_layer();
        draw(&mut plain, &tools, 10.0, 60.0);
        let Some(Op::Paint { stroke, .. }) = ops(&plain, layer).last().cloned() else {
            panic!("a stroke");
        };
        let stroke = &plain.document().store.strokes[&stroke];
        assert_eq!(stroke.brush, ugu_core::brush::BRUSHES[3].brush);
        assert_eq!(stroke.width, 17.0);
    }

    // A pixel brush chosen out of pixel mode is the user's and stays.
    let pencil = ugu_core::brush::find("pixel-pencil").unwrap();
    tools.choose_preset(false, Tool::Pen, pencil);
    assert_eq!(tools.plain.pen.preset.id, pencil.id);
    assert!(!tools.fill.antialias, "pixel art fills aliased");
}

#[test]
fn pixel_mode_stays_through_animation_changes() {
    let mut session = session();
    session.set_pixel_mode(true).unwrap();
    let wobble = session.document().wobble;
    session.set_animation(12, 10.0, wobble).unwrap();
    assert!(session.pixel_mode());
}

#[test]
fn a_stroke_takes_the_tool_s_opacity_and_each_preset_keeps_its_own() {
    let mut session = session();
    let mut tools = Tools::default();
    tools.plain.pen.opacity = 0.4;
    draw(&mut session, &tools, 10.0, 60.0);
    let drawn = session.document().store.strokes.values().next().unwrap();
    assert_eq!(drawn.brush.opacity, 0.4);

    let other = &ugu_core::brush::BRUSHES[2];
    let first = tools.plain.pen.preset;
    tools.plain.choose(Tool::Pen, other);
    assert_eq!(tools.plain.pen.opacity, other.brush.opacity);
    tools.plain.choose(Tool::Pen, first);
    assert_eq!(tools.plain.pen.opacity, 0.4);
}

#[test]
fn a_pixel_brush_draws_opaque_whatever_the_opacity() {
    let mut session = session();
    session.set_pixel_mode(true).unwrap();
    let mut tools = Tools::default();
    tools.pixel.pen.opacity = 0.3;
    draw(&mut session, &tools, 10.0, 60.0);
    let drawn = session.document().store.strokes.values().next().unwrap();
    assert_eq!(drawn.brush.engine, BrushEngine::Pixel);
    assert_eq!(drawn.brush.opacity, 1.0);
}

#[test]
fn a_shape_is_one_pen_stroke_along_its_outline() {
    let mut session = session();
    let tools = Tools {
        tool: Tool::Shape,
        color: Rgba8([200, 10, 10, 255]),
        shape: ShapeSettings {
            shape: Shape::Rectangle,
            ..ShapeSettings::DEFAULT
        },
        ..Tools::default()
    };
    let layer = session.current_layer();
    session.begin_shape(&tools, [10.0, 20.0]).unwrap();
    assert!(session.drag_shape([30.0, 30.0], Drag::default()));
    assert!(!session.drag_shape([30.0, 30.0], Drag::default()));
    let outcome = session.end_shape([60.0, 70.0], Drag::default()).unwrap();
    assert!(matches!(outcome, Outcome::Committed(_)));
    assert!(session.live().is_none());
    let [Op::Paint { stroke, .. }] = ops(&session, layer)[..] else {
        panic!("one stroke");
    };
    let stroke = &session.document().store.strokes[&stroke];
    let pen = tools.plain.pen;
    assert_eq!(stroke.width, pen.width);
    assert_eq!(stroke.brush.engine, pen.preset.brush.engine);
    assert_eq!(stroke.color, tools.color);
    let expected: Vec<[f32; 2]> = outline(tools.shape, [10.0, 20.0], [60.0, 70.0], Drag::default())
        .into_iter()
        .map(|point| point.map(|value| value as f32))
        .collect();
    let points: Vec<[f32; 2]> = stroke
        .points
        .iter()
        .map(|point| [point.x, point.y])
        .collect();
    assert_eq!(points, expected);
    assert!(stroke.points.iter().all(|point| point.pressure == 1.0));
    assert_eq!(session.undo_label(), Some("Draw"));

    // A click draws nothing; Esc's cancel leaves nothing.
    session.begin_shape(&tools, [5.0, 5.0]).unwrap();
    assert_eq!(
        session.end_shape([5.2, 5.0], Drag::default()),
        Ok(Outcome::NoChange)
    );
    session.begin_shape(&tools, [5.0, 5.0]).unwrap();
    session.drag_shape([50.0, 50.0], Drag::default());
    session.cancel_stroke();
    assert_eq!(ops(&session, layer).len(), 1);
}

#[test]
fn a_filled_shape_fills_under_its_outline_in_one_step_or_fills_alone() {
    let mut session = session();
    let layer = session.current_layer();
    let mut tools = Tools {
        tool: Tool::Shape,
        color: Rgba8([10, 120, 200, 200]),
        shape: ShapeSettings {
            shape: Shape::Rectangle,
            paint: ShapePaint::Both,
            ..ShapeSettings::DEFAULT
        },
        ..Tools::default()
    };
    tools.plain.pen.opacity = 0.5;
    session.begin_shape(&tools, [10.0, 20.0]).unwrap();
    session.end_shape([60.0, 70.0], Drag::default()).unwrap();
    let [
        Op::Fill {
            coverage, color, ..
        },
        Op::Paint { .. },
    ] = ops(&session, layer)[..]
    else {
        panic!("a fill under a stroke: {:?}", ops(&session, layer));
    };
    assert_eq!(color, Rgba8([10, 120, 200, 100]));
    let mask = &session.document().store.masks[&coverage];
    assert!(mask.contains(35, 45));
    assert!(!mask.contains(5, 45) && !mask.contains(65, 45));
    assert_eq!(session.undo_label(), Some("Draw"));
    session.undo().unwrap();
    assert!(ops(&session, layer).is_empty());

    tools.shape.paint = ShapePaint::Fill;
    session.begin_shape(&tools, [10.0, 20.0]).unwrap();
    assert!(!session.live().unwrap().outlined);
    session.end_shape([60.0, 70.0], Drag::default()).unwrap();
    assert!(matches!(ops(&session, layer)[..], [Op::Fill { .. }]));

    // A line has nothing inside, so it is drawn whatever the choice.
    tools.shape.shape = Shape::Line;
    session.begin_shape(&tools, [10.0, 20.0]).unwrap();
    session.end_shape([60.0, 70.0], Drag::default()).unwrap();
    assert!(matches!(
        ops(&session, layer)[..],
        [Op::Fill { .. }, Op::Paint { .. }]
    ));
}

#[test]
fn a_shape_in_pixel_mode_runs_through_pixel_centres() {
    let mut document = Document::new([64, 64]);
    document.pixel = true;
    let mut session = Session::new(document, false);
    let tools = Tools {
        tool: Tool::Shape,
        shape: ShapeSettings {
            shape: Shape::Line,
            ..ShapeSettings::DEFAULT
        },
        ..Tools::default()
    };
    session.begin_shape(&tools, [3.2, 4.9]).unwrap();
    session.end_shape([20.7, 4.1], Drag::default()).unwrap();
    let layer = session.current_layer();
    let [Op::Paint { stroke, .. }] = ops(&session, layer)[..] else {
        panic!("one stroke");
    };
    let stroke = &session.document().store.strokes[&stroke];
    assert_eq!(stroke.brush.engine, BrushEngine::Pixel);
    assert!(stroke.points.iter().all(|point| point.y == 4.5));
    let ends = [stroke.points[0].x, stroke.points.last().unwrap().x];
    assert_eq!(ends, [3.5, 20.5]);
}

#[test]
fn a_filled_shape_in_pixel_mode_takes_its_edge_pixels_on_every_side() {
    for shape in [Shape::Rectangle, Shape::Ellipse] {
        let mut document = Document::new([40, 40]);
        document.pixel = true;
        let mut session = Session::new(document, false);
        let tools = Tools {
            tool: Tool::Shape,
            shape: ShapeSettings {
                shape,
                paint: ShapePaint::Fill,
                ..ShapeSettings::DEFAULT
            },
            ..Tools::default()
        };
        // From the centre of pixel 10 to the centre of pixel 30 both ways.
        session.begin_shape(&tools, [10.2, 10.7]).unwrap();
        session.end_shape([30.9, 30.1], Drag::default()).unwrap();
        let layer = session.current_layer();
        let [Op::Fill { coverage, .. }] = ops(&session, layer)[..] else {
            panic!("one fill: {:?}", ops(&session, layer));
        };
        let mask = &session.document().store.masks[&coverage];
        for y in 0..40 {
            for x in 0..40 {
                // Mirrored about the middle, at 20.5, either way.
                let at = mask.contains(x, y);
                assert_eq!(at, mask.contains(40 - x, y), "{shape:?} {x},{y}");
                assert_eq!(at, mask.contains(x, 40 - y), "{shape:?} {x},{y}");
            }
        }
        assert!(mask.contains(10, 20) && mask.contains(30, 20), "{shape:?}");
        assert!(!mask.contains(9, 20) && !mask.contains(31, 20), "{shape:?}");
    }
}

fn moved_by(x: f64) -> ugu_core::keys::Pose {
    ugu_core::keys::Pose {
        offset: [x, 0.0],
        ..ugu_core::keys::Pose::REST
    }
}

#[test]
fn drawing_on_a_moved_layer_goes_into_its_own_pixels() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    // Moved 30 right on frame 0, back where it is drawn on frame 10.
    session.set_pose(layer, moved_by(30.0), [0.0, 0.0]).unwrap();
    session.set_frame(10).unwrap();
    session.set_pose(layer, moved_by(0.0), [0.0, 0.0]).unwrap();
    session.set_frame(0).unwrap();
    assert_eq!(session.to_layer([50.0, 50.0]), [20.0, 50.0]);
    draw(&mut session, &tools, 40.0, 80.0);
    let stroke = session.document().store.strokes.values().next().unwrap();
    let xs: Vec<f32> = stroke.points.iter().map(|point| point.x).collect();
    assert_eq!(xs[0], 10.0);
    assert_eq!(*xs.last().unwrap(), 50.0);
    // On frame 10 the layer, and so the stroke, is shown where it is drawn.
    assert_eq!(session.document().transform(layer, 10), None);
}

#[test]
fn a_selection_cuts_a_moved_layer_where_it_is_shown() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    session
        .set_pose(layer, moved_by(100.0), [0.0, 0.0])
        .unwrap();
    session.history_mut().unwrap().select(
        "Select",
        Selection::of_shape(
            &ugu_core::selection::Shape::Rectangle([110.0, 0.0], [150.0, 100.0]),
            [200, 100],
        ),
    );
    session.begin_stroke(&tools, at(120.0, 50.0, 0.0)).unwrap();
    let clip = session.live().unwrap().clip.clone().unwrap();
    assert_eq!(clip.mask().bounds[0], 10);
    session.cancel_stroke();
    // A selection over none of its pixels refuses the stroke.
    session.history_mut().unwrap().select(
        "Select",
        Selection::of_shape(
            &ugu_core::selection::Shape::Rectangle([0.0, 0.0], [50.0, 100.0]),
            [200, 100],
        ),
    );
    assert_eq!(
        session.begin_stroke(&tools, at(20.0, 50.0, 0.0)),
        Err(StrokeRefused::OutsideSelection)
    );
}

#[test]
fn a_pose_keys_the_frame_shown_and_toggling_takes_it_away() {
    let mut session = session();
    let layer = session.current_layer();
    session.set_frame(4).unwrap();
    session
        .set_pose(layer, moved_by(8.0), [100.0, 50.0])
        .unwrap();
    let keys = session.document().layer(layer).unwrap().keys().clone();
    assert_eq!(keys.pivot, [100.0, 50.0]);
    assert_eq!(keys.keys.len(), 1);
    assert_eq!(keys.keys[0].frame, 4);
    assert_eq!(session.undo_label(), Some("Move layer on this frame"));
    // A later pose keeps the first pivot.
    session.set_frame(9).unwrap();
    session.set_pose(layer, moved_by(2.0), [0.0, 0.0]).unwrap();
    assert_eq!(
        session.document().layer(layer).unwrap().keys().pivot,
        [100.0, 50.0]
    );
    // Toggling on a frame without a key keys the pose shown there.
    session.set_frame(6).unwrap();
    let shown = session.pose(layer);
    session.toggle_key(layer, [0.0, 0.0]).unwrap();
    assert_eq!(session.key_here(layer).unwrap().pose, shown);
    session.toggle_key(layer, [0.0, 0.0]).unwrap();
    assert!(session.key_here(layer).is_none());
    assert_eq!(session.neighbour_key(1), Some(9));
    assert_eq!(session.neighbour_key(-1), Some(4));
}

#[test]
fn a_dragged_pose_shows_until_it_is_keyed_or_dropped() {
    let mut session = session();
    let layer = session.current_layer();
    let revision = session.revision();
    session.drag_pose(layer, moved_by(5.0));
    assert_eq!(session.pose(layer), moved_by(5.0));
    assert!(session.shown_document().transform(layer, 0).is_some());
    assert!(session.document().transform(layer, 0).is_none());
    assert_eq!(session.revision(), revision);
    assert!(session.cancel_pose());
    assert_eq!(session.pose(layer), ugu_core::keys::Pose::REST);
    session.drag_pose(layer, moved_by(5.0));
    session.end_pose([0.0, 0.0]).unwrap();
    assert_eq!(session.pose(layer), moved_by(5.0));
    assert!(session.posing().is_none());
}

#[test]
fn a_pasted_key_takes_the_pose_and_easing_on_this_frame_in_one_step() {
    let mut session = session();
    let layer = session.current_layer();
    let key = ugu_core::keys::Key {
        frame: 0,
        pose: ugu_core::keys::Pose {
            offset: [5.0, 6.0],
            opacity: 0.5,
            ..ugu_core::keys::Pose::REST
        },
        ease: ugu_core::keys::Ease::Hold,
    };
    session.set_frame(3).unwrap();
    session.paste_key(layer, key, [1.0, 2.0]).unwrap();
    let pasted = session.key_here(layer).unwrap();
    assert_eq!(
        (pasted.frame, pasted.pose, pasted.ease),
        (3, key.pose, key.ease)
    );
    assert_eq!(
        session.document().layer(layer).unwrap().keys().pivot,
        [1.0, 2.0]
    );
    assert_eq!(session.undo_label(), Some("Paste key"));
}

#[test]
fn a_new_pivot_keeps_every_key_where_it_puts_the_layer() {
    let mut session = session();
    let layer = session.current_layer();
    let pose = ugu_core::keys::Pose {
        offset: [3.0, -2.0],
        rotation: 30.0,
        scale: 1.5,
        flip: true,
        opacity: 1.0,
    };
    session.set_pose(layer, pose, [10.0, 10.0]).unwrap();
    let before = session.document().transform(layer, 0).unwrap();
    session.set_pivot(layer, [80.0, 40.0]).unwrap();
    let after = session.document().transform(layer, 0).unwrap();
    for point in [[0.0, 0.0], [50.0, 70.0]] {
        let [a, b] = [before.apply(point), after.apply(point)];
        assert!((a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    }
}

#[test]
fn cels_follow_the_frame_and_an_empty_frame_gets_a_new_cel_to_draw_on() {
    let mut session = session();
    let tools = Tools::default();
    let first = session.current_layer();
    session.make_animation_folder().unwrap();
    let folder = session.folder().unwrap();
    assert_eq!(session.current_layer(), first);
    // Frame 5 shows nothing: the folder becomes current.
    session.set_frame(5).unwrap();
    session.expose(None).unwrap();
    assert_eq!(session.current_layer(), folder);
    session.set_frame(0).unwrap();
    assert_eq!(session.current_layer(), folder);
    session.select_layer(first).unwrap();
    session.set_frame(6).unwrap();
    assert_eq!(session.current_layer(), folder);
    // Drawing there adds a cel shown from frame 6, without asking.
    draw(&mut session, &tools, 10.0, 40.0);
    let cel = session.current_layer();
    assert_ne!(cel, first);
    assert_eq!(session.exposed(folder), Some(cel));
    assert_eq!(session.undo_label(), Some("Draw"));
    session.set_frame(2).unwrap();
    assert_eq!(session.current_layer(), first);
    session.set_frame(7).unwrap();
    assert_eq!(session.current_layer(), cel);
    // New cel on another frame.
    session.set_frame(12).unwrap();
    session.new_cel().unwrap();
    let third = session.current_layer();
    assert_eq!(session.exposed(folder), Some(third));
    assert_eq!(session.document().validate(), Ok(()));
}

#[test]
fn a_cel_added_to_draw_on_is_one_step_with_the_drawing_and_gone_without_it() {
    let mut session = session();
    let tools = Tools::default();
    session.make_animation_folder().unwrap();
    let folder = session.folder().unwrap();
    session.set_frame(6).unwrap();
    session.expose(None).unwrap();
    let cels = |session: &Session| match &session.document().layer(folder).unwrap().kind {
        LayerKind::Group(group) => group.children.len(),
        LayerKind::Paint(_) => unreachable!(),
    };
    let label = session.undo_label().map(str::to_owned);
    // A stroke given up leaves no cel and no step.
    session.begin_stroke(&tools, at(10.0, 50.0, 0.0)).unwrap();
    session.cancel_stroke();
    assert_eq!(cels(&session), 1);
    assert_eq!(session.undo_label().map(str::to_owned), label);
    assert_eq!(session.current_layer(), folder);
    // Nor does one refused, or a fill, outside the selection.
    session.history_mut().unwrap().select(
        "Select",
        Selection::of_shape(
            &ugu_core::selection::Shape::Rectangle([0.0, 0.0], [5.0, 5.0]),
            [200, 100],
        ),
    );
    assert_eq!(
        session.bucket(&tools, [100.0, 50.0], None),
        Err(FillError::OutsideSelection)
    );
    assert_eq!(cels(&session), 1);
    assert_eq!(session.current_layer(), folder);
    session.deselect().unwrap();
    // One drawn is one step with its cel.
    draw(&mut session, &tools, 10.0, 40.0);
    assert_eq!(cels(&session), 2);
    assert_eq!(session.undo_label(), Some("Draw"));
    session.undo().unwrap();
    assert_eq!(cels(&session), 1);
    assert_eq!(session.undo_label(), Some("Deselect"));
}

#[test]
fn fewer_frames_drop_the_keys_past_them_in_the_same_step() {
    let mut session = session();
    let layer = session.current_layer();
    session.set_frame(25).unwrap();
    session.set_pose(layer, moved_by(3.0), [0.0, 0.0]).unwrap();
    let document = session.document();
    let (fps, wobble) = (document.frames_per_second, document.wobble);
    session.set_animation(10, fps, wobble).unwrap();
    assert!(session.document().layer(layer).unwrap().keys().is_empty());
    session.undo().unwrap();
    assert_eq!(
        session.document().layer(layer).unwrap().keys().keys.len(),
        1
    );
}

#[test]
fn a_bucket_on_a_moved_layer_fills_its_own_pixels_under_the_click() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    session.set_pose(layer, moved_by(50.0), [0.0, 0.0]).unwrap();
    // A reference of the layer alone in its own pixels: transparent.
    let reference = vec![[0u8; 4]; 200 * 100];
    session
        .bucket(&tools, [60.0, 50.0], Some(&reference))
        .unwrap();
    let Op::Fill { coverage, .. } = ops(&session, layer)[0] else {
        panic!("a fill");
    };
    let mask = &session.document().store.masks[&coverage];
    // The whole layer is one area; it is filled where it is drawn.
    assert_eq!(mask.bounds, [0, 0, 200, 100]);
}

/// The strokes drawn on `layer`, in order.
fn strokes_on(session: &Session, layer: LayerId) -> Vec<Stroke> {
    ops(session, layer)
        .iter()
        .filter_map(|op| match op {
            Op::Paint { stroke, .. } | Op::Erase { stroke, .. } => {
                Some(session.document().store.strokes[stroke].clone())
            }
            _ => None,
        })
        .collect()
}

fn mirrored(session: &mut Session) {
    let symmetry = Symmetry {
        on: true,
        ..session.symmetry()
    };
    session.set_symmetry(symmetry);
}

#[test]
fn the_symmetry_ruler_draws_the_mirrored_stroke_in_the_same_undo_step() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    mirrored(&mut session);
    draw(&mut session, &tools, 10.0, 60.0);
    let strokes = strokes_on(&session, layer);
    assert_eq!(strokes.len(), 2);
    let [hand, copy] = [&strokes[0], &strokes[1]];
    assert_eq!(hand.points.len(), copy.points.len());
    for (hand, copy) in hand.points.iter().zip(copy.points.iter()) {
        // Mirrored about x = 100, the middle of the 200 wide canvas.
        assert_eq!(
            [copy.x, copy.y, copy.pressure],
            [200.0 - hand.x, hand.y, hand.pressure]
        );
    }
    assert_ne!(hand.seed, copy.seed, "each wobbles as a stroke of its own");
    assert_eq!(
        [hand.width, hand.brush.opacity],
        [copy.width, copy.brush.opacity]
    );
    assert!(session.undo().unwrap());
    assert!(ops(&session, layer).is_empty());
}

/// Two closed boxes mirrored about x = 100, the ruler's upright line.
fn two_boxes() -> Vec<[u8; 4]> {
    let mut pixels = vec![[0; 4]; 200 * 100];
    for [left, right] in [[20, 60], [140, 180]] {
        for y in 20..80 {
            for x in left..right {
                if !(left + 2..right - 2).contains(&x) || !(22..78).contains(&y) {
                    pixels[y * 200 + x] = [0, 0, 0, 255];
                }
            }
        }
    }
    pixels
}

#[test]
fn the_ruler_fills_the_mirrored_area_too_in_one_fill() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    let reference = two_boxes();
    let read = Some(reference.as_slice());
    let filled = |session: &Session| -> Vec<[i32; 4]> {
        fills(session, layer)
            .into_iter()
            .map(|(coverage, ..)| session.document().store.masks[&coverage].bounds)
            .collect()
    };
    session.bucket(&tools, [40.0, 50.0], read).unwrap();
    assert_eq!(filled(&session), [[21, 21, 38, 58]]);
    assert!(session.undo().unwrap());
    // With the ruler, the mirrored box is filled by the same fill, undone
    // in one step.
    mirrored(&mut session);
    session.bucket(&tools, [40.0, 50.0], read).unwrap();
    assert_eq!(filled(&session), [[21, 21, 158, 58]]);
    assert!(session.undo().unwrap());
    assert!(fills(&session, layer).is_empty());
    // A repeat that lands in the area clicked adds nothing.
    let mut wide = vec![[0; 4]; 200 * 100];
    for y in 20..80 {
        for x in 20..180 {
            if !(22..178).contains(&x) || !(22..78).contains(&y) {
                wide[y * 200 + x] = [0, 0, 0, 255];
            }
        }
    }
    session
        .bucket(&tools, [40.0, 50.0], Some(wide.as_slice()))
        .unwrap();
    assert_eq!(filled(&session), [[21, 21, 158, 58]]);
}

#[test]
fn the_ruler_mirrors_erasing_and_the_lift_too() {
    let mut session = session();
    let mut tools = Tools::default();
    let layer = session.current_layer();
    mirrored(&mut session);
    tools.tool = Tool::Eraser;
    session.begin_stroke(&tools, at(10.0, 50.0, 0.0)).unwrap();
    session.extend_stroke(at(20.0, 50.0, 8.0));
    // Lifted close to the last point, which the lift replaces.
    session.end_stroke(at(20.25, 50.5, 16.0)).unwrap();
    let ops = ops(&session, layer);
    assert!(matches!(ops[..], [Op::Erase { .. }, Op::Erase { .. }]));
    let strokes = strokes_on(&session, layer);
    let last = |stroke: &Stroke| *stroke.points.last().unwrap();
    assert_eq!([last(&strokes[0]).x, last(&strokes[0]).y], [20.25, 50.5]);
    assert_eq!([last(&strokes[1]).x, last(&strokes[1]).y], [179.75, 50.5]);
    assert_eq!(strokes[0].points.len(), strokes[1].points.len());
}

#[test]
fn the_ruler_is_on_the_canvas_as_shown_on_a_moved_layer() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    session.set_pose(layer, moved_by(30.0), [0.0, 0.0]).unwrap();
    mirrored(&mut session);
    draw(&mut session, &tools, 40.0, 60.0);
    let strokes = strokes_on(&session, layer);
    // Shown at 40, mirrored to 160 on the canvas: 10 and 130 in the layer.
    assert_eq!(strokes[0].points[0].x, 10.0);
    assert_eq!(strokes[1].points[0].x, 130.0);
}

#[test]
fn the_rulers_centre_keeps_its_share_of_the_canvas() {
    let mut session = session();
    let symmetry = Symmetry {
        centre: [50.0, 25.0],
        ..session.symmetry()
    };
    session.set_symmetry(symmetry);
    session
        .resample_image([400, 200], Sampling::Smooth)
        .unwrap();
    assert_eq!(session.symmetry().centre, [100.0, 50.0]);
    session.undo().unwrap();
    assert_eq!(session.symmetry().centre, [50.0, 25.0]);
}

#[test]
fn shapes_are_not_repeated_by_the_ruler() {
    let mut session = session();
    let tools = Tools::default();
    let layer = session.current_layer();
    mirrored(&mut session);
    session.begin_shape(&tools, [10.0, 10.0]).unwrap();
    session.drag_shape([40.0, 40.0], Drag::default());
    session.end_shape([40.0, 40.0], Drag::default()).unwrap();
    assert_eq!(strokes_on(&session, layer).len(), 1);
}
