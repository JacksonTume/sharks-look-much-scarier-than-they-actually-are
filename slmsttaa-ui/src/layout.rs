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

    /// The same rectangle scaled by `factor` **about its centre**, so it grows
    /// and shrinks in place rather than from its top-left corner.
    ///
    /// Which is the only sensible way to animate something appearing inside a
    /// well: scaled from the corner, a checkbox tick would slide diagonally into
    /// position instead of blooming where it belongs.
    pub fn scale(&self, factor: f32) -> Rect {
        let factor = factor.max(0.0);
        let (w, h) = (self.w * factor, self.h * factor);
        Rect::new(
            self.x + (self.w - w) * 0.5,
            self.y + (self.h - h) * 0.5,
            w,
            h,
        )
    }
}

/// The rows a [`scroll_area_virtual`](crate::Ui::scroll_area_virtual) will place.
///
/// A list this size cannot be laid out by walking it — that is the whole point —
/// so the container is *told* its shape instead of measuring it. Two numbers are
/// enough to answer everything an ordinary scroll area has to walk its children
/// for: how tall the content is, which rows the viewport covers, and where each
/// one goes.
///
/// ```
/// # use slmsttaa_ui::{Anchor, RecordingPainter, Rows, Ui, UiInput, UiState};
/// # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
/// # let mut ui = Ui::new(&mut p, UiInput::default(), &mut s);
/// # let names: Vec<String> = Vec::new();
/// # let selected = None;
/// # ui.panel(Anchor::TopLeft, 300.0, |ui| {
/// ui.scroll_area_virtual("roster", 300.0, Rows::uniform(names.len(), 22.0).reveal(selected), |ui, index| {
///     ui.label(&names[index]);
/// });
/// # });
/// ```
///
/// **The fields are private on purpose.** A public `count`/`height` pair would
/// freeze "every row is the same height" into the type, and the next thing to ask
/// for this is a list whose rows differ. Passing the shape as a value rather than
/// as two arguments is what lets that arrive as another constructor instead of as
/// a second container.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rows {
    count: usize,
    height: f32,
    reveal: Option<usize>,
}

impl Rows {
    /// `count` rows, every one `height` points tall.
    ///
    /// A non-positive `height` is treated as an empty list rather than a panic or
    /// a division by zero: a caller computing a row height from a theme or a font
    /// can reach zero legitimately, and a frame that draws nothing is a better
    /// answer than one that does not run.
    pub fn uniform(count: usize, height: f32) -> Self {
        Self {
            count: if height > 0.0 { count } else { 0 },
            height,
            reveal: None,
        }
    }

    /// Scroll so row `index` is in view, if it is not already.
    ///
    /// The scroll area cannot work this out for itself. It knows where its rows
    /// are, but a row it did not place has no rectangle to compare against the
    /// viewport, and the row a consumer wants to reveal is by definition usually
    /// one that is off-screen. `None` — which is the default — leaves the offset
    /// alone.
    ///
    /// Deliberately an index and not a widget id: this is arithmetic over the
    /// list's shape, and it knows nothing about focus, keyboards, or the tab
    /// ring. See [`Ui::scroll_area_virtual`](crate::Ui::scroll_area_virtual) for
    /// what a virtualized list still cannot do with a keyboard.
    pub fn reveal(mut self, index: Option<usize>) -> Self {
        self.reveal = index.filter(|&i| i < self.count);
        self
    }

    /// How many rows there are.
    pub fn count(&self) -> usize {
        self.count
    }

    /// How tall the whole list is, which an ordinary scroll area has to walk its
    /// children to find out.
    pub(crate) fn total_height(&self) -> f32 {
        self.count as f32 * self.height
    }

    /// The top edge of row `index`, relative to the top of the content.
    pub(crate) fn top(&self, index: usize) -> f32 {
        index as f32 * self.height
    }

    /// How tall one row is.
    pub(crate) fn height(&self) -> f32 {
        self.height
    }

    /// Which row a consumer asked to have brought into view.
    pub(crate) fn revealed(&self) -> Option<usize> {
        self.reveal
    }

    /// The rows a viewport `height` points tall shows at `offset`.
    ///
    /// **Floors the start and ceils the end**, so the partly-visible rows at both
    /// edges are placed. Dropping them would leave a blank strip at the top of
    /// the viewport for as long as the offset is not an exact multiple of a row —
    /// which, because the drawn offset eases, is most of the time.
    pub(crate) fn range(&self, offset: f32, height: f32) -> std::ops::Range<usize> {
        if self.count == 0 || self.height <= 0.0 || height <= 0.0 {
            return 0..0;
        }
        let first = (offset / self.height).floor().max(0.0) as usize;
        let last = ((offset + height) / self.height).ceil().max(0.0) as usize;
        first.min(self.count)..last.min(self.count)
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
