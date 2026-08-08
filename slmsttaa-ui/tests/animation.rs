//! Animation tests: that eased values converge in *time* rather than in frames,
//! that a host which ignores the clock gets the toolkit it had before UI Slice 6,
//! and that a collapsing section actually collapses.
//!
//! Motion is the hardest thing in this crate to assert on, because the honest
//! question ("does it look right?") is not one a `RecordingPainter` can answer.
//! What it *can* answer is everything around that: whether the same fade takes
//! the same milliseconds at 60 and 144 Hz, whether a value settles exactly on its
//! target or merely near it, and whether a widget that leaves the screen comes
//! back settled instead of mid-fade. Those are the parts that break silently.

use slmsttaa_ui::{
    anim, theme::Motion, Anchor, Color, DrawCmd, RecordingPainter, Theme, Ui, UiInput, UiState,
};

/// The panel geometry, restated as elsewhere in this suite.
const MARGIN: f32 = 12.0;
const PANEL_W: f32 = 340.0;
const PAD: f32 = 10.0;

/// One 60 Hz frame.
const FRAME: f32 = 1.0 / 60.0;

/// A pointer resting on the panel's first row, with `dt` seconds since the last
/// frame.
fn hovering(dt: f32) -> UiInput<'static> {
    UiInput {
        cursor: Some((100.0, MARGIN + PAD + 8.0)),
        dt,
        ..UiInput::default()
    }
}

/// A pointer parked far away from any widget.
fn away(dt: f32) -> UiInput<'static> {
    UiInput {
        cursor: Some((900.0, 600.0)),
        dt,
        ..UiInput::default()
    }
}

/// Press the panel's first row.
fn clicking(dt: f32) -> UiInput<'static> {
    UiInput {
        primary_held: true,
        primary_pressed: true,
        ..hovering(dt)
    }
}

/// Every color the frame drew with.
fn colors(p: &RecordingPainter) -> Vec<Color> {
    p.cmds
        .iter()
        .map(|c| match *c {
            DrawCmd::Rect { color, .. }
            | DrawCmd::Text { color, .. }
            | DrawCmd::Polyline { color, .. }
            | DrawCmd::Polygon { color, .. } => color,
            DrawCmd::Image { tint, .. } => tint,
        })
        .collect()
}

/// The panel background's height, which is the first thing painted in layer
/// order.
fn panel_height(p: &RecordingPainter) -> f32 {
    match p.in_layer_order()[0] {
        DrawCmd::Rect { rect, .. } => rect.h,
        other => panic!("expected the panel background first, got {other:?}"),
    }
}

// --- The core -------------------------------------------------------------

#[test]
fn a_fade_covers_the_same_ground_however_it_is_stepped() {
    // The bug this exists to catch is the `value += (target - value) * 0.2` one:
    // it converges in a fixed number of *frames*, so the same transition runs at
    // half speed on a 30 Hz machine and nobody notices until they change monitor.
    let coarse = anim::approach(0.0, 1.0, 20.0, 0.1);
    let fine = (0..10).fold(0.0, |v, _| anim::approach(v, 1.0, 20.0, 0.01));
    assert!(
        (coarse - fine).abs() < 1.0e-5,
        "one 100 ms step ({coarse}) and ten 10 ms steps ({fine}) must agree",
    );
}

#[test]
fn a_fade_settles_exactly_on_its_target() {
    // Exponential decay never arrives. It has to be made to, or a section is
    // forever 0.001 short of open and takes the clipped path for the rest of the
    // program.
    let settled = (0..200).fold(0.0_f32, |v, _| anim::approach(v, 1.0, 20.0, FRAME));
    assert_eq!(settled, 1.0);

    let back = (0..200).fold(settled, |v, _| anim::approach(v, 0.0, 20.0, FRAME));
    assert_eq!(back, 0.0);
}

#[test]
fn an_endpoint_is_reproduced_exactly() {
    // `tests/theme.rs` asserts every color reaching the painter is one the theme
    // supplied, and a resting widget is at t == 0 or t == 1. If the mix were even
    // slightly lossy at the ends, every widget in the crate would rest on a color
    // no token accounts for.
    let (a, b) = ([0.1, 0.2, 0.3, 0.4], [0.5, 0.6, 0.7, 0.8]);
    assert_eq!(anim::lerp(a, b, 0.0), a);
    assert_eq!(anim::lerp(a, b, 1.0), b);
    assert_eq!(anim::fade(a, 1.0), a);
}

// --- The seam -------------------------------------------------------------

#[test]
fn a_host_that_never_sets_dt_gets_the_toolkit_it_had_before() {
    // `dt` is optional at the seam: leave it at zero and everything snaps, which
    // is precisely the behavior every widget had through UI Slice 5. This is what
    // lets the rest of this suite ignore that animation exists.
    let theme = Theme::dark();
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    {
        let mut ui = Ui::new(&mut painter, hovering(0.0), &mut state);
        ui.set_theme(theme);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.button("go").show();
        });
    }
    assert!(
        colors(&painter).contains(&theme.color.primary_hover),
        "with no clock, a hovered button must be fully hovered on the first frame",
    );
}

#[test]
fn motion_none_disables_easing_without_a_widget_knowing() {
    // Turning motion off is a theme value, not a flag threaded through every
    // widget, so nothing can forget to check it.
    let mut theme = Theme::dark();
    theme.motion = Motion::none();

    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let frame = |input: UiInput, p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        let mut ui = Ui::new(p, input, s);
        ui.set_theme(theme);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.button("go").show();
        });
    };

    frame(away(FRAME), &mut painter, &mut state);
    frame(hovering(FRAME), &mut painter, &mut state);
    assert!(
        colors(&painter).contains(&theme.color.primary_hover),
        "an infinite rate must arrive in one frame",
    );
}

// --- Widgets --------------------------------------------------------------

#[test]
fn a_hover_fade_passes_through_colors_the_theme_did_not_name() {
    // The flip side of `tests/theme.rs`: mid-transition a widget is *supposed* to
    // be drawing something between two tokens. If this ever stopped being true
    // the fade would have silently become a snap.
    let theme = Theme::dark();
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let frame = |input: UiInput, p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        let mut ui = Ui::new(p, input, s);
        ui.set_theme(theme);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.button("go").show();
        });
    };

    // Settle unhovered, then hover for a single frame.
    frame(away(FRAME), &mut painter, &mut state);
    frame(hovering(FRAME), &mut painter, &mut state);

    let drawn = colors(&painter);
    assert!(
        !drawn.contains(&theme.color.primary) && !drawn.contains(&theme.color.primary_hover),
        "one frame in, the fill should be between the two fills, not on either",
    );
}

#[test]
fn a_press_scrim_is_not_drawn_at_all_when_it_is_invisible() {
    // A fully transparent rectangle is a primitive the GPU still chews on and a
    // color no theme token accounts for. Widgets skip the draw instead.
    let theme = Theme::dark();
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let count = |p: &RecordingPainter| p.cmds.len();
    {
        let mut ui = Ui::new(&mut painter, away(FRAME), &mut state);
        ui.set_theme(theme);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| ui.button("go").show());
    }
    let idle = count(&painter);

    painter.clear();
    {
        let mut ui = Ui::new(&mut painter, clicking(FRAME), &mut state);
        ui.set_theme(theme);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| ui.button("go").show());
    }
    assert!(
        count(&painter) > idle,
        "pressing adds a scrim and a focus ring; idling draws neither",
    );
}

#[test]
fn a_widget_that_leaves_the_screen_comes_back_settled() {
    // The sweep. Without it, animated values accumulate for every id ever seen,
    // and worse, a row that reappears would resume a fade from a hover the user
    // stopped caring about several seconds ago.
    let theme = Theme::dark();
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let frame = |show: bool, input: UiInput, p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        let mut ui = Ui::new(p, input, s);
        ui.set_theme(theme);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            if show {
                ui.button("go").show();
            }
        });
    };

    // Hover it until it is fully hot.
    for _ in 0..30 {
        frame(true, hovering(FRAME), &mut painter, &mut state);
    }
    assert!(colors(&painter).contains(&theme.color.primary_hover));

    // Two frames without it, one to stop asking for its slots and one for the
    // sweep to notice, then bring it back with the pointer elsewhere.
    frame(false, away(FRAME), &mut painter, &mut state);
    frame(false, away(FRAME), &mut painter, &mut state);
    frame(true, away(FRAME), &mut painter, &mut state);

    assert!(
        colors(&painter).contains(&theme.color.primary),
        "it should return at rest, not fade down from a forgotten hover",
    );
}

// --- The section ----------------------------------------------------------

/// One frame of a panel holding a single section with two rows in it.
fn section_frame(input: UiInput, p: &mut RecordingPainter, s: &mut UiState) {
    let mut value = 0.5_f32;
    p.clear();
    let mut ui = Ui::new(p, input, s);
    ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
        ui.section("Shape", |ui| {
            ui.slider("frequency", &mut value, 0.0, 1.0).show();
            ui.slider("octaves", &mut value, 0.0, 1.0).show();
        });
    });
}

#[test]
fn a_collapsing_section_shrinks_over_several_frames() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    section_frame(away(FRAME), &mut painter, &mut state);
    let expanded = panel_height(&painter);

    // Click the heading, then watch the panel come down.
    section_frame(clicking(FRAME), &mut painter, &mut state);
    let first = panel_height(&painter);
    assert!(
        first < expanded,
        "the panel must start shrinking on the frame of the click",
    );
    assert!(
        first > expanded * 0.5,
        "but nowhere near all the way: this is a transition, not a snap \
         (was {first}, expanded {expanded})",
    );

    let mut previous = first;
    for _ in 0..60 {
        section_frame(away(FRAME), &mut painter, &mut state);
        let now = panel_height(&painter);
        assert!(now <= previous, "the collapse must be monotonic");
        previous = now;
    }
    assert!(
        previous < expanded * 0.5,
        "a second later it should be fully collapsed (got {previous})",
    );

    // And it stays there rather than creeping.
    section_frame(away(FRAME), &mut painter, &mut state);
    assert_eq!(panel_height(&painter), previous);
}

#[test]
fn a_collapsing_section_clips_its_contents_but_an_open_one_does_not() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    section_frame(away(FRAME), &mut painter, &mut state);
    assert!(
        painter.cmds.iter().all(|c| c.clip().is_none()),
        "a fully open section must take the unclipped fast path",
    );

    section_frame(clicking(FRAME), &mut painter, &mut state);
    section_frame(away(FRAME), &mut painter, &mut state);
    assert!(
        painter.cmds.iter().any(|c| c.clip().is_some()),
        "mid-collapse, the contents have to be clipped or they would spill \
         over the rows below",
    );
}

#[test]
fn a_collapsed_sections_contents_are_not_declared_at_all() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    section_frame(away(0.0), &mut painter, &mut state);
    assert!(painter.visible_texts().contains(&"frequency"));

    // No clock, so the collapse completes in one frame.
    section_frame(clicking(0.0), &mut painter, &mut state);
    section_frame(away(0.0), &mut painter, &mut state);
    assert!(
        !painter.texts().contains(&"frequency"),
        "a closed section skips its closure entirely: it does not draw its \
         rows and clip them away",
    );
}

// --- Scrolling ------------------------------------------------------------

#[test]
fn a_wheel_notch_glides_rather_than_teleporting() {
    let theme = Theme::dark();
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let frame = |scroll: f32, dt: f32, p: &mut RecordingPainter, s: &mut UiState| {
        let mut value = 0.5_f32;
        p.clear();
        let input = UiInput {
            cursor: Some((100.0, MARGIN + PAD + 20.0)),
            scroll_delta: scroll,
            dt,
            ..UiInput::default()
        };
        let mut ui = Ui::new(p, input, s);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.scroll_area("body", 100.0, |ui| {
                for i in 0..20 {
                    ui.slider(&format!("knob {i}"), &mut value, 0.0, 1.0).show();
                }
            });
        });
        // Where the first row ended up, which is the scroll offset made visible.
        match p.cmds.iter().find_map(|c| match c {
            DrawCmd::Text { y, text, .. } if text == "knob 0" => Some(*y),
            _ => None,
        }) {
            Some(y) => y,
            None => panic!("the first row should still be drawn, clipped or not"),
        }
    };

    // Two frames to let the scroll area measure its contents, then one notch.
    frame(0.0, FRAME, &mut painter, &mut state);
    let resting = frame(0.0, FRAME, &mut painter, &mut state);
    let after_one_frame = frame(-1.0, FRAME, &mut painter, &mut state);

    let moved = resting - after_one_frame;
    assert!(moved > 0.0, "the wheel scrolled the wrong way");
    assert!(
        moved < theme.control.scroll_speed,
        "one frame should cover part of a notch, not all of it (got {moved} of \
         {})",
        theme.control.scroll_speed,
    );

    // It gets there in the end.
    let mut settled = after_one_frame;
    for _ in 0..60 {
        settled = frame(0.0, FRAME, &mut painter, &mut state);
    }
    assert!((resting - settled - theme.control.scroll_speed).abs() < 0.01);
}
