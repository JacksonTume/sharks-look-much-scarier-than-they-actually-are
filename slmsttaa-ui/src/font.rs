//! The font: Inter, baked to a signed distance field, and the **one** source of
//! text metrics in the project.
//!
//! That last part is the whole point of this module living here rather than in
//! the engine, and it is worth being explicit about because the alternative
//! looked perfectly reasonable.
//!
//! # The bug this module exists to prevent
//!
//! Through Slice 4, `text_size` was implemented **twice** — once on the engine's
//! `Overlay` and once on [`RecordingPainter`](crate::RecordingPainter) — and both
//! said `chars().count() * px`. That agreed only because the 8x8 bitmap font was
//! a monospace grid. Every alignment in the toolkit is computed from it:
//! [`Ui::label_value`](crate::Ui::label_value), right-aligned slider readouts,
//! centred button labels.
//!
//! Make the advances proportional and the two implementations diverge — and then
//! **the tests measure a different font from the screen**. `tests/regions.rs`
//! asserts right-alignment against the recording painter, so the suite stays
//! green while the demo's readouts drift off the edge of the panel. That is the
//! same failure mode as Slice 1's id bug: green tests, wrong picture.
//!
//! So the metrics live in the crate that does the layout, both painters read
//! them, and the divergence is not prevented by review — it is unrepresentable.
//!
//! # What it costs
//!
//! A [`Painter`](crate::Painter) no longer chooses its own font. The trait used
//! to carry `text_size`, which meant an implementor could in principle render
//! text with different metrics; now the contract is that a painter draws *this*
//! font, and the toolkit does the measuring. That is a genuine narrowing of the
//! downward seam, taken deliberately: a seam wide enough for two fonts is a seam
//! wide enough for two *disagreeing* fonts, and nothing has ever wanted the
//! second one.
//!
//! # Units
//!
//! Everything in [`metrics`] is in **em units** — a fraction of the render size.
//! Multiply by `px` to get logical points. This is what lets one bake serve the
//! whole type scale, and it is why the toolkit still never learns the display's
//! scale factor.
//!
//! # Zero dependencies, still
//!
//! The atlas is [`include_bytes!`]d and [`metrics`] is generated Rust. Nothing
//! here parses a TTF or rasterizes anything — that happens offline in
//! `fontbake/`, which is a separate workspace member precisely so its rasterizer
//! cannot reach this crate's build graph.

pub mod metrics;

/// The signed-distance-field atlas: raw R8 texels,
/// [`ATLAS_W`](metrics::ATLAS_W) x [`ATLAS_H`](metrics::ATLAS_H), no header.
///
/// A painter uploads this verbatim. `0.5` is the glyph edge, higher is further
/// inside — see [`aa_band`] for turning a sample into coverage.
pub const ATLAS: &[u8] = include_bytes!("font/atlas.bin");

/// Which cut of the face to draw in.
///
/// Two, because the type scale distinguishes headings from body text and nothing
/// has asked for a third. Each is a separate set of glyphs in the same atlas, so
/// mixing weights in one frame costs nothing — still one texture, one draw call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Weight {
    /// Body text, labels, readouts, button faces.
    #[default]
    Regular,
    /// Titles and section headings.
    SemiBold,
}

/// One glyph's metrics and its place in [`ATLAS`].
///
/// All lengths are em units. `x`/`y` are the quad's top-left **relative to the
/// pen position on the baseline**, y down — so `y` is normally negative, and both
/// already include the distance field's padding, which is why the quad drawn is
/// exactly the region the field covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    /// How far the pen moves after drawing this glyph.
    pub advance: f32,
    /// Quad left, relative to the pen.
    pub x: f32,
    /// Quad top, relative to the baseline, positive down.
    pub y: f32,
    /// Quad width. Zero for a glyph with no ink, such as a space.
    pub w: f32,
    /// Quad height. Zero for a glyph with no ink.
    pub h: f32,
    /// Atlas UVs, `[u0, v0, u1, v1]`.
    pub uv: [f32; 4],
}

impl Glyph {
    /// Whether this glyph has anything to draw. A space does not.
    pub fn has_ink(&self) -> bool {
        self.w > 0.0 && self.h > 0.0
    }
}

// The generated tables are indexed in step with `CHARS`, which `glyph` relies on
// for its binary search. A bake that got this wrong would otherwise show up as
// subtly wrong glyphs rather than as an error.
const _: () = assert!(metrics::CHARS.len() == metrics::REGULAR.len());
const _: () = assert!(metrics::CHARS.len() == metrics::SEMIBOLD.len());

/// The glyph table for `weight`, indexed in step with [`metrics::CHARS`].
fn table(weight: Weight) -> &'static [Glyph] {
    match weight {
        Weight::Regular => &metrics::REGULAR,
        Weight::SemiBold => &metrics::SEMIBOLD,
    }
}

/// The glyph for `ch`, or the tofu box if it was never baked.
///
/// Deliberately infallible: an unbaked character draws a visible `□` rather than
/// silently vanishing, so a missing glyph is a bug you can see. The charset is
/// printable ASCII, a named handful of symbols (`…`, `°`, `±`, `×`, `·`, `→`,
/// `←`, `✓`, `▶`, `■`, `□`), and 134 Latin-Extended letters for consumers that
/// render names they did not author — see `fontbake/src/main.rs` to add to it.
///
/// Combining marks are **not** baked and cannot usefully be: a glyph here has a
/// positive advance and no anchor, so a mark would sit beside its base letter
/// rather than over it. See `WISHLIST.md`.
pub fn glyph(ch: char, weight: Weight) -> &'static Glyph {
    let table = table(weight);
    match metrics::CHARS.binary_search(&ch) {
        Ok(i) => &table[i],
        Err(_) => &table[metrics::TOFU],
    }
}

/// How far the pen moves after `ch`, in points.
pub fn advance(ch: char, px: f32, weight: Weight) -> f32 {
    glyph(ch, weight).advance * px
}

/// The width of a text run, in points.
///
/// A plain sum of advances: there is no kerning table, so this is exact rather
/// than approximate. Kerning would make a run narrower than the sum of its
/// parts and every measurement here would have to replicate it — not worth it
/// for UI text at 15 to 24 points, and recorded in the roadmap rather than
/// silently skipped.
pub fn text_width(text: &str, px: f32, weight: Weight) -> f32 {
    text.chars().map(|c| advance(c, px, weight)).sum()
}

/// The size a text run occupies: `[width, line height]`, in points.
///
/// The height is the **line box**, not the ink — it includes the descender space
/// below the baseline whether or not the run has any descenders, so successive
/// rows stack consistently. To centre text in a control, use [`centered_top`],
/// which centres the ink instead.
pub fn text_size(text: &str, px: f32, weight: Weight) -> [f32; 2] {
    [text_width(text, px, weight), line_height(px)]
}

/// Baseline to line-box top, in points.
pub fn ascent(px: f32) -> f32 {
    metrics::ASCENT * px
}

/// Baseline to line-box bottom, in points.
pub fn descent(px: f32) -> f32 {
    metrics::DESCENT * px
}

/// The distance between successive baselines, in points.
pub fn line_height(px: f32) -> f32 {
    metrics::LINE_HEIGHT * px
}

/// The height of a capital, in points.
pub fn cap_height(px: f32) -> f32 {
    metrics::CAP_HEIGHT * px
}

/// The height of a lowercase `x`, in points.
pub fn x_height(px: f32) -> f32 {
    metrics::X_HEIGHT * px
}

/// The baseline for a run whose line-box top is at `top`.
///
/// [`Painter::text`](crate::Painter::text) takes the line-box top, so this is
/// the conversion a painter does on the way to glyph quads. Widgets rarely need
/// it — they want [`centered_top`].
pub fn baseline(top: f32, px: f32) -> f32 {
    top + ascent(px)
}

/// The line-box top that optically centres `px` text in a box `h` tall starting
/// at `y`.
///
/// **Centres the capitals, not the line box.** A line box reserves descender
/// space below the baseline that no capital occupies, so centring the box makes
/// a run of capitals and digits — which is most UI text — sit visibly high in its
/// control. This is the one piece of vertical arithmetic every widget needs, and
/// through Slice 4 each one did it by hand with a hard-coded offset that only
/// worked because the bitmap font's cell was its cap height.
pub fn centered_top(y: f32, h: f32, px: f32) -> f32 {
    // Put the baseline so the caps straddle the centre, then back out to the top.
    y + (h + cap_height(px)) / 2.0 - ascent(px)
}

/// Half-width of the antialiasing band, in **sampled field units**, for text
/// rendered at `physical_px`.
///
/// A painter fades coverage with `smoothstep(0.5 - band, 0.5 + band, sample)`.
///
/// The argument is **physical pixels**, not points, which is why this is called
/// by the painter and not by a widget: the toolkit never learns the display's
/// scale factor, and the band depends on it — the same run needs a narrower band
/// on a 2x display because each screen pixel covers less of the field.
///
/// `overlay.wgsl` deliberately uses no derivatives (so the WebGL2 fallback
/// behaves identically to WebGPU), so `fwidth` is unavailable and the band has
/// to be computed here and passed in. That is no loss: the exact render size is
/// known on the CPU, where `fwidth` only estimates it.
pub fn aa_band(physical_px: f32) -> f32 {
    if physical_px <= 0.0 {
        return 0.5;
    }
    // A sample s encodes signed distance d (in bake pixels) as
    // s = 0.5 - d / (2 * SPREAD). One screen pixel is BAKE_PPEM / physical_px
    // bake pixels, and we want a half-pixel band, hence the 4.
    let band = metrics::BAKE_PPEM / (physical_px * 4.0 * metrics::SPREAD_PX);
    // Clamped at both ends: a degenerate smoothstep at huge sizes, and a glyph
    // washed out to nothing at absurdly small ones.
    band.clamp(1.0e-4, 0.5)
}
