//! Rectangles, and the vertical cursor widgets are placed with.
//!
//! Layout today is exactly as simple as one fixed panel demands: a `y` that runs
//! down the panel, one widget per row, full width. That is the honest state of
//! it — allocate-from-available-rect, rows, columns, and alignment are UI Slice
//! 3, pulled in when the terrain panel wants a button row.

/// An axis-aligned rectangle in physical pixels, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// A rectangle from its top-left corner and size.
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// The right edge (`x + w`).
    pub fn max_x(&self) -> f32 {
        self.x + self.w
    }

    /// The bottom edge (`y + h`).
    pub fn max_y(&self) -> f32 {
        self.y + self.h
    }

    /// Whether `point` lies within the rectangle (edges inclusive).
    pub fn contains(&self, (px, py): (f32, f32)) -> bool {
        px >= self.x && px <= self.max_x() && py >= self.y && py <= self.max_y()
    }
}

/// The running placement cursor for one panel's top-to-bottom layout.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Layout {
    /// Where the panel started, so the total height is known at drop time.
    origin_y: f32,
    /// The next free `y`.
    cursor_y: f32,
}

impl Layout {
    /// Start laying out at `origin_y`, with the first row `pad` below it.
    pub(crate) fn new(origin_y: f32, pad: f32) -> Self {
        Self {
            origin_y,
            cursor_y: origin_y + pad,
        }
    }

    /// The `y` the next widget draws at.
    pub(crate) fn y(&self) -> f32 {
        self.cursor_y
    }

    /// Consume `dy` pixels of vertical space.
    pub(crate) fn advance(&mut self, dy: f32) {
        self.cursor_y += dy;
    }

    /// How tall the panel has grown so far.
    pub(crate) fn height(&self) -> f32 {
        self.cursor_y - self.origin_y
    }
}
