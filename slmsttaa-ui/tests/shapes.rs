//! The three primitives UI Slice 10 added: stroked paths, filled convex
//! polygons, and textured quads.
//!
//! These assert on **what was asked for**, not on what it became. That is the
//! whole reason the seam was drawn where it was: the toolkit hands the painter a
//! shape and the painter renders it, so [`DrawCmd::Polyline`] carries the points
//! a widget meant rather than the triangles one implementation chose. A test
//! that asserted on triangles would be a test of the overlay, written in the
//! crate that does not contain it.
//!
//! # What this file cannot claim
//!
//! Everything below the seam. The capsule distance field, the feather ring
//! around a filled polygon, the one-physical-pixel antialiasing band, and the
//! `Rgba8Unorm` round-trip an image's bytes make are all on a GPU, and this
//! project has no headless path to one (engine `ROADMAP.md`, *Waiting on a
//! roadblock* — engine-side readback is the unbuilt prerequisite). They are
//! checked by `cargo xtask shoot terrain --script capture/terrain-plot.script`
//! and by looking at it. Naming the gap beats implying it is covered.

use slmsttaa_ui::{
    Anchor, Color, DrawCmd, ImageId, Layer, Painter, RecordingPainter, Rect, Ui, UiInput, UiState,
};

const PANEL_W: f32 = 340.0;
const RED: Color = [1.0, 0.0, 0.0, 1.0];
const BLUE: Color = [0.0, 0.0, 1.0, 1.0];

/// Drive a closure against a fresh recorder, with no `Ui` in the way.
///
/// Most of these are statements about the painter itself rather than about
/// layout, so they talk to it directly — the same thing `clipping.rs` does for
/// its first few cases.
fn record(draw: impl FnOnce(&mut RecordingPainter)) -> RecordingPainter {
    let mut painter = RecordingPainter::default();
    draw(&mut painter);
    painter
}

#[test]
fn a_polyline_records_its_points_unchanged() {
    // The load-bearing test in this file. It pins the decision that the toolkit
    // does **not** tessellate: if anything above the seam ever starts decimating
    // a long series, closing an open path, or reordering points, this fails —
    // and it fails here rather than as a picture nobody compares.
    let path = [(0.0, 0.0), (10.0, 4.0), (20.0, 4.0), (30.0, 25.5)];
    let painter = record(|p| p.polyline(&path, 2.0, RED));

    let found: Vec<&DrawCmd> = painter.polylines().collect();
    match found.as_slice() {
        [DrawCmd::Polyline {
            points,
            width,
            color,
            ..
        }] => {
            assert_eq!(points.as_slice(), path.as_slice());
            assert_eq!(*width, 2.0);
            assert_eq!(*color, RED);
        }
        other => panic!("expected one polyline, got {other:?}"),
    }
}

#[test]
fn a_polyline_is_open_and_a_polygon_is_closed() {
    // Two contracts a consumer would otherwise have to discover by drawing one
    // and looking. A path does not join its ends; a polygon does not need its
    // first point repeated to.
    let triangle = [(0.0, 0.0), (10.0, 0.0), (5.0, 8.0)];

    let stroked = record(|p| p.polyline(&triangle, 1.0, RED));
    match &stroked.cmds[0] {
        DrawCmd::Polyline { points, .. } => {
            assert_eq!(points.len(), 3, "an open path keeps exactly its points");
            assert_ne!(
                points.first(),
                points.last(),
                "an open path does not close itself"
            );
        }
        other => panic!("expected a polyline, got {other:?}"),
    }

    let filled = record(|p| p.convex_polygon(&triangle, RED));
    match &filled.cmds[0] {
        DrawCmd::Polygon { points, .. } => {
            assert_eq!(
                points.len(),
                3,
                "a polygon closes implicitly, not by a point"
            );
        }
        other => panic!("expected a polygon, got {other:?}"),
    }
}

#[test]
fn degenerate_shapes_draw_nothing() {
    // The contract lives on the *trait*, so both implementations obey it and a
    // test can never see a command the screen would not draw.
    //
    // `stroke_rect` is in here because it is where this went wrong once already:
    // the overlay skipped a zero-width stroke from Slice 2 on, and the recorder
    // recorded one anyway, so the two painters disagreed about an empty draw for
    // eight slices. That is `text_size`'s failure in miniature and it was fixed
    // alongside these.
    let cases: Vec<(&str, RecordingPainter)> = vec![
        ("no points", record(|p| p.polyline(&[], 1.0, RED))),
        ("one point", record(|p| p.polyline(&[(0.0, 0.0)], 1.0, RED))),
        (
            "zero width",
            record(|p| p.polyline(&[(0.0, 0.0), (5.0, 5.0)], 0.0, RED)),
        ),
        (
            "negative width",
            record(|p| p.polyline(&[(0.0, 0.0), (5.0, 5.0)], -1.0, RED)),
        ),
        ("empty polygon", record(|p| p.convex_polygon(&[], RED))),
        (
            "two-point polygon",
            record(|p| p.convex_polygon(&[(0.0, 0.0), (1.0, 1.0)], RED)),
        ),
        (
            "zero-width stroke_rect",
            record(|p| p.stroke_rect(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0, 0.0, RED)),
        ),
    ];

    for (name, painter) in cases {
        assert!(
            painter.cmds.is_empty(),
            "{name} should draw nothing, drew {:?}",
            painter.cmds
        );
    }
}

#[test]
fn layer_and_clip_are_recorded_on_every_new_primitive() {
    // The likeliest implementation slip: a new emit path that forgets to stamp
    // the clip in force. `clipping.rs` makes this claim for rects and text; the
    // three new shapes belong under the same one.
    let region = Rect::new(0.0, 0.0, 50.0, 50.0);
    let painter = record(|p| {
        p.set_layer(Layer::Popup);
        p.push_clip(region);
        p.polyline(&[(0.0, 0.0), (5.0, 5.0)], 1.0, RED);
        p.convex_polygon(&[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)], RED);
        p.image_full(Rect::new(1.0, 1.0, 8.0, 8.0), ImageId::from_raw(0), RED);
        p.pop_clip();
    });

    assert_eq!(painter.cmds.len(), 3);
    for cmd in &painter.cmds {
        assert_eq!(cmd.layer(), Layer::Popup, "{cmd:?} lost its layer");
        assert_eq!(cmd.clip(), Some(region), "{cmd:?} lost its clip");
    }
}

#[test]
fn nested_clips_still_intersect_for_the_new_shapes() {
    // Clips shrink and never widen — the property a scroll area rests on. Worth
    // restating for shapes that arrived after the containers did.
    let outer = Rect::new(0.0, 0.0, 40.0, 40.0);
    let painter = record(|p| {
        p.push_clip(outer);
        // Wider than the region it sits inside, so a replacing clip would show.
        p.push_clip(Rect::new(0.0, 0.0, 400.0, 400.0));
        p.convex_polygon(&[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)], RED);
        p.pop_clip();
        p.pop_clip();
    });

    assert_eq!(painter.cmds[0].clip(), Some(outer));
}

#[test]
fn a_polygon_declared_first_can_still_paint_behind_a_line() {
    // What a chart actually does: an area fill in one layer and its curve in
    // another, declared in whatever order the drawing code finds convenient.
    let painter = record(|p| {
        p.set_layer(Layer::Panel);
        p.polyline(&[(0.0, 0.0), (10.0, 10.0)], 1.0, BLUE);
        p.set_layer(Layer::Base);
        p.convex_polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], RED);
    });

    // Declared second...
    assert!(matches!(painter.cmds[1], DrawCmd::Polygon { .. }));
    // ...painted first.
    let ordered = painter.in_layer_order();
    assert!(matches!(ordered[0], DrawCmd::Polygon { .. }));
    assert!(matches!(ordered[1], DrawCmd::Polyline { .. }));
}

#[test]
fn an_image_passes_its_uv_and_tint_through_untouched() {
    let id = ImageId::from_raw(3);
    let sub = [0.25, 0.5, 0.75, 1.0];
    let painter = record(|p| {
        p.image(Rect::new(4.0, 6.0, 20.0, 10.0), id, sub, BLUE);
        p.image_full(Rect::new(0.0, 0.0, 1.0, 1.0), id, RED);
    });

    let mut images = painter.images();
    match images.next().expect("the sub-rect draw") {
        DrawCmd::Image {
            rect,
            image,
            uv,
            tint,
            ..
        } => {
            assert_eq!(*rect, Rect::new(4.0, 6.0, 20.0, 10.0));
            assert_eq!(*image, id);
            assert_eq!(*uv, sub);
            assert_eq!(*tint, BLUE);
        }
        other => panic!("expected an image, got {other:?}"),
    }
    match images.next().expect("the whole-image draw") {
        // The convenience is exactly the general call with the whole sheet.
        DrawCmd::Image { uv, .. } => assert_eq!(*uv, [0.0, 0.0, 1.0, 1.0]),
        other => panic!("expected an image, got {other:?}"),
    }

    // `ImageId` survives the round trip a consumer puts it through, which is the
    // only reason `from_raw`/`raw` are public.
    assert_eq!(ImageId::from_raw(id.raw()), id);
}

#[test]
fn a_chart_built_from_the_public_seam_is_clipped_by_its_scroll_area() {
    // The integration statement, and the one that matters most: a *consumer* can
    // write a chart from `allocate` + `painter` alone, and a container the
    // toolkit already had will clip it without knowing what it is.
    //
    // Nothing here is a widget this crate ships. That is the point — the wishlist
    // asked for three painter primitives on the argument that charts are content,
    // and this is that argument compiled.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let samples = [0.0f32, 0.6, 0.2, 1.0, 0.35];
    let scrub = 0.5f32;

    let viewport = {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        let mut viewport = Rect::new(0.0, 0.0, 0.0, 0.0);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            viewport = ui.scroll_area("body", 120.0, |ui| {
                let plot = ui.allocate([0.0, 60.0]);
                let step = plot.w / (samples.len() - 1) as f32;
                let pts: Vec<(f32, f32)> = samples
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (plot.x + i as f32 * step, plot.max_y() - v * plot.h))
                    .collect();

                let marker = plot.x + scrub * plot.w;
                let p = ui.painter();
                for pair in pts.windows(2) {
                    p.convex_polygon(
                        &[
                            pair[0],
                            pair[1],
                            (pair[1].0, plot.max_y()),
                            (pair[0].0, plot.max_y()),
                        ],
                        [1.0, 0.0, 0.0, 0.2],
                    );
                }
                p.polyline(&pts, 1.5, RED);
                p.polyline(&[(marker, plot.y), (marker, plot.max_y())], 1.0, BLUE);
                plot
            });
        });
        viewport
    };

    // Every point the chart drew landed inside the box it was given. A chart that
    // computes its own coordinates is exactly the thing that gets this wrong.
    let mut points = 0;
    for cmd in &painter.cmds {
        let pts = match cmd {
            DrawCmd::Polyline { points, .. } | DrawCmd::Polygon { points, .. } => points,
            _ => continue,
        };
        for &(x, y) in pts {
            assert!(
                x >= viewport.x - 0.01 && x <= viewport.max_x() + 0.01,
                "{x} is outside the plot's own rect {viewport:?}",
            );
            assert!(
                y >= viewport.y - 0.01 && y <= viewport.max_y() + 0.01,
                "{y} is outside the plot's own rect {viewport:?}",
            );
            points += 1;
        }
        // And the scroll area clipped it without being told what it was holding.
        assert!(
            cmd.clip().is_some(),
            "a shape inside a scroll area should carry its clip",
        );
    }
    assert!(points > 0, "the chart drew nothing to check");

    // The marker is a linear function of the scrub, which is the one arithmetic
    // claim a plot makes that a reader can check by eye.
    let marker = painter
        .polylines()
        .filter_map(|c| match c {
            DrawCmd::Polyline { points, color, .. } if *color == BLUE => Some(points[0].0),
            _ => None,
        })
        .next()
        .expect("the scrub marker");
    assert!((marker - (viewport.x + scrub * viewport.w)).abs() < 0.01);
}
