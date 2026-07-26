//! Hit-testing and interaction tests.
//!
//! Interaction is the least verifiable code in the project by eye — a slider
//! that releases its drag one frame early looks fine in a screenshot. Here it is
//! just arithmetic over a [`UiInput`] the test writes by hand.

use slmsttaa_ui::{Anchor, RecordingPainter, Ui, UiInput, UiState};

const MARGIN: f32 = 12.0;
const PANEL_W: f32 = 340.0;
const PAD: f32 = 10.0;
const CONTENT_X: f32 = MARGIN + PAD;
const CONTENT_W: f32 = 320.0;
const ROW_H: f32 = 24.0;
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
        ..Default::default()
    }
}

/// A pointer at `(x, y)` with the button still held from an earlier frame.
fn dragging(x: f32, y: f32) -> UiInput {
    UiInput {
        cursor: Some((x, y)),
        primary_held: true,
        primary_pressed: false,
        ..Default::default()
    }
}

/// One frame: declare `body` inside the default top-left panel and return
/// whatever it reports.
fn panel<T>(
    painter: &mut RecordingPainter,
    state: &mut UiState,
    input: UiInput,
    body: impl FnOnce(&mut Ui) -> T,
) -> T {
    let mut ui = Ui::new(painter, input, state);
    ui.panel(Anchor::TopLeft, PANEL_W, body)
}

#[test]
fn button_fires_only_on_a_press_inside_it() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    // The first widget's row starts one pad below the panel top.
    let inside = (CONTENT_X + 40.0, MARGIN + PAD + 8.0);
    let press = |painter: &mut RecordingPainter, state: &mut UiState, input| {
        panel(painter, state, input, |ui| ui.button("go").clicked)
    };

    let click_inside = press(&mut painter, &mut state, clicking(inside.0, inside.1));
    let click_outside = press(&mut painter, &mut state, clicking(inside.0, 900.0));
    let hover_only = press(&mut painter, &mut state, hovering(inside.0, inside.1));
    // A held-over button from a previous frame is not a new click.
    let still_held = press(&mut painter, &mut state, dragging(inside.0, inside.1));

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
    let row_y = MARGIN + PAD + 8.0;

    {
        let mut ui = Ui::new(&mut painter, clicking(CONTENT_X + 4.0, row_y), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            assert!(ui.checkbox("wireframe", &mut flag).changed);
        });
        assert!(ui.changed(), "a toggle is a value change");
    }
    assert!(flag);

    // Clicking the label, not just the box, toggles it back — the whole row is
    // the hit target.
    let toggled_back = panel(
        &mut painter,
        &mut state,
        clicking(CONTENT_X + 200.0, row_y),
        |ui| ui.checkbox("wireframe", &mut flag).changed,
    );
    assert!(toggled_back);
    assert!(!flag);

    // Hovering leaves it alone.
    {
        let mut ui = Ui::new(&mut painter, hovering(CONTENT_X + 4.0, row_y), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            let r = ui.checkbox("wireframe", &mut flag);
            assert!(!r.changed);
            assert!(r.hovered, "it should still report the hover");
        });
        assert!(!ui.changed());
    }
    assert!(!flag);
}

/// A point inside the first slider's grab band: the label/value line sits at the
/// top of the row, the track five points under it.
fn slider_band_y() -> f32 {
    MARGIN + PAD + TEXT_PX + 5.0 + 2.0
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
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            assert!(ui.slider("t", &mut value, 0.0, 100.0).show().changed);
        });
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
    panel(
        &mut painter,
        &mut state,
        clicking(CONTENT_X, slider_band_y()),
        |ui| {
            ui.slider("t", &mut value, 0.0, 100.0).show();
        },
    );

    // Frame 2: the cursor is way off the widget but the button is still down,
    // so the slider keeps following it — clamped to the track's range.
    {
        let mut ui = Ui::new(&mut painter, dragging(2000.0, 900.0), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            let r = ui.slider("t", &mut value, 0.0, 100.0).show();
            assert!(r.held, "the slider still owns the pointer");
            assert!(!r.hovered, "even though the cursor is nowhere near it");
        });
        assert!(ui.wants_pointer(), "an active drag must hold the pointer");
    }
    assert_eq!(value, 100.0);

    // Frame 3: button released. The drag lets go, and a stray cursor no longer
    // moves the value.
    panel(&mut painter, &mut state, hovering(2000.0, 900.0), |ui| {
        ui.slider("t", &mut value, 0.0, 100.0).show();
    });
    {
        let mut ui = Ui::new(
            &mut painter,
            hovering(CONTENT_X, slider_band_y()),
            &mut state,
        );
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.slider("t", &mut value, 0.0, 100.0).show();
        });
        assert!(!ui.changed(), "hovering a released slider must not edit it");
    }
    assert_eq!(value, 100.0);
}

#[test]
fn sliders_with_the_same_label_do_not_share_a_drag() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let (mut first, mut second) = (0.0_f32, 0.0_f32);

    // Press on the *second* slider's band. A duplicate label in one scope is
    // re-hashed into its own id, so the drag must not be handed to the first.
    let second_band = slider_band_y() + 40.0;
    panel(
        &mut painter,
        &mut state,
        clicking(CONTENT_X + CONTENT_W, second_band),
        |ui| {
            ui.slider("amount", &mut first, 0.0, 1.0).show();
            ui.slider("amount", &mut second, 0.0, 1.0).show();
        },
    );

    assert_eq!(first, 0.0, "the first slider should not have moved");
    assert_eq!(second, 1.0);
}

#[test]
fn wants_pointer_covers_the_panel_and_nothing_else() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    // Lay out a few rows so the panel has a real height, then ask in the same
    // frame — each panel contributes its rectangle as it closes.
    let asking = |painter: &mut RecordingPainter, state: &mut UiState, input| {
        let mut ui = Ui::new(painter, input, state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            for _ in 0..4 {
                ui.label("row");
            }
        });
        ui.wants_pointer()
    };

    let over_panel = asking(
        &mut painter,
        &mut state,
        hovering(MARGIN + 5.0, MARGIN + 5.0),
    );
    let over_scene = asking(&mut painter, &mut state, hovering(800.0, 600.0));
    let no_cursor = asking(&mut painter, &mut state, UiInput::default());

    assert!(over_panel, "the camera must not orbit while over the panel");
    assert!(!over_scene, "the scene keeps the pointer everywhere else");
    assert!(!no_cursor, "an unseen cursor hits nothing");
}

#[test]
fn wants_pointer_covers_every_panel() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let viewport = (1280.0, 720.0);

    let asking = |painter: &mut RecordingPainter, state: &mut UiState, at: (f32, f32)| {
        let input = UiInput {
            cursor: Some(at),
            viewport,
            ..Default::default()
        };
        let mut ui = Ui::new(painter, input, state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.label("params");
        });
        ui.panel(Anchor::TopRight, 170.0, |ui| {
            ui.label("hud");
        });
        ui.wants_pointer()
    };

    // The second panel is not an afterthought: the pointer over *either* one
    // has to suppress the camera.
    assert!(asking(
        &mut painter,
        &mut state,
        (MARGIN + 5.0, MARGIN + 5.0)
    ));
    assert!(asking(
        &mut painter,
        &mut state,
        (viewport.0 - MARGIN - 5.0, MARGIN + 5.0)
    ));
    // The gap between them is still the scene.
    assert!(!asking(&mut painter, &mut state, (700.0, MARGIN + 5.0)));
}

#[test]
fn every_widget_reports_where_it_landed() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut flag = false;
    let mut value = 0.5_f32;

    let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
    let (label, button, check, slider, readout) = ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
        // Even a label: a consumer needs its rectangle to hang a tooltip on it.
        let label = ui.label("read only");
        let button = ui.button("go");
        let check = ui.checkbox("on", &mut flag);
        let slider = ui.slider("t", &mut value, 0.0, 1.0).show();
        let readout = ui.label_value("fps", "60");
        (label, button, check, slider, readout)
    });

    for r in [label, button, check, slider, readout] {
        assert!(r.rect.w > 0.0 && r.rect.h > 0.0, "{r:?} has no area");
        assert!(r.open, "only sections ever report open == false");
    }
    // Laid out top to bottom, without overlapping.
    assert!(label.rect.max_y() <= button.rect.y);
    assert!(button.rect.max_y() <= check.rect.y);
    assert!(check.rect.max_y() <= slider.rect.y);
    assert!(slider.rect.max_y() <= readout.rect.y);
}

#[test]
fn widgets_sharing_a_row_report_side_by_side_rectangles() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
    let (left, right) = ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
        ui.horizontal(|ui| {
            let left = ui.sized([100.0, ROW_H]).button("a");
            let right = ui.button("b");
            (left, right)
        })
    });

    // The vertical ordering assertion above is the wrong shape for a row: these
    // two share a `y` and are separated horizontally instead.
    assert_eq!(left.rect.y, right.rect.y);
    assert!(left.rect.max_x() <= right.rect.x, "the cells overlap");
    assert!(left.rect.w > 0.0 && right.rect.w > 0.0);
}
