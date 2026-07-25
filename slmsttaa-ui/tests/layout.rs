//! Layout tests: where widgets actually land.
//!
//! These drive a real [`Ui`] against a [`RecordingPainter`] and assert on the
//! primitives that come out — no GPU, no window. Everything here goes through
//! the public API only, which doubles as a check that the API is usable from
//! outside the crate.

use slmsttaa_ui::{DrawCmd, Painter, RecordingPainter, Ui, UiInput, UiState};

/// The panel's fixed geometry, restated here rather than imported: these are
/// private constants, and a test that reads the same constant as the code can't
/// catch it changing. Update deliberately if the panel is restyled.
const PANEL_X: f32 = 12.0;
const PANEL_Y: f32 = 12.0;
const PANEL_W: f32 = 340.0;
const PAD: f32 = 10.0;
const ROW_H: f32 = 24.0;
const CONTENT_X: f32 = PANEL_X + PAD;

/// The recorded rectangles, as `(x, y, w, h)`.
fn rects(p: &RecordingPainter) -> Vec<(f32, f32, f32, f32)> {
    p.cmds
        .iter()
        .filter_map(|c| match *c {
            DrawCmd::Rect { x, y, w, h, .. } => Some((x, y, w, h)),
            _ => None,
        })
        .collect()
}

/// The recorded text runs, as `(x, y, text)`.
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
fn panel_background_is_drawn_before_any_widget() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.label("hello");
    }

    // The background is the very first primitive, so widgets land on top of it.
    let (x, y, w, _h) = rects(&painter)[0];
    assert_eq!((x, y, w), (PANEL_X, PANEL_Y, PANEL_W));
    assert!(matches!(painter.cmds[0], DrawCmd::Rect { .. }));
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
fn panel_height_grows_to_fit_its_contents() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    // Frame 1: nothing is known yet, so the background falls back to one row.
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        for _ in 0..5 {
            ui.label("row");
        }
    }
    let first_frame_bg = rects(&painter)[0].3;
    assert_eq!(first_frame_bg, ROW_H + PAD);

    // Frame 2: the background is sized from what frame 1 laid out. This is the
    // deliberate one-frame lag the ordered draw layers of UI Slice 1 retire.
    painter.clear();
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        for _ in 0..5 {
            ui.label("row");
        }
    }
    let second_frame_bg = rects(&painter)[0].3;
    // Top pad, five rows, bottom pad.
    assert_eq!(second_frame_bg, PAD + 5.0 * ROW_H + PAD);
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

    // Background, track, fill, knob — the fill is half the track's width.
    let r = rects(&painter);
    let track_w = r[1].2;
    let fill_w = r[2].2;
    assert_eq!(fill_w, track_w * 0.5);
}

#[test]
fn text_size_is_a_monospace_grid() {
    // Layout math assumes the engine's monospace bitmap font; the recorder has
    // to agree with it or every assertion above measures the wrong thing.
    let painter = RecordingPainter::default();
    assert_eq!(painter.text_size("abcd", 16.0), [64.0, 16.0]);
    assert_eq!(painter.text_size("", 16.0), [0.0, 16.0]);
}
