//! Region tests: rows, columns, indent, and right-alignment.
//!
//! These are the primitives UI Slice 3 added, and they are exactly the kind of
//! arithmetic that is easy to get subtly wrong and invisible until someone
//! squints at a screenshot. Everything goes through the public API against a
//! [`RecordingPainter`].

use slmsttaa_ui::{Anchor, DrawCmd, Painter, RecordingPainter, Rect, Ui, UiInput, UiState};

/// Restated rather than imported, as elsewhere in this suite.
const MARGIN: f32 = 12.0;
const PANEL_W: f32 = 340.0;
const PAD: f32 = 10.0;
const GAP: f32 = 8.0;
const INDENT: f32 = 16.0;
const ROW_H: f32 = 24.0;
const TEXT_PX: f32 = 16.0;
const CONTENT_X: f32 = MARGIN + PAD;
const CONTENT_W: f32 = PANEL_W - 2.0 * PAD;

/// Run one frame's worth of widgets inside a default top-left panel.
fn frame<T>(declare: impl FnOnce(&mut Ui) -> T) -> (RecordingPainter, T) {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let result = {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, declare)
    };
    (painter, result)
}

/// The recorded text runs as `(x, y, text)`, in call order.
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
fn horizontal_packs_left_to_right_with_a_gap() {
    let (_, rects) = frame(|ui| {
        ui.horizontal(|ui| {
            let a = ui.sized([80.0, ROW_H]).label("a").rect;
            let b = ui.sized([60.0, ROW_H]).label("b").rect;
            (a, b)
        })
    });
    let (a, b) = rects;

    assert_eq!((a.x, a.w), (CONTENT_X, 80.0));
    assert_eq!((b.x, b.w), (CONTENT_X + 80.0 + GAP, 60.0));
    // Same line, not stacked.
    assert_eq!(a.y, b.y);
    assert_eq!(a.y, MARGIN + PAD);
}

#[test]
fn zero_width_in_a_row_takes_the_remaining_space() {
    let (_, rects) = frame(|ui| {
        ui.horizontal(|ui| {
            let fixed = ui.sized([100.0, ROW_H]).label("fixed").rect;
            // The idiom: everything but the last member gets an explicit size.
            let rest = ui.label("rest").rect;
            (fixed, rest)
        })
    });
    let (fixed, rest) = rects;

    assert_eq!(rest.x, CONTENT_X + 100.0 + GAP);
    assert_eq!(rest.max_x(), CONTENT_X + CONTENT_W);
    assert_eq!(rest.w, CONTENT_W - fixed.w - GAP);
}

#[test]
fn a_row_advances_the_parent_by_its_tallest_member() {
    let (_, after) = frame(|ui| {
        ui.horizontal(|ui| {
            ui.sized([80.0, ROW_H]).label("short");
            ui.sized([80.0, 3.0 * ROW_H]).label("tall");
        });
        // Whatever comes next must clear the whole row, not just the first cell.
        ui.label("below").rect
    });

    assert_eq!(after.y, MARGIN + PAD + 3.0 * ROW_H);
    assert_eq!(after.x, CONTENT_X);
    assert_eq!(
        after.w, CONTENT_W,
        "the row did not leak its narrower width"
    );
}

#[test]
fn right_packs_against_the_content_edge() {
    let (_, rects) = frame(|ui| {
        ui.right(|ui| {
            let outer = ui.sized([50.0, ROW_H]).label("outer").rect;
            let inner = ui.sized([30.0, ROW_H]).label("inner").rect;
            (outer, inner)
        })
    });
    let (outer, inner) = rects;

    // First declared is hard against the right edge; the next one is inboard.
    assert_eq!(outer.max_x(), CONTENT_X + CONTENT_W);
    assert_eq!(inner.max_x(), outer.x - GAP);
}

#[test]
fn label_value_puts_the_value_on_the_right_edge() {
    let (painter, _) = frame(|ui| ui.label_value("area exponent m", "0.50"));
    let runs = texts(&painter);

    assert_eq!(runs[0].2, "area exponent m");
    assert_eq!(runs[0].0, CONTENT_X, "the label is flush left");

    // The value's right edge lands on the content edge, so a longer label can
    // never push it out of the panel — which is what the one-string
    // `"label: value"` row did until it got cut mid-glyph by the clip rect.
    assert_eq!(runs[1].2, "0.50");
    let value_w = painter.text_size("0.50", TEXT_PX)[0];
    assert_eq!(runs[1].0 + value_w, CONTENT_X + CONTENT_W);
    // Same row, not stacked.
    assert_eq!(runs[0].1, runs[1].1);
}

#[test]
fn a_row_inside_right_gives_label_left_and_value_right() {
    // The general composition `label_value` is a shorthand for, spelled out —
    // this is what a consumer writes for a row the toolkit never shipped.
    let (_, rects) = frame(|ui| {
        ui.horizontal(|ui| {
            let left = ui.sized([100.0, ROW_H]).label("label").rect;
            let right = ui.right(|ui| ui.sized([40.0, ROW_H]).label("value").rect);
            (left, right)
        })
    });
    let (left, right) = rects;

    assert_eq!(left.x, CONTENT_X);
    assert_eq!(right.max_x(), CONTENT_X + CONTENT_W);
    assert_eq!(left.y, right.y);
}

#[test]
fn columns_split_the_content_width_evenly() {
    let (_, cells) = frame(|ui| {
        let mut cells: Vec<Rect> = Vec::new();
        ui.columns(3, |ui, i| {
            cells.push(ui.button(["a", "b", "c"][i]).rect);
        });
        cells
    });

    let expected_w = (CONTENT_W - 2.0 * GAP) / 3.0;
    for (i, cell) in cells.iter().enumerate() {
        assert_eq!(cell.w, expected_w, "column {i} is the wrong width");
        assert_eq!(cell.x, CONTENT_X + (expected_w + GAP) * i as f32);
        assert_eq!(cell.y, MARGIN + PAD, "all three share a line");
    }
    // The three cells plus two gaps fill the content width exactly.
    assert_eq!(cells[2].max_x(), CONTENT_X + CONTENT_W);
}

#[test]
fn columns_of_different_lengths_clear_the_tallest() {
    let (_, after) = frame(|ui| {
        ui.columns(2, |ui, i| {
            for _ in 0..=i {
                ui.label("row");
            }
        });
        ui.label("below").rect
    });

    // Column 0 has one row, column 1 has two — the next widget clears both.
    assert_eq!(after.y, MARGIN + PAD + 2.0 * ROW_H);
}

#[test]
fn columns_of_zero_is_a_no_op() {
    let (_, after) = frame(|ui| {
        ui.columns(0, |ui, _| {
            ui.label("never");
        });
        ui.label("first").rect
    });
    assert_eq!(after.y, MARGIN + PAD);
}

#[test]
fn indent_steps_in_from_the_left_and_gives_the_width_back() {
    let (_, rects) = frame(|ui| {
        let outer = ui.label("outer").rect;
        let inner = ui.indent(|ui| ui.label("inner").rect);
        let after = ui.label("after").rect;
        (outer, inner, after)
    });
    let (outer, inner, after) = rects;

    assert_eq!(inner.x, outer.x + INDENT);
    assert_eq!(inner.w, outer.w - INDENT);
    assert_eq!(inner.y, outer.max_y());
    // Closing the indent restores the full width, and consumes its height.
    assert_eq!((after.x, after.w), (CONTENT_X, CONTENT_W));
    assert_eq!(after.y, inner.max_y());
}

#[test]
fn sized_applies_to_exactly_one_widget() {
    let (_, rects) = frame(|ui| {
        let forced = ui.sized([120.0, 40.0]).label("forced").rect;
        let normal = ui.label("normal").rect;
        (forced, normal)
    });
    let (forced, normal) = rects;

    assert_eq!((forced.w, forced.h), (120.0, 40.0));
    // The override cleared itself; the next widget is back to the defaults.
    assert_eq!((normal.w, normal.h), (CONTENT_W, ROW_H));
    assert_eq!(normal.y, forced.max_y());
}

#[test]
fn a_compact_slider_fits_its_track_between_the_label_and_the_value() {
    use slmsttaa_ui::SliderLayout;

    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 0.5_f32;
    let track = {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.slider("m", &mut value, 0.0, 1.0)
                .layout(SliderLayout::Compact)
                .show()
                .rect
        })
    };

    let label_w = painter.text_size("m", TEXT_PX)[0];
    let value_w = painter.text_size("0.50", TEXT_PX)[0];
    assert_eq!(track.x, CONTENT_X + label_w + GAP);
    assert_eq!(track.max_x(), CONTENT_X + CONTENT_W - value_w - GAP);
    // One row tall, where the stacked layout is two.
    assert!(track.h <= ROW_H);
}

#[test]
fn a_slider_formats_its_own_readout() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 0.003_f32;
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.slider("decimals", &mut value, 0.0, 1.0)
                .decimals(4)
                .show();
            ui.slider("custom", &mut value, 0.0, 1.0)
                .value_fmt(|v| format!("{v:.1e}"))
                .show();
        });
    }

    let runs: Vec<String> = texts(&painter).into_iter().map(|(_, _, t)| t).collect();
    assert!(
        runs.contains(&"0.0030".to_string()),
        "decimals(4): {runs:?}"
    );
    assert!(runs.contains(&"3.0e-3".to_string()), "value_fmt: {runs:?}");
    // The label is its own run, not glued to the value.
    assert!(runs.contains(&"decimals".to_string()));
}
