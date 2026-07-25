//! # slmsttaa-ui — a small, decoupled immediate-mode UI toolkit
//!
//! The controls half of [SLMSTTAA](https://github.com/Jackson-Tume/sharks-look-much-scarier-than-they-actually-are).
//! It lets a consumer expose knobs without touching a GPU — and it is a separate
//! crate on purpose, because that turns "the UI never sees `wgpu`" from a
//! comment into a compile error.
//!
//! It has **zero dependencies**, and that is a rule rather than a coincidence:
//! no `wgpu`, no `winit`, not even the engine. See the crate README for the
//! dependency-direction argument.
//!
//! ## The two seams
//!
//! - **Downward, from the renderer.** The UI draws through the [`Painter`]
//!   trait — `rect` / `text` / `text_size` — and reads a [`UiInput`] snapshot
//!   the host fills in. The engine's overlay is one `Painter`;
//!   [`RecordingPainter`] is another.
//! - **Upward, from the consumer.** Widgets borrow the consumer's own
//!   `&mut f32` / `&mut bool`, so the UI has no idea *what* it controls. Erosion
//!   parameters live in the terrain demo, which is where they belong.
//!
//! ## Immediate mode
//!
//! The consumer re-declares the whole panel every frame from its current state.
//! The only thing that survives between frames is a tiny [`UiState`] — which
//! slider is being dragged, and last frame's panel height. There is no retained
//! widget tree; that keeps the surface small and sidesteps the "accidentally
//! rebuild a worse Bevy" trap.
//!
//! ## Using it
//!
//! Through the engine, this is `renderer.ui()` and the painter is wired up for
//! you. Standalone — which is also how it is tested — you supply the painter:
//!
//! ```
//! use slmsttaa_ui::{RecordingPainter, Ui, UiInput, UiState};
//!
//! // Owned by the host, and outlive the frame.
//! let mut painter = RecordingPainter::default();
//! let mut state = UiState::default();
//! let mut erodibility = 0.003_f32;
//!
//! let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
//! ui.title("Erosion");
//! ui.label("60 fps");
//! ui.slider_fmt("erodibility", &mut erodibility, 0.0, 0.006, 4);
//! if ui.button("new seed") { /* reseed */ }
//! let recompute = ui.changed();
//! # let _ = recompute;
//! ```

#![deny(missing_docs)]

mod interact;
mod layout;
mod painter;
mod theme;
mod widgets;

pub use interact::{UiInput, UiState};
pub use layout::Rect;
pub use painter::{Color, DrawCmd, Painter, RecordingPainter};

use layout::Layout;

/// One frame of the immediate-mode UI: a single left-anchored panel.
///
/// Construct it at the top of your update (via `Renderer::ui` when you are using
/// the engine), declare widgets top-to-bottom, then read [`Ui::changed`].
/// Dropping it records the laid-out height so next frame's background fits.
pub struct Ui<'a> {
    painter: &'a mut dyn Painter,
    input: UiInput,
    state: &'a mut UiState,
    layout: Layout,
    /// Monotonic widget counter, hashed into stable per-widget ids.
    seq: u64,
    /// Whether any value-editing widget changed a bound value this frame.
    changed: bool,
}

impl<'a> Ui<'a> {
    /// Begin a UI frame against `painter`, this frame's `input`, and the host's
    /// persistent `state`.
    ///
    /// Drawing starts immediately: the panel background goes down first, sized
    /// from last frame's height (the contents are laid out top-down, so this
    /// frame's height isn't known yet). Layout is stable frame-to-frame, so it
    /// is correct from the second frame on — and the ordered draw layers that
    /// retire this trick are UI Slice 1.
    pub fn new(painter: &'a mut dyn Painter, input: UiInput, state: &'a mut UiState) -> Self {
        let bg = panel_rect(state.panel_height);
        painter.rect(bg.x, bg.y, bg.w, bg.h, theme::COL_PANEL);

        Self {
            painter,
            input,
            state,
            layout: Layout::new(theme::PANEL_Y, theme::PAD),
            seq: 0,
            changed: false,
        }
    }

    /// Whether the pointer is over the panel (or actively dragging a widget), so
    /// the consumer can suppress world interactions like a camera drag.
    pub fn wants_pointer(&self) -> bool {
        self.state.active.is_some() || self.input.hits(panel_rect(self.state.panel_height))
    }

    /// Whether any slider or checkbox edited its bound value this frame — the
    /// signal a consumer uses to recompute derived state (e.g. re-run erosion).
    pub fn changed(&self) -> bool {
        self.changed
    }

    // --- Widget-facing internals -------------------------------------------
    // Private, but visible to the `widgets` submodules, which are descendants of
    // this module. UI Slice 1 promotes the allocate/interact/painter trio to
    // public API so a consumer can write a widget this crate never shipped.

    /// The next unused widget id for `label`.
    fn next_id(&mut self, label: &str) -> u64 {
        let id = interact::hash_id(self.seq, label);
        self.seq += 1;
        id
    }

    /// Claim a full-width row `h` tall and return its rectangle.
    fn row(&mut self, h: f32) -> Rect {
        Rect::new(theme::CONTENT_X, self.layout.y(), theme::CONTENT_W, h)
    }

    /// Whether the pointer is inside `rect` this frame.
    fn hovered(&self, rect: Rect) -> bool {
        self.input.hits(rect)
    }
}

impl Drop for Ui<'_> {
    fn drop(&mut self) {
        // Record the laid-out height so next frame's background fits.
        self.state.panel_height = self.layout.height();
    }
}

/// The panel's outer rectangle for a given content height.
///
/// Shared by the background fill and the `wants_pointer` hit-test so the two can
/// never disagree about where the panel is.
fn panel_rect(content_height: f32) -> Rect {
    Rect::new(
        theme::PANEL_X,
        theme::PANEL_Y,
        theme::PANEL_W,
        content_height.max(theme::ROW_H) + theme::PAD,
    )
}
