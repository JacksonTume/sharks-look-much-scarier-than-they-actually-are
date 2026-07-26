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
//! The consumer re-declares every panel each frame from its current state. The
//! only thing that survives between frames is a small [`UiState`] — the
//! hot/active/focused ids, which sections are collapsed, how far each scroll
//! area has scrolled, and where each panel ended up. There is no retained widget
//! tree; that keeps the surface small and sidesteps the "accidentally rebuild a
//! worse Bevy" trap.
//!
//! ## Writing your own widget
//!
//! Everything the built-in widgets use is public: [`Ui::allocate`] claims space,
//! [`Ui::interact`] hit-tests it and returns a [`Response`], [`Ui::painter`]
//! draws it, and [`Ui::theme`] hands over the same semantic tokens they style
//! themselves from. A widget this crate never shipped is therefore not
//! second-class — if one ever needs private access, the seam is wrong and the
//! seam gets fixed.
//!
//! ```
//! use slmsttaa_ui::{Rect, Response, Ui};
//!
//! /// A read-only bar. Nothing here is privileged, and nothing here names a
//! /// literal color — so it restyles with everything else.
//! fn meter(ui: &mut Ui, label: &str, t: f32) -> Response {
//!     let theme = *ui.theme();
//!     let id = ui.next_id(label);
//!     let row = ui.allocate([0.0, theme.control.row_h]);
//!     let response = ui.interact(row, id);
//!
//!     let fill = if response.hovered { theme.color.accent_hover } else { theme.color.accent };
//!     let filled = Rect::new(row.x, row.y, row.w * t.clamp(0.0, 1.0), row.h);
//!     ui.painter().fill_rect(row, theme.radius.md, theme.color.surface);
//!     ui.painter().fill_rect(filled, theme.radius.md, fill);
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
//! use slmsttaa_ui::{Anchor, RecordingPainter, Theme, Ui, UiInput, UiState};
//!
//! // Owned by the host, and outlive the frame.
//! let mut painter = RecordingPainter::default();
//! let mut state = UiState::default();
//! let mut erodibility = 0.003_f32;
//! let theme = Theme::dark();
//!
//! let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
//! ui.set_theme(theme);
//! ui.panel(Anchor::TopLeft, theme.panel_w, |ui| {
//!     ui.title("Erosion");
//!     ui.label_value("fps", "60");
//!     if ui.section("Fluvial").open {
//!         ui.slider("erodibility", &mut erodibility, 0.0, 0.006).decimals(4).show();
//!     }
//!     if ui.button("new seed").show().clicked { /* reseed */ }
//! });
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
pub use theme::{Size, Theme, Variant};
pub use widgets::{Button, Slider, SliderLayout};

use layout::{Dir, Region};

/// Which corner of the window a panel is pinned to.
///
/// Corners rather than edges: a panel is sized by its caller and grows downward
/// to fit its contents, so "centered on the left edge" would need a height
/// nothing knows yet. Four corners is what the demo needed and what the layout
/// can answer honestly.
///
/// **Top and bottom are not symmetric.** A top-anchored panel knows where its
/// first row goes before it lays anything out. A bottom-anchored one does not —
/// it has to place its contents before its height exists — so it positions from
/// *last* frame's height and settles on the second frame. That is the same
/// one-frame lag the panel background used to have, confined now to the case
/// that genuinely can't avoid it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Anchor {
    /// Pinned to the top-left corner.
    TopLeft,
    /// Pinned to the top-right corner.
    TopRight,
    /// Pinned to the bottom-left corner. Positions from last frame's height.
    BottomLeft,
    /// Pinned to the bottom-right corner. Positions from last frame's height.
    BottomRight,
}

impl Anchor {
    /// The label this anchor's panel is identified by. One panel per corner is
    /// all anything has wanted, so the corner *is* the identity.
    fn key(self) -> &'static str {
        match self {
            Anchor::TopLeft => "panel:top-left",
            Anchor::TopRight => "panel:top-right",
            Anchor::BottomLeft => "panel:bottom-left",
            Anchor::BottomRight => "panel:bottom-right",
        }
    }

    /// Whether this anchor has to borrow last frame's height to place itself.
    fn is_bottom(self) -> bool {
        matches!(self, Anchor::BottomLeft | Anchor::BottomRight)
    }

    /// Whether this anchor measures from the right edge of the viewport.
    fn is_right(self) -> bool {
        matches!(self, Anchor::TopRight | Anchor::BottomRight)
    }
}

/// One scope on the id stack: what it is, and which ids it has handed out.
///
/// Ids are `hash(scope, label)` — no position, so a widget survives rows
/// appearing above it. `used` exists only to catch the resulting collision when
/// one scope declares the same label twice.
struct Scope {
    id: u64,
    used: Vec<u64>,
}

/// One frame of the immediate-mode UI.
///
/// Construct it at the top of your update (via `Renderer::ui` when you are using
/// the engine), declare one or more [`panel`](Ui::panel)s, then read
/// [`Ui::changed`]. There is exactly one `Ui` per frame however many panels it
/// holds, which is what keeps hover and focus coherent across them and lets
/// [`Ui::wants_pointer`] answer for all of them at once.
pub struct Ui<'a> {
    painter: &'a mut dyn Painter,
    input: UiInput,
    state: &'a mut UiState,
    /// Every token the widgets style themselves from. Copied by value into each
    /// widget that needs it, which is why it is [`Copy`].
    theme: Theme,
    /// The layout stack; never empty (the root region is pushed on construction).
    regions: Vec<Region>,
    /// The id scope stack; never empty (the root scope is pushed on construction).
    scopes: Vec<Scope>,
    /// A one-shot size override set by [`Ui::sized`], consumed by the next
    /// [`Ui::allocate`].
    next_size: Option<[f32; 2]>,
    /// Where each panel closed this frame landed, for [`Ui::wants_pointer`].
    panels: Vec<Rect>,
    /// Whether any value-editing widget changed a bound value this frame.
    changed: bool,
}

impl<'a> Ui<'a> {
    /// Begin a UI frame against `painter`, this frame's `input`, and the host's
    /// persistent `state`.
    ///
    /// Nothing is drawn yet — not even a panel background, which is painted into
    /// [`Layer::Base`] when the panel closes and its final height is known. That
    /// is what retired the old "size the background from *last* frame's height"
    /// hack: with ordered layers, declaration order and paint order stop being
    /// the same thing.
    ///
    /// The frame starts with a root region covering the whole viewport, so
    /// declaring a widget outside any [`panel`](Ui::panel) is not an error — it
    /// simply lands bare in the top-left corner with no background behind it.
    /// That is almost always a mistake, and never a crash.
    ///
    /// The frame starts on [`Theme::dark`]; a consumer with its own theme calls
    /// [`Ui::set_theme`] next.
    pub fn new(painter: &'a mut dyn Painter, input: UiInput, state: &'a mut UiState) -> Self {
        // `hot` is recomputed from scratch every frame; `active` and `focused`
        // deliberately persist.
        state.hot = None;
        painter.set_layer(Layer::Panel);

        let (vw, vh) = input.viewport;
        Self {
            painter,
            input,
            state,
            theme: Theme::default(),
            regions: vec![Region::vertical(Rect::new(0.0, 0.0, vw, vh))],
            scopes: vec![Scope {
                id: 0,
                used: Vec::new(),
            }],
            next_size: None,
            panels: Vec::new(),
            changed: false,
        }
    }

    /// Whether the pointer is over any panel (or actively dragging a widget), so
    /// the consumer can suppress world interactions like a camera drag.
    ///
    /// Call it **after** your panels have closed. Each one contributes its exact
    /// rectangle as it closes, so unlike the old single-panel version this needs
    /// no fallback to last frame — but a panel that hasn't been declared yet
    /// naturally can't be hit.
    pub fn wants_pointer(&self) -> bool {
        if self.state.active.is_some() {
            return true;
        }
        self.panels.iter().any(|rect| self.input.hits(*rect))
    }

    /// Whether any value-editing widget changed a bound value this frame — the
    /// signal a consumer uses to recompute derived state (e.g. re-run erosion).
    pub fn changed(&self) -> bool {
        self.changed
    }

    // --- The unprivileged seam ---------------------------------------------
    //
    // `allocate` / `interact` / `painter` / `next_id` / `theme` are public
    // *together*, because a widget needs all five. Anything this crate's own
    // widgets can do, a consumer's can too (UI roadmap Slice 1).

    /// The tokens this frame is styled from.
    ///
    /// Read it the way the built-in widgets do — `let theme = *ui.theme();` up
    /// front, so the copy outlives the mutable borrow [`Ui::painter`] takes.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Restyle the rest of this frame.
    ///
    /// Call it once at the top, before any panel. It applies to every widget
    /// declared afterward, so a mid-frame swap is legal (and is how a consumer
    /// would theme one panel differently) — it simply isn't retroactive.
    ///
    /// Immediate mode all the way down: the theme is not remembered between
    /// frames, because the consumer already owns the value and re-declaring the
    /// UI each frame is the premise of the whole design. Nothing style-shaped
    /// accumulates in [`UiState`].
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Claim the next `[width, height]` of space and return its rectangle.
    ///
    /// A non-positive width means **"whatever is left"** — the full width in a
    /// top-to-bottom region, and the space between the cursor and the far edge
    /// inside a [`horizontal`](Ui::horizontal) row. Every built-in widget passes
    /// `0.0`, which is why the same widget fills a panel on its own line and
    /// shares a row when it is on one.
    ///
    /// The cursor advances by `height` whether or not you draw anything there,
    /// so include a widget's trailing gap in what you ask for. A caller that
    /// wants a specific size regardless sets it with [`Ui::sized`] first.
    pub fn allocate(&mut self, [width, height]: [f32; 2]) -> Rect {
        let [width, height] = self.next_size.take().unwrap_or([width, height]);
        let gap = self.theme.space.gap;
        self.region_mut().place(width, height, gap)
    }

    /// Force the size of the *next* widget, overriding what it asks for.
    ///
    /// One-shot: it applies to the next [`allocate`](Ui::allocate) and then
    /// clears itself. This is how a row gets uneven cells —
    /// `ui.sized([80.0, 24.0]).button("new").show()` — without every widget
    /// growing a width argument it would ignore nine times out of ten.
    pub fn sized(&mut self, size: [f32; 2]) -> &mut Self {
        self.next_size = Some(size);
        self
    }

    /// The region widgets are currently being placed into.
    fn region(&self) -> &Region {
        self.regions.last().expect("root region is never popped")
    }

    /// The region widgets are currently being placed into, mutably.
    fn region_mut(&mut self) -> &mut Region {
        self.regions
            .last_mut()
            .expect("root region is never popped")
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
            focused: self.state.focused == Some(id),
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

    /// Add `points` of empty space along the current direction of flow —
    /// vertical in a panel, horizontal inside a row.
    pub fn spacing(&mut self, points: f32) {
        self.region_mut().advance_main(points);
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

    // --- Containers ---------------------------------------------------------
    //
    // All five take a closure rather than a `begin`/`end` pair, for the reason
    // `scroll_area` does: an unbalanced pair would desync the region stack (and
    // the clip stack with it), and a closure makes that unrepresentable.
    //
    // None of them push an id scope. Ids must never depend on position — see
    // `interact::hash_id` for the drag-killing bug that established that — and a
    // container that silently scoped by cell index would put position straight
    // back in. `columns` hands the caller its index so it can `push_id` when the
    // labels genuinely repeat.

    /// Declare a panel pinned to `anchor`, `width` points wide.
    ///
    /// The panel's background and hairline border are painted into
    /// [`Layer::Base`] when the closure returns and its height is finally known,
    /// so they land *behind* everything declared inside it.
    ///
    /// ```
    /// # use slmsttaa_ui::{Anchor, RecordingPainter, Theme, Ui, UiInput, UiState};
    /// # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
    /// # let input = UiInput { viewport: (1280.0, 720.0), ..UiInput::default() };
    /// # let mut ui = Ui::new(&mut p, input, &mut s);
    /// # let mut wireframe = false;
    /// ui.panel(Anchor::TopLeft, Theme::default().panel_w, |ui| {
    ///     ui.title("Terrain");
    /// });
    /// ui.panel(Anchor::TopRight, 170.0, |ui| {
    ///     ui.label_value("fps", "60");
    ///     ui.checkbox("wireframe", &mut wireframe);
    /// });
    /// ```
    pub fn panel<R>(
        &mut self,
        anchor: Anchor,
        width: f32,
        add_contents: impl FnOnce(&mut Ui<'a>) -> R,
    ) -> R {
        let theme = self.theme;
        let (margin, pad) = (theme.space.margin, theme.space.pad);

        let id = self.next_id(anchor.key());
        self.scopes.push(Scope {
            id,
            used: Vec::new(),
        });

        // Narrower than its own padding would be a panel with negative content
        // width, which `place` would clamp to zero anyway — say so up front.
        let width = width.max(2.0 * pad);
        let (vw, vh) = self.input.viewport;

        let x = if anchor.is_right() {
            vw - margin - width
        } else {
            margin
        };
        let y = if anchor.is_bottom() {
            // The one place a frame of lag is unavoidable: the contents have to
            // be placed before they have been measured.
            let previous = self
                .state
                .panel_rect(id)
                .map_or(theme.control.row_h + 2.0 * pad, |r| r.h);
            vh - margin - previous
        } else {
            margin
        };

        let content = Rect::new(x + pad, y + pad, width - 2.0 * pad, (vh - y - pad).max(0.0));
        self.regions.push(Region::vertical(content));
        let result = add_contents(self);
        let region = self.regions.pop().expect("panel pushed a region");

        let bg = Rect::new(
            x,
            y,
            width,
            pad + region.consumed_height().max(theme.control.row_h) + pad,
        );
        self.painter.set_layer(Layer::Base);
        self.painter
            .fill_rect(bg, theme.radius.lg, theme.color.background);
        // A hairline border does the job a drop shadow would, for a fraction of
        // the shader: over a bright patch of terrain the panel still has an edge.
        self.painter.stroke_rect(
            bg,
            theme.radius.lg,
            theme.control.border,
            theme.color.border,
        );
        self.painter.set_layer(Layer::Panel);

        self.state.set_panel_rect(id, bg);
        self.panels.push(bg);
        self.pop_id();
        result
    }

    /// Declare widgets side by side, packed left to right.
    ///
    /// Inside the closure `allocate([0.0, h])` means "the rest of the line"
    /// rather than "the full width", so give all but the last widget an explicit
    /// size with [`Ui::sized`]. The row consumes as much vertical space as its
    /// tallest member.
    ///
    /// ```
    /// # use slmsttaa_ui::{Anchor, RecordingPainter, Theme, Ui, UiInput, UiState};
    /// # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
    /// # let mut ui = Ui::new(&mut p, UiInput::default(), &mut s);
    /// # ui.panel(Anchor::TopLeft, Theme::default().panel_w, |ui| {
    /// ui.horizontal(|ui| {
    ///     ui.sized([120.0, 24.0]).label("seed");
    ///     ui.button("new").show();
    /// });
    /// # });
    /// ```
    pub fn horizontal<R>(&mut self, add_contents: impl FnOnce(&mut Ui<'a>) -> R) -> R {
        self.line(Dir::LeftToRight, add_contents)
    }

    /// Declare widgets packed against the **right** edge, first one outermost.
    ///
    /// This is right-alignment. Used on its own it right-aligns within the
    /// panel; used inside a [`horizontal`](Ui::horizontal) it fills from the
    /// right while the row fills from the left, which is the label-left,
    /// value-right shape [`Ui::label_value`] is built on.
    pub fn right<R>(&mut self, add_contents: impl FnOnce(&mut Ui<'a>) -> R) -> R {
        self.line(Dir::RightToLeft, add_contents)
    }

    /// Shared body of [`Ui::horizontal`] and [`Ui::right`].
    fn line<R>(&mut self, dir: Dir, add_contents: impl FnOnce(&mut Ui<'a>) -> R) -> R {
        let avail = self.region().next_line();
        self.regions.push(Region::row(avail, dir));
        let result = add_contents(self);
        let row = self.regions.pop().expect("line pushed a region");
        self.region_mut().advance_block(row.consumed_height());
        result
    }

    /// Split the available width into `count` equal columns and run `cell` once
    /// per column, laying out top-to-bottom inside each.
    ///
    /// This is the button-row primitive. The rows below it start beneath the
    /// *tallest* column, so columns of different lengths don't overlap anything.
    ///
    /// ```
    /// # use slmsttaa_ui::{Anchor, RecordingPainter, Theme, Ui, UiInput, UiState};
    /// # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
    /// # let mut ui = Ui::new(&mut p, UiInput::default(), &mut s);
    /// # ui.panel(Anchor::TopLeft, Theme::default().panel_w, |ui| {
    /// const PRESETS: [&str; 3] = ["alps", "dunes", "mesa"];
    /// ui.columns(3, |ui, i| {
    ///     if ui.button(PRESETS[i]).show().clicked { /* load it */ }
    /// });
    /// # });
    /// ```
    pub fn columns(&mut self, count: usize, mut cell: impl FnMut(&mut Ui<'a>, usize)) {
        if count == 0 {
            return;
        }
        let gap = self.theme.space.gap;
        let line = self.region().next_line();
        let gaps = gap * (count - 1) as f32;
        let w = ((line.w - gaps) / count as f32).max(0.0);

        let mut tallest: f32 = 0.0;
        for i in 0..count {
            let x = line.x + (w + gap) * i as f32;
            self.regions
                .push(Region::vertical(Rect::new(x, line.y, w, line.h)));
            cell(self, i);
            let column = self.regions.pop().expect("columns pushed a region");
            tallest = tallest.max(column.consumed_height());
        }
        self.region_mut().advance_block(tallest);
    }

    /// Declare widgets stepped in from the left by [`Space::indent`], for rows
    /// that belong to the toggle above them.
    ///
    /// [`Space::indent`]: theme::Space::indent
    pub fn indent<R>(&mut self, add_contents: impl FnOnce(&mut Ui<'a>) -> R) -> R {
        let step = self.theme.space.indent;
        let line = self.region().next_line();
        let inner = Rect::new(line.x + step, line.y, (line.w - step).max(0.0), line.h);
        self.regions.push(Region::vertical(inner));
        let result = add_contents(self);
        let region = self.regions.pop().expect("indent pushed a region");
        self.region_mut().advance_block(region.consumed_height());
        result
    }

    /// Declare widgets inside a scrollable, clipped region at most `max_height`
    /// points tall.
    ///
    /// Contents are laid out in full and the region is clipped to its viewport,
    /// so overflow is *hidden* rather than painted over the rest of the screen —
    /// which is why this could not exist before the painter could clip. The
    /// wheel scrolls it while the pointer is inside, and a slim indicator appears
    /// on the right only when there is something to scroll to.
    ///
    /// Takes a closure rather than `begin`/`end` calls deliberately: an
    /// unbalanced pair would desync the clip stack, and this makes that
    /// unrepresentable.
    ///
    /// ```
    /// # use slmsttaa_ui::{Anchor, RecordingPainter, Theme, Ui, UiInput, UiState};
    /// # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
    /// # let mut ui = Ui::new(&mut p, UiInput::default(), &mut s);
    /// # let mut value = 0.0_f32;
    /// # ui.panel(Anchor::TopLeft, Theme::default().panel_w, |ui| {
    /// ui.scroll_area("params", 300.0, |ui| {
    ///     for i in 0..40 {
    ///         ui.slider(&format!("knob {i}"), &mut value, 0.0, 1.0).show();
    ///     }
    /// });
    /// # });
    /// ```
    pub fn scroll_area<R>(
        &mut self,
        label: &str,
        max_height: f32,
        add_contents: impl FnOnce(&mut Ui<'a>) -> R,
    ) -> R {
        let id = self.next_id(label);
        let line = self.region().next_line();
        let top = line.y;

        // Last frame's content height decides this frame's viewport and whether
        // a scrollbar is needed. A scroll area that has never been laid out
        // simply takes its full height on the first frame and settles on the
        // second — invisible in practice, because nothing has scrolled yet.
        let previous_content = self.state.scroll_offset(content_key(id)).max(0.0);
        let viewport_h = if previous_content > 0.0 {
            previous_content.min(max_height)
        } else {
            max_height
        };
        let viewport = Rect::new(line.x, top, line.w, viewport_h);

        // Wheel input, applied before laying out so the contents land in their
        // scrolled position this frame rather than next.
        let max_offset = (previous_content - max_height).max(0.0);
        let mut offset = self.state.scroll_offset(id).clamp(0.0, max_offset);
        if max_offset > 0.0 && self.input.hits(viewport) {
            let speed = self.theme.control.scroll_speed;
            offset = (offset - self.input.scroll_delta * speed).clamp(0.0, max_offset);
        }
        self.state.set_scroll_offset(id, offset);

        // Lay the contents out in their own region, shifted up by the offset and
        // clipped to the viewport. Because it is a child region, the enclosing
        // panel's cursor never moves — so the panel's height is measured from
        // the viewport, not from however far the contents actually ran.
        self.painter.push_clip(viewport);
        self.regions.push(Region::vertical(Rect::new(
            line.x,
            top - offset,
            line.w,
            line.h,
        )));
        let result = add_contents(self);
        let content_h = self
            .regions
            .pop()
            .expect("scroll area pushed a region")
            .consumed_height();
        self.painter.pop_clip();

        // Remember what the contents measured, and leave the cursor just below
        // the viewport so whatever follows isn't overlapped.
        self.state.set_scroll_offset(content_key(id), content_h);
        self.region_mut().advance_block(viewport_h.min(content_h));

        if content_h > max_height {
            self.draw_scrollbar(viewport, offset, content_h);
        }
        result
    }

    /// The slim overflow indicator drawn inside a scroll area's right edge.
    fn draw_scrollbar(&mut self, viewport: Rect, offset: f32, content_h: f32) {
        let theme = self.theme;
        let bar_w = theme.control.scrollbar_w;

        let visible = (viewport.h / content_h).clamp(0.0, 1.0);
        let travel = (content_h - viewport.h).max(f32::EPSILON);
        let thumb_h = (viewport.h * visible).max(theme.control.row_h);
        let thumb_y = viewport.y + (viewport.h - thumb_h) * (offset / travel).clamp(0.0, 1.0);
        let x = viewport.max_x() - bar_w;

        let radius = bar_w * 0.5;
        self.painter.fill_rect(
            Rect::new(x, viewport.y, bar_w, viewport.h),
            radius,
            theme.color.surface,
        );
        self.painter.fill_rect(
            Rect::new(x, thumb_y, bar_w, thumb_h),
            radius,
            theme.color.muted,
        );
    }
}

/// The key a scroll area's *content height* is remembered under.
///
/// It shares the scroll map with the offset rather than adding a second one; the
/// offset by itself is meaningless without knowing how far there is to scroll.
fn content_key(id: u64) -> u64 {
    id ^ 0x9E37_79B9_7F4A_7C15
}
