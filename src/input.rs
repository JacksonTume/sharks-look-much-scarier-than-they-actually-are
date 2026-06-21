//! Per-frame input state, decoupled from `winit`.
//!
//! The engine owns the event loop (see [`crate::app`]), so the consumer never
//! sees a raw `winit` event. Instead the loop funnels keyboard and mouse events
//! into an [`Input`] snapshot, and the consumer reads it each frame through
//! [`Renderer::input`](crate::Renderer::input).
//!
//! Holding the engine/consumer boundary (roadmap principle 1) means the public
//! surface here must speak in **engine** types — [`Key`], [`MouseButton`] — never
//! `winit`'s. The `winit`→engine translation lives in the `pub(crate)` methods,
//! which the consumer cannot reach.
//!
//! Two flavors of state live here:
//!
//! - **Held state** ([`Input::is_key_held`], [`Input::is_mouse_held`]) persists
//!   across frames until the key/button is released.
//! - **Per-frame deltas** ([`Input::mouse_delta`], [`Input::scroll_delta`]) are
//!   accumulated during a frame and zeroed by [`Input::end_frame`] once the frame
//!   has been drawn, so each `update` sees only that frame's motion.
//! - **Press edges** ([`Input::is_mouse_pressed`]) fire only on the frame a button
//!   went down, which is what point-and-click UI hit-testing wants — distinct from
//!   the held state that drives a camera drag.
//!
//! The absolute [`Input::cursor_position`] is also exposed (not just the delta),
//! because screen-space UI needs to know *where* the pointer is, not only how far
//! it moved.

/// A keyboard key the engine reports to the consumer.
///
/// Deliberately minimal: just the keys the current demos drive a camera with. Add
/// variants here (and to the mapping in [`Input::on_keyboard`]) when a consumer
/// demands them — never speculatively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    W,
    A,
    S,
    D,
    Up,
    Down,
    Left,
    Right,
}

impl Key {
    /// How many variants exist — the size of the held-key table.
    const COUNT: usize = 8;

    /// Index into the held-key table.
    fn index(self) -> usize {
        self as usize
    }
}

/// A mouse button the engine reports to the consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    /// How many variants exist — the size of the held-button table.
    const COUNT: usize = 3;

    /// Index into the held-button table.
    fn index(self) -> usize {
        self as usize
    }
}

/// A snapshot of input the consumer reads once per frame.
///
/// Built and maintained by the engine's event loop; the consumer only ever reads
/// it via [`Renderer::input`](crate::Renderer::input).
#[derive(Debug, Default)]
pub struct Input {
    /// Held state per [`Key`], indexed by [`Key::index`].
    keys: [bool; Key::COUNT],
    /// Held state per [`MouseButton`], indexed by [`MouseButton::index`].
    buttons: [bool; MouseButton::COUNT],
    /// Buttons that transitioned to pressed *this frame* (press edge). Cleared by
    /// [`Input::end_frame`]; drives click hit-testing in the UI.
    pressed: [bool; MouseButton::COUNT],
    /// Last cursor position seen, used to turn absolute moves into deltas.
    cursor: Option<(f32, f32)>,
    /// Net cursor motion accumulated this frame, in physical pixels.
    mouse_delta: (f32, f32),
    /// Net wheel scroll accumulated this frame (positive = scroll up / zoom in).
    scroll_delta: f32,
}

impl Input {
    /// Whether `key` is currently held down.
    pub fn is_key_held(&self, key: Key) -> bool {
        self.keys[key.index()]
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

    /// Record a key press/release. Unmapped keys are ignored.
    pub(crate) fn on_keyboard(&mut self, event: &winit::event::KeyEvent) {
        use winit::keyboard::{KeyCode, PhysicalKey};

        let key = match event.physical_key {
            PhysicalKey::Code(KeyCode::KeyW) => Key::W,
            PhysicalKey::Code(KeyCode::KeyA) => Key::A,
            PhysicalKey::Code(KeyCode::KeyS) => Key::S,
            PhysicalKey::Code(KeyCode::KeyD) => Key::D,
            PhysicalKey::Code(KeyCode::ArrowUp) => Key::Up,
            PhysicalKey::Code(KeyCode::ArrowDown) => Key::Down,
            PhysicalKey::Code(KeyCode::ArrowLeft) => Key::Left,
            PhysicalKey::Code(KeyCode::ArrowRight) => Key::Right,
            _ => return,
        };
        self.keys[key.index()] = event.state.is_pressed();
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
            _ => return,
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

    /// Clear the per-frame deltas after a frame has been consumed. Held key and
    /// button state is preserved.
    pub(crate) fn end_frame(&mut self) {
        self.mouse_delta = (0.0, 0.0);
        self.scroll_delta = 0.0;
        self.pressed = [false; MouseButton::COUNT];
    }
}
