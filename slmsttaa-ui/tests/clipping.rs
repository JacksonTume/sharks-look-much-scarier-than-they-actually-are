//! Clipping and scroll-area tests.
//!
//! Clipping is invisible in the recorded draw list unless you look for it — a
//! clipped-away widget is still *drawn*, it just doesn't survive the fragment
//! shader. So these assert on the clip rectangle attached to each primitive,
//! which is exactly what the painter hands the GPU.

use slmsttaa_ui::{Anchor, DrawCmd, Painter, RecordingPainter, Rect, Ui, UiInput, UiState};

const MARGIN: f32 = 12.0;
const PANEL_W: f32 = 340.0;
const PAD: f32 = 10.0;
const CONTENT_X: f32 = MARGIN + PAD;
const ROW_H: f32 = 24.0;

/// A pointer parked over the panel, scrolling by `notches`.
fn wheel(notches: f32) -> UiInput {
    UiInput {
        cursor: Some((CONTENT_X + 10.0, MARGIN + PAD + 10.0)),
        scroll_delta: notches,
        ..Default::default()
    }
}

#[test]
fn a_clip_region_is_recorded_on_everything_drawn_inside_it() {
    let mut painter = RecordingPainter::default();
    let region = Rect::new(0.0, 0.0, 100.0, 50.0);

    painter.rect(Rect::new(0.0, 0.0, 10.0, 10.0), [1.0; 4]);
    painter.push_clip(region);
    painter.rect(Rect::new(0.0, 0.0, 10.0, 10.0), [1.0; 4]);
    painter.text(0.0, 0.0, "inside", 16.0, [1.0; 4]);
    painter.pop_clip();
    painter.rect(Rect::new(0.0, 0.0, 10.0, 10.0), [1.0; 4]);

    let clips: Vec<Option<Rect>> = painter.cmds.iter().map(|c| c.clip()).collect();
    assert_eq!(clips, vec![None, Some(region), Some(region), None]);
}

#[test]
fn nested_clips_intersect_rather_than_replace() {
    let mut painter = RecordingPainter::default();

    painter.push_clip(Rect::new(0.0, 0.0, 100.0, 100.0));
    // A wider inner region must not widen what is visible.
    painter.push_clip(Rect::new(50.0, 50.0, 500.0, 500.0));
    painter.rect(Rect::new(0.0, 0.0, 10.0, 10.0), [1.0; 4]);

    assert_eq!(
        painter.cmds[0].clip(),
        Some(Rect::new(50.0, 50.0, 50.0, 50.0)),
        "the inner clip is bounded by the outer one"
    );
}

#[test]
fn disjoint_clips_hide_everything() {
    let mut painter = RecordingPainter::default();
    painter.push_clip(Rect::new(0.0, 0.0, 10.0, 10.0));
    painter.push_clip(Rect::new(500.0, 500.0, 10.0, 10.0));
    painter.rect(Rect::new(0.0, 0.0, 10.0, 10.0), [1.0; 4]);

    assert!(
        painter.cmds[0].clip().unwrap().is_empty(),
        "no overlap means nothing can be painted"
    );
}

#[test]
fn scroll_area_clips_its_contents_to_the_viewport() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.scroll_area("body", 100.0, |ui| {
                for i in 0..20 {
                    ui.label(&format!("row {i}"));
                }
            });
        });
    }

    // Every row inside carries the viewport clip — that is what stops the
    // overflow painting across the 3D scene.
    let clipped = painter
        .cmds
        .iter()
        .filter(|c| matches!(c, DrawCmd::Text { .. }))
        .all(|c| c.clip().is_some_and(|r| r.h <= 100.0));
    assert!(clipped, "scroll-area contents must be clipped");
}

#[test]
fn scrolling_moves_the_contents_and_stops_at_the_ends() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    // 20 rows of content in a 100-point viewport.
    let frame = |input: UiInput, p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        {
            let mut ui = Ui::new(p, input, s);
            ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
                ui.scroll_area("body", 100.0, |ui| {
                    for i in 0..20 {
                        ui.label(&format!("row {i}"));
                    }
                });
            });
        }
        // The y of the first row, which is what moves as we scroll.
        p.cmds
            .iter()
            .find_map(|c| match c {
                DrawCmd::Text { y, .. } => Some(*y),
                _ => None,
            })
            .expect("rows were drawn")
    };

    // Frame 1 establishes the content height; nothing has scrolled yet.
    let top = frame(UiInput::default(), &mut painter, &mut state);

    // Scrolling down moves the contents up.
    let scrolled = frame(wheel(-2.0), &mut painter, &mut state);
    assert!(
        scrolled < top,
        "scrolling down should move rows up ({scrolled} vs {top})"
    );

    // Scrolling far past the end clamps rather than running away.
    for _ in 0..50 {
        frame(wheel(-5.0), &mut painter, &mut state);
    }
    let bottom = frame(UiInput::default(), &mut painter, &mut state);
    let content_h = 20.0 * ROW_H;
    assert!(
        bottom >= top - (content_h - 100.0) - 0.5,
        "scrolling must clamp at the bottom, got {bottom}"
    );

    // And back up clamps at the top.
    for _ in 0..50 {
        frame(wheel(5.0), &mut painter, &mut state);
    }
    let back = frame(UiInput::default(), &mut painter, &mut state);
    assert_eq!(back, top, "scrolling back up returns to exactly the top");
}

#[test]
fn a_scroll_area_that_fits_does_not_scroll() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let frame = |input: UiInput, p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        {
            let mut ui = Ui::new(p, input, s);
            ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
                ui.scroll_area("body", 500.0, |ui| {
                    ui.label("only");
                    ui.label("two rows");
                });
            });
        }
        p.cmds
            .iter()
            .find_map(|c| match c {
                DrawCmd::Text { y, .. } => Some(*y),
                _ => None,
            })
            .expect("rows were drawn")
    };

    let top = frame(UiInput::default(), &mut painter, &mut state);
    let after_wheel = frame(wheel(-10.0), &mut painter, &mut state);
    assert_eq!(
        top, after_wheel,
        "there is nothing to scroll to, so the wheel does nothing"
    );
}

#[test]
fn a_scroll_area_does_not_swallow_the_panels_height() {
    // The scroll area lays its contents out in a child region, shifted by the
    // scroll offset. If that shift leaked into the enclosing panel's cursor, the
    // background would be measured from the wrong place and come out wrong.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.label("header");
            ui.scroll_area("body", 100.0, |ui| {
                for i in 0..20 {
                    ui.label(&format!("row {i}"));
                }
            });
        });
    }

    let bg = painter.in_layer_order()[0];
    let height = match bg {
        DrawCmd::Rect { rect, .. } => rect.h,
        other => panic!("expected the panel background, got {other:?}"),
    };
    // One header row + a 100-point viewport + the panel's padding — not the
    // full 20 rows of content that were laid out inside the viewport.
    assert!(
        height < PAD + ROW_H + 100.0 + PAD + 1.0,
        "the panel should be sized to the viewport, not the content ({height})"
    );
    assert!(height > ROW_H + 100.0, "…but must still contain it");
}
