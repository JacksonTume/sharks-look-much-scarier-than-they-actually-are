//! Focus traversal, keyboard activation, and who gets the keyboard.
//!
//! None of this is visible in a screenshot. A tab ring that skips one control,
//! an Enter that fires twice under auto-repeat, a focused field that lets a
//! camera keep flying — each looks exactly like working software until you try
//! it, which is the same argument `interaction.rs` makes for the pointer.

use slmsttaa_ui::{
    Anchor, Event, Key, KeyEvent, Modifiers, RecordingPainter, Theme, Ui, UiInput, UiState,
};

const PANEL_W: f32 = 340.0;

/// One key press with no modifiers.
fn press(key: Key) -> Event {
    Event::Key(KeyEvent {
        key,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    })
}

/// One key press with Shift down.
fn shift_press(key: Key) -> Event {
    Event::Key(KeyEvent {
        key,
        pressed: true,
        repeat: false,
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    })
}

/// A frame whose keyboard log is `events`.
fn typing(events: &[Event]) -> UiInput<'_> {
    UiInput {
        events,
        ..Default::default()
    }
}

/// A pointer at `(x, y)` on the frame the button goes down.
fn clicking(x: f32, y: f32) -> UiInput<'static> {
    UiInput {
        cursor: Some((x, y)),
        primary_held: true,
        primary_pressed: true,
        ..Default::default()
    }
}

/// One frame: declare `body` inside the default top-left panel.
fn frame<T>(
    painter: &mut RecordingPainter,
    state: &mut UiState,
    input: UiInput,
    body: impl FnOnce(&mut Ui) -> T,
) -> T {
    let mut ui = Ui::new(painter, input, state);
    ui.panel(Anchor::TopLeft, PANEL_W, body)
}

/// Three buttons, reporting which of them holds focus.
fn three_buttons(ui: &mut Ui) -> Option<usize> {
    let mut focused = None;
    for (i, label) in ["one", "two", "three"].iter().enumerate() {
        if ui.button(label).show().focused {
            focused = Some(i);
        }
    }
    focused
}

#[test]
fn tab_walks_the_ring_and_wraps() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    // The very first frame has no ring to walk yet — it is being built as the
    // widgets declare themselves — so nothing has focus.
    let first = frame(&mut painter, &mut state, typing(&[press(Key::Tab)]), |ui| {
        three_buttons(ui)
    });
    assert_eq!(first, None);

    // From here the ring exists, and Tab enters at the top and walks down.
    for expected in [0, 1, 2, 0] {
        let got = frame(&mut painter, &mut state, typing(&[press(Key::Tab)]), |ui| {
            three_buttons(ui)
        });
        assert_eq!(got, Some(expected), "tab landed on the wrong control");
    }
}

#[test]
fn shift_tab_walks_it_backwards() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    frame(&mut painter, &mut state, UiInput::default(), three_buttons);

    // Entering backwards starts at the end, which is what makes Shift-Tab from
    // nowhere reach the last control rather than the first.
    for expected in [2, 1, 0, 2] {
        let got = frame(
            &mut painter,
            &mut state,
            typing(&[shift_press(Key::Tab)]),
            three_buttons,
        );
        assert_eq!(got, Some(expected));
    }
}

#[test]
fn focus_survives_a_row_appearing_above_it() {
    // The other half of the id bug UI Slice 1 was rewritten by. The tab *ring*
    // is positional, deliberately — but a widget's identity is not, so focus
    // stays on the same button when a status row is inserted above it.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let declare = |ui: &mut Ui, status: bool| {
        if status {
            ui.label("rebuilding...");
        }
        three_buttons(ui)
    };

    frame(&mut painter, &mut state, UiInput::default(), |ui| {
        declare(ui, false)
    });
    frame(&mut painter, &mut state, typing(&[press(Key::Tab)]), |ui| {
        declare(ui, false)
    });
    let after = frame(&mut painter, &mut state, UiInput::default(), |ui| {
        declare(ui, true)
    });

    assert_eq!(after, Some(0), "an inserted row moved the focus");
}

#[test]
fn enter_and_space_activate_a_focused_button() {
    for key in [Key::Enter, Key::Space] {
        let mut painter = RecordingPainter::default();
        let mut state = UiState::default();

        // Nothing focused: the key does nothing at all.
        let cold = frame(&mut painter, &mut state, typing(&[press(key)]), |ui| {
            ui.button("go").show().clicked
        });
        assert!(!cold, "{key:?} fired a button that did not have focus");

        frame(&mut painter, &mut state, typing(&[press(Key::Tab)]), |ui| {
            ui.button("go").show().clicked
        });
        let hot = frame(&mut painter, &mut state, typing(&[press(key)]), |ui| {
            ui.button("go").show().clicked
        });
        assert!(hot, "{key:?} did not activate the focused button");
    }
}

#[test]
fn auto_repeat_does_not_re_fire_a_button() {
    // Holding Enter on a "delete" button should delete once. This is the whole
    // reason `KeyEvent` carries `repeat` across the seam.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    frame(&mut painter, &mut state, UiInput::default(), |ui| {
        ui.button("go").show()
    });
    frame(&mut painter, &mut state, typing(&[press(Key::Tab)]), |ui| {
        ui.button("go").show()
    });

    let repeat = Event::Key(KeyEvent {
        key: Key::Enter,
        pressed: true,
        repeat: true,
        modifiers: Modifiers::default(),
    });
    let fired = frame(&mut painter, &mut state, typing(&[repeat]), |ui| {
        ui.button("go").show().clicked
    });
    assert!(!fired);
}

/// One slider frame, so the arrow test reads as a sequence of keystrokes rather
/// than as scaffolding.
fn slider_frame(
    painter: &mut RecordingPainter,
    state: &mut UiState,
    input: UiInput,
    value: &mut f32,
) {
    frame(painter, state, input, |ui| {
        ui.slider("t", value, 0.0, 1.0).show()
    });
}

#[test]
fn arrows_nudge_a_focused_slider_and_home_end_pin_it() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 0.5_f32;
    let (p, s) = (&mut painter, &mut state);

    // Unfocused, the arrows are somebody else's — a camera's, usually. (This is
    // also the frame that builds the tab ring the next one walks.)
    slider_frame(p, s, typing(&[press(Key::Right)]), &mut value);
    assert_eq!(value, 0.5);

    slider_frame(p, s, typing(&[press(Key::Tab)]), &mut value);
    slider_frame(p, s, typing(&[press(Key::Right)]), &mut value);
    assert!(
        (value - 0.51).abs() < 1.0e-5,
        "one arrow moves 1% of the range, got {value}"
    );

    slider_frame(p, s, typing(&[press(Key::PageUp)]), &mut value);
    assert!((value - 0.61).abs() < 1.0e-5, "one page moves 10%");

    slider_frame(p, s, typing(&[press(Key::Home)]), &mut value);
    assert_eq!(value, 0.0);
    slider_frame(p, s, typing(&[press(Key::End)]), &mut value);
    assert_eq!(value, 1.0);
}

#[test]
fn escape_gives_focus_up_and_is_consumed_only_when_it_had_some() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    // Bound rather than inlined: the `Ui` outlives the statement that builds it,
    // so the log has to as well.
    let escape = [press(Key::Escape)];

    // Nothing focused — the UI does not claim the key, so a consumer's own
    // Escape binding still fires. This is what makes "Escape leaves the field,
    // Escape again deselects" work without either side knowing about the other.
    let mut ui = Ui::new(&mut painter, typing(&escape), &mut state);
    ui.panel(Anchor::TopLeft, PANEL_W, three_buttons);
    assert!(!ui.wants_keyboard());
    drop(ui);

    frame(
        &mut painter,
        &mut state,
        typing(&[press(Key::Tab)]),
        three_buttons,
    );

    let mut ui = Ui::new(&mut painter, typing(&escape), &mut state);
    let focused = ui.panel(Anchor::TopLeft, PANEL_W, three_buttons);
    assert_eq!(focused, None, "escape did not drop the focus");
    assert!(ui.wants_keyboard(), "escape was not claimed by the UI");
}

#[test]
fn a_focused_button_does_not_claim_the_keyboard_by_itself() {
    // The distinction `wants_keyboard` exists to draw. A button binds only Enter
    // and Space, so clicking one must not silently kill a consumer's WASD.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    let mut ui = Ui::new(&mut painter, clicking(30.0, 30.0), &mut state);
    ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
        assert!(
            ui.button("go").show().focused,
            "the click missed the button"
        );
    });
    assert!(!ui.wants_keyboard());
    drop(ui);

    // On the frame it *is* activated, it says so.
    let enter = [press(Key::Enter)];
    let mut ui = Ui::new(&mut painter, typing(&enter), &mut state);
    ui.panel(Anchor::TopLeft, PANEL_W, |ui| ui.button("go").show());
    assert!(ui.wants_keyboard());
}

#[test]
fn a_focused_slider_claims_the_keyboard_continuously() {
    // A slider binds the arrows, and a consumer's camera reads *held* keys — so
    // the claim has to hold on every frame, not only the ones a key arrives on.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = 0.5_f32;

    slider_frame(&mut painter, &mut state, UiInput::default(), &mut value);
    slider_frame(
        &mut painter,
        &mut state,
        typing(&[press(Key::Tab)]),
        &mut value,
    );

    let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
    ui.set_theme(Theme::dark());
    ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
        ui.slider("t", &mut value, 0.0, 1.0).show()
    });
    assert!(ui.wants_keyboard(), "a quiet frame gave the keyboard back");
}

#[test]
fn a_consumer_can_drive_focus_itself() {
    // `focusable` + `set_focus` are public so a consumer's own widget — a
    // walkable list, say — is a first-class member of the tab ring.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();

    // Each row reports its own id alongside its focus, which is also how a
    // consumer would drive focus to one by name.
    fn rows(ui: &mut Ui) -> Vec<(u64, bool)> {
        (0..3)
            .map(|i| {
                let id = ui.next_id(&format!("row {i}"));
                ui.focusable(id);
                let rect = ui.allocate([0.0, 20.0]);
                (id, ui.interact(rect, id).focused)
            })
            .collect()
    }
    let focus_flags = |rows: Vec<(u64, bool)>| rows.iter().map(|(_, f)| *f).collect::<Vec<_>>();

    let ids = frame(&mut painter, &mut state, UiInput::default(), rows);
    let walked = frame(&mut painter, &mut state, typing(&[press(Key::Tab)]), rows);
    assert_eq!(focus_flags(walked), vec![true, false, false]);

    // And it can hand focus wherever it likes — here to the last row, which no
    // amount of Tab-from-the-top would have reached in one step.
    let last = ids[2].0;
    let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
    let chosen = ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
        ui.set_focus(Some(last));
        rows(ui)
    });
    assert_eq!(focus_flags(chosen), vec![false, false, true]);
}
