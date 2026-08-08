//! Clipping and scroll-area tests.
//!
//! Clipping is invisible in the recorded draw list unless you look for it — a
//! clipped-away widget is still *drawn*, it just doesn't survive the fragment
//! shader. So these assert on the clip rectangle attached to each primitive,
//! which is exactly what the painter hands the GPU.

use slmsttaa_ui::{
    font, Anchor, DrawCmd, Painter, RecordingPainter, Rect, Theme, Ui, UiInput, UiState, Weight,
};

const MARGIN: f32 = 12.0;
const PANEL_W: f32 = 340.0;
const PAD: f32 = 10.0;
const CONTENT_X: f32 = MARGIN + PAD;
const ROW_H: f32 = 24.0;

/// A pointer parked over the panel, scrolling by `notches`.
fn wheel(notches: f32) -> UiInput<'static> {
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
    painter.text(0.0, 0.0, "inside", 16.0, Weight::Regular, [1.0; 4]);
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
fn a_scroll_area_keeps_its_contents_clear_of_the_scrollbar() {
    // The scrollbar is drawn inside the viewport's right edge, so the contents
    // have to stop short of it. They didn't until UI Slice 5, and nothing caught
    // it: the bitmap font left a quarter of every glyph cell blank on the right,
    // so a right-aligned readout's *ink* cleared the bar even though its box
    // didn't. A proportional font has no such slack, and the bar started covering
    // the last digit of every value in the panel.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 0.5_f32;

    // Two frames: the first measures the content, the second knows to scroll.
    for _ in 0..2 {
        painter.clear();
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.scroll_area("body", 100.0, |ui| {
                for i in 0..20 {
                    ui.slider(&format!("knob {i}"), &mut value, 0.0, 1.0).show();
                }
            });
        });
    }

    // The bar sits in the last `scrollbar_w` points of the content width.
    let content_edge = MARGIN + PANEL_W - PAD;
    let bar_left = content_edge - Theme::dark().control.scrollbar_w;

    // Every run — labels and right-aligned readouts alike — ends left of the bar.
    for cmd in &painter.cmds {
        if let DrawCmd::Text {
            x,
            text,
            px,
            weight,
            ..
        } = cmd
        {
            let right = x + font::text_width(text, *px, *weight);
            assert!(
                right <= bar_left,
                "{text:?} ends at {right}, but the scrollbar starts at {bar_left}"
            );
        }
    }
}

/// The x of every run whose text satisfies `pick`, in draw order.
fn xs_of(painter: &RecordingPainter, pick: impl Fn(&str) -> bool) -> Vec<f32> {
    painter
        .cmds
        .iter()
        .filter_map(|c| match c {
            DrawCmd::Text { x, text, .. } if pick(text) => Some(*x),
            _ => None,
        })
        .collect()
}

#[test]
fn a_header_outside_a_scroll_area_does_not_line_up_with_its_body() {
    // The bug `scroll_area_headed` exists to prevent, asserted so the test below
    // is known to be testing something real. A header laid out in the panel gets
    // the full content width; the body inside the area is inset by the gutter,
    // so their columns diverge.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.columns(2, |ui, i| {
                ui.label(["RANK", "NAME"][i]);
            });
            ui.scroll_area("body", 100.0, |ui| {
                for r in 0..20 {
                    ui.columns(2, |ui, i| {
                        ui.label(&format!("{}{r}", ["a", "b"][i]));
                    });
                }
            });
        });
    }

    let head = xs_of(&painter, |t| t == "RANK" || t == "NAME");
    let first_row = xs_of(&painter, |t| t == "a0" || t == "b0");
    assert_eq!(head.len(), 2);
    assert_eq!(first_row.len(), 2);
    assert_eq!(head[0], first_row[0], "the left edge is shared either way");
    assert_ne!(
        head[1], first_row[1],
        "the second column must drift — that is the bug"
    );
}

#[test]
fn a_headed_scroll_area_lays_its_header_out_at_the_bodys_width() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.scroll_area_headed(
                "roster",
                100.0,
                |ui| {
                    ui.columns(2, |ui, i| {
                        ui.label(["RANK", "NAME"][i]);
                    })
                },
                |ui| {
                    for r in 0..20 {
                        ui.columns(2, |ui, i| {
                            ui.label(&format!("{}{r}", ["a", "b"][i]));
                        });
                    }
                },
            );
        });
    }

    let head = xs_of(&painter, |t| t == "RANK" || t == "NAME");
    let first_row = xs_of(&painter, |t| t == "a0" || t == "b0");
    assert_eq!(
        head, first_row,
        "every heading must sit exactly above its own column"
    );
}

#[test]
fn a_headed_scroll_area_keeps_its_header_still_and_unclipped() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let frame = |input: UiInput, p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        let mut ui = Ui::new(p, input, s);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.scroll_area_headed(
                "roster",
                100.0,
                |ui| ui.label("HEAD"),
                |ui| {
                    for r in 0..20 {
                        ui.label(&format!("row {r}"));
                    }
                },
            );
        });
    };

    let head_of = |p: &RecordingPainter| {
        p.cmds
            .iter()
            .find_map(|c| match c {
                DrawCmd::Text { text, y, .. } if text == "HEAD" => Some((*y, c.clip())),
                _ => None,
            })
            .expect("the header was drawn")
    };
    let first_row_of = |p: &RecordingPainter| {
        p.cmds
            .iter()
            .find_map(|c| match c {
                DrawCmd::Text { text, y, .. } if text == "row 0" => Some(*y),
                _ => None,
            })
            .expect("the body was drawn")
    };

    // Frame one measures the content; frame two knows there is somewhere to go.
    frame(UiInput::default(), &mut painter, &mut state);
    let (head_y, head_clip) = head_of(&painter);
    let row_y = first_row_of(&painter);
    assert!(head_clip.is_none(), "the header is outside the clip");
    assert!(head_y < row_y, "the header sits above the body");

    // `wheel` parks the pointer in the header band, deliberately: a sticky
    // header catches the wheel on the body's behalf.
    for _ in 0..4 {
        frame(wheel(-2.0), &mut painter, &mut state);
    }
    let (scrolled_head_y, scrolled_head_clip) = head_of(&painter);
    assert_eq!(scrolled_head_y, head_y, "the header must not scroll away");
    assert!(scrolled_head_clip.is_none());
    assert!(
        first_row_of(&painter) < row_y,
        "…while the body underneath it did scroll"
    );
}

#[test]
fn an_empty_header_costs_a_scroll_area_nothing() {
    // `scroll_area` is `scroll_area_headed` with a no-op header, so the two must
    // be geometrically identical. This is what stops the plain case regressing.
    let rows = |ui: &mut Ui| {
        for r in 0..20 {
            ui.label(&format!("row {r}"));
        }
    };

    let mut plain = RecordingPainter::default();
    let mut plain_state = UiState::default();
    {
        let mut ui = Ui::new(&mut plain, UiInput::default(), &mut plain_state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.scroll_area("body", 100.0, rows);
        });
    }

    let mut headed = RecordingPainter::default();
    let mut headed_state = UiState::default();
    {
        let mut ui = Ui::new(&mut headed, UiInput::default(), &mut headed_state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.scroll_area_headed("body", 100.0, |_| {}, rows);
        });
    }

    assert_eq!(plain.cmds, headed.cmds);
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
