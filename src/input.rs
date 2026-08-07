//! Per-frame input state, decoupled from `winit`.
//!
//! The engine owns the event loop (see [`crate::app`]), so the consumer never
//! sees a raw `winit` event. Instead the loop funnels keyboard and mouse events
//! into an [`Input`] snapshot, and the consumer reads it each frame through
//! [`Renderer::input`](crate::Renderer::input).
//!
//! Holding the engine/consumer boundary (roadmap principle 1) means the public
//! surface here must speak in **engine** types — [`Key`], [`MouseButton`],
//! [`Modifiers`] — never `winit`'s. The `winit`→engine translation lives in the
//! `pub(crate)` methods, which the consumer cannot reach.
//!
//! Three flavors of state live here:
//!
//! - **Held state** ([`Input::is_key_held`], [`Input::is_mouse_held`]) persists
//!   across frames until the key/button is released.
//! - **Per-frame deltas** ([`Input::mouse_delta`], [`Input::scroll_delta`]) are
//!   accumulated during a frame and zeroed by [`Input::end_frame`] once the frame
//!   has been drawn, so each `update` sees only that frame's motion.
//! - **Press edges** ([`Input::is_mouse_pressed`], [`Input::is_key_pressed`]) fire
//!   only on the frame a button or key went down, which is what point-and-click UI
//!   hit-testing and one-shot shortcuts want — distinct from the held state that
//!   drives a camera drag.
//!
//! The absolute [`Input::cursor_position`] is also exposed (not just the delta),
//! because screen-space UI needs to know *where* the pointer is, not only how far
//! it moved.
//!
//! # Why there is an event *log* as well
//!
//! The three flavors above are all **levels** — they answer "what is true at the
//! end of this frame". A text field needs something they cannot express: *order*.
//! Type `abc` and then hit Backspace inside one frame and the result depends
//! entirely on which happened first, and a set of flags has thrown that away.
//!
//! So keyboard input is additionally recorded as an ordered [`Event`] log
//! ([`Input::events`]), drained each frame alongside the deltas. It carries typed
//! characters too, which is the other thing a level cannot hold — see
//! [`Event::Text`] for why those are a separate channel from [`Key`] rather than
//! derived from it.

/// A keyboard key the engine reports to the consumer.
///
/// **Physical** positions, not layout-dependent labels: [`Key::W`] is whichever
/// key sits where `W` does on a US layout, which is `Z` on AZERTY. That is what a
/// consumer binding movement keys wants. It is emphatically *not* what a consumer
/// wants for text — see [`Event::Text`].
///
/// The set was chosen by what consumers have asked for: letters and digits so
/// arbitrary shortcuts can be bound, the arrows and editing keys a text field and
/// keyboard navigation need, and nothing else. Function keys, the numpad and the
/// punctuation keys are absent until something wants one.
///
/// [`Key::PageDown`] must stay last — [`Key::COUNT`] is derived from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// The `A` key.
    A,
    /// The `B` key.
    B,
    /// The `C` key.
    C,
    /// The `D` key.
    D,
    /// The `E` key.
    E,
    /// The `F` key.
    F,
    /// The `G` key.
    G,
    /// The `H` key.
    H,
    /// The `I` key.
    I,
    /// The `J` key.
    J,
    /// The `K` key.
    K,
    /// The `L` key.
    L,
    /// The `M` key.
    M,
    /// The `N` key.
    N,
    /// The `O` key.
    O,
    /// The `P` key.
    P,
    /// The `Q` key.
    Q,
    /// The `R` key.
    R,
    /// The `S` key.
    S,
    /// The `T` key.
    T,
    /// The `U` key.
    U,
    /// The `V` key.
    V,
    /// The `W` key.
    W,
    /// The `X` key.
    X,
    /// The `Y` key.
    Y,
    /// The `Z` key.
    Z,
    /// The `0` key on the number row.
    Digit0,
    /// The `1` key on the number row.
    Digit1,
    /// The `2` key on the number row.
    Digit2,
    /// The `3` key on the number row.
    Digit3,
    /// The `4` key on the number row.
    Digit4,
    /// The `5` key on the number row.
    Digit5,
    /// The `6` key on the number row.
    Digit6,
    /// The `7` key on the number row.
    Digit7,
    /// The `8` key on the number row.
    Digit8,
    /// The `9` key on the number row.
    Digit9,
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// The left arrow.
    Left,
    /// The right arrow.
    Right,
    /// Escape.
    Escape,
    /// Tab.
    Tab,
    /// Enter / Return.
    Enter,
    /// The space bar. Also produces a `' '` [`Event::Text`].
    Space,
    /// Backspace — deletes backward from the caret.
    Backspace,
    /// Delete — deletes forward from the caret.
    Delete,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down. **Keep this last**; [`Key::COUNT`] is derived from it.
    PageDown,
}

impl Key {
    /// How many variants exist — the size of the held-key table.
    ///
    /// Derived from the last variant rather than typed out, so adding a key in
    /// the middle cannot silently under-size the table.
    pub const COUNT: usize = Key::PageDown as usize + 1;

    /// Index into the held-key table.
    fn index(self) -> usize {
        self as usize
    }

    /// Map a `winit` physical key onto ours, or `None` if we don't report it.
    fn from_winit(physical: winit::keyboard::PhysicalKey) -> Option<Self> {
        use winit::keyboard::{KeyCode, PhysicalKey};

        let PhysicalKey::Code(code) = physical else {
            return None;
        };
        Some(match code {
            KeyCode::KeyA => Key::A,
            KeyCode::KeyB => Key::B,
            KeyCode::KeyC => Key::C,
            KeyCode::KeyD => Key::D,
            KeyCode::KeyE => Key::E,
            KeyCode::KeyF => Key::F,
            KeyCode::KeyG => Key::G,
            KeyCode::KeyH => Key::H,
            KeyCode::KeyI => Key::I,
            KeyCode::KeyJ => Key::J,
            KeyCode::KeyK => Key::K,
            KeyCode::KeyL => Key::L,
            KeyCode::KeyM => Key::M,
            KeyCode::KeyN => Key::N,
            KeyCode::KeyO => Key::O,
            KeyCode::KeyP => Key::P,
            KeyCode::KeyQ => Key::Q,
            KeyCode::KeyR => Key::R,
            KeyCode::KeyS => Key::S,
            KeyCode::KeyT => Key::T,
            KeyCode::KeyU => Key::U,
            KeyCode::KeyV => Key::V,
            KeyCode::KeyW => Key::W,
            KeyCode::KeyX => Key::X,
            KeyCode::KeyY => Key::Y,
            KeyCode::KeyZ => Key::Z,
            KeyCode::Digit0 => Key::Digit0,
            KeyCode::Digit1 => Key::Digit1,
            KeyCode::Digit2 => Key::Digit2,
            KeyCode::Digit3 => Key::Digit3,
            KeyCode::Digit4 => Key::Digit4,
            KeyCode::Digit5 => Key::Digit5,
            KeyCode::Digit6 => Key::Digit6,
            KeyCode::Digit7 => Key::Digit7,
            KeyCode::Digit8 => Key::Digit8,
            KeyCode::Digit9 => Key::Digit9,
            KeyCode::ArrowUp => Key::Up,
            KeyCode::ArrowDown => Key::Down,
            KeyCode::ArrowLeft => Key::Left,
            KeyCode::ArrowRight => Key::Right,
            KeyCode::Escape => Key::Escape,
            KeyCode::Tab => Key::Tab,
            KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
            KeyCode::Space => Key::Space,
            KeyCode::Backspace => Key::Backspace,
            KeyCode::Delete => Key::Delete,
            KeyCode::Home => Key::Home,
            KeyCode::End => Key::End,
            KeyCode::PageUp => Key::PageUp,
            KeyCode::PageDown => Key::PageDown,
            _ => return None,
        })
    }
}

/// Which modifier keys were down.
///
/// A snapshot rather than four booleans scattered around, so it can ride along on
/// every [`KeyEvent`] *and* be read as the current state via
/// [`Input::modifiers`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Either Shift.
    pub shift: bool,
    /// Either Ctrl.
    pub ctrl: bool,
    /// Either Alt (Option on macOS).
    pub alt: bool,
    /// The platform "logo" key — Windows, Command, or Super.
    pub logo: bool,
}

impl Modifiers {
    /// Whether the platform's **shortcut** modifier is down: Command on macOS,
    /// Ctrl everywhere else.
    ///
    /// Bind `Ctrl+C`-shaped shortcuts through this rather than through
    /// [`Modifiers::ctrl`], or the same binary is wrong on one platform.
    pub fn command(&self) -> bool {
        if cfg!(target_os = "macos") {
            self.logo
        } else {
            self.ctrl
        }
    }

    /// Whether no modifier at all is down — the guard a plain, unmodified
    /// shortcut wants so it doesn't also fire under Ctrl.
    pub fn none(&self) -> bool {
        !self.shift && !self.ctrl && !self.alt && !self.logo
    }
}

/// One keyboard transition, as recorded in [`Input::events`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    /// Which key moved.
    pub key: Key,
    /// `true` on the way down, `false` on the way up.
    pub pressed: bool,
    /// Whether this is the operating system's auto-repeat rather than a fresh
    /// press. A text field honors repeats (holding Backspace should keep
    /// deleting); a one-shot shortcut ignores them.
    pub repeat: bool,
    /// The modifiers that were down at the moment of the transition.
    pub modifiers: Modifiers,
}

/// One entry in the ordered per-frame input log.
///
/// Ordering is the entire reason this exists — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A key went down or came up.
    Key(KeyEvent),
    /// A character was **typed**.
    ///
    /// Deliberately a separate channel from [`Key`], and not derivable from one.
    /// The platform has already applied the keyboard layout, the shift state, and
    /// any dead-key composition by the time it reports this; reconstructing `'A'`
    /// from `Key::A` plus a shift flag is the classic way to produce software that
    /// only works on a US layout.
    ///
    /// Control characters are filtered out on the way in, so this is always
    /// something a font can draw. (`winit` reports `"\r"` for Enter and `"\t"`
    /// for Tab in the same field, on both native and the web.)
    Text(char),
}

/// A mouse button the engine reports to the consumer.
///
/// [`MouseButton::Forward`] must stay last — [`MouseButton::COUNT`] is derived
/// from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// The primary button.
    Left,
    /// The secondary button.
    Right,
    /// The wheel click.
    Middle,
    /// The thumb button that conventionally means "back" (mouse-4).
    Back,
    /// The thumb button that conventionally means "forward" (mouse-5). **Keep
    /// this last**; [`MouseButton::COUNT`] is derived from it.
    Forward,
}

impl MouseButton {
    /// How many variants exist — the size of the held-button table.
    pub const COUNT: usize = MouseButton::Forward as usize + 1;

    /// Index into the held-button table.
    fn index(self) -> usize {
        self as usize
    }
}

/// A snapshot of input the consumer reads once per frame.
///
/// Built and maintained by the engine's event loop; the consumer only ever reads
/// it via [`Renderer::input`](crate::Renderer::input).
#[derive(Debug)]
pub struct Input {
    /// Held state per [`Key`], indexed by [`Key::index`].
    keys: [bool; Key::COUNT],
    /// Keys that transitioned to pressed *this frame*, auto-repeat excluded.
    /// Cleared by [`Input::end_frame`].
    keys_pressed: [bool; Key::COUNT],
    /// Held state per [`MouseButton`], indexed by [`MouseButton::index`].
    buttons: [bool; MouseButton::COUNT],
    /// Buttons that transitioned to pressed *this frame* (press edge). Cleared by
    /// [`Input::end_frame`]; drives click hit-testing in the UI.
    pressed: [bool; MouseButton::COUNT],
    /// Which modifiers are currently down.
    modifiers: Modifiers,
    /// This frame's keyboard events, **in the order they arrived**. Drained by
    /// [`Input::end_frame`].
    events: Vec<Event>,
    /// Last cursor position seen, used to turn absolute moves into deltas.
    cursor: Option<(f32, f32)>,
    /// Net cursor motion accumulated this frame, in physical pixels.
    mouse_delta: (f32, f32),
    /// Net wheel scroll accumulated this frame (positive = scroll up / zoom in).
    scroll_delta: f32,
}

/// Whether `modifiers` make the keystroke a **shortcut** rather than typing.
///
/// Ctrl+Alt is excluded because that is AltGr on a European layout, and it types
/// real characters — `@` on a German keyboard. Shift alone is obviously typing.
fn is_shortcut(modifiers: Modifiers) -> bool {
    (modifiers.ctrl && !modifiers.alt) || modifiers.logo
}

/// Written out rather than derived: `Default` stops at 32-element arrays, and
/// the key tables are longer than that.
impl Default for Input {
    fn default() -> Self {
        Self {
            keys: [false; Key::COUNT],
            keys_pressed: [false; Key::COUNT],
            buttons: [false; MouseButton::COUNT],
            pressed: [false; MouseButton::COUNT],
            modifiers: Modifiers::default(),
            events: Vec::new(),
            cursor: None,
            mouse_delta: (0.0, 0.0),
            scroll_delta: 0.0,
        }
    }
}

impl Input {
    /// Whether `key` is currently held down.
    pub fn is_key_held(&self, key: Key) -> bool {
        self.keys[key.index()]
    }

    /// Whether `key` went down *this frame* — a press edge, true for one frame
    /// only, and **not** repeated by the operating system's auto-repeat.
    ///
    /// This is what a one-shot shortcut ("Delete removes the selection") wants.
    /// Use [`Input::is_key_held`] for continuous motion, and [`Input::events`]
    /// when auto-repeat *should* count.
    pub fn is_key_pressed(&self, key: Key) -> bool {
        self.keys_pressed[key.index()]
    }

    /// Which modifier keys are currently down.
    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// This frame's keyboard events, in the order they arrived.
    ///
    /// Cleared each frame by [`Input::end_frame`]. Reach for this when order or
    /// typed characters matter; the level getters above are simpler for
    /// everything else.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Whether `button` is currently held down.
    pub fn is_mouse_held(&self, button: MouseButton) -> bool {
        self.buttons[button.index()]
    }

    /// Whether `button` was pressed *this frame* (a press edge, true for one
    /// frame only). Use this for click activation; use [`Input::is_mouse_held`]
    /// for drags.
    pub fn is_mouse_pressed(&self, button: MouseButton) -> bool {
        self.pressed[button.index()]
    }

    /// The cursor's last-known position in physical pixels (`(x, y)`, origin
    /// top-left), or `None` if it hasn't been seen yet. Screen-space UI hit-tests
    /// against this.
    pub fn cursor_position(&self) -> Option<(f32, f32)> {
        self.cursor
    }

    /// Net cursor motion this frame, in physical pixels (`(dx, dy)`).
    ///
    /// `dy` is positive downward (screen convention). Cleared each frame by
    /// [`Input::end_frame`], so a stationary cursor reports `(0.0, 0.0)`.
    pub fn mouse_delta(&self) -> (f32, f32) {
        self.mouse_delta
    }

    /// Net wheel scroll this frame; positive means scrolling up. Cleared each
    /// frame by [`Input::end_frame`].
    pub fn scroll_delta(&self) -> f32 {
        self.scroll_delta
    }

    // --- Engine-internal accumulation ------------------------------------
    //
    // These take `winit` types and map them onto the engine enums above. They are
    // `pub(crate)` so the boundary holds: a consumer can read `Input` but cannot
    // feed it `winit` events.
    //
    // The `winit`→engine translation and the accumulation are split on purpose.
    // `winit::event::KeyEvent` carries a private field, so no test outside `winit`
    // can build one — the accumulation half would be untestable if the two were
    // one function.

    /// Record a key press/release. Unmapped keys are ignored.
    pub(crate) fn on_keyboard(&mut self, event: &winit::event::KeyEvent) {
        let pressed = event.state.is_pressed();
        if let Some(key) = Key::from_winit(event.physical_key) {
            self.push_key(key, pressed, event.repeat);
        }
        // Typed text rides along on the press, already layout- and shift-resolved
        // by the platform. Releases carry none.
        //
        // A keystroke held under a shortcut modifier is **not typing**, and the
        // platform does not always agree: Windows reports `text: Some("a")` for
        // Ctrl+A, so a text field that trusted this channel would insert an `a`
        // every time someone tried to select all. Found by pressing Ctrl+A in the
        // editor demo and watching the name become "ac".
        //
        // Alt is deliberately *not* disqualifying on its own, and neither is
        // Ctrl+Alt: that combination is AltGr on a European layout, and it is how
        // a German keyboard types `@`.
        if pressed && !is_shortcut(self.modifiers) {
            if let Some(text) = &event.text {
                for ch in text.chars() {
                    self.push_text(ch);
                }
            }
        }
    }

    /// Record which modifiers are now down.
    pub(crate) fn on_modifiers(&mut self, modifiers: &winit::event::Modifiers) {
        let state = modifiers.state();
        self.modifiers = Modifiers {
            shift: state.shift_key(),
            ctrl: state.control_key(),
            alt: state.alt_key(),
            logo: state.super_key(),
        };
    }

    /// Fold one key transition into the held state, the press edges, and the log.
    fn push_key(&mut self, key: Key, pressed: bool, repeat: bool) {
        // A press edge is a release→press transition, and auto-repeat is neither.
        if pressed && !repeat && !self.keys[key.index()] {
            self.keys_pressed[key.index()] = true;
        }
        self.keys[key.index()] = pressed;
        self.events.push(Event::Key(KeyEvent {
            key,
            pressed,
            repeat,
            modifiers: self.modifiers,
        }));
    }

    /// Append one typed character, unless it is a control code.
    fn push_text(&mut self, ch: char) {
        if !ch.is_control() {
            self.events.push(Event::Text(ch));
        }
    }

    /// Feed pasted text in as if it had been typed.
    ///
    /// That framing is the whole design: a paste is a run of characters arriving
    /// at the caret, which is exactly what typing is, so nothing downstream — not
    /// the UI seam, not the text field — needs a concept of a clipboard. Newlines
    /// are control characters and are dropped on the way in, which is also what a
    /// single-line field wants.
    pub(crate) fn push_pasted(&mut self, text: &str) {
        for ch in text.chars() {
            self.push_text(ch);
        }
    }

    /// Record a mouse button press/release. Unmapped buttons are ignored.
    pub(crate) fn on_mouse_button(
        &mut self,
        state: winit::event::ElementState,
        button: winit::event::MouseButton,
    ) {
        let button = match button {
            winit::event::MouseButton::Left => MouseButton::Left,
            winit::event::MouseButton::Right => MouseButton::Right,
            winit::event::MouseButton::Middle => MouseButton::Middle,
            winit::event::MouseButton::Back => MouseButton::Back,
            winit::event::MouseButton::Forward => MouseButton::Forward,
            winit::event::MouseButton::Other(_) => return,
        };
        let pressed = state.is_pressed();
        // A press edge is a release→press transition this frame.
        if pressed && !self.buttons[button.index()] {
            self.pressed[button.index()] = true;
        }
        self.buttons[button.index()] = pressed;
    }

    /// Record an absolute cursor position, accumulating the delta from the last.
    pub(crate) fn on_cursor_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        let now = (position.x as f32, position.y as f32);
        if let Some((lx, ly)) = self.cursor {
            self.mouse_delta.0 += now.0 - lx;
            self.mouse_delta.1 += now.1 - ly;
        }
        self.cursor = Some(now);
    }

    /// Record a wheel scroll event, accumulating into this frame's delta.
    pub(crate) fn on_scroll(&mut self, delta: winit::event::MouseScrollDelta) {
        self.scroll_delta += match delta {
            winit::event::MouseScrollDelta::LineDelta(_, y) => y,
            // Trackpads report pixels; scale down so it's comparable to lines.
            winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 50.0,
        };
    }

    /// Clear the per-frame deltas, press edges and event log after a frame has
    /// been consumed. Held key/button state and the modifiers are preserved.
    pub(crate) fn end_frame(&mut self) {
        self.mouse_delta = (0.0, 0.0);
        self.scroll_delta = 0.0;
        self.pressed = [false; MouseButton::COUNT];
        self.keys_pressed = [false; Key::COUNT];
        // Reuses the allocation: a frame's worth of keystrokes is a handful, and
        // this runs every frame forever.
        self.events.clear();
    }

    /// Throw away accumulated motion, keeping button state and press edges.
    ///
    /// For the screenshot harness, which parks the engine on a frame and then
    /// warps the cursor somewhere to click. All of that motion arrives as one
    /// enormous `mouse_delta` on the next real frame, and a consumer that orbits
    /// a camera by the delta would snap round before the click was even read.
    ///
    /// The press edges deliberately survive: the whole point of holding a frame
    /// is that a click delivered during it should land on the frame that follows.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn discard_motion(&mut self) {
        self.mouse_delta = (0.0, 0.0);
        self.scroll_delta = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The held-key table has to be as long as the enum, or a key at the end
    /// indexes out of bounds. Cheap to assert, catastrophic to get wrong.
    #[test]
    fn the_tables_cover_every_variant() {
        let mut input = Input::default();
        input.push_key(Key::PageDown, true, false);
        assert!(input.is_key_held(Key::PageDown));
        assert_eq!(Key::COUNT, 50);

        input.on_mouse_button_for_test(MouseButton::Forward, true);
        assert!(input.is_mouse_held(MouseButton::Forward));
        assert_eq!(MouseButton::COUNT, 5);
    }

    #[test]
    fn a_press_edge_lasts_exactly_one_frame() {
        let mut input = Input::default();
        input.push_key(Key::Delete, true, false);

        assert!(input.is_key_pressed(Key::Delete));
        assert!(input.is_key_held(Key::Delete));

        input.end_frame();
        // Still held — nothing released it — but the edge is spent.
        assert!(!input.is_key_pressed(Key::Delete));
        assert!(input.is_key_held(Key::Delete));
    }

    #[test]
    fn auto_repeat_is_not_a_press_edge() {
        let mut input = Input::default();
        input.push_key(Key::Backspace, true, false);
        input.end_frame();

        // The OS keeps sending presses while the key is down. A shortcut bound to
        // Backspace must not fire sixty times a second.
        input.push_key(Key::Backspace, true, true);
        assert!(!input.is_key_pressed(Key::Backspace));
        assert!(input.is_key_held(Key::Backspace));

        // But the log still carries them, flagged — which is what lets a text
        // field delete continuously while a shortcut doesn't.
        let repeats = input
            .events()
            .iter()
            .filter(|e| matches!(e, Event::Key(k) if k.repeat))
            .count();
        assert_eq!(repeats, 1);
    }

    #[test]
    fn releasing_and_pressing_again_is_a_fresh_edge() {
        let mut input = Input::default();
        input.push_key(Key::Enter, true, false);
        input.push_key(Key::Enter, false, false);
        input.end_frame();

        input.push_key(Key::Enter, true, false);
        assert!(input.is_key_pressed(Key::Enter));
    }

    #[test]
    fn the_log_preserves_order_across_kinds() {
        // The whole reason the log exists: "type ab, then backspace" and
        // "backspace, then type ab" are different, and no set of flags can tell
        // them apart.
        let mut input = Input::default();
        input.push_text('a');
        input.push_text('b');
        input.push_key(Key::Backspace, true, false);

        assert_eq!(
            input.events(),
            &[
                Event::Text('a'),
                Event::Text('b'),
                Event::Key(KeyEvent {
                    key: Key::Backspace,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::default(),
                }),
            ]
        );

        input.end_frame();
        assert!(input.events().is_empty());
    }

    #[test]
    fn a_shortcut_is_not_typing() {
        // Windows reports `text: Some("a")` for Ctrl+A, so this rule is the only
        // thing between "select all" and a stray `a` in every text field. The
        // AltGr case is the exception that makes it a rule rather than a
        // blanket ban on modifiers.
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let altgr = Modifiers {
            ctrl: true,
            alt: true,
            ..Default::default()
        };
        assert!(is_shortcut(ctrl));
        assert!(!is_shortcut(altgr));
        assert!(!is_shortcut(Modifiers {
            shift: true,
            ..Default::default()
        }));
    }

    #[test]
    fn control_characters_never_reach_the_log() {
        // `winit` reports "\r" for Enter and "\t" for Tab in the same field it
        // reports real text in. A text field that inserted those would grow
        // invisible glyphs.
        let mut input = Input::default();
        input.push_text('\r');
        input.push_text('\t');
        input.push_text('\u{8}');
        input.push_text(' ');

        assert_eq!(input.events(), &[Event::Text(' ')]);
    }

    #[test]
    fn events_carry_the_modifiers_that_were_down() {
        let mut input = Input {
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
            ..Default::default()
        };
        input.push_key(Key::C, true, false);

        let Some(Event::Key(event)) = input.events().first() else {
            panic!("expected a key event");
        };
        assert!(event.modifiers.ctrl);
        assert!(!event.modifiers.none());
    }

    #[test]
    fn command_follows_the_platform() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let logo = Modifiers {
            logo: true,
            ..Default::default()
        };
        // Exactly one of the two is the shortcut modifier, whichever way this
        // builds — which is the point of the helper.
        assert_ne!(ctrl.command(), logo.command());
        assert_eq!(ctrl.command(), !cfg!(target_os = "macos"));
    }

    impl Input {
        /// The mouse half of the accumulation, reachable without a `winit` event.
        fn on_mouse_button_for_test(&mut self, button: MouseButton, pressed: bool) {
            if pressed && !self.buttons[button.index()] {
                self.pressed[button.index()] = true;
            }
            self.buttons[button.index()] = pressed;
        }
    }
}
