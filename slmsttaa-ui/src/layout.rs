//! Rectangles, and the vertical cursor widgets are placed with.
//!
//! Layout today is exactly as simple as one fixed panel demands: a `y` that runs
//! down the panel, one widget per row, full width. That is the honest state of
//! it — allocate-from-available-rect, rows, columns, and alignment are UI Slice
//! 3, pulled in when the terrain panel wants a button row.

/// An axis-aligned rectangle in logical points, origin top-left.
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

    /// The overlap of two rectangles, or an empty rectangle if they don't touch.
    ///
    /// Nested clip regions intersect rather than replace, so a scroll area
    /// inside a panel can never paint outside the panel.
    pub fn intersect(&self, other: Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let max_x = self.max_x().min(other.max_x());
        let max_y = self.max_y().min(other.max_y());
        Rect::new(x, y, (max_x - x).max(0.0), (max_y - y).max(0.0))
    }

    /// Whether the rectangle encloses no area, and so hides anything clipped to it.
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// The same rectangle inset by `points` on every side.
    pub fn shrink(&self, points: f32) -> Rect {
        Rect::new(
            self.x + points,
            self.y + points,
            (self.w - 2.0 * points).max(0.0),
            (self.h - 2.0 * points).max(0.0),
        )
    }

    /// The same rectangle moved by `(dx, dy)`.
    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
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

    /// Consume `dy` points of vertical space.
    pub(crate) fn advance(&mut self, dy: f32) {
        self.cursor_y += dy;
    }

    /// Move the cursor to an absolute `y`, leaving the origin alone.
    ///
    /// A scroll area needs this: its contents are laid out from a shifted
    /// position, but the *panel's* height still has to be measured from where
    /// the panel actually started.
    pub(crate) fn set_y(&mut self, y: f32) {
        self.cursor_y = y;
    }

    /// How tall the panel has grown so far.
    pub(crate) fn height(&self) -> f32 {
        self.cursor_y - self.origin_y
    }
}
