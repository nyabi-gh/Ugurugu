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
    session.set_tool(Tool::Eraser);
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
    session.set_tool(Tool::Eraser);
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

fn drag(
    session: &mut Session,
    kind: ShapeKind,
    from: [f64; 2],
    to: [f64; 2],
    how: Combine,
) -> bool {
    session.selection_shape = kind;
    session.begin_selection(from, how);
    session.extend_selection([(from[0] + to[0]) / 2.0, from[1]]);
    session.end_selection(to).unwrap()
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
    session.selection_shape = ShapeKind::Freehand;
    session.begin_selection([-20.0, 10.0], Combine::Replace);
    assert!(!session.extend_selection([-19.0, 10.4]));
    for point in [[60.0, 10.0], [60.0, 300.0], [10.0, 60.0]] {
        assert!(session.extend_selection(point));
    }
    let lasso = session.lasso().unwrap();
    assert_eq!(lasso.points[0], [0.0, 10.0]);
    assert_eq!(lasso.points[2], [60.0, 100.0]);
    assert!(session.end_selection([10.0, 60.0]).unwrap());
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
    session.begin_selection([0.0, 0.0], Combine::Replace);
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
    assert!(!session.invert_selection());
    assert!(session.select_all());
    assert_eq!(selected(&session), Some([0, 0, 200, 100]));
    assert!(session.invert_selection());
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
    session.invert_selection();
    assert_eq!(selected(&session), Some([100, 0, 100, 100]));
}

#[test]
fn strokes_in_a_selection_are_cut_to_it_sharing_one_stored_mask() {
    let mut session = session();
    let layer = session.current_layer();
    draw(&mut session, 10.0, 60.0);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [20.0, 0.0],
        [40.0, 100.0],
        Combine::Replace,
    );
    draw(&mut session, 10.0, 60.0);
    session.set_tool(Tool::Eraser);
    draw(&mut session, 10.0, 60.0);
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
    session.select_all();
    session.set_tool(Tool::Pen);
    draw(&mut session, 10.0, 60.0);
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
            } => Some((coverage, color, antialias, clip)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_wand_selects_the_area_clicked_and_combines_like_a_shape() {
    let mut session = session();
    let reference = boxed();
    let read = Some(reference.as_slice());
    assert_eq!(session.wand([60.0, 50.0], Combine::Replace, read), Ok(true));
    assert_eq!(selected(&session), Some([42, 22, 76, 56]));
    assert_eq!(session.undo_label(), Some("Select area"));
    // The outside, added, rings the box.
    assert_eq!(session.wand([5.0, 5.0], Combine::Add, read), Ok(true));
    assert_eq!(selected(&session), Some([0, 0, 200, 100]));
    assert_eq!(
        session.wand([60.0, 50.0], Combine::Subtract, read),
        Ok(true)
    );
    assert!(!session.selection().unwrap().mask().contains(60, 50));
    // On a line nothing is found: replacing deselects and says why, adding
    // keeps the selection.
    assert_eq!(
        session.wand([40.5, 50.0], Combine::Add, read),
        Err(FillError::NothingThere)
    );
    assert!(session.selection().is_some());
    assert_eq!(
        session.wand([40.5, 50.0], Combine::Replace, read),
        Err(FillError::NothingThere)
    );
    assert!(session.selection().is_none());
    assert_eq!(
        session.wand([60.0, 50.0], Combine::Replace, None),
        Err(FillError::NoReference)
    );
    // Off the canvas, as a click with the selection tool.
    session.wand([60.0, 50.0], Combine::Replace, read).unwrap();
    assert_eq!(session.wand([-3.0, 50.0], Combine::Replace, read), Ok(true));
    assert!(session.selection().is_none());
    // Colour comparison with a tolerance passes the line.
    session.fill.by_colour = true;
    session.fill.tolerance = 255;
    session.wand([60.0, 50.0], Combine::Replace, read).unwrap();
    assert_eq!(selected(&session), Some([0, 0, 200, 100]));
}

#[test]
fn the_bucket_fills_the_area_clicked_once_and_only_inside_the_selection() {
    let mut session = session();
    let layer = session.current_layer();
    let reference = boxed();
    let read = Some(reference.as_slice());
    session.pen.color = Rgba8([200, 30, 30, 255]);
    assert!(matches!(
        session.bucket([60.0, 50.0], read),
        Ok(Outcome::Committed(_))
    ));
    let [(coverage, color, antialias, clip)] = fills(&session, layer)[..] else {
        panic!("one fill");
    };
    assert_eq!(
        (color, antialias, clip),
        (Rgba8([200, 30, 30, 255]), true, None)
    );
    assert_eq!(
        session.document().store.masks[&coverage].bounds,
        [42, 22, 76, 56]
    );
    assert_eq!(session.undo_label(), Some("Fill"));
    assert!(session.undo().unwrap());
    assert!(fills(&session, layer).is_empty());
    assert!(session.document().store.masks.is_empty());
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
        session.bucket([10.0, 10.0], read),
        Err(FillError::OutsideSelection)
    );
    session.bucket([60.0, 50.0], read).unwrap();
    let [(coverage, _, _, Some(clip))] = fills(&session, layer)[..] else {
        panic!("one fill cut to the selection");
    };
    assert_ne!(coverage, clip);
    assert_eq!(
        &session.document().store.masks[&clip],
        session.selection().unwrap().mask()
    );
    assert_eq!(
        session.bucket([41.0, 50.0], read),
        Err(FillError::OutsideSelection)
    );
    session.deselect();
    assert_eq!(
        session.bucket([41.0, 50.0], read),
        Err(FillError::NothingThere)
    );
    assert_eq!(session.bucket([300.0, 50.0], read), Ok(Outcome::NoChange));
    session
        .update_layer(layer, "Hide layer", |layer| layer.visible = false)
        .unwrap();
    assert_eq!(
        session.bucket([60.0, 50.0], read),
        Err(FillError::HiddenLayer)
    );
}

#[test]
fn filling_the_selection_or_a_painted_shape_is_one_fill_cut_to_the_selection() {
    let mut session = session();
    let layer = session.current_layer();
    assert_eq!(session.fill_selection(), Err(FillError::NoSelection));
    drag(
        &mut session,
        ShapeKind::Ellipse,
        [20.0, 20.0],
        [80.0, 70.0],
        Combine::Replace,
    );
    session.fill_selection().unwrap();
    let [(coverage, _, _, Some(clip))] = fills(&session, layer)[..] else {
        panic!("one fill cut to the selection");
    };
    // The selection is stored once, for both.
    assert_eq!(coverage, clip);
    assert_eq!(session.document().store.masks.len(), 1);
    session.select_all();
    session.fill_selection().unwrap();
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
    session.lasso_paints = true;
    assert!(drag(
        &mut session,
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
    assert!(!drag(
        &mut session,
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
    let layer = session.current_layer();
    draw(&mut session, 10.0, 60.0);
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
    draw(&mut session, 10.0, 30.0);
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
    session.set_frame(3);
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

#[test]
fn escape_cancels_a_pending_transform_before_deselecting_and_changing_tool_applies_it() {
    let mut session = session();
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
    session.set_tool(session.tool());
    assert!(session.pending().is_some(), "the same tool keeps it");
    session.set_tool(Tool::Fill);
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
    let source = session.current_layer();
    draw(&mut session, 10.0, 150.0);
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
        let layer = session.current_layer();
        draw(&mut session, 10.0, 60.0);
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
    draw(&mut session, 10.0, 60.0);
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
    session.selection_shape = ShapeKind::Freehand;
    session.begin_selection([10.0, 10.0], Combine::Replace);
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
    let layer = session.current_layer();
    session.set_tool(Tool::Text);
    session.place_text([30.0, 40.0], boxed_text()).unwrap();
    assert!(session.is_dirty());
    assert!(ops(&session, layer).is_empty());
    // Moving keeps the seeds: the strokes move as they are.
    let before = session.text_drawing().unwrap();
    session.place_text([35.0, 40.0], boxed_text()).unwrap();
    let after = session.text_drawing().unwrap();
    assert_eq!(after.strokes.len(), 2);
    for (a, b) in before.strokes.iter().zip(&after.strokes) {
        assert_eq!(a.seed, b.seed);
        assert_eq!(a.points[0].x + 5.0, b.points[0].x);
    }
    session.pen.color = Rgba8([10, 20, 30, 255]);
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
        .place_text([0.0, 0.0], Arc::new(ugu_core::text::Outline::default()))
        .unwrap();
    assert_eq!(session.apply_text(), Ok(Outcome::NoChange));
}

#[test]
fn filled_text_fills_under_its_outlines_and_keeps_inside_the_selection() {
    let mut session = session();
    let layer = session.current_layer();
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [0.0, 0.0],
        [40.0, 50.0],
        Combine::Replace,
    );
    session.text.filled = true;
    session.place_text([30.0, 40.0], boxed_text()).unwrap();
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
    let layer = session.current_layer();
    draw(&mut session, 10.0, 60.0);
    session.set_tool(Tool::Text);
    session.place_text([30.0, 20.0], boxed_text()).unwrap();
    let label = session.undo_label().map(str::to_owned);
    assert!(session.undo().unwrap());
    assert_eq!(session.placed_text(), None);
    assert_eq!(session.undo_label().map(str::to_owned), label);
    session.place_text([30.0, 20.0], boxed_text()).unwrap();
    assert!(session.escape());
    assert_eq!(session.placed_text(), None);
    assert_eq!(ops(&session, layer).len(), 1);

    // Changing tools or layers applies it.
    session.place_text([30.0, 20.0], boxed_text()).unwrap();
    session.set_tool(Tool::Pen);
    assert_eq!(ops(&session, layer).len(), 3);
    session.set_tool(Tool::Text);
    session.place_text([30.0, 20.0], boxed_text()).unwrap();
    session.add_layer().unwrap();
    assert_eq!(ops(&session, layer).len(), 5);
    assert_eq!(session.placed_text(), None);

    // A hidden layer takes no text.
    let hidden = session.current_layer();
    session
        .update_layer(hidden, "Hide", |layer| layer.visible = false)
        .unwrap();
    assert_eq!(
        session.place_text([0.0, 0.0], boxed_text()),
        Err(FillError::HiddenLayer)
    );
}

#[test]
fn the_eyedropper_sets_the_pen_colour_without_an_edit() {
    let mut session = Session::new(Document::new([200, 100]), true);
    session.set_tool(Tool::Eyedropper);
    let (revision, label) = (session.revision(), session.undo_label().map(str::to_owned));
    // Half-transparent red, premultiplied.
    assert!(session.pick_color([128, 0, 0, 128]));
    assert_eq!(session.pen.color, Rgba8([255, 0, 0, 128]));
    assert!(!session.pick_color([128, 0, 0, 128]), "the same colour");
    assert!(!session.pick_color([0, 0, 0, 0]), "a clear pixel");
    assert_eq!(session.pen.color, Rgba8([255, 0, 0, 128]));
    assert_eq!(session.eraser.color, ToolSettings::ERASER.color);
    assert_eq!(session.revision(), revision);
    assert_eq!(session.undo_label().map(str::to_owned), label);
    assert!(!session.is_dirty());
}

#[test]
fn picking_leaves_a_pending_transform_and_placed_text_pending() {
    let mut session = session();
    draw(&mut session, 10.0, 60.0);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [5.0, 40.0],
        [70.0, 60.0],
        Combine::Replace,
    );
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(20.0, 0.0));
    session.pick_color([0, 0, 255, 255]);
    assert!(session.pending().is_some());
    session.cancel_transform();
    session.set_tool(Tool::Text);
    session.place_text([30.0, 40.0], boxed_text()).unwrap();
    session.pick_color([0, 255, 0, 255]);
    assert!(session.placed_text().is_some());
}

#[test]
fn restyling_changes_what_the_selection_touches_in_one_step() {
    let mut session = session();
    let layer = session.current_layer();
    draw(&mut session, 10.0, 60.0);
    session.pen.width = 3.0;
    session.pen.color = Rgba8([0, 0, 255, 255]);
    draw(&mut session, 120.0, 180.0);
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
    draw(&mut session, 10.0, 60.0);
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
    session.set_tool(Tool::Text);
    session.place_text([30.0, 10.0], boxed_text()).unwrap();
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
    let layer = session.current_layer();
    draw(&mut session, 10.0, 60.0);
    session.select_all();
    session
        .update_layer(layer, "Hide", |layer| layer.visible = false)
        .unwrap();
    assert_eq!(session.touched(), Err(FillError::HiddenLayer));
}

#[test]
fn a_stroke_begun_while_a_transform_is_pending_is_cut_to_the_moved_selection() {
    let mut session = session();
    let layer = session.current_layer();
    draw(&mut session, 10.0, 60.0);
    drag(
        &mut session,
        ShapeKind::Rectangle,
        [5.0, 40.0],
        [70.0, 60.0],
        Combine::Replace,
    );
    session.begin_transform().unwrap();
    session.set_transform(Affine::translation(100.0, 0.0));
    session.begin_stroke(at(120.0, 50.0, 0.0)).unwrap();
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
    let red = Rgba8([255, 0, 0, 255]);
    let blue = Rgba8([0, 0, 255, 128]);
    let green = Rgba8([0, 255, 0, 255]);
    session.pen.color = red;
    draw(&mut session, 10.0, 60.0);
    assert_eq!(session.colors.colors(), [red]);
    session.set_tool(Tool::Eraser);
    draw(&mut session, 10.0, 30.0);
    assert_eq!(session.colors.colors(), [red]);
    session.set_tool(Tool::Fill);
    session.pen.color = blue;
    assert!(matches!(
        session.bucket([60.0, 50.0], Some(boxed().as_slice())),
        Ok(Outcome::Committed(_))
    ));
    assert_eq!(session.colors.colors(), [blue, red]);
    session.set_tool(Tool::Text);
    session.pen.color = green;
    session.place_text([30.0, 40.0], boxed_text()).unwrap();
    session.apply_text().unwrap();
    assert_eq!(session.colors.colors(), [green, blue, red]);
    // Drawing red again moves it to the front; undo keeps the history.
    session.set_tool(Tool::Pen);
    session.pen.color = red;
    draw(&mut session, 10.0, 60.0);
    session.undo().unwrap();
    assert_eq!(session.colors.colors(), [red, green, blue]);
}

#[test]
fn tools_carry_over_to_another_document() {
    let mut session = session();
    session.set_tool(Tool::Wand);
    session.pen.color = Rgba8([1, 2, 3, 255]);
    session.choose_preset(Tool::Pen, &ugu_core::brush::BRUSHES[3]);
    session.pen.width = 17.0;
    session.choose_preset(Tool::Pen, &ugu_core::brush::BRUSHES[0]);
    session.fill.tolerance = 90;
    session.text.size = 100.0;
    session.colors.record(Rgba8([9, 9, 9, 255]));
    let tools = session.tools();
    let mut next = Session::new(Document::new([50, 50]), true);
    assert_eq!(next.tools(), Tools::default());
    next.set_tools(tools.clone());
    assert_eq!(next.tools(), tools);
    next.choose_preset(Tool::Pen, &ugu_core::brush::BRUSHES[3]);
    assert_eq!(next.pen.width, 17.0);
    assert_eq!(next.tool(), Tool::Wand);
}
