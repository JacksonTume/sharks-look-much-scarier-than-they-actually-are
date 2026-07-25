//! The **downward seam**: the drawing surface the toolkit paints onto.
//!
//! The UI never sees a GPU. It emits rectangles and text runs through
//! [`Painter`], and something else decides what that means — the engine's
//! `renderer::overlay::Overlay` turns them into vertices, while
//! [`RecordingPainter`] just remembers them so tests can assert on layout.
//!
//! Widening this trait is how the toolkit gains new visual capabilities (rounded
//! corners, clipping, borders). That is deliberate: because a `Painter` impl
//! lives in the *other* crate, every new capability has to arrive as a
//! considered widening of this trait rather than a reach into renderer
//! internals.

/// An RGBA color in `[0, 1]`, the only color type the UI speaks.
pub type Color = [f32; 4];

/// A 2D drawing surface the UI paints onto, in physical pixels with the origin
/// at the top-left (matching cursor coordinates).
///
/// Implementors only need to fill rectangles and stamp text; anything that can
/// do those two things can host this UI.
pub trait Painter {
    /// Fill an axis-aligned rectangle at `(x, y)` (top-left) of size `w`×`h`.
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color);

    /// Draw a left-aligned, single-line text run with its top-left at `(x, y)`.
    /// `px` is the (square) cell size of each glyph in pixels.
    fn text(&mut self, x: f32, y: f32, text: &str, px: f32, color: Color);

    /// The size a text run would occupy: `[width, height]` in pixels.
    fn text_size(&self, text: &str, px: f32) -> [f32; 2];
}

/// A single primitive recorded by [`RecordingPainter`].
#[derive(Debug, Clone, PartialEq)]
pub enum DrawCmd {
    /// A filled rectangle: `(x, y, w, h)` plus its color.
    Rect {
        /// Top-left x, in physical pixels.
        x: f32,
        /// Top-left y, in physical pixels.
        y: f32,
        /// Width in physical pixels.
        w: f32,
        /// Height in physical pixels.
        h: f32,
        /// Fill color.
        color: Color,
    },
    /// A single-line text run.
    Text {
        /// Top-left x, in physical pixels.
        x: f32,
        /// Top-left y, in physical pixels.
        y: f32,
        /// The run's contents.
        text: String,
        /// Glyph cell size in pixels.
        px: f32,
        /// Text color.
        color: Color,
    },
}

/// A headless [`Painter`] that records what it was asked to draw.
///
/// This is what makes the toolkit the one genuinely testable corner of the
/// project: no GPU, no window, no async. Drive a [`Ui`](crate::Ui) against one
/// of these and assert on the resulting [`DrawCmd`]s to pin down layout math and
/// hit-testing.
///
/// Text metrics assume the same monospace grid the engine's embedded bitmap font
/// uses (advance == cell size), so recorded layout matches what the overlay
/// produces.
#[derive(Debug, Default, Clone)]
pub struct RecordingPainter {
    /// Everything drawn since construction (or the last [`RecordingPainter::clear`]).
    pub cmds: Vec<DrawCmd>,
}

impl RecordingPainter {
    /// Drop all recorded commands, e.g. between simulated frames.
    pub fn clear(&mut self) {
        self.cmds.clear();
    }

    /// The recorded rectangles, in draw order.
    pub fn rects(&self) -> impl Iterator<Item = &DrawCmd> {
        self.cmds
            .iter()
            .filter(|c| matches!(c, DrawCmd::Rect { .. }))
    }

    /// The text of every recorded run, in draw order.
    pub fn texts(&self) -> Vec<&str> {
        self.cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

impl Painter for RecordingPainter {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.cmds.push(DrawCmd::Rect { x, y, w, h, color });
    }

    fn text(&mut self, x: f32, y: f32, text: &str, px: f32, color: Color) {
        self.cmds.push(DrawCmd::Text {
            x,
            y,
            text: text.to_string(),
            px,
            color,
        });
    }

    fn text_size(&self, text: &str, px: f32) -> [f32; 2] {
        [text.chars().count() as f32 * px, px]
    }
}
