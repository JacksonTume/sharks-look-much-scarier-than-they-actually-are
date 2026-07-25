//! # slmsttaa-ui — a small, decoupled immediate-mode UI toolkit
//!
//! The controls half of [SLMSTTAA](https://github.com/JacksonTume/sharks-look-much-scarier-than-they-actually-are).
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
//!   trait — `rect` / `text` / `text_size` / `set_layer` — and reads a
//!   [`UiInput`] snapshot the host fills in. The engine's overlay is one
//!   `Painter`; [`RecordingPainter`] is another.
//! - **Upward, from the consumer.** Widgets borrow the consumer's own
//!   `&mut f32` / `&mut bool`, so the UI has no idea *what* it controls. Erosion
//!   parameters live in the terrain demo, which is where they belong.
//!
//! ## Immediate mode
//!
//! The consumer re-declares the whole panel every frame from its current state.
//! The only thing that survives between frames is a small [`UiState`] — the
//! hot/active/focused ids, which sections are collapsed, and the panel's height.
//! There is no retained widget tree; that keeps the surface small and sidesteps
//! the "accidentally rebuild a worse Bevy" trap.
//!
//! ## Writing your own widget
//!
//! Everything the built-in widgets use is public: [`Ui::allocate`] claims space,
//! [`Ui::interact`] hit-tests it and returns a [`Response`], [`Ui::painter`]
//! draws it, and [`theme`] holds the metrics and colors that make it match. A
//! widget this crate never shipped is therefore not second-class — if one ever
//! needs private access, the seam is wrong and the seam gets fixed.
//!
//! ```
//! use slmsttaa_ui::{theme, Response, Ui};
//!
//! /// A read-only bar. Nothing here is privileged.
//! fn meter(ui: &mut Ui, label: &str, t: f32) -> Response {
//!     let id = ui.next_id(label);
//!     let row = ui.allocate([0.0, theme::ROW_H]);
//!     let response = ui.interact(row, id);
//!
//!     let fill = if response.hovered { theme::COL_ACCENT_HOT } else { theme::COL_ACCENT };
//!     ui.painter().rect(row.x, row.y, row.w, row.h, theme::COL_TRACK);
//!     ui.painter().rect(row.x, row.y, row.w * t.clamp(0.0, 1.0), row.h, fill);
//!     response
//! }
//! ```
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
//! if ui.section("Fluvial").open {
//!     ui.slider_fmt("erodibility", &mut erodibility, 0.0, 0.006, 4);
//! }
//! if ui.button("new seed").clicked { /* reseed */ }
//! let recompute = ui.changed();
//! # let _ = recompute;
//! ```

#![deny(missing_docs)]

mod interact;
mod layout;
mod painter;
pub mod theme;
mod widgets;

pub use interact::{Response, UiInput, UiState};
pub use layout::Rect;
pub use painter::{Color, DrawCmd, Layer, Painter, RecordingPainter};

use layout::Layout;

/// One scope on the id stack: what it is, and which ids it has handed out.
///
/// Ids are `hash(scope, label)` — no position, so a widget survives rows
/// appearing above it. `used` exists only to catch the resulting collision when
/// one scope declares the same label twice.
struct Scope {
    id: u64,
    used: Vec<u64>,
}

/// One frame of the immediate-mode UI: a single left-anchored panel.
///
/// Construct it at the top of your update (via `Renderer::ui` when you are using
/// the engine), declare widgets top-to-bottom, then read [`Ui::changed`].
/// Dropping it paints the panel background behind everything that was declared.
pub struct Ui<'a> {
    painter: &'a mut dyn Painter,
    input: UiInput,
    state: &'a mut UiState,
    layout: Layout,
    /// The id scope stack; never empty (the root scope is pushed on construction).
    scopes: Vec<Scope>,
    /// Whether any value-editing widget changed a bound value this frame.
    changed: bool,
}

impl<'a> Ui<'a> {
    /// Begin a UI frame against `painter`, this frame's `input`, and the host's
    /// persistent `state`.
    ///
    /// Nothing is drawn yet — not even the panel background, which is painted
    /// into [`Layer::Base`] when this is dropped and its final height is known.
    /// That is what retired the old "size the background from *last* frame's
    /// height" hack: with ordered layers, declaration order and paint order stop
    /// being the same thing.
    pub fn new(painter: &'a mut dyn Painter, input: UiInput, state: &'a mut UiState) -> Self {
        // `hot` is recomputed from scratch every frame; `active` and `focused`
        // deliberately persist.
        state.hot = None;
        painter.set_layer(Layer::Panel);

        Self {
            painter,
            input,
            state,
            layout: Layout::new(theme::PANEL_Y, theme::PAD),
            scopes: vec![Scope {
                id: 0,
                used: Vec::new(),
            }],
            changed: false,
        }
    }

    /// Whether the pointer is over the panel (or actively dragging a widget), so
    /// the consumer can suppress world interactions like a camera drag.
    ///
    /// Call it after declaring your widgets: it measures the panel laid out so
    /// far, falling back to last frame's height so an early call is never
    /// *smaller* than the panel really is.
    pub fn wants_pointer(&self) -> bool {
        if self.state.active.is_some() {
            return true;
        }
        let height = self.layout.height().max(self.state.panel_height);
        self.input.hits(panel_rect(height))
    }

    /// Whether any value-editing widget changed a bound value this frame — the
    /// signal a consumer uses to recompute derived state (e.g. re-run erosion).
    pub fn changed(&self) -> bool {
        self.changed
    }

    // --- The unprivileged seam ---------------------------------------------
    //
    // `allocate` / `interact` / `painter` / `next_id` are public *together*,
    // because a widget needs all four. Anything this crate's own widgets can do,
    // a consumer's can too (UI roadmap Slice 1).

    /// Claim the next `[width, height]` of panel space and return its rectangle.
    ///
    /// A non-positive width means "the full content width", which is what every
    /// built-in widget passes — real horizontal layout is UI Slice 3. The layout
    /// cursor advances by `height` whether or not you draw anything there, so
    /// include a widget's trailing gap in what you ask for.
    pub fn allocate(&mut self, [width, height]: [f32; 2]) -> Rect {
        let w = if width > 0.0 { width } else { theme::CONTENT_W };
        let rect = Rect::new(theme::CONTENT_X, self.layout.y(), w, height);
        self.layout.advance(height);
        rect
    }

    /// Hit-test `rect` for the widget `id` and update the interaction state.
    ///
    /// This is where hot / active / focused are maintained:
    ///
    /// - **hot** is set while the pointer is inside `rect`.
    /// - **active** is claimed on the press edge and released when the button
    ///   comes up — *wherever the pointer has moved to by then*. That is what
    ///   lets a slider keep tracking a cursor dragged off its track.
    /// - **focused** follows clicks, so a click elsewhere takes focus away.
    ///
    /// The returned [`Response`] has `changed: false`; a widget that edits a
    /// value sets that itself (and should also call [`Ui::mark_changed`]).
    pub fn interact(&mut self, rect: Rect, id: u64) -> Response {
        let hovered = self.input.hits(rect);
        if hovered {
            self.state.hot = Some(id);
        }

        let clicked = hovered && self.input.primary_pressed;
        if clicked {
            self.state.active = Some(id);
            self.state.focused = Some(id);
        } else if self.input.primary_pressed {
            // A press that landed somewhere else takes focus away from us.
            if self.state.focused == Some(id) {
                self.state.focused = None;
            }
        }
        if self.state.active == Some(id) && !self.input.primary_held {
            self.state.active = None;
        }

        Response {
            id,
            rect,
            hovered,
            held: self.state.active == Some(id),
            clicked,
            changed: false,
            open: true,
        }
    }

    /// The painter this frame draws through, for widgets that need to draw
    /// something the built-ins don't.
    pub fn painter(&mut self) -> &mut dyn Painter {
        self.painter
    }

    /// The id for `label` in the enclosing scope.
    ///
    /// The id depends on the label and the scope, **not** on where the widget
    /// sits in the panel — so a row appearing above it (a status line, an
    /// expanding section) doesn't change its identity, and an in-progress drag
    /// survives. That is not a nicety: order-keyed ids broke slider dragging
    /// outright, because the terrain panel grows a status row the instant a
    /// slider moves.
    ///
    /// Call it **once** per widget. Declaring the same label twice in one scope
    /// would otherwise collide, so the duplicate is re-hashed into a distinct id
    /// — which keeps them independent, but means the *second* one's identity
    /// depends on the first still being there. Wrap repeated widgets in
    /// [`Ui::push_id`] when their state has to be durable.
    pub fn next_id(&mut self, label: &str) -> u64 {
        let scope = self.scopes.last_mut().expect("root scope is never popped");
        let mut id = interact::hash_id(scope.id, label);
        // Deterministic re-hash chain, so the Nth duplicate is always the same
        // id for the same N.
        while scope.used.contains(&id) {
            id = interact::hash_id(id, "\u{1}dup");
        }
        scope.used.push(id);
        id
    }

    /// Open a nested id scope, so ids inside it are stable against edits
    /// outside it. Pair with [`Ui::pop_id`].
    ///
    /// [`Ui::section`] does this for you; reach for it directly when generating
    /// widgets in a loop, where the loop index is the only thing distinguishing
    /// one iteration's widgets from the next's.
    pub fn push_id(&mut self, label: &str) {
        let id = self.next_id(label);
        self.scopes.push(Scope {
            id,
            used: Vec::new(),
        });
    }

    /// Close the scope opened by [`Ui::push_id`].
    pub fn pop_id(&mut self) {
        // The root scope stays: ids must keep working even if a consumer's
        // push/pop pairing is off.
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Report that a widget edited its bound value, so [`Ui::changed`] sees it.
    pub fn mark_changed(&mut self) {
        self.changed = true;
    }

    /// Add `points` of vertical space.
    pub fn spacing(&mut self, points: f32) {
        self.layout.advance(points);
    }

    /// Whether the pointer is over `rect` this frame.
    ///
    /// Prefer [`Ui::interact`] — this is for a widget that wants to test a
    /// sub-rectangle (a slider's grab band, say) without claiming interaction
    /// state for it.
    pub fn hovered(&self, rect: Rect) -> bool {
        self.input.hits(rect)
    }

    /// This frame's pointer state, for a widget that needs the raw cursor —
    /// a slider mapping the cursor's x onto its track, for instance.
    pub fn input(&self) -> UiInput {
        self.input
    }

    /// Whether this widget is currently capturing the pointer.
    pub fn is_active(&self, id: u64) -> bool {
        self.state.active == Some(id)
    }
}

impl Drop for Ui<'_> {
    fn drop(&mut self) {
        let height = self.layout.height();
        // Paint the background *now*, at the height the contents actually came
        // out to, but into the layer that flushes first — so it lands behind
        // everything declared above it. This is the whole reason layers exist.
        self.painter.set_layer(Layer::Base);
        let bg = panel_rect(height);
        self.painter.rect(bg.x, bg.y, bg.w, bg.h, theme::COL_PANEL);
        self.painter.set_layer(Layer::Panel);

        // Still recorded, but only so `wants_pointer` has an answer before this
        // frame's widgets have been declared.
        self.state.panel_height = height;
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
