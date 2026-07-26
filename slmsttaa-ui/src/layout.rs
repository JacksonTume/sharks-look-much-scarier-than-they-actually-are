//! Rectangles, and the regions widgets are placed inside.
//!
//! Layout is a **stack of regions**. A region owns a rectangle it may place
//! things in, a cursor into that rectangle, and a direction. `Ui` keeps the
//! stack; a panel, a row, a column, an indent, and a scroll area each push one,
//! run a closure, and pop.
//!
//! That stack is the whole of UI Slice 3. Everything above it — [`Ui::panel`],
//! [`Ui::horizontal`], [`Ui::columns`], [`Ui::indent`] — is a few lines of
//! push/run/pop, because the interesting question ("where does the next widget
//! go, and how wide is it?") is answered in exactly one place: [`Region::place`].
//!
//! [`Ui::panel`]: crate::Ui::panel
//! [`Ui::horizontal`]: crate::Ui::horizontal
//! [`Ui::columns`]: crate::Ui::columns
//! [`Ui::indent`]: crate::Ui::indent

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

/// Which way a region hands out space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dir {
    /// Stacked top to bottom, each row the full available width. The default,
    /// and what a panel, a column, and an indent all are.
    Vertical,
    /// Packed left to right along one line.
    LeftToRight,
    /// Packed right to left along one line, so the *first* thing declared ends
    /// up hard against the right edge. This is right-alignment.
    RightToLeft,
}

/// One frame of layout: a rectangle to place things in, and a cursor into it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Region {
    /// What this region may lay out inside, in absolute points.
    ///
    /// For a vertical region the height is advisory — a panel grows to fit its
    /// contents rather than being clipped to this — but the width is binding:
    /// it is what `[0.0, h]` means by "full width".
    avail: Rect,
    /// Where the next widget goes. For a right-to-left region this is the
    /// *right* edge of the next widget, not its left.
    cursor: (f32, f32),
    dir: Dir,
    /// The tallest thing placed on this line, so the parent knows how far down
    /// to move when the region closes. Only rows use it.
    line_h: f32,
    /// Where the cursor started, so consumed height is known at close.
    origin: (f32, f32),
}

impl Region {
    /// A top-to-bottom region filling `avail`.
    pub(crate) fn vertical(avail: Rect) -> Self {
        Self {
            avail,
            cursor: (avail.x, avail.y),
            dir: Dir::Vertical,
            line_h: 0.0,
            origin: (avail.x, avail.y),
        }
    }

    /// A single-line region filling `avail`, packing in `dir`.
    ///
    /// A right-to-left row starts its cursor at the right edge, which is the
    /// whole trick behind [`Ui::right`](crate::Ui::right).
    pub(crate) fn row(avail: Rect, dir: Dir) -> Self {
        let x = match dir {
            Dir::RightToLeft => avail.max_x(),
            _ => avail.x,
        };
        Self {
            avail,
            cursor: (x, avail.y),
            dir,
            line_h: 0.0,
            origin: (x, avail.y),
        }
    }

    /// Claim `[width, height]` and return where it landed.
    ///
    /// A non-positive width means "whatever is left": the full available width
    /// in a vertical region, and the space between the cursor and the far edge
    /// in a row. `gap` separates neighbours on a line and is not applied
    /// vertically — a widget's trailing gap is part of the height it asks for.
    pub(crate) fn place(&mut self, width: f32, height: f32, gap: f32) -> Rect {
        match self.dir {
            Dir::Vertical => {
                let w = if width > 0.0 {
                    width.min(self.avail.w)
                } else {
                    self.avail.w
                };
                let rect = Rect::new(self.avail.x, self.cursor.1, w, height);
                self.cursor.1 += height;
                rect
            }
            Dir::LeftToRight => {
                let remaining = (self.avail.max_x() - self.cursor.0).max(0.0);
                let w = if width > 0.0 {
                    width.min(remaining)
                } else {
                    remaining
                };
                let rect = Rect::new(self.cursor.0, self.avail.y, w, height);
                self.cursor.0 += w + gap;
                self.line_h = self.line_h.max(height);
                rect
            }
            Dir::RightToLeft => {
                let remaining = (self.cursor.0 - self.avail.x).max(0.0);
                let w = if width > 0.0 {
                    width.min(remaining)
                } else {
                    remaining
                };
                let rect = Rect::new(self.cursor.0 - w, self.avail.y, w, height);
                self.cursor.0 -= w + gap;
                self.line_h = self.line_h.max(height);
                rect
            }
        }
    }

    /// The rectangle a child region should be given: everything still free,
    /// starting at the cursor.
    pub(crate) fn next_line(&self) -> Rect {
        match self.dir {
            Dir::Vertical => Rect::new(
                self.avail.x,
                self.cursor.1,
                self.avail.w,
                (self.avail.max_y() - self.cursor.1).max(0.0),
            ),
            Dir::LeftToRight => Rect::new(
                self.cursor.0,
                self.avail.y,
                (self.avail.max_x() - self.cursor.0).max(0.0),
                self.avail.h,
            ),
            Dir::RightToLeft => Rect::new(
                self.avail.x,
                self.avail.y,
                (self.cursor.0 - self.avail.x).max(0.0),
                self.avail.h,
            ),
        }
    }

    /// Consume space along the direction of flow. This is `Ui::spacing`.
    pub(crate) fn advance_main(&mut self, amount: f32) {
        match self.dir {
            Dir::Vertical => self.cursor.1 += amount,
            Dir::LeftToRight => self.cursor.0 += amount,
            Dir::RightToLeft => self.cursor.0 -= amount,
        }
    }

    /// Consume `height` points of vertical space, which is what a parent does
    /// when a child region closes.
    ///
    /// A row absorbs it into its line height instead of moving its cursor: the
    /// child already spanned the width it needed.
    pub(crate) fn advance_block(&mut self, height: f32) {
        match self.dir {
            Dir::Vertical => self.cursor.1 += height,
            _ => self.line_h = self.line_h.max(height),
        }
    }

    /// How much vertical space this region ended up using.
    pub(crate) fn consumed_height(&self) -> f32 {
        match self.dir {
            Dir::Vertical => self.cursor.1 - self.origin.1,
            _ => self.line_h,
        }
    }
}
