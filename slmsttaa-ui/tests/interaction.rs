//! Hit-testing and interaction tests.
//!
//! Interaction is the least verifiable code in the project by eye — a slider
//! that releases its drag one frame early looks fine in a screenshot. Here it is
//! just arithmetic over a [`UiInput`] the test writes by hand.

use slmsttaa_ui::{RecordingPainter, Ui, UiInput, UiState};

const PANEL_X: f32 = 12.0;
const PANEL_Y: f32 = 12.0;
const PAD: f32 = 10.0;
const CONTENT_X: f32 = PANEL_X + PAD;
const CONTENT_W: f32 = 320.0;
const TEXT_PX: f32 = 16.0;

/// A pointer resting at `(x, y)` with the button up.
fn hovering(x: f32, y: f32) -> UiInput {
    UiInput {
        cursor: Some((x, y)),
        ..Default::default()
    }
}

/// A pointer at `(x, y)` on the frame the button goes down.
fn clicking(x: f32, y: f32) -> UiInput {
    UiInput {
        cursor: Some((x, y)),
        primary_held: true,
        primary_pressed: true,
    }
}

/// A pointer at `(x, y)` with the button still held from an earlier frame.
fn dragging(x: f32, y: f32) -> UiInput {
    UiInput {
        cursor: Some((x, y)),
        primary_held: true,
        primary_pressed: false,
    }
}

#[test]
fn button_fires_only_on_a_press_inside_it() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    // The first widget's row starts one pad below the panel top.
    let inside = (CONTENT_X + 40.0, PANEL_Y + PAD + 8.0);
    let below_the_panel = (inside.0, 900.0);

    let click_inside = {
        let mut ui = Ui::new(&mut painter, clicking(inside.0, inside.1), &mut state);
        ui.button("go")
    };
    let click_outside = {
        let mut ui = Ui::new(
            &mut painter,
            clicking(below_the_panel.0, below_the_panel.1),
            &mut state,
        );
        ui.button("go")
    };
    let hover_only = {
        let mut ui = Ui::new(&mut painter, hovering(inside.0, inside.1), &mut state);
        ui.button("go")
    };
    // A held-over button from a previous frame is not a new click.
    let still_held = {
        let mut ui = Ui::new(&mut painter, dragging(inside.0, inside.1), &mut state);
        ui.button("go")
    };

    assert!(click_inside, "a press inside the button should click it");
    assert!(!click_outside, "a press elsewhere should not");
    assert!(!hover_only, "hovering is not clicking");
    assert!(!still_held, "holding should fire once, not every frame");
}

#[test]
fn checkbox_toggles_on_press_and_reports_changed() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut flag = false;
    let row_y = PANEL_Y + PAD + 8.0;

    {
        let mut ui = Ui::new(&mut painter, clicking(CONTENT_X + 4.0, row_y), &mut state);
        assert!(ui.checkbox("wireframe", &mut flag));
        assert!(ui.changed(), "a toggle is a value change");
    }
    assert!(flag);

    // Clicking the label, not just the box, toggles it back — the whole row is
    // the hit target.
    {
        let mut ui = Ui::new(&mut painter, clicking(CONTENT_X + 200.0, row_y), &mut state);
        assert!(ui.checkbox("wireframe", &mut flag));
    }
    assert!(!flag);

    // Hovering leaves it alone.
    {
        let mut ui = Ui::new(&mut painter, hovering(CONTENT_X + 4.0, row_y), &mut state);
        assert!(!ui.checkbox("wireframe", &mut flag));
        assert!(!ui.changed());
    }
    assert!(!flag);
}

/// A point inside the first slider's grab band: the header line sits at the top
/// of the row, the track five pixels under it.
fn slider_band_y() -> f32 {
    PANEL_Y + PAD + TEXT_PX + 5.0 + 2.0
}

#[test]
fn slider_jumps_to_the_pressed_position() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 0.0_f32;

    {
        let mut ui = Ui::new(
            &mut painter,
            clicking(CONTENT_X + CONTENT_W * 0.5, slider_band_y()),
            &mut state,
        );
        assert!(ui.slider("t", &mut value, 0.0, 100.0));
        assert!(ui.changed());
    }
    assert!((value - 50.0).abs() < 0.01, "value was {value}");
}

#[test]
fn slider_drag_survives_the_cursor_leaving_the_track() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 0.0_f32;

    // Frame 1: grab the knob at the left end.
    {
        let mut ui = Ui::new(
            &mut painter,
            clicking(CONTENT_X, slider_band_y()),
            &mut state,
        );
        ui.slider("t", &mut value, 0.0, 100.0);
    }

    // Frame 2: the cursor is way off the widget but the button is still down,
    // so the slider keeps following it — clamped to the track's range.
    {
        let mut ui = Ui::new(&mut painter, dragging(2000.0, 900.0), &mut state);
        ui.slider("t", &mut value, 0.0, 100.0);
        assert!(ui.wants_pointer(), "an active drag must hold the pointer");
    }
    assert_eq!(value, 100.0);

    // Frame 3: button released. The drag lets go, and a stray cursor no longer
    // moves the value.
    {
        let mut ui = Ui::new(&mut painter, hovering(2000.0, 900.0), &mut state);
        ui.slider("t", &mut value, 0.0, 100.0);
    }
    {
        let mut ui = Ui::new(
            &mut painter,
            hovering(CONTENT_X, slider_band_y()),
            &mut state,
        );
        ui.slider("t", &mut value, 0.0, 100.0);
        assert!(!ui.changed(), "hovering a released slider must not edit it");
    }
    assert_eq!(value, 100.0);
}

#[test]
fn sliders_with_the_same_label_do_not_share_a_drag() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let (mut first, mut second) = (0.0_f32, 0.0_f32);

    // Press on the *second* slider's band. Ids are label + call index, so the
    // duplicate label must not hand the drag to the first one.
    let second_band = slider_band_y() + 40.0;
    {
        let mut ui = Ui::new(
            &mut painter,
            clicking(CONTENT_X + CONTENT_W, second_band),
            &mut state,
        );
        ui.slider("amount", &mut first, 0.0, 1.0);
        ui.slider("amount", &mut second, 0.0, 1.0);
    }

    assert_eq!(first, 0.0, "the first slider should not have moved");
    assert_eq!(second, 1.0);
}

#[test]
fn wants_pointer_covers_the_panel_and_nothing_else() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    // Lay out a few rows so the panel has a real height for the next frame.
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        for _ in 0..4 {
            ui.label("row");
        }
    }

    let over_panel = {
        let ui = Ui::new(
            &mut painter,
            hovering(PANEL_X + 5.0, PANEL_Y + 5.0),
            &mut state,
        );
        ui.wants_pointer()
    };
    let over_scene = {
        let ui = Ui::new(&mut painter, hovering(800.0, 600.0), &mut state);
        ui.wants_pointer()
    };
    let no_cursor = {
        let ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.wants_pointer()
    };

    assert!(over_panel, "the camera must not orbit while over the panel");
    assert!(!over_scene, "the scene keeps the pointer everywhere else");
    assert!(!no_cursor, "an unseen cursor hits nothing");
}
