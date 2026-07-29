//! Layout tests: where widgets actually land, and in what order they paint.
//!
//! These drive a real [`Ui`] against a [`RecordingPainter`] and assert on the
//! primitives that come out — no GPU, no window. Everything here goes through
//! the public API only, which doubles as a check that the API is usable from
//! outside the crate.

use slmsttaa_ui::{
    font, Anchor, DrawCmd, Layer, RecordingPainter, Rect, Ui, UiInput, UiState, Weight,
};

/// The panel's geometry, restated here rather than imported: a test that reads
/// the same constant as the code can't catch it changing. Update deliberately if
/// the panel is restyled.
const MARGIN: f32 = 12.0;
const PANEL_W: f32 = 340.0;
const PAD: f32 = 10.0;
const ROW_H: f32 = 24.0;
const CONTENT_X: f32 = MARGIN + PAD;

/// A viewport big enough that a right-anchored panel has somewhere to go.
const VIEWPORT: (f32, f32) = (1280.0, 720.0);

/// The recorded *fills*, in call order. Strokes are skipped: they are outlines
/// drawn over a fill of the same bounds, and would double every entry.
fn rects(p: &RecordingPainter) -> Vec<Rect> {
    p.cmds
        .iter()
        .filter_map(|c| match *c {
            DrawCmd::Rect {
                rect, border: 0.0, ..
            } => Some(rect),
            _ => None,
        })
        .collect()
}

/// The recorded text runs, as `(x, y, text)`, in call order.
fn texts(p: &RecordingPainter) -> Vec<(f32, f32, String)> {
    p.cmds
        .iter()
        .filter_map(|c| match c {
            DrawCmd::Text { x, y, text, .. } => Some((*x, *y, text.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn panel_background_paints_behind_widgets_despite_being_declared_last() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.label("hello");
        });
    }

    // Declared last — it is the only point at which the height is known...
    let last = painter.cmds.last().unwrap();
    assert_eq!(last.layer(), Layer::Base);

    // ...but painted first, because Base flushes before Panel. This is the
    // whole reason layers exist.
    let ordered = painter.in_layer_order();
    match ordered[0] {
        DrawCmd::Rect { rect, radius, .. } => {
            assert_eq!((rect.x, rect.y, rect.w), (MARGIN, MARGIN, PANEL_W));
            assert!(*radius > 0.0, "the panel has rounded corners");
        }
        other => panic!("expected the panel background, got {other:?}"),
    }
    // The panel is a fill plus a hairline border, both behind the widgets.
    assert_eq!(ordered[1].layer(), Layer::Base);
    assert!(
        matches!(ordered[1], DrawCmd::Rect { border, .. } if *border > 0.0),
        "expected the panel border"
    );
    assert!(ordered[2..].iter().all(|c| c.layer() == Layer::Panel));
}

#[test]
fn panel_height_fits_its_contents_on_the_very_first_frame() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            for _ in 0..5 {
                ui.label("row");
            }
        });
    }

    // No one-frame lag: the background is emitted after layout, so it is right
    // immediately. (Before draw layers this was `ROW_H + PAD` on frame 1 and
    // only correct from frame 2 on.)
    let bg = painter.in_layer_order()[0];
    match bg {
        // Top pad, five rows, bottom pad.
        DrawCmd::Rect { rect, .. } => assert_eq!(rect.h, PAD + 5.0 * ROW_H + PAD),
        other => panic!("expected the panel background, got {other:?}"),
    }
}

#[test]
fn labels_stack_one_row_apart() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.label("first");
            ui.label("second");
        });
    }

    let runs = texts(&painter);
    assert_eq!(runs[0].2, "first");
    assert_eq!(runs[1].2, "second");
    // Both flush left inside the panel padding, one row apart.
    assert_eq!(runs[0].0, CONTENT_X);
    assert_eq!(runs[1].0, CONTENT_X);
    assert_eq!(runs[1].1 - runs[0].1, ROW_H);
    // The first row starts one pad below the panel's top edge — but the *text*
    // inside it no longer starts there. A label centres its ink in its row, which
    // under the bitmap font was indistinguishable from drawing at the row top
    // (cell height == row text height) and is not any more.
    let row_y = MARGIN + PAD;
    assert_eq!(runs[0].1, font::centered_top(row_y, ROW_H, 19.0));
    assert!(
        runs[0].1 > row_y && runs[0].1 < row_y + ROW_H,
        "the run sits inside its row"
    );
}

#[test]
fn slider_fill_tracks_the_value() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 5.0_f32;
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.slider("half", &mut value, 0.0, 10.0).show();
        });
    }

    // Track, fill, knob (the background is recorded after them, when the panel
    // closes).
    let r = rects(&painter);
    let track_w = r[0].w;
    let fill_w = r[1].w;
    assert_eq!(fill_w, track_w * 0.5);
}

/// The height of a panel's background — the only public read-out of how tall it
/// came out, and exactly what a viewer sees.
fn panel_height(p: &RecordingPainter) -> f32 {
    match p.in_layer_order()[0] {
        DrawCmd::Rect { rect, .. } => rect.h,
        other => panic!("expected the panel background first, got {other:?}"),
    }
}

#[test]
fn collapsing_a_section_shrinks_the_panel_to_its_heading() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 0.0_f32;

    // One section, expanded by default, with content inside it.
    let mut frame = |input: UiInput, painter: &mut RecordingPainter, state: &mut UiState| {
        painter.clear();
        let mut ui = Ui::new(painter, input, state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.section("Shape", |ui| {
                ui.slider("frequency", &mut value, 0.0, 1.0).show();
                ui.slider("octaves", &mut value, 0.0, 1.0).show();
            });
        });
    };

    frame(UiInput::default(), &mut painter, &mut state);
    let expanded = panel_height(&painter);

    // Click the heading. Its row starts at the top of the panel's padding.
    let click = UiInput {
        cursor: Some((CONTENT_X + 20.0, MARGIN + PAD + 8.0)),
        primary_held: true,
        primary_pressed: true,
        ..Default::default()
    };
    frame(click, &mut painter, &mut state);
    let collapsed = panel_height(&painter);

    assert!(
        collapsed < expanded,
        "collapsed ({collapsed}) should be shorter than expanded ({expanded})"
    );

    // And it stays collapsed on later frames — the state outlives the frame.
    frame(UiInput::default(), &mut painter, &mut state);
    assert_eq!(panel_height(&painter), collapsed);
}

#[test]
fn two_panels_land_at_opposite_corners() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let input = UiInput {
        viewport: VIEWPORT,
        ..Default::default()
    };
    {
        let mut ui = Ui::new(&mut painter, input, &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.label("params");
        });
        ui.panel(Anchor::TopRight, 170.0, |ui| {
            ui.label("hud");
        });
    }

    let backgrounds: Vec<Rect> = painter
        .in_layer_order()
        .iter()
        .filter_map(|c| match **c {
            DrawCmd::Rect { rect, border, .. } if c.layer() == Layer::Base && border == 0.0 => {
                Some(rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(backgrounds.len(), 2, "one background per panel");

    let (left, right) = (backgrounds[0], backgrounds[1]);
    assert_eq!(left.x, MARGIN);
    // The right panel measures from the far edge, not from the left one.
    assert_eq!(right.max_x(), VIEWPORT.0 - MARGIN);
    assert_eq!(right.w, 170.0);
    // Same top edge; they do not overlap.
    assert_eq!(left.y, right.y);
    assert!(left.max_x() < right.x);
}

#[test]
fn a_narrow_panel_narrows_its_widgets() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 1.0_f32;
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, 200.0, |ui| {
            ui.slider("knob", &mut value, 0.0, 1.0).show();
        });
    }

    // A widget asking for "full width" gets the width of *its* panel, which is
    // the whole point of the available rect.
    let track = rects(&painter)[0];
    assert_eq!(track.x, CONTENT_X);
    assert_eq!(track.w, 200.0 - 2.0 * PAD);
}

#[test]
fn a_bottom_anchored_panel_settles_on_the_second_frame() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let input = UiInput {
        viewport: VIEWPORT,
        ..Default::default()
    };

    let frame = |painter: &mut RecordingPainter, state: &mut UiState| {
        painter.clear();
        let mut ui = Ui::new(painter, input, state);
        ui.panel(Anchor::BottomLeft, PANEL_W, |ui| {
            for _ in 0..4 {
                ui.label("row");
            }
        });
    };

    // Frame one places from a guess, so the bottom edge is wrong...
    frame(&mut painter, &mut state);
    let first = match painter.in_layer_order()[0] {
        DrawCmd::Rect { rect, .. } => *rect,
        other => panic!("expected the panel background, got {other:?}"),
    };
    assert_ne!(first.max_y(), VIEWPORT.1 - MARGIN);

    // ...and frame two, now knowing the height, sits on the margin exactly.
    frame(&mut painter, &mut state);
    let second = match painter.in_layer_order()[0] {
        DrawCmd::Rect { rect, .. } => *rect,
        other => panic!("expected the panel background, got {other:?}"),
    };
    assert_eq!(second.max_y(), VIEWPORT.1 - MARGIN);
    assert_eq!(second.h, PAD + 4.0 * ROW_H + PAD);
}

#[test]
fn a_text_run_is_taller_than_its_em_size() {
    // The assumption every assertion above used to rest on, now stated out loud.
    //
    // Under the 8x8 bitmap font a run was exactly `px` tall, so row heights could
    // be written as `px + something` and vertical centring as `(h - px) / 2`.
    // A real face has a line box *larger* than its em size — ascent plus descent
    // — and ink *smaller* than it. Both directions matter, and getting either
    // backwards puts text a few points off centre in every control at once, which
    // is exactly the kind of wrongness a screenshot makes you squint at.
    let px = 19.0;
    let line = font::line_height(px);
    let cap = font::cap_height(px);

    assert!(line > px, "line box {line} should exceed the em size {px}");
    assert!(
        cap < px,
        "capitals {cap} should be shorter than the em size {px}"
    );
    assert_eq!(font::text_size("abcd", px, Weight::Regular)[1], line);

    // Centring puts the capitals' midpoint on the box's midpoint — not the line
    // box's, which would leave the text sitting high by half the descender.
    let top = font::centered_top(100.0, 24.0, px);
    let cap_mid = top + font::ascent(px) - cap / 2.0;
    assert!(
        (cap_mid - 112.0).abs() < 0.001,
        "cap midpoint {cap_mid} should be the box midpoint 112"
    );
}
