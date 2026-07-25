//! Id-stack tests.
//!
//! Ids are invisible until they're wrong, and then they are baffling: a drag
//! jumps to the wrong slider, or a section forgets it was collapsed because a
//! row was added somewhere above it. These pin the property that matters —
//! *an id depends on a widget's position within its section, not within the
//! whole panel*.

use slmsttaa_ui::{RecordingPainter, Ui, UiInput, UiState};

/// Run `declare` against a throwaway panel and return whatever ids it picked
/// out. `next_id` is public, so a test asks for ids exactly like a widget does.
fn ids_of<T>(declare: impl FnOnce(&mut Ui) -> T) -> T {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
    declare(&mut ui)
}

#[test]
fn the_same_label_in_the_same_place_is_the_same_id() {
    let declare = |ui: &mut Ui| {
        ui.push_id("section");
        let id = ui.next_id("frequency");
        ui.pop_id();
        id
    };
    assert_eq!(
        ids_of(declare),
        ids_of(declare),
        "ids must be stable frame to frame"
    );
}

#[test]
fn duplicate_labels_in_one_scope_get_distinct_ids() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);

    let first = ui.next_id("amount");
    let second = ui.next_id("amount");
    assert_ne!(
        first, second,
        "two widgets with one label must still be tellable apart"
    );
}

#[test]
fn the_same_label_in_different_scopes_gets_different_ids() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);

    ui.push_id("fluvial");
    let fluvial_rate = ui.next_id("rate");
    ui.pop_id();

    ui.push_id("thermal");
    let thermal_rate = ui.next_id("rate");
    ui.pop_id();

    assert_ne!(
        fluvial_rate, thermal_rate,
        "scoping is what lets two sections share a label"
    );
}

#[test]
fn adding_a_widget_in_one_scope_does_not_renumber_another() {
    // This is the property a flat sequence counter did *not* have, and the
    // reason the id stack exists: with a flat counter, inserting a row into the
    // first section shifted every id after it, so the section below forgot
    // whether it was collapsed and any in-progress drag jumped widgets.
    // Returns the id of "target", which lives in the *second* scope.
    let target_id = |extra_row: bool| {
        ids_of(move |ui: &mut Ui| {
            ui.push_id("first");
            ui.next_id("a");
            if extra_row {
                ui.next_id("newly added row");
            }
            ui.pop_id();

            ui.push_id("second");
            let target = ui.next_id("target");
            ui.pop_id();
            target
        })
    };

    assert_eq!(
        target_id(false),
        target_id(true),
        "a row added to another section must not renumber this one"
    );
}

#[test]
fn pop_id_never_unbalances_the_root_scope() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);

    // Over-popping is a consumer bug, but it must not panic or poison ids.
    ui.pop_id();
    ui.pop_id();
    let id = ui.next_id("still works");
    assert_ne!(id, 0);
}

#[test]
fn a_row_appearing_above_a_slider_mid_drag_does_not_break_the_drag() {
    // The bug this pins was found by hand, not by these tests, and it is the
    // most damaging kind: the terrain panel shows a "release to rebuild..." row
    // only while a rebuild is pending — which becomes true the instant a slider
    // is first moved. So a row appears above every slider *between the press
    // frame and the next one*. With ids keyed by declaration order, that
    // renumbered the slider, `active` stopped matching it, and the drag died
    // after exactly one frame: clicking snapped the value, dragging did nothing.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 0.0_f32;

    // Panel geometry: the first row starts one pad below the panel's top, and a
    // slider's grab band sits under its header line.
    let first_band_y = 12.0 + 10.0 + 16.0 + 5.0 + 2.0;
    let (content_x, content_w) = (22.0, 320.0);

    let mut frame = |input: UiInput, show_status: bool, v: &mut f32| {
        painter.clear();
        let mut ui = Ui::new(&mut painter, input, &mut state);
        if show_status {
            ui.label("release to rebuild...");
        }
        ui.slider("erodibility", v, 0.0, 100.0)
    };

    // Frame 1: press at the far left of the track. No status row yet.
    let press = UiInput {
        cursor: Some((content_x, first_band_y)),
        primary_held: true,
        primary_pressed: true,
    };
    frame(press, false, &mut value);
    assert_eq!(value, 0.0, "the press snaps to the left end");

    // Frame 2: still held, cursor dragged right — and now the status row exists,
    // so the slider has shifted a row down the panel.
    let drag = UiInput {
        cursor: Some((content_x + content_w, first_band_y)),
        primary_held: true,
        primary_pressed: false,
    };
    let response = frame(drag, true, &mut value);

    assert!(response.held, "the slider must still own the pointer");
    assert_eq!(value, 100.0, "and must still track the cursor");
}

#[test]
fn a_section_stays_collapsed_when_a_row_is_added_above_it() {
    // A section's collapsed state is keyed by its id, so an order-dependent id
    // would mean that adding one row anywhere above it silently re-expands it —
    // a bug that would look like the panel randomly forgetting itself.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let frame = |extra_row: bool, p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        let mut ui = Ui::new(p, UiInput::default(), s);
        ui.label("fps");
        if extra_row {
            ui.label("a row added later");
        }
        ui.section("Grid").open
    };
    let click_heading = |p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        let mut ui = Ui::new(
            p,
            UiInput {
                // The heading sits one row below the panel's first row.
                cursor: Some((40.0, 12.0 + 10.0 + 24.0 + 8.0)),
                primary_held: true,
                primary_pressed: true,
            },
            s,
        );
        ui.label("fps");
        ui.section("Grid").open
    };

    assert!(frame(false, &mut painter, &mut state), "starts expanded");
    assert!(
        !click_heading(&mut painter, &mut state),
        "click collapses it"
    );
    assert!(
        !frame(true, &mut painter, &mut state),
        "and it stays collapsed even though a row appeared above it"
    );
}

#[test]
fn two_sections_with_the_same_label_collapse_independently() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    // Both headings read "Detail"; they are distinguished only by their scope.
    let frame = |input: UiInput, p: &mut RecordingPainter, s: &mut UiState| {
        p.clear();
        let mut ui = Ui::new(p, input, s);
        ui.push_id("fluvial");
        let a = ui.section("Detail").open;
        ui.pop_id();
        ui.push_id("thermal");
        let b = ui.section("Detail").open;
        ui.pop_id();
        (a, b)
    };

    assert_eq!(
        frame(UiInput::default(), &mut painter, &mut state),
        (true, true),
        "sections start expanded"
    );

    // Click the first heading only.
    let click = UiInput {
        cursor: Some((40.0, 12.0 + 10.0 + 8.0)),
        primary_held: true,
        primary_pressed: true,
    };
    let (a, b) = frame(click, &mut painter, &mut state);
    assert!(!a, "the clicked section collapsed");
    assert!(b, "its same-named sibling did not");
}
