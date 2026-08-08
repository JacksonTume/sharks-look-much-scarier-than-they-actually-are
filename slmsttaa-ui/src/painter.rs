//! The **downward seam**: the drawing surface the toolkit paints onto.
//!
//! The UI never sees a GPU. It emits rectangles and text runs through
//! [`Painter`], and something else decides what that means — the engine's
//! `renderer::overlay::Overlay` turns them into vertices, while
//! [`RecordingPainter`] just remembers them so tests can assert on layout.
//!
//! Widening this trait is how the toolkit gains new visual capabilities. That is
//! deliberate: because a `Painter` impl lives in the *other* crate, every new
//! capability has to arrive as a considered widening of this trait rather than a
//! reach into renderer internals.
//!
//! Coordinates are **logical points**, not physical pixels. A painter that
//! renders to a HiDPI surface scales them on the way out; the toolkit never
//! learns the display's scale factor, so its layout math is resolution-agnostic.
//!
//! # What is deliberately *not* on this trait
//!
//! `text_size`. It used to be here, and it was the most dangerous method in the
//! crate: implemented once per painter, so the recording painter and the engine's
//! overlay could disagree about how wide a string is, and every test would still
//! pass. Measuring now lives in [`font`](crate::font), which both painters read.
//! See that module for the full argument and what it cost.
//!
//! The consequence is a real narrowing: **a painter does not choose the font.**
//! It is handed a run, a size, a [`Weight`], and the glyph geometry it needs from
//! [`font::glyph`](crate::font::glyph), and its job is to put those quads on the
//! screen.

use crate::font::Weight;
use crate::{font, Rect};

/// An RGBA color in `[0, 1]`, the only color type the UI speaks.
pub type Color = [f32; 4];

/// A handle to an image a [`Painter`] can draw.
///
/// Opaque on purpose: this crate never learns what an image *is*, only that a
/// painter can be handed one back. It lives here rather than in the engine for
/// the same reason [`Color`] and [`Rect`] do — it is part of the vocabulary of
/// the [`Painter`] trait, and a trait cannot speak a type its own crate cannot
/// name. The engine mints these and resolves them to textures; a consumer holds
/// one and gives it back.
///
/// That is a different answer from the one the keyboard got, where `Key` is
/// declared on both sides of the seam and translated. The difference is who
/// holds the value: nothing outside the engine ever holds a `Key`, but a
/// consumer holds an `ImageId` between creating an image and drawing it, so two
/// types would cost it a conversion in its own code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageId(u32);

impl ImageId {
    /// Mint an id from a painter's own index.
    ///
    /// For [`Painter`] implementors. It is public because the seam is
    /// unprivileged — a consumer writing its own painter has to be able to issue
    /// these, exactly as the engine's does — not because a consumer should be
    /// forging them.
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// The index this id was minted from.
    pub fn raw(self) -> u32 {
        self.0
    }
}

/// Which stacking bucket a primitive is drawn into.
///
/// An immediate-mode UI declares widgets top-to-bottom, but *draws* them in an
/// order that has nothing to do with declaration order: a panel background is
/// declared last (it is the only point at which its height is known) and must
/// land behind everything; a dropdown is declared inside the row that spawned it
/// and must land in front of every later row.
///
/// Layers decouple the two. A painter accumulates into one bucket per layer and
/// flushes them in this order, so declaration order only decides what happens
/// *within* a layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Layer {
    /// Behind everything: panel and window backgrounds.
    Base,
    /// Ordinary widget content. The default.
    #[default]
    Panel,
    /// Menus, dropdowns, and anything that must escape its row.
    Popup,
    /// Always on top.
    Tooltip,
}

impl Layer {
    /// Every layer, in flush order (back to front).
    pub const ALL: [Layer; 4] = [Layer::Base, Layer::Panel, Layer::Popup, Layer::Tooltip];

    /// How many layers exist — the size of a per-layer bucket array.
    pub const COUNT: usize = 4;

    /// Index into a per-layer bucket array, matching [`Layer::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// A 2D drawing surface the UI paints onto, in logical points with the origin
/// at the top-left (matching cursor coordinates).
pub trait Painter {
    /// Fill `rect`, with corners rounded by `radius` points (`0.0` is square).
    ///
    /// A radius at or above half the shorter side gives a capsule, which is how
    /// slider tracks and knobs are drawn.
    fn fill_rect(&mut self, rect: Rect, radius: f32, color: Color);

    /// Stroke `rect`'s outline `width` points thick, drawn *inside* the
    /// rectangle so a stroked and a filled rect of the same bounds line up.
    fn stroke_rect(&mut self, rect: Rect, radius: f32, width: f32, color: Color);

    /// Draw a left-aligned, single-line text run at size `px` in `weight`, with
    /// the top-left of its **line box** at `(x, y)`.
    ///
    /// `px` is the em size — the conventional meaning of a font size — so the
    /// ink does not fill the line box: the baseline sits
    /// [`font::ascent(px)`](crate::font::ascent) below `y` and capitals reach
    /// [`font::cap_height(px)`](crate::font::cap_height) above it. Measure a run
    /// with [`font::text_size`](crate::font::text_size) and centre one in a
    /// control with [`font::centered_top`](crate::font::centered_top); do not
    /// assume the run is `px` tall, which is what the old bitmap font allowed.
    fn text(&mut self, x: f32, y: f32, text: &str, px: f32, weight: Weight, color: Color);

    /// Direct subsequent primitives into `layer` until this is called again.
    ///
    /// Implementors must keep one accumulation bucket per [`Layer`] and emit
    /// them in [`Layer::ALL`] order, preserving call order within each bucket.
    fn set_layer(&mut self, layer: Layer);

    /// Restrict subsequent drawing to `rect`, until the matching
    /// [`Painter::pop_clip`].
    ///
    /// Clips **intersect** rather than replace, so pushing a region inside an
    /// existing one can only ever shrink what is visible. This is what makes a
    /// scroll area possible: its contents are laid out beyond its bounds and the
    /// clip is what stops them painting over the rest of the screen.
    fn push_clip(&mut self, rect: Rect);

    /// Undo the innermost [`Painter::push_clip`].
    fn pop_clip(&mut self);

    /// Stroke the open path through `points`, `width` points thick.
    ///
    /// **Open**: the last point is not joined back to the first. A closed outline
    /// is the caller repeating its first point, which is the honest way round —
    /// there is no way to un-close a closed primitive.
    ///
    /// Joins and caps are **round**, and are not configurable. A segment is drawn
    /// as the set of points within half `width` of it — a swept disc — so both
    /// fall out of the shape rather than being constructed, which is why they are
    /// free and why they are the only ones on offer.
    ///
    /// Fewer than two points, or a `width` at or below zero, draws nothing.
    ///
    /// **A translucent stroke double-blends at its joints.** Consecutive segments
    /// overlap where they meet, and straight alpha compositing puts `a` over `a`
    /// as `1 - (1 - a)²` — a visibly darker dot at every vertex. Opaque strokes
    /// are unaffected, and [`Painter::convex_polygon`] has no overlap at all, so
    /// a translucent *fill* is clean where a translucent *outline* is not.
    ///
    /// The painter does not decimate. One point per pixel of travel is the useful
    /// limit; a caller plotting more samples than the run is wide should thin
    /// them first.
    fn polyline(&mut self, points: &[(f32, f32)], width: f32, color: Color);

    /// Fill the **convex** polygon through `points`, which is implicitly closed.
    ///
    /// Convexity is a precondition, not a check: a concave input still draws, as
    /// a fan from the centroid, which is wrong in a way that looks like a bug
    /// because it is one — in the caller. Winding may be either way round.
    ///
    /// Fewer than three points draws nothing.
    ///
    /// Unlike [`Painter::polyline`] this never overlaps itself, so it composites
    /// correctly at any alpha — an area fill under a curve is exact.
    fn convex_polygon(&mut self, points: &[(f32, f32)], color: Color);

    /// Draw the sub-rectangle `uv` of `image`, stretched to fill `rect`.
    ///
    /// `uv` is `[u0, v0, u1, v1]` in `0..=1` with `[u0, v0]` landing at `rect`'s
    /// **top-left**, matching how a glyph's atlas rectangle is read. `tint`
    /// multiplies, so opaque white draws the image untouched and a themed color
    /// shades a monochrome sheet.
    ///
    /// An [`ImageId`] is minted by the painter — see [`ImageId::from_raw`]. A
    /// painter that has never issued one draws nothing here.
    ///
    /// **One image per frame.** The engine's overlay keeps the whole UI to a
    /// single draw call, which binds one texture; a frame that draws two
    /// different images cannot be honored, and the first one wins. Pack what a
    /// frame needs into one atlas and address it with `uv`.
    fn image(&mut self, rect: Rect, image: ImageId, uv: [f32; 4], tint: Color);

    /// Fill `rect` with square corners — the common case.
    fn rect(&mut self, rect: Rect, color: Color) {
        self.fill_rect(rect, 0.0, color);
    }

    /// Draw the whole of `image` into `rect`.
    fn image_full(&mut self, rect: Rect, image: ImageId, tint: Color) {
        self.image(rect, image, [0.0, 0.0, 1.0, 1.0], tint);
    }
}

/// A single primitive recorded by [`RecordingPainter`].
#[derive(Debug, Clone, PartialEq)]
pub enum DrawCmd {
    /// A filled or stroked rectangle.
    Rect {
        /// The bounds drawn.
        rect: Rect,
        /// Corner radius in points; `0.0` is square.
        radius: f32,
        /// Stroke width in points, or `0.0` for a solid fill.
        border: f32,
        /// The color.
        color: Color,
        /// The layer this was drawn into.
        layer: Layer,
        /// The clip region in force, if any.
        clip: Option<Rect>,
    },
    /// A single-line text run.
    Text {
        /// Line-box top-left x, in logical points.
        x: f32,
        /// Line-box top-left y, in logical points.
        y: f32,
        /// The run's contents.
        text: String,
        /// Em size in points.
        px: f32,
        /// Which cut of the face.
        weight: Weight,
        /// Text color.
        color: Color,
        /// The layer this was drawn into.
        layer: Layer,
        /// The clip region in force, if any.
        clip: Option<Rect>,
    },
    /// An open stroked path.
    ///
    /// Recorded as the points that were asked for, **not** as the triangles they
    /// become. That is the seam working as designed: the toolkit describes a
    /// shape and the painter renders it, so a test here asserts what a widget
    /// meant rather than how one implementation drew it.
    Polyline {
        /// The path, in logical points, in order.
        points: Vec<(f32, f32)>,
        /// Stroke thickness in points.
        width: f32,
        /// The color.
        color: Color,
        /// The layer this was drawn into.
        layer: Layer,
        /// The clip region in force, if any.
        clip: Option<Rect>,
    },
    /// A filled convex polygon, implicitly closed.
    Polygon {
        /// The outline, in logical points, in order.
        points: Vec<(f32, f32)>,
        /// The fill color.
        color: Color,
        /// The layer this was drawn into.
        layer: Layer,
        /// The clip region in force, if any.
        clip: Option<Rect>,
    },
    /// A textured quad.
    Image {
        /// The bounds drawn.
        rect: Rect,
        /// Which image.
        image: ImageId,
        /// The sub-rectangle sampled, `[u0, v0, u1, v1]` in `0..=1`.
        uv: [f32; 4],
        /// The multiplied tint.
        tint: Color,
        /// The layer this was drawn into.
        layer: Layer,
        /// The clip region in force, if any.
        clip: Option<Rect>,
    },
}

impl DrawCmd {
    /// The layer this primitive was drawn into.
    pub fn layer(&self) -> Layer {
        // Exhaustive, with no catch-all arm, so a primitive added later is a
        // compile error here rather than one that silently sorts to `Base`.
        match *self {
            DrawCmd::Rect { layer, .. }
            | DrawCmd::Text { layer, .. }
            | DrawCmd::Polyline { layer, .. }
            | DrawCmd::Polygon { layer, .. }
            | DrawCmd::Image { layer, .. } => layer,
        }
    }

    /// The clip region in force when this was drawn.
    pub fn clip(&self) -> Option<Rect> {
        match *self {
            DrawCmd::Rect { clip, .. }
            | DrawCmd::Text { clip, .. }
            | DrawCmd::Polyline { clip, .. }
            | DrawCmd::Polygon { clip, .. }
            | DrawCmd::Image { clip, .. } => clip,
        }
    }
}

/// A headless [`Painter`] that records what it was asked to draw.
///
/// This is what makes the toolkit the one genuinely testable corner of the
/// project: no GPU, no window, no async. Drive a [`Ui`](crate::Ui) against one
/// of these and assert on the resulting [`DrawCmd`]s to pin down layout math and
/// hit-testing.
///
/// Text metrics are not this painter's business, and that is the point: it and
/// the engine's overlay both measure through [`font`](crate::font), so recorded
/// layout matches what a viewer sees by construction rather than by two
/// implementations happening to agree.
#[derive(Debug, Default, Clone)]
pub struct RecordingPainter {
    /// Everything drawn since construction (or the last [`RecordingPainter::clear`]),
    /// in **call** order. For the order these would actually reach a screen, see
    /// [`RecordingPainter::in_layer_order`].
    pub cmds: Vec<DrawCmd>,
    /// The layer subsequent primitives land in.
    layer: Layer,
    /// The clip stack, each entry already intersected with the one below it.
    clips: Vec<Rect>,
}

impl RecordingPainter {
    /// Drop all recorded commands, e.g. between simulated frames. Layer and clip
    /// state reset too, matching what a real painter does per frame.
    pub fn clear(&mut self) {
        self.cmds.clear();
        self.layer = Layer::default();
        self.clips.clear();
    }

    /// The recorded commands as a real painter would emit them: sorted by layer,
    /// call order preserved within each layer.
    ///
    /// Assert against *this* when a test cares what ends up on top.
    pub fn in_layer_order(&self) -> Vec<&DrawCmd> {
        let mut out: Vec<&DrawCmd> = self.cmds.iter().collect();
        // Stable, so within-layer call order survives.
        out.sort_by_key(|c| c.layer());
        out
    }

    /// The recorded rectangles, in call order.
    pub fn rects(&self) -> impl Iterator<Item = &DrawCmd> {
        self.cmds
            .iter()
            .filter(|c| matches!(c, DrawCmd::Rect { .. }))
    }

    /// The text of every recorded run, in call order.
    pub fn texts(&self) -> Vec<&str> {
        self.cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The text of runs that would actually be visible — those not entirely
    /// outside their clip region. What a viewer would read.
    pub fn visible_texts(&self) -> Vec<&str> {
        self.cmds
            .iter()
            .filter_map(|c| match c {
                DrawCmd::Text {
                    text,
                    x,
                    y,
                    px,
                    weight,
                    ..
                } => {
                    // Measured through `font`, the same as the layout that placed
                    // it — so this reports what a viewer would actually read.
                    let [w, h] = font::text_size(text, *px, *weight);
                    let bounds = Rect::new(*x, *y, w, h);
                    match c.clip() {
                        Some(clip) if clip.intersect(bounds).is_empty() => None,
                        _ => Some(text.as_str()),
                    }
                }
                _ => None,
            })
            .collect()
    }

    /// The recorded stroked paths, in call order.
    pub fn polylines(&self) -> impl Iterator<Item = &DrawCmd> {
        self.cmds
            .iter()
            .filter(|c| matches!(c, DrawCmd::Polyline { .. }))
    }

    /// The recorded filled polygons, in call order.
    pub fn polygons(&self) -> impl Iterator<Item = &DrawCmd> {
        self.cmds
            .iter()
            .filter(|c| matches!(c, DrawCmd::Polygon { .. }))
    }

    /// The recorded textured quads, in call order.
    pub fn images(&self) -> impl Iterator<Item = &DrawCmd> {
        self.cmds
            .iter()
            .filter(|c| matches!(c, DrawCmd::Image { .. }))
    }

    /// The clip region currently in force.
    fn clip(&self) -> Option<Rect> {
        self.clips.last().copied()
    }
}

impl Painter for RecordingPainter {
    fn fill_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.cmds.push(DrawCmd::Rect {
            rect,
            radius,
            border: 0.0,
            color,
            layer: self.layer,
            clip: self.clip(),
        });
    }

    fn stroke_rect(&mut self, rect: Rect, radius: f32, width: f32, color: Color) {
        // The degenerate guard is the trait's, not an implementation detail: the
        // overlay has skipped a zero-width stroke since Slice 2, and recording one
        // here anyway meant a test could see a command the screen never drew. That
        // is `text_size`'s failure in miniature, and it lived here until Slice 10.
        if width <= 0.0 {
            return;
        }
        self.cmds.push(DrawCmd::Rect {
            rect,
            radius,
            border: width,
            color,
            layer: self.layer,
            clip: self.clip(),
        });
    }

    fn text(&mut self, x: f32, y: f32, text: &str, px: f32, weight: Weight, color: Color) {
        self.cmds.push(DrawCmd::Text {
            x,
            y,
            text: text.to_string(),
            px,
            weight,
            color,
            layer: self.layer,
            clip: self.clip(),
        });
    }

    fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }

    fn push_clip(&mut self, rect: Rect) {
        let clipped = match self.clip() {
            Some(current) => current.intersect(rect),
            None => rect,
        };
        self.clips.push(clipped);
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
    }

    fn polyline(&mut self, points: &[(f32, f32)], width: f32, color: Color) {
        if points.len() < 2 || width <= 0.0 {
            return;
        }
        self.cmds.push(DrawCmd::Polyline {
            // Recorded verbatim. Nothing here decimates, closes, or reorders —
            // if it did, a test asserting on these points would be asserting on
            // the recorder rather than on the widget that called it.
            points: points.to_vec(),
            width,
            color,
            layer: self.layer,
            clip: self.clip(),
        });
    }

    fn convex_polygon(&mut self, points: &[(f32, f32)], color: Color) {
        if points.len() < 3 {
            return;
        }
        self.cmds.push(DrawCmd::Polygon {
            points: points.to_vec(),
            color,
            layer: self.layer,
            clip: self.clip(),
        });
    }

    fn image(&mut self, rect: Rect, image: ImageId, uv: [f32; 4], tint: Color) {
        self.cmds.push(DrawCmd::Image {
            rect,
            image,
            uv,
            tint,
            layer: self.layer,
            clip: self.clip(),
        });
    }
}
