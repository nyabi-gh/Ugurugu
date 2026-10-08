// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use super::*;
use ugu_core::ops::Op;
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

fn draw(session: &mut Session, from: f64, to: f64) -> Outcome {
    session.begin_stroke(at(from, 50.0, 0.0)).unwrap();
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
    let layer = session.current_layer();
    assert!(!session.is_dirty() || session.undo_label().is_none());
    assert!(matches!(
        draw(&mut session, 10.0, 60.0),
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
    let revision = session.revision();
    session.begin_stroke(at(10.0, 10.0, 0.0)).unwrap();
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
    draw(&mut session, 10.0, 61.0);
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
    session.pen.stabilizer = 1.0;
    session.begin_stroke(at(0.0, 50.0, 0.0)).unwrap();
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
    let pressed = |x: f64, pressure: f32, time: f64| InputPoint {
        position: [x, 20.0],
        pressure: Some(pressure),
        time,
    };
    session.begin_stroke(pressed(10.0, 0.0, 0.0)).unwrap();
    session.extend_stroke(pressed(20.0, 0.4, 8.0));
    session.extend_stroke(pressed(30.0, 0.7, 16.0));
    session.end_stroke(pressed(40.0, 0.0, 24.0)).unwrap();
    let stroke = session.document().store.strokes.values().next().unwrap();
    let pressures: Vec<f32> = stroke.points.iter().map(|point| point.pressure).collect();
    // The first touch never reaches zero width; the lift carries 0.7.
    assert_eq!(pressures, [0.05, 0.4, 0.7, 0.7]);
}

#[test]
fn a_hidden_or_missing_layer_refuses_to_draw() {
    let mut session = session();
    let layer = session.current_layer();
    session
        .update_layer(layer, "Hide layer", |layer| layer.visible = false)
        .unwrap();
    assert_eq!(
        session.begin_stroke(at(1.0, 1.0, 0.0)),
        Err(StrokeRefused::HiddenLayer)
    );
    session.select_layer(LayerId(99));
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
        }),
    });
    // The layer inside the group is current from the start.
    let mut session = Session::new(document, true);
    assert_eq!(session.current_layer(), LayerId(1));
    assert_eq!(
        session.begin_stroke(at(1.0, 1.0, 0.0)),
        Err(StrokeRefused::HiddenLayer)
    );
    session
        .update_layer(LayerId(10), "Show group", |layer| layer.visible = true)
        .unwrap();
    assert_eq!(session.begin_stroke(at(1.0, 1.0, 0.0)), Ok(()));
}

#[test]
fn the_eraser_commits_an_erase_operation() {
    let mut session = session();
    draw(&mut session, 10.0, 60.0);
    session.tool = Tool::Eraser;
    draw(&mut session, 20.0, 30.0);
    let layer = session.current_layer();
    assert!(matches!(ops(&session, layer)[1], Op::Erase { .. }));
    assert_eq!(session.undo_label(), Some("Erase"));
}

#[test]
fn each_preset_keeps_its_width_and_stabilizer_and_draws_with_its_brush() {
    let find = |id| ugu_core::brush::find(id).unwrap();
    let mut session = session();
    session.pen.width = 11.0;
    session.pen.antialias = true;
    session.choose_preset(Tool::Pen, find("soft-airbrush"));
    assert_eq!(session.pen.width, find("soft-airbrush").size);
    session.pen.stabilizer = 0.4;
    draw(&mut session, 10.0, 60.0);
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
    session.choose_preset(Tool::Pen, find("ink-pen"));
    assert_eq!((session.pen.width, session.pen.stabilizer), (11.0, 0.0));
    session.choose_preset(Tool::Pen, find("soft-airbrush"));
    assert_eq!(
        (session.pen.width, session.pen.stabilizer),
        (find("soft-airbrush").size, 0.4)
    );
    // An eraser takes its preset's.
    session.tool = Tool::Eraser;
    session.choose_preset(Tool::Eraser, find("kneaded-eraser"));
    draw(&mut session, 20.0, 30.0);
    assert_eq!(brush(&session), find("kneaded-eraser").brush);
}

#[test]
fn strokes_get_different_seeds() {
    let mut session = session();
    draw(&mut session, 10.0, 60.0);
    draw(&mut session, 10.0, 60.0);
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
    session.select_layer(second);
    session.remove_layer().unwrap();
    assert_eq!(session.current_layer(), first);
    assert_eq!(session.remove_layer().unwrap(), Outcome::NoChange);
}

#[test]
fn merging_down_makes_the_lower_layer_current() {
    let mut session = session();
    let lower = session.current_layer();
    draw(&mut session, 10.0, 60.0);
    session.add_layer().unwrap();
    draw(&mut session, 10.0, 60.0);
    session.merge_down().unwrap();
    assert_eq!(session.current_layer(), lower);
    assert_eq!(session.document().layers.len(), 1);
    assert_eq!(session.undo_label(), Some("Merge down"));
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
    draw(&mut session, 10.0, 60.0);
    let saved = session.state();
    draw(&mut session, 10.0, 60.0);
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
    let mut times = Vec::new();
    for round in 0..100 {
        let y = f64::from(round) * 10.0;
        session.begin_stroke(at(10.0, y, 0.0)).unwrap();
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
    let bottom = session.current_layer();
    session.add_layer().unwrap();
    let middle = session.current_layer();
    session.add_layer().unwrap();
    let top = session.current_layer();

    session.select_layer(middle);
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

    session.select_layer(top);
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

    session.select_layer(group);
    assert_eq!(
        session.begin_stroke(at(10.0, 10.0, 0.0)),
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
    session.add_group().unwrap();
    let lower = session.current_layer();
    draw(&mut session, 10.0, 60.0);
    session.add_layer().unwrap();
    let upper = session.current_layer();
    draw(&mut session, 20.0, 70.0);
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
    let only = session.current_layer();
    session.add_group().unwrap();
    let group = session.document().layers[0].id;
    session.select_layer(group);
    assert_eq!(session.remove_layer().unwrap(), Outcome::NoChange);
    session.select_layer(only);
    assert_eq!(session.remove_layer().unwrap(), Outcome::NoChange);
    // Added beside the group, not in it.
    session.select_layer(group);
    session.add_layer().unwrap();
    session.select_layer(group);
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
