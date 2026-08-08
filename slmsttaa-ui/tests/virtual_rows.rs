//! Virtualized scroll areas: which rows get placed, and where.
//!
//! A plain `scroll_area` lays out every child and lets the painter throw most of
//! them away. `scroll_area_virtual` places only the rows the viewport covers —
//! which means the draw list is now *evidence* rather than a formality, because
//! a row that should have been placed and wasn't leaves a hole nothing else
//! reports.
//!
//! Two assertions carry most of the weight. One is that a virtualized list and a
//! real one put the same rows in the same places for the same input, which is the
//! whole claim in a sentence. The other is that the range follows the **eased**
//! offset: a wheel notch glides over a couple of frames, and computing the range
//! from where the list is heading rather than from where it is leaves a blank
//! strip at the top of the viewport for exactly as long as the glide lasts.

use slmsttaa_ui::{
    font, Anchor, DrawCmd, RecordingPainter, Rect, Rows, Theme, Ui, UiInput, UiState,
};

/// Restated rather than imported, as elsewhere in this suite.
const MARGIN: f32 = 12.0;
const PANEL_W: f32 = 340.0;
const PAD: f32 = 10.0;
const CONTENT_X: f32 = MARGIN + PAD;
const ROW_H: f32 = 24.0;
const VIEWPORT_H: f32 = 100.0;
/// `floor(0/24) .. ceil(100/24)` — four whole rows and the partial fifth.
const VISIBLE: usize = 5;

/// A pointer parked over the panel, scrolling by `notches`.
fn wheel(notches: f32) -> UiInput<'static> {
    UiInput {
        cursor: Some((CONTENT_X + 10.0, MARGIN + PAD + 10.0)),
        scroll_delta: notches,
        ..Default::default()
    }
}

/// One row: a full-width plate and its name, so a test can read back the
/// geometry and the text from the same frame.
fn row(ui: &mut Ui, index: usize) {
    let theme = *ui.theme();
    let rect = ui.allocate([0.0, ROW_H]);
    let (px, weight) = theme.text.body.parts();
    let y = font::centered_top(rect.y, rect.h, px);
    let name = format!("row {index}");
    let painter = ui.painter();
    painter.fill_rect(rect, 0.0, PLATE);
    painter.text(rect.x, y, &name, px, weight, theme.color.foreground);
}

/// The colour a test row's plate is painted, chosen so nothing else in the frame
/// wears it — the scrollbar's own track is `color.surface`, and a filter that
/// picked that up would count the bar as a sixth row.
const PLATE: [f32; 4] = [0.5, 0.0, 0.5, 1.0];

/// Every row plate that was drawn, in call order.
fn plates(p: &RecordingPainter) -> Vec<Rect> {
    p.cmds
        .iter()
        .filter_map(|c| match c {
            DrawCmd::Rect { rect, color, .. } if *color == PLATE => Some(*rect),
            _ => None,
        })
        .collect()
}

/// Every row name that was drawn, in call order.
fn names(p: &RecordingPainter) -> Vec<String> {
    p.cmds
        .iter()
        .filter_map(|c| match c {
            DrawCmd::Text { text, .. } if text.starts_with("row ") => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// One frame of a virtualized list of `count` rows.
fn virtual_frame(
    count: usize,
    input: UiInput,
    p: &mut RecordingPainter,
    s: &mut UiState,
) -> Vec<Rect> {
    p.clear();
    {
        let mut ui = Ui::new(p, input, s);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.scroll_area_virtual("body", VIEWPORT_H, Rows::uniform(count, ROW_H), row);
        });
    }
    plates(p)
}

/// The same list, laid out in full by an ordinary scroll area.
fn plain_frame(count: usize, input: UiInput, p: &mut RecordingPainter, s: &mut UiState) {
    p.clear();
    let mut ui = Ui::new(p, input, s);
    ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
        ui.scroll_area("body", VIEWPORT_H, |ui| {
            for index in 0..count {
                row(ui, index);
            }
        });
    });
}

#[test]
fn only_the_visible_rows_are_declared() {
    // The headline claim: five thousand rows cost five.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let rows = virtual_frame(5_000, UiInput::default(), &mut painter, &mut state);

    assert_eq!(
        rows.len(),
        VISIBLE,
        "a {VIEWPORT_H}-point viewport over {ROW_H}-point rows shows {VISIBLE}, \
         whatever the list's length"
    );
    assert_eq!(
        names(&painter),
        ["row 0", "row 1", "row 2", "row 3", "row 4"],
        "and they are the rows at the top, in order"
    );
}

#[test]
fn a_virtual_list_scrolls_to_the_same_place_as_a_real_one() {
    // The equivalence test, and the one that would catch a whole class of
    // off-by-one: whatever the wheel does to a list that lays itself out in
    // full, it must do to one that does not.
    let (mut vp, mut vs) = (RecordingPainter::default(), UiState::default());
    let (mut pp, mut ps) = (RecordingPainter::default(), UiState::default());
    const COUNT: usize = 200;

    // Frame one apiece: the plain area is still measuring its contents here, so
    // the comparison starts on frame two.
    virtual_frame(COUNT, UiInput::default(), &mut vp, &mut vs);
    plain_frame(COUNT, UiInput::default(), &mut pp, &mut ps);

    // A wheel notch, then the frames it takes to glide, then two more notches
    // and a rest — enough of a walk to leave a rounding error somewhere visible.
    for notches in [0.0, -1.0, 0.0, 0.0, 0.0, -2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        let input = if notches == 0.0 {
            UiInput::default()
        } else {
            wheel(notches)
        };
        virtual_frame(COUNT, input, &mut vp, &mut vs);
        plain_frame(COUNT, input, &mut pp, &mut ps);

        assert_eq!(
            vp.visible_texts(),
            pp.visible_texts(),
            "the two lists must read identically at every point in the scroll"
        );
    }

    // And the geometry, not just the glyphs: every plate the virtual list drew is
    // one the real list drew at exactly the same place.
    let virtual_plates = plates(&vp);
    let plain_plates = plates(&pp);
    assert!(!virtual_plates.is_empty(), "something should be on screen");
    for rect in &virtual_plates {
        assert!(
            plain_plates.contains(rect),
            "{rect:?} is not where the real list put that row"
        );
    }
}

#[test]
fn the_content_height_is_known_on_the_first_frame() {
    // A plain scroll area measures its contents by laying them out, so on frame
    // one it does not yet know there is anywhere to scroll to. A virtualized one
    // is told, so it does.
    let (mut vp, mut vs) = (RecordingPainter::default(), UiState::default());
    let (mut pp, mut ps) = (RecordingPainter::default(), UiState::default());

    let scrolled = virtual_frame(200, wheel(-2.0), &mut vp, &mut vs);
    plain_frame(200, wheel(-2.0), &mut pp, &mut ps);

    // Two notches, applied on the very first frame. A brand-new eased value
    // starts *at* its target rather than gliding toward it, so this is exact.
    assert_eq!(
        names(&vp)[0],
        "row 2",
        "56 points down a list of 24-point rows starts partway through row 2"
    );
    assert_eq!(
        names(&pp)[0],
        "row 0",
        "the plain area is still measuring its contents, so it does not yet know \
         there is anywhere to scroll to"
    );

    // The same row, in both lists, 56 points apart.
    let speed = Theme::dark().control.scroll_speed;
    let plain_row_2 = plates(&pp)[2].y;
    let virtual_row_2 = scrolled[0].y;
    assert!(
        (plain_row_2 - virtual_row_2 - 2.0 * speed).abs() < 0.01,
        "row 2 should sit {} points higher, got {}",
        2.0 * speed,
        plain_row_2 - virtual_row_2
    );
}

#[test]
fn the_range_follows_the_eased_offset_not_the_target() {
    // What is drawn eases toward the wheel's target over a couple of frames, so
    // the offset spends most of its life *between* rows. Taking the range from
    // the target instead would place the rows the list is heading for and leave a
    // strip of nothing at the top of the viewport until the glide finished.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let resting = virtual_frame(200, UiInput::default(), &mut painter, &mut state);
    let viewport_top = resting[0].y;

    let input = UiInput {
        dt: 1.0 / 60.0,
        ..wheel(-1.0)
    };
    let mid_glide = virtual_frame(200, input, &mut painter, &mut state);

    let first = mid_glide[0];
    let shift = viewport_top - first.y;
    let notch = Theme::dark().control.scroll_speed;

    // Part of a notch, which is what "mid-glide" means and what makes this frame
    // the interesting one: the offset is between two rows rather than on one.
    assert!(
        shift > 0.0 && shift < notch,
        "this frame should be partway through a {notch}-point notch, got {shift}"
    );
    // The row covering the viewport's top edge is drawn. Place the rows at the
    // eased offset but pick the range from the target and this is the assertion
    // that fails: the top row becomes the one the list is *heading* for, drawn
    // below where it belongs, with a strip of nothing above it.
    assert!(
        first.y < viewport_top && first.max_y() > viewport_top,
        "the top row must straddle the viewport's top edge ({} to {}, edge at \
         {viewport_top})",
        first.y,
        first.max_y()
    );
}

#[test]
fn a_row_that_overruns_its_height_does_not_move_the_rows_below_it() {
    // Rows are placed from the list's shape, not from a shared cursor. A row that
    // asks for more than it was promised is contained rather than allowed to
    // shove every row beneath it out of agreement with the scrollbar.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.scroll_area_virtual(
                "body",
                VIEWPORT_H,
                Rows::uniform(200, ROW_H),
                |ui, index| {
                    // Twice the height it was allotted.
                    let rect = ui.allocate([0.0, ROW_H * 2.0]);
                    let _ = index;
                    ui.painter().fill_rect(rect, 0.0, PLATE);
                },
            );
        });
    }

    let rows = plates(&painter);
    for pair in rows.windows(2) {
        assert_eq!(
            pair[1].y - pair[0].y,
            ROW_H,
            "rows stay one row apart however tall their contents are"
        );
    }
}

#[test]
fn the_header_lines_up_with_virtualized_rows() {
    // Slice 8's property, which a second container could quietly lose: the
    // header is laid out at the body's width, so a right-aligned heading shares
    // an edge with the cells beneath it.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.scroll_area_virtual_headed(
                "body",
                VIEWPORT_H,
                Rows::uniform(200, ROW_H),
                |ui| {
                    let theme = *ui.theme();
                    let rect = ui.allocate([0.0, ROW_H]);
                    let (px, weight) = theme.text.body.parts();
                    let w = font::text_width("#", px, weight);
                    ui.painter()
                        .text(rect.max_x() - w, rect.y, "#", px, weight, theme.color.muted);
                },
                |ui, _| {
                    let theme = *ui.theme();
                    let rect = ui.allocate([0.0, ROW_H]);
                    let (px, weight) = theme.text.body.parts();
                    let w = font::text_width("#", px, weight);
                    ui.painter()
                        .text(rect.max_x() - w, rect.y, "#", px, weight, theme.color.muted);
                },
            );
        });
    }

    let xs: Vec<f32> = painter
        .cmds
        .iter()
        .filter_map(|c| match c {
            DrawCmd::Text { x, text, .. } if text == "#" => Some(*x),
            _ => None,
        })
        .collect();
    assert!(xs.len() > 1, "a header and some rows were drawn");
    assert!(
        xs.iter().all(|x| *x == xs[0]),
        "every heading must sit exactly above its own column, got {xs:?}"
    );
}

#[test]
fn the_last_row_is_reachable_and_nothing_lies_beyond_it() {
    // The clamp at the bottom, which is where an off-by-one in `range` lives.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    const COUNT: usize = 200;

    let resting = virtual_frame(COUNT, UiInput::default(), &mut painter, &mut state);
    let viewport_bottom = resting[0].y + VIEWPORT_H;

    for _ in 0..80 {
        virtual_frame(COUNT, wheel(-5.0), &mut painter, &mut state);
    }
    let bottom = virtual_frame(COUNT, UiInput::default(), &mut painter, &mut state);

    let drawn = names(&painter);
    assert_eq!(
        drawn.last().map(String::as_str),
        Some("row 199"),
        "scrolling to the end must reach the last row, got {drawn:?}"
    );
    assert!(
        (bottom.last().unwrap().max_y() - viewport_bottom).abs() < 0.01,
        "and it comes to rest with its bottom edge on the viewport's"
    );
    // 4800 points of content in a 100-point viewport clamps at 4700, which is not
    // a whole number of 24-point rows — so the top one is partial and there are
    // five, not four.
    assert_eq!(
        bottom.len(),
        VISIBLE,
        "including the partial row at the top"
    );
}

#[test]
fn an_empty_or_zero_height_list_draws_nothing_and_does_not_panic() {
    // Both are reachable by accident — an unfiltered roster that matched nothing,
    // and a row height computed from a font before the font is ready.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    assert!(
        virtual_frame(0, UiInput::default(), &mut painter, &mut state).is_empty(),
        "no rows, nothing drawn"
    );

    painter.clear();
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.scroll_area_virtual("zero", VIEWPORT_H, Rows::uniform(500, 0.0), row);
        });
    }
    assert!(
        plates(&painter).is_empty(),
        "a zero-height row is an empty list, not a division by zero"
    );
}

#[test]
fn a_virtual_list_that_fits_does_not_scroll() {
    // Mirrors the plain area's own test: with nothing to scroll to, the wheel
    // must do nothing rather than drift.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let top = virtual_frame(2, UiInput::default(), &mut painter, &mut state);
    let after = virtual_frame(2, wheel(-10.0), &mut painter, &mut state);

    assert_eq!(top, after, "there is nothing to scroll to");
    assert_eq!(names(&painter), ["row 0", "row 1"]);
}

#[test]
fn revealing_a_row_brings_it_into_view() {
    // The replacement for the focus chase, which cannot work here: a row that was
    // never placed has no rectangle to compare against the viewport, so the
    // caller names the row instead of waiting to be shown where it landed.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let frame = |reveal: Option<usize>, p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        {
            let mut ui = Ui::new(p, UiInput::default(), s);
            ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
                ui.scroll_area_virtual(
                    "body",
                    VIEWPORT_H,
                    Rows::uniform(200, ROW_H).reveal(reveal),
                    row,
                );
            });
        }
        names(p)
    };

    let at_rest = frame(None, &mut painter, &mut state);
    assert_eq!(at_rest[0], "row 0");

    let revealed = frame(Some(150), &mut painter, &mut state);
    assert!(
        revealed.contains(&"row 150".to_string()),
        "row 150 should have been brought into view, got {revealed:?}"
    );

    // And back up, which takes the other branch of the same arithmetic.
    let back = frame(Some(3), &mut painter, &mut state);
    assert!(
        back.contains(&"row 3".to_string()),
        "and back the other way, got {back:?}"
    );
}

#[test]
fn revealing_the_same_row_twice_does_not_fight_the_wheel() {
    // A consumer passes the row it wants visible on every frame that row is
    // selected, not on the one frame it became selected. Acting on the value
    // rather than on a change to it would drag the view back to the selection the
    // instant the wheel moved away — so the ask is edge-triggered.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let frame = |input: UiInput, p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        {
            let mut ui = Ui::new(p, input, s);
            ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
                ui.scroll_area_virtual(
                    "body",
                    VIEWPORT_H,
                    Rows::uniform(200, ROW_H).reveal(Some(100)),
                    row,
                );
            });
        }
        names(p)
    };

    // The reveal lands once...
    let revealed = frame(UiInput::default(), &mut painter, &mut state);
    assert!(revealed.contains(&"row 100".to_string()));

    // ...and then the wheel is free to go somewhere else and stay there.
    for _ in 0..40 {
        frame(wheel(5.0), &mut painter, &mut state);
    }
    let walked_away = frame(UiInput::default(), &mut painter, &mut state);
    assert_eq!(
        walked_away[0], "row 0",
        "the wheel must win over a reveal that is not new, got {walked_away:?}"
    );
}

#[test]
fn a_headed_virtual_list_catches_the_wheel_over_its_header() {
    // The other property Slice 8 discovered by being surprised: a sticky header
    // is part of the same scrollable thing to a reader, and `wheel` parks the
    // pointer in the header band on purpose.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let frame = |input: UiInput, p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        {
            let mut ui = Ui::new(p, input, s);
            ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
                ui.scroll_area_virtual_headed(
                    "body",
                    VIEWPORT_H,
                    Rows::uniform(200, ROW_H),
                    |ui| {
                        ui.label("HEAD");
                    },
                    row,
                );
            });
        }
        names(p)
    };

    frame(UiInput::default(), &mut painter, &mut state);
    for _ in 0..10 {
        frame(wheel(-2.0), &mut painter, &mut state);
    }
    let scrolled = frame(UiInput::default(), &mut painter, &mut state);

    assert_ne!(
        scrolled[0], "row 0",
        "the wheel must scroll the body while the pointer is over the header"
    );
    assert!(
        painter.texts().contains(&"HEAD"),
        "and the header itself stays put and drawn"
    );
}
