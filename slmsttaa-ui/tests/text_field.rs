//! Text editing: the caret, the selection, and the byte arithmetic underneath.
//!
//! This is the least eyeball-verifiable widget in the crate. A caret one glyph
//! out, a Backspace that eats two characters of a name with a macron in it, a
//! run that scrolls the wrong way — all of them look like a working text box in
//! a screenshot, and the last one only shows up on a string longer than the
//! field.

use slmsttaa_ui::{
    font, Anchor, DrawCmd, Event, Key, KeyEvent, Modifiers, RecordingPainter, Theme, Ui, UiInput,
    UiState,
};

const PANEL_W: f32 = 340.0;

/// One key press, with `mods` down.
fn key(key: Key, mods: Modifiers) -> Event {
    Event::Key(KeyEvent {
        key,
        pressed: true,
        repeat: false,
        modifiers: mods,
    })
}

/// One unmodified key press.
fn press(k: Key) -> Event {
    key(k, Modifiers::default())
}

/// One key press with Shift down.
fn shift(k: Key) -> Event {
    key(
        k,
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    )
}

/// One key press with the platform's shortcut modifier down.
fn command(k: Key) -> Event {
    let mut mods = Modifiers::default();
    if cfg!(target_os = "macos") {
        mods.logo = true;
    } else {
        mods.ctrl = true;
    }
    key(k, mods)
}

/// The characters of `text`, as the host would deliver them.
fn typed(text: &str) -> Vec<Event> {
    text.chars().map(Event::Text).collect()
}

/// A driver for one field across frames: it holds the persistent state so a test
/// reads as a sequence of keystrokes.
struct Field {
    painter: RecordingPainter,
    state: UiState,
    value: String,
}

impl Field {
    /// A field containing `value`, already focused — which is the state every
    /// test below wants, and takes two frames to reach through the tab ring.
    fn focused(value: &str) -> Self {
        let mut field = Field {
            painter: RecordingPainter::default(),
            state: UiState::default(),
            value: value.to_string(),
        };
        // Frame one builds the tab ring; frame two walks onto it.
        field.frame(&[]);
        field.frame(&[press(Key::Tab)]);
        field
    }

    /// Run one frame with `events` in the log, and report the field's response.
    fn frame(&mut self, events: &[Event]) -> Report {
        let input = UiInput {
            events,
            ..Default::default()
        };
        self.painter.cmds.clear();
        let mut ui = Ui::new(&mut self.painter, input, &mut self.state);
        ui.set_theme(Theme::dark());
        let response = ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.text_field("name", &mut self.value).show()
        });
        Report {
            changed: response.changed,
            submitted: response.submitted,
            wants_keyboard: ui.wants_keyboard(),
        }
    }

    /// The runs the frame actually drew, in order.
    fn drawn(&self) -> Vec<String> {
        self.painter
            .cmds
            .iter()
            .filter_map(|cmd| match cmd {
                DrawCmd::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}

/// What one frame reported.
struct Report {
    changed: bool,
    submitted: bool,
    wants_keyboard: bool,
}

#[test]
fn typing_inserts_at_the_caret() {
    let mut field = Field::focused("");
    let report = field.frame(&typed("hello"));

    assert_eq!(field.value, "hello");
    assert!(report.changed);
    assert!(
        report.wants_keyboard,
        "a focused field must eat the keyboard"
    );
}

#[test]
fn the_log_is_replayed_in_order() {
    // The property the whole seam design exists for. Both of these happen inside
    // one frame; a set of flags could not tell them apart.
    let mut early = Field::focused("");
    let mut events = typed("ab");
    events.push(press(Key::Backspace));
    early.frame(&events);
    assert_eq!(early.value, "a");

    let mut late = Field::focused("");
    let mut events = vec![press(Key::Backspace)];
    events.extend(typed("ab"));
    late.frame(&events);
    assert_eq!(late.value, "ab");
}

#[test]
fn one_frame_of_typing_matches_several() {
    let mut together = Field::focused("");
    together.frame(&typed("wide"));

    let mut apart = Field::focused("");
    for ch in "wide".chars() {
        apart.frame(&[Event::Text(ch)]);
    }

    assert_eq!(together.value, apart.value);
}

#[test]
fn backspace_and_delete_work_from_the_caret() {
    let mut field = Field::focused("abcd");
    // The caret starts at 0 — nothing has clicked or moved it — so Backspace has
    // nothing behind it and Delete eats forward.
    field.frame(&[press(Key::Backspace)]);
    assert_eq!(field.value, "abcd");

    field.frame(&[press(Key::Delete)]);
    assert_eq!(field.value, "bcd");

    field.frame(&[press(Key::End), press(Key::Backspace)]);
    assert_eq!(field.value, "bc");
}

#[test]
fn a_multi_byte_character_is_never_split() {
    // `Ōsawa` is six characters and seven bytes. A caret counted in characters
    // and applied to bytes lands inside the macron and panics — which is the bug
    // this crate's own wishlist predicted, one name in six.
    let mut field = Field::focused("Ōsawa");
    assert_eq!(
        field.value.len(),
        6,
        "expected a multi-byte first character"
    );

    field.frame(&[press(Key::Delete)]);
    assert_eq!(field.value, "sawa", "delete ate part of a codepoint");

    let mut field = Field::focused("Ōsawa");
    field.frame(&[press(Key::End), press(Key::Backspace)]);
    assert_eq!(field.value, "Ōsaw");

    // And walking the caret over it and back leaves the string alone.
    let mut field = Field::focused("Ōsawa");
    field.frame(&[press(Key::Right), press(Key::Left), press(Key::Backspace)]);
    assert_eq!(field.value, "Ōsawa");
}

#[test]
fn typing_replaces_a_selection() {
    let mut field = Field::focused("abcd");
    // Select the first two characters, then type over them.
    field.frame(&[shift(Key::Right), shift(Key::Right)]);
    field.frame(&typed("X"));
    assert_eq!(field.value, "Xcd");
}

#[test]
fn select_all_then_delete_empties_the_field() {
    let mut field = Field::focused("erodibility");
    field.frame(&[command(Key::A), press(Key::Backspace)]);
    assert_eq!(field.value, "");
}

#[test]
fn a_plain_arrow_collapses_a_selection_to_its_edge() {
    // Select "ab", then press Left: the caret goes to the *start* of the
    // selection rather than one further left, which is what every editor does.
    let mut field = Field::focused("abcd");
    field.frame(&[shift(Key::Right), shift(Key::Right), press(Key::Left)]);
    field.frame(&typed("-"));
    assert_eq!(field.value, "-abcd");

    let mut field = Field::focused("abcd");
    field.frame(&[shift(Key::Right), shift(Key::Right), press(Key::Right)]);
    field.frame(&typed("-"));
    assert_eq!(field.value, "ab-cd");
}

#[test]
fn copy_and_cut_hand_text_to_the_host() {
    let mut field = Field::focused("alps");
    field.frame(&[command(Key::A), command(Key::C)]);
    assert_eq!(field.state.take_clipboard().as_deref(), Some("alps"));
    assert_eq!(
        field.value, "alps",
        "copy is not supposed to remove anything"
    );
    // Drained, so the host is not handed the same text twice.
    assert_eq!(field.state.take_clipboard(), None);

    field.frame(&[command(Key::A), command(Key::X)]);
    assert_eq!(field.state.take_clipboard().as_deref(), Some("alps"));
    assert_eq!(field.value, "");
}

#[test]
fn enter_submits_without_changing_anything() {
    let mut field = Field::focused("query");
    let report = field.frame(&[press(Key::Enter)]);
    assert!(report.submitted);
    assert!(!report.changed);
    assert_eq!(field.value, "query");
}

#[test]
fn escape_leaves_the_field_and_typing_stops() {
    let mut field = Field::focused("name");
    let report = field.frame(&[press(Key::Escape)]);
    assert!(report.wants_keyboard, "escape was the UI's to consume");

    // Focus is gone, so the next keystroke belongs to the consumer — and the
    // field must not eat it.
    let report = field.frame(&typed("x"));
    assert_eq!(field.value, "name");
    assert!(!report.wants_keyboard);
}

#[test]
fn an_unfocused_field_ignores_the_keyboard_entirely() {
    let mut field = Field {
        painter: RecordingPainter::default(),
        state: UiState::default(),
        value: String::from("box"),
    };
    let report = field.frame(&typed("zzz"));
    assert_eq!(field.value, "box");
    assert!(!report.changed);
    assert!(!report.wants_keyboard);
}

#[test]
fn the_placeholder_shows_only_while_the_field_is_empty() {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let mut value = String::new();

    let mut run = |painter: &mut RecordingPainter, value: &mut String| {
        painter.cmds.clear();
        let mut ui = Ui::new(painter, UiInput::default(), &mut state);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.text_field("name", value).placeholder("unnamed").show()
        });
    };

    run(&mut painter, &mut value);
    assert!(painter.visible_texts().contains(&"unnamed"));

    value.push_str("crag");
    run(&mut painter, &mut value);
    let texts = painter.visible_texts();
    assert!(texts.contains(&"crag"));
    assert!(!texts.contains(&"unnamed"));
}

#[test]
fn a_run_longer_than_the_field_scrolls_to_keep_the_caret_in_view() {
    // The field is ~316 points wide inside the default panel; this string is far
    // wider, so with the caret at the end the *start* of it has to be off-screen
    // to the left. Nothing about that is visible in the draw list except the
    // position the run was drawn at, which is exactly what this checks.
    let long = "the quick brown fox jumps over the lazy dog and keeps going";
    let mut field = Field::focused(long);

    let px = Theme::dark().text.body.parts().0;
    let width = font::text_width(long, px, Theme::dark().text.body.parts().1);
    assert!(width > PANEL_W, "the fixture stopped being long enough");

    // Caret at home: the run starts at its natural left edge.
    field.frame(&[press(Key::Home)]);
    let at_home = run_x(&field);

    // Caret at the end: the run has been pulled left to bring it into view.
    field.frame(&[press(Key::End)]);
    let at_end = run_x(&field);

    assert!(
        at_end < at_home,
        "the run did not scroll: {at_end} is not left of {at_home}"
    );
    assert!(
        at_home - at_end >= width - PANEL_W,
        "the caret is still off the right edge"
    );

    // And walking back unwinds it rather than leaving the text stranded.
    field.frame(&[press(Key::Home)]);
    assert!((run_x(&field) - at_home).abs() < 0.01);
}

/// Where the field's own run was drawn, in points.
fn run_x(field: &Field) -> f32 {
    field
        .painter
        .cmds
        .iter()
        .find_map(|cmd| match cmd {
            DrawCmd::Text { x, text, .. } if text.starts_with("the quick") => Some(*x),
            _ => None,
        })
        .expect("the field drew no run")
}

#[test]
fn every_color_a_selected_field_draws_comes_from_the_theme() {
    // `tests/theme.rs` sweeps the roster, but it cannot reach this widget's
    // focused-with-a-selection state — which is exactly where a hand-picked
    // highlight colour would hide.
    let mut field = Field::focused("abcd");
    field.frame(&[command(Key::A)]);

    let theme = Theme::dark();
    let tokens = [
        theme.color.background,
        theme.color.foreground,
        theme.color.muted,
        theme.color.surface,
        theme.color.border,
        theme.color.ring,
        theme.color.selection,
        theme.color.accent,
        theme.color.accent_hover,
    ];
    for cmd in &field.painter.cmds {
        let color = match *cmd {
            DrawCmd::Rect { color, .. } | DrawCmd::Text { color, .. } => color,
        };
        assert!(
            tokens.iter().any(|token| {
                // Fades and hover blends land between two tokens, so match on
                // the hue and let the alpha move.
                token[0] == color[0] && token[1] == color[1] && token[2] == color[2]
            }),
            "{color:?} is not one of the theme's colours",
        );
    }
    assert!(
        field.drawn().iter().any(|text| text == "abcd"),
        "the field drew no text at all",
    );
}
