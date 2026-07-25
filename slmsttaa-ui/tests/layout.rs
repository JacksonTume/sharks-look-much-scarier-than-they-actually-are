//! Layout tests: where widgets actually land, and in what order they paint.
//!
//! These drive a real [`Ui`] against a [`RecordingPainter`] and assert on the
//! primitives that come out — no GPU, no window. Everything here goes through
//! the public API only, which doubles as a check that the API is usable from
//! outside the crate.

use slmsttaa_ui::{DrawCmd, Layer, Painter, RecordingPainter, Rect, Ui, UiInput, UiState};

/// The panel's fixed geometry, restated here rather than imported: a test that
/// reads the same constant as the code can't catch it changing. Update
/// deliberately if the panel is restyled.
const PANEL_X: f32 = 12.0;
const PANEL_Y: f32 = 12.0;
const PANEL_W: f32 = 340.0;
const PAD: f32 = 10.0;
const ROW_H: f32 = 24.0;
const CONTENT_X: f32 = PANEL_X + PAD;

/// The recorded *fills*, in call order. Strokes are skipped: they are outlines
/// drawn over a fill of the same bounds, and would double every entry.
fn rects(p: &RecordingPainter) -> Vec<Rect> {
    p.cmds
        .iter()
        .filter_map(|c| match *c {
            DrawCmd::Rect { rect, border, .. } if border == 0.0 => Some(rect),
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
        ui.label("hello");
    }

    // Declared last — it is the only point at which the height is known...
    let last = painter.cmds.last().unwrap();
    assert_eq!(last.layer(), Layer::Base);

    // ...but painted first, because Base flushes before Panel. This is the
    // whole reason layers exist.
    let ordered = painter.in_layer_order();
    match ordered[0] {
        DrawCmd::Rect { rect, radius, .. } => {
            assert_eq!((rect.x, rect.y, rect.w), (PANEL_X, PANEL_Y, PANEL_W));
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
        for _ in 0..5 {
            ui.label("row");
        }
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
        ui.label("first");
        ui.label("second");
    }

    let runs = texts(&painter);
    assert_eq!(runs[0].2, "first");
    assert_eq!(runs[1].2, "second");
    // Both flush left inside the panel padding, one row apart.
    assert_eq!(runs[0].0, CONTENT_X);
    assert_eq!(runs[1].0, CONTENT_X);
    assert_eq!(runs[1].1 - runs[0].1, ROW_H);
    // The first row starts one pad below the panel's top edge.
    assert_eq!(runs[0].1, PANEL_Y + PAD);
}

#[test]
fn slider_fill_tracks_the_value() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 5.0_f32;
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.slider("half", &mut value, 0.0, 10.0);
    }

    // Track, fill, knob (the background is recorded after them, at drop).
    let r = rects(&painter);
    let track_w = r[0].w;
    let fill_w = r[1].w;
    assert_eq!(fill_w, track_w * 0.5);
}

/// The height of the panel background — the only public read-out of how tall
/// the panel came out, and exactly what a viewer sees.
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
        if ui.section("Shape").open {
            ui.slider("frequency", &mut value, 0.0, 1.0);
            ui.slider("octaves", &mut value, 0.0, 1.0);
        }
    };

    frame(UiInput::default(), &mut painter, &mut state);
    let expanded = panel_height(&painter);

    // Click the heading. Its row starts at the top of the panel's padding.
    let click = UiInput {
        cursor: Some((CONTENT_X + 20.0, PANEL_Y + PAD + 8.0)),
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
fn text_size_is_a_monospace_grid() {
    // Layout math assumes the engine's monospace bitmap font; the recorder has
    // to agree with it or every assertion above measures the wrong thing.
    let painter = RecordingPainter::default();
    assert_eq!(painter.text_size("abcd", 16.0), [64.0, 16.0]);
    assert_eq!(painter.text_size("", 16.0), [0.0, 16.0]);
}
