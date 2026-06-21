//! A small, modular, **decoupled** immediate-mode UI framework.
//!
//! This is the engine's answer to "let a consumer expose controls without
//! touching the GPU". It is deliberately built as its own layer with two clean
//! seams, mirroring the engine/consumer decoupling one level down:
//!
//! - **Decoupled from the renderer (downward).** The UI never sees `wgpu`. It
//!   draws through the [`Painter`] trait — `rect` / `text` / `text_size` — and
//!   reads input through [`Input`]. Anything that can paint a rectangle and a
//!   string can host this UI; the engine's overlay is just one [`Painter`]
//!   implementation (`renderer::overlay::Overlay`).
//! - **Decoupled from the consumer (upward).** The UI knows nothing about
//!   *what* it controls. Widgets borrow the consumer's own `&mut f32` /
//!   `&mut bool`, so the parameters live in the demo (roadmap principle 3); the
//!   UI just edits them in place and reports whether anything changed.
//!
//! It is **immediate-mode**: the consumer re-declares the whole panel every
//! frame from its current state, and the only thing that persists between frames
//! is a tiny [`UiState`] (which slider is being dragged, and last frame's panel
//! height for the background). There is no retained widget tree — that keeps the
//! surface small and sidesteps the "accidentally rebuild a worse Bevy" trap
//! (roadmap principle 2).
//!
//! Typical use from a consumer's `update`:
//!
//! ```no_run
//! # use slmsttaa::Renderer;
//! # fn demo(renderer: &mut Renderer, time: &mut f32, fps: f32) {
//! let mut ui = renderer.ui();
//! ui.title("Erosion");
//! ui.label(&format!("FPS: {fps:.0}"));
//! ui.slider("time (ky)", time, 0.0, 1000.0);
//! if ui.button("regenerate") { /* ... */ }
//! let recompute = ui.changed();
//! # let _ = recompute;
//! # }
//! ```

use crate::input::{Input, MouseButton};

/// An RGBA color in `[0, 1]`, the only color type the UI speaks.
pub type Color = [f32; 4];

/// A 2D drawing surface the UI paints onto, in physical pixels with the origin
/// at the top-left (matching cursor coordinates).
///
/// This trait is the seam that keeps the UI independent of the renderer: the
/// engine's overlay implements it, but so could a test double or an entirely
/// different backend. Implementors only need to fill rectangles and stamp text.
pub trait Painter {
    /// Fill an axis-aligned rectangle at `(x, y)` (top-left) of size `w`×`h`.
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color);

    /// Draw a left-aligned, single-line text run with its top-left at `(x, y)`.
    /// `px` is the (square) cell size of each glyph in pixels.
    fn text(&mut self, x: f32, y: f32, text: &str, px: f32, color: Color);

    /// The size a text run would occupy: `[width, height]` in pixels.
    fn text_size(&self, text: &str, px: f32) -> [f32; 2];
}

/// Persistent UI state that survives between frames.
///
/// Immediate-mode UIs need almost no retained state; this is all of it. The
/// engine owns one of these inside the [`Renderer`](crate::Renderer) and hands
/// the UI a borrow each frame via [`Renderer::ui`](crate::Renderer::ui).
#[derive(Debug, Default)]
pub struct UiState {
    /// The id of the widget currently capturing the pointer (a slider being
    /// dragged), so the drag continues even if the cursor leaves the track.
    active: Option<u64>,
    /// Last frame's panel height, used to draw the background behind a panel
    /// whose height we only know after laying out its contents.
    panel_height: f32,
}

// --- Theme -----------------------------------------------------------------
// A handful of constants rather than a configurable style system (KISS): a
// single demo doesn't justify theming machinery yet.

const PANEL_X: f32 = 12.0;
const PANEL_Y: f32 = 12.0;
const PANEL_W: f32 = 340.0;
const PAD: f32 = 10.0;
const TEXT_PX: f32 = 16.0;
const TITLE_PX: f32 = 20.0;
const ROW_H: f32 = 24.0;
const TRACK_H: f32 = 8.0;
const KNOB_W: f32 = 10.0;

const SECTION_PX: f32 = 15.0;

const COL_PANEL: Color = [0.04, 0.06, 0.09, 0.78];
const COL_TEXT: Color = [0.86, 0.90, 0.95, 1.0];
const COL_MUTED: Color = [0.55, 0.60, 0.68, 1.0];
const COL_SECTION: Color = [0.45, 0.66, 0.92, 1.0];
const COL_ACCENT: Color = [0.26, 0.59, 0.98, 1.0];
const COL_ACCENT_HOT: Color = [0.42, 0.72, 1.0, 1.0];
const COL_TRACK: Color = [1.0, 1.0, 1.0, 0.14];
const COL_BTN: Color = [0.18, 0.32, 0.55, 1.0];
const COL_BTN_HOT: Color = [0.26, 0.46, 0.78, 1.0];

/// One frame of the immediate-mode UI: a single left-anchored panel.
///
/// Construct it via [`Renderer::ui`](crate::Renderer::ui) at the top of your
/// `update`, declare widgets top-to-bottom, then read [`Ui::changed`]. Dropping
/// it finalizes the panel background height for next frame.
pub struct Ui<'a> {
    painter: &'a mut dyn Painter,
    input: &'a Input,
    state: &'a mut UiState,
    /// Top-left of the panel and the running layout cursor (`y`).
    origin_y: f32,
    cursor_y: f32,
    /// Monotonic widget counter, hashed into stable per-widget ids.
    seq: u64,
    /// Whether any value-editing widget changed a bound value this frame.
    changed: bool,
}

impl<'a> Ui<'a> {
    /// Begin a UI frame. The engine calls this; consumers go through
    /// [`Renderer::ui`](crate::Renderer::ui).
    pub fn new(painter: &'a mut dyn Painter, input: &'a Input, state: &'a mut UiState) -> Self {
        // Draw the panel background first, sized from last frame's height (it is
        // laid out top-down, so this frame's height isn't known yet). Layout is
        // stable frame-to-frame, so this is correct after the first frame.
        let height = state.panel_height.max(ROW_H);
        painter.rect(PANEL_X, PANEL_Y, PANEL_W, height + PAD, COL_PANEL);

        Self {
            painter,
            input,
            state,
            origin_y: PANEL_Y,
            cursor_y: PANEL_Y + PAD,
            seq: 0,
            changed: false,
        }
    }

    /// Whether the pointer is over the panel (or actively dragging a widget),
    /// so the consumer can suppress world interactions like camera drag.
    pub fn wants_pointer(&self) -> bool {
        if self.state.active.is_some() {
            return true;
        }
        let height = self.state.panel_height.max(ROW_H) + PAD;
        match self.input.cursor_position() {
            Some((px, py)) => {
                (PANEL_X..=PANEL_X + PANEL_W).contains(&px)
                    && (PANEL_Y..=PANEL_Y + height).contains(&py)
            }
            None => false,
        }
    }

    /// Whether any slider or checkbox edited its bound value this frame — the
    /// signal a consumer uses to recompute derived state (e.g. re-run erosion).
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// A bold heading row, underlined with a short accent bar.
    pub fn title(&mut self, text: &str) {
        self.painter
            .text(PANEL_X + PAD, self.cursor_y, text, TITLE_PX, COL_TEXT);
        // A short accent rule under the title gives the panel a clear header
        // instead of a flat wall of text.
        let underline_y = self.cursor_y + TITLE_PX + 3.0;
        let tw = self.painter.text_size(text, TITLE_PX)[0];
        self.painter
            .rect(PANEL_X + PAD, underline_y, tw.max(40.0), 2.0, COL_ACCENT);
        self.cursor_y += TITLE_PX + 12.0;
    }

    /// A section sub-heading: smaller than [`Ui::title`] and accent-colored, for
    /// grouping related widgets within a panel.
    pub fn section(&mut self, text: &str) {
        self.cursor_y += 2.0;
        self.painter
            .text(PANEL_X + PAD, self.cursor_y, text, SECTION_PX, COL_SECTION);
        self.cursor_y += SECTION_PX + 6.0;
    }

    /// A plain, full-width text row.
    pub fn label(&mut self, text: &str) {
        self.painter
            .text(PANEL_X + PAD, self.cursor_y, text, TEXT_PX, COL_TEXT);
        self.cursor_y += ROW_H;
    }

    /// A muted text row (for secondary readouts / hints).
    pub fn label_muted(&mut self, text: &str) {
        self.painter
            .text(PANEL_X + PAD, self.cursor_y, text, TEXT_PX, COL_MUTED);
        self.cursor_y += ROW_H;
    }

    /// A thin horizontal divider.
    pub fn separator(&mut self) {
        self.cursor_y += 4.0;
        self.painter.rect(
            PANEL_X + PAD,
            self.cursor_y,
            PANEL_W - 2.0 * PAD,
            1.0,
            COL_TRACK,
        );
        self.cursor_y += 8.0;
    }

    /// A clickable button. Returns `true` on the frame it is clicked.
    pub fn button(&mut self, label: &str) -> bool {
        let id = self.next_id(label);
        let x = PANEL_X + PAD;
        let w = PANEL_W - 2.0 * PAD;
        let h = ROW_H - 4.0;
        let y = self.cursor_y;

        let hovered = self.point_in(x, y, w, h);
        let clicked = hovered && self.input.is_mouse_pressed(MouseButton::Left);

        let bg = if hovered { COL_BTN_HOT } else { COL_BTN };
        self.painter.rect(x, y, w, h, bg);
        // Center the label horizontally within the button.
        let tw = self.painter.text_size(label, TEXT_PX)[0];
        let tx = x + (w - tw) * 0.5;
        let ty = y + (h - TEXT_PX) * 0.5;
        self.painter.text(tx, ty, label, TEXT_PX, COL_TEXT);

        self.cursor_y += ROW_H;
        let _ = id;
        clicked
    }

    /// A labeled toggle. Edits `value` in place; returns `true` if it changed.
    pub fn checkbox(&mut self, label: &str, value: &mut bool) -> bool {
        let id = self.next_id(label);
        let x = PANEL_X + PAD;
        let y = self.cursor_y;
        let box_sz = TEXT_PX;

        let hovered = self.point_in(x, y, PANEL_W - 2.0 * PAD, ROW_H);
        let mut changed = false;
        if hovered && self.input.is_mouse_pressed(MouseButton::Left) {
            *value = !*value;
            changed = true;
            self.changed = true;
        }

        // Box, then a fill if checked.
        self.painter.rect(x, y, box_sz, box_sz, COL_TRACK);
        if *value {
            let inset = 3.0;
            self.painter.rect(
                x + inset,
                y + inset,
                box_sz - 2.0 * inset,
                box_sz - 2.0 * inset,
                if hovered { COL_ACCENT_HOT } else { COL_ACCENT },
            );
        }
        self.painter
            .text(x + box_sz + 8.0, y, label, TEXT_PX, COL_TEXT);

        self.cursor_y += ROW_H;
        let _ = id;
        changed
    }

    /// A labeled, draggable float slider over `[min, max]`. Edits `value` in
    /// place and returns `true` if it changed this frame.
    ///
    /// Renders as a `label: value` line over a track with a draggable knob.
    pub fn slider(&mut self, label: &str, value: &mut f32, min: f32, max: f32) -> bool {
        self.slider_fmt(label, value, min, max, 2)
    }

    /// [`Ui::slider`] with control over how many decimals the value shows.
    pub fn slider_fmt(
        &mut self,
        label: &str,
        value: &mut f32,
        min: f32,
        max: f32,
        decimals: usize,
    ) -> bool {
        let id = self.next_id(label);
        let x = PANEL_X + PAD;
        let w = PANEL_W - 2.0 * PAD;

        // Header line: "label: value".
        let header = format!("{label}: {value:.decimals$}");
        self.painter
            .text(x, self.cursor_y, &header, TEXT_PX, COL_TEXT);
        let track_y = self.cursor_y + TEXT_PX + 5.0;

        // Hit band is taller than the visible track so it's easy to grab.
        let band_y = track_y - 6.0;
        let band_h = TRACK_H + 12.0;
        let hovered = self.point_in(x, band_y, w, band_h);
        let held = self.input.is_mouse_held(MouseButton::Left);

        // Capture / release the drag.
        if hovered && self.input.is_mouse_pressed(MouseButton::Left) {
            self.state.active = Some(id);
        }
        if self.state.active == Some(id) && !held {
            self.state.active = None;
        }

        let span = (max - min).max(f32::EPSILON);
        let mut changed = false;
        if self.state.active == Some(id) {
            if let Some((px, _)) = self.input.cursor_position() {
                let t = ((px - x) / w).clamp(0.0, 1.0);
                let new_val = min + t * span;
                if (new_val - *value).abs() > f32::EPSILON {
                    *value = new_val;
                    changed = true;
                    self.changed = true;
                }
            }
        }

        // Track, filled portion, and knob.
        let t = ((*value - min) / span).clamp(0.0, 1.0);
        self.painter.rect(x, track_y, w, TRACK_H, COL_TRACK);
        self.painter.rect(x, track_y, w * t, TRACK_H, COL_ACCENT);
        let knob_x = (x + w * t - KNOB_W * 0.5).clamp(x, x + w - KNOB_W);
        let knob_col = if self.state.active == Some(id) || hovered {
            COL_ACCENT_HOT
        } else {
            COL_TEXT
        };
        self.painter
            .rect(knob_x, track_y - 4.0, KNOB_W, TRACK_H + 8.0, knob_col);

        self.cursor_y += ROW_H + TEXT_PX;
        changed
    }

    /// Hash a widget label + sequence index into a stable id.
    fn next_id(&mut self, label: &str) -> u64 {
        // FNV-1a over the label, mixed with the call index so duplicate labels
        // still get distinct ids.
        let mut h: u64 = 0xcbf29ce484222325 ^ self.seq.wrapping_mul(0x100000001b3);
        for b in label.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        self.seq += 1;
        h
    }

    /// Whether the pointer is inside the given rectangle this frame.
    fn point_in(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        match self.input.cursor_position() {
            Some((px, py)) => px >= x && px <= x + w && py >= y && py <= y + h,
            None => false,
        }
    }
}

impl Drop for Ui<'_> {
    fn drop(&mut self) {
        // Record the laid-out height so next frame's background fits.
        self.state.panel_height = self.cursor_y - self.origin_y;
    }
}
