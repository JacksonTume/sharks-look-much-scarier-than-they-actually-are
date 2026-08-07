//! `fontbake` — the offline glyph baker behind `slmsttaa-ui`'s typography.
//!
//! Turns the Inter TTFs in `assets/` into two committed artifacts:
//!
//! - `slmsttaa-ui/src/font/atlas.bin` — a single-channel signed-distance-field
//!   atlas, raw R8, no header. The engine uploads it verbatim.
//! - `slmsttaa-ui/src/font/metrics.rs` — generated Rust: per-glyph advance,
//!   quad, and atlas UVs, plus the font-level vertical metrics.
//!
//! # Why offline
//!
//! `slmsttaa-ui` must stay a leaf crate with an empty `[dependencies]` — that is
//! the property that makes "the UI can't see `wgpu`" a compile error rather than
//! a convention. A rasterizer at runtime (or in a `build.rs`, which is still a
//! build-graph dependency) would end that. So the rasterizer lives here, runs by
//! hand, and the toolkit only ever sees bytes.
//!
//! # Why a distance field
//!
//! One bake serves every size. The alternative — coverage bitmaps baked per size
//! — is crisper at small sizes but needs a re-bake whenever the type scale moves.
//! The cost is real and worth stating: an SDF upsampled to 15pt body text has
//! softer stems than a coverage bake would. It is mitigated here by baking at
//! [`BAKE_PPEM`] with a supersampled distance transform, not eliminated.
//!
//! # Usage
//!
//! ```sh
//! cargo run -p fontbake                          # bake, writing both artifacts
//! cargo run -p fontbake -- --preview atlas.pgm   # also dump a viewable atlas
//! ```

use std::path::{Path, PathBuf};

/// The Inter release the committed TTFs came from, recorded so a re-bake is
/// reproducible rather than "whatever was upstream that day".
const INTER_VERSION: &str = "4.1";

/// Pixels-per-em the distance field is baked at.
///
/// The field is resolution-independent in principle, but not in practice: it is
/// sampled from a rasterization, so this bounds how much shape detail survives.
/// 48 comfortably exceeds the largest size in the type scale (~24pt), which is
/// the point at which upsampling would start to visibly round corners.
const BAKE_PPEM: f32 = 48.0;

/// How far the distance field extends outside the glyph, in bake-ppem pixels.
///
/// This is the whole dynamic range: a fragment further than this from an edge
/// clamps to fully-inside or fully-outside. It has to cover the widest
/// antialiasing band any render size asks for — at 24pt the band is
/// `24/48 = 0.5` px of field per pixel of screen, so 6 is generous.
const SPREAD: f32 = 6.0;

/// Supersampling factor for the distance transform.
///
/// The mask is rasterized at `BAKE_PPEM * SS` and the distance transform runs on
/// that, so edge positions are resolved to about `1/SS` of a bake pixel. Without
/// this the field is quantized to whole bake pixels and the glyphs shimmer.
const SS: usize = 4;

/// Atlas width. Height is whatever the packer needs, rounded to a power of two.
const ATLAS_W: usize = 512;

/// Non-ASCII characters worth the atlas space.
///
/// A named list rather than a Unicode range, so every addition is a reviewable
/// line: `…` is what `fit_text` will truncate with, the rest are what a
/// parameter panel reaches for. `□` is the tofu drawn for anything unbaked —
/// visible, so a missing glyph is a bug you can see rather than a silent gap.
const EXTRAS: &[char] = &['…', '°', '±', '×', '·', '→', '←', '✓', '▶', '■', '□'];

/// The Latin letters a consumer's *content* needs, as opposed to its chrome.
///
/// ASCII plus [`EXTRAS`] is enough to label a parameter panel, and stops being
/// enough the moment a consumer renders names it did not author. The second
/// consumer (The Matchmaker, a management sim over a procedurally generated
/// world) draws from name pools where **one name in six** carries something
/// outside ASCII — so this is a visible correctness bug for it, not polish.
///
/// **This list is a measurement, not a guess.** It is the exact set of
/// non-ASCII codepoints reachable across that consumer's 150,502 pool entries,
/// which is why it is a sparse list rather than the four whole Unicode blocks
/// it is drawn from: those blocks are 512 codepoints and this is 134 of them,
/// and every glyph is atlas bytes shipped in every wasm bundle. A consumer whose
/// data grows past this list is the one positioned to notice — its own suite
/// asserts over its whole pool, because this crate cannot know what a pool is.
///
/// Four codepoints its data reaches are deliberately **not** here: the combining
/// marks `U+0300`, `U+0301`, `U+030B` and `U+0361`, which appear where Unicode
/// has no precomposed form (Yoruba `ọ́`, Thai stacked tones, an ALA-LC tie bar).
/// A bake is one glyph per codepoint at a positive advance, so a combining mark
/// cannot render *as* a mark here — it would sit beside its base letter instead
/// of over it. Baking them would replace a visible tofu with a plausible-looking
/// wrong word, which is worse. They stay unbaked and are recorded as a real gap
/// in `slmsttaa-ui/WISHLIST.md`.
#[rustfmt::skip]
const LATIN_EXTENDED: &[char] = &[
    // Latin-1 Supplement — 40
    'À', 'Á', 'Â', 'Ä', 'Å', 'Ç', 'È', 'É', 'Í', 'Ñ', 'Ó', 'Õ', 'Ö', 'Ú',
    'Ü', 'à', 'á', 'â', 'ã', 'ä', 'å', 'ç', 'è', 'é', 'ê', 'ë', 'ì', 'í',
    'î', 'ï', 'ñ', 'ò', 'ó', 'ô', 'õ', 'ö', 'ù', 'ú', 'ü', 'ý',
    // Latin Extended-A — 53
    'Ā', 'ā', 'ă', 'ą', 'Ć', 'ć', 'Č', 'č', 'Ď', 'ď', 'Ē', 'ē', 'ė', 'ę',
    'ě', 'Ğ', 'ğ', 'Ģ', 'ģ', 'ī', 'İ', 'Ķ', 'ķ', 'ĺ', 'ļ', 'Ľ', 'ľ', 'ń',
    'ņ', 'ň', 'Ō', 'ō', 'ő', 'Ř', 'ř', 'Ś', 'ś', 'Ş', 'ş', 'Š', 'š', 'ť',
    'ũ', 'Ū', 'ū', 'ŭ', 'ů', 'ű', 'ź', 'Ż', 'ż', 'Ž', 'ž',
    // Latin Extended-B — 11
    'ơ', 'ư', 'ǎ', 'ǐ', 'ǒ', 'ǔ', 'ǭ', 'Ș', 'ș', 'Ț', 'ț',
    // Latin Extended Additional — 30
    'ḍ', 'Ḣ', 'Ḥ', 'ḥ', 'ṭ', 'ạ', 'ả', 'ấ', 'ầ', 'ẩ', 'ậ', 'ắ', 'ế', 'ề',
    'ễ', 'ệ', 'ị', 'ọ', 'ố', 'ồ', 'ổ', 'ớ', 'ờ', 'ợ', 'ụ', 'Ứ', 'ứ', 'ử',
    'ữ', 'ỳ',
];

/// The character substituted for anything not in the charset.
const TOFU: char = '□';

/// The weights baked, and the order they appear in the atlas and in
/// `metrics.rs`. Must match `slmsttaa_ui::font::Weight`.
const WEIGHTS: &[(&str, &str)] = &[
    ("REGULAR", "Inter-Regular.ttf"),
    ("SEMIBOLD", "Inter-SemiBold.ttf"),
];

fn main() {
    let preview = parse_preview_arg();

    let root = repo_root();
    let assets = root.join("fontbake/assets");
    let out_dir = root.join("slmsttaa-ui/src/font");

    let charset = charset();
    println!(
        "fontbake: Inter {INTER_VERSION}, {} glyphs x {} weights, {BAKE_PPEM}ppem, \
         spread {SPREAD}px, {SS}x supersampled",
        charset.len(),
        WEIGHTS.len()
    );

    // Vertical metrics come from the regular weight; the semibold's differ by
    // rounding at most, and one line height for the family is the point.
    let mut vertical = None;
    let mut faces = Vec::new();

    for (name, file) in WEIGHTS {
        let path = assets.join(file);
        let bytes =
            std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let font = fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
            .unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()));

        let missing: Vec<char> = charset
            .iter()
            .copied()
            .filter(|&c| font.lookup_glyph_index(c) == 0)
            .collect();
        assert!(
            missing.is_empty(),
            "{file} has no glyph for {missing:?} — remove them from EXTRAS or pick another face"
        );

        if vertical.is_none() {
            vertical = Some(measure_vertical(&font));
        }
        let glyphs: Vec<Baked> = charset.iter().map(|&c| bake_glyph(&font, c)).collect();
        faces.push((*name, glyphs));
    }

    let vertical = vertical.expect("at least one weight");
    println!(
        "  vertical: ascent {:.4}em descent {:.4}em line {:.4}em cap {:.4}em x-height {:.4}em",
        vertical.ascent, vertical.descent, vertical.line_height, vertical.cap, vertical.x_height
    );

    // Tabular figures. Inter's proportional digits are not all one width, which
    // makes a dragged slider's readout shuffle sideways as its digits change. No
    // OpenType feature is available here (fontdue applies none), so the advance
    // is overridden to the widest digit's and each digit is re-centred in it.
    for (name, glyphs) in &mut faces {
        let before: Vec<f32> = digit_advances(glyphs);
        let shift = tabularize(glyphs);
        let after: Vec<f32> = digit_advances(glyphs);
        let span = |v: &[f32]| {
            v.iter().copied().fold(f32::MAX, f32::min)..v.iter().copied().fold(0.0f32, f32::max)
        };
        let (b, a) = (span(&before), span(&after));
        println!(
            "  {name}: digit advances {:.4}..{:.4}em -> {:.4}..{:.4}em \
             (widest digit wins, up to {shift:.4}em added)",
            b.start, b.end, a.start, a.end
        );
    }

    // One atlas for both weights: same texture, same sampler, one bind group.
    let mut all: Vec<(usize, usize)> = Vec::new(); // (face, glyph)
    for (fi, (_, glyphs)) in faces.iter().enumerate() {
        for (gi, glyph) in glyphs.iter().enumerate() {
            if glyph.px_w > 0 {
                all.push((fi, gi));
            }
        }
    }
    let atlas_h = pack(&mut faces, &mut all);
    let atlas = compose(&faces, atlas_h);
    let inked: usize = faces
        .iter()
        .flat_map(|(_, g)| g.iter())
        .map(|g| g.px_w * g.px_h)
        .sum();
    println!(
        "  atlas: {ATLAS_W}x{atlas_h} R8 = {} KiB, {} packed glyphs, {:.0}% packing efficiency",
        atlas.len() / 1024,
        all.len(),
        100.0 * inked as f32 / atlas.len() as f32,
    );

    std::fs::create_dir_all(&out_dir).expect("create font dir");
    std::fs::write(out_dir.join("atlas.bin"), &atlas).expect("write atlas.bin");
    std::fs::write(
        out_dir.join("metrics.rs"),
        emit_metrics(&faces, &charset, &vertical, atlas_h),
    )
    .expect("write metrics.rs");
    println!("  wrote {}/{{atlas.bin,metrics.rs}}", out_dir.display());

    if let Some(path) = preview {
        write_pgm(&path, &atlas, ATLAS_W, atlas_h);
        println!("  wrote preview {}", path.display());
    }
}

// --- Charset ---------------------------------------------------------------

/// Printable ASCII plus [`EXTRAS`], sorted and deduplicated so the emitted table
/// can be binary-searched.
fn charset() -> Vec<char> {
    let mut chars: Vec<char> = (0x20u8..=0x7E).map(|b| b as char).collect();
    chars.extend_from_slice(EXTRAS);
    chars.extend_from_slice(LATIN_EXTENDED);
    chars.sort_unstable();
    chars.dedup();
    chars
}

// --- Vertical metrics ------------------------------------------------------

/// Font-level metrics, all in em units so the toolkit can scale them by `px`.
struct Vertical {
    /// Baseline to line-box top, positive up.
    ascent: f32,
    /// Baseline to line-box bottom, positive down.
    descent: f32,
    /// `ascent + descent + line_gap`.
    line_height: f32,
    /// Height of a capital. What text is optically centred on.
    cap: f32,
    /// Height of a lowercase `x`.
    x_height: f32,
}

/// Read the horizontal line metrics, and *measure* cap and x-height.
///
/// The vertical metrics come from the font's `hhea` table; cap and x-height are
/// taken from the rasterized height of `H` and `x`, because fontdue exposes no
/// `OS/2` table. Measuring is honest here — it is the height of the ink, which
/// is what centring cares about.
fn measure_vertical(font: &fontdue::Font) -> Vertical {
    let px = BAKE_PPEM * SS as f32;
    let m = font
        .horizontal_line_metrics(px)
        .expect("Inter has horizontal line metrics");
    let ink_height = |c: char| font.rasterize(c, px).0.height as f32 / px;
    Vertical {
        ascent: m.ascent / px,
        descent: -m.descent / px,
        line_height: m.new_line_size / px,
        cap: ink_height('H'),
        x_height: ink_height('x'),
    }
}

// --- Per-glyph bake --------------------------------------------------------

/// One baked glyph: its metrics in em units, and its distance field.
struct Baked {
    ch: char,
    /// Pen advance, em units.
    advance: f32,
    /// Quad top-left relative to the pen position on the baseline, em units,
    /// y **down**. Includes the [`SPREAD`] padding, so the quad drawn is exactly
    /// the region the field covers.
    x: f32,
    y: f32,
    /// Quad size, em units. Zero for a glyph with no ink (space).
    w: f32,
    h: f32,
    /// Field size in atlas texels.
    px_w: usize,
    px_h: usize,
    /// The field itself, `px_w * px_h` bytes, row-major top-to-bottom.
    field: Vec<u8>,
    /// Position in the atlas, filled in by [`pack`].
    ax: usize,
    ay: usize,
}

/// Rasterize `ch` at high resolution, distance-transform it, and downsample to a
/// [`BAKE_PPEM`] field.
fn bake_glyph(font: &fontdue::Font, ch: char) -> Baked {
    let hi_px = BAKE_PPEM * SS as f32;
    let (m, coverage) = font.rasterize(ch, hi_px);
    let advance = m.advance_width / hi_px;

    // No ink (space): advance only, no atlas entry.
    if m.width == 0 || m.height == 0 {
        return Baked {
            ch,
            advance,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            px_w: 0,
            px_h: 0,
            field: Vec::new(),
            ax: 0,
            ay: 0,
        };
    }

    // Pad by the spread so the field has room to fall off, and round the padded
    // extent up to a whole multiple of SS. That last part matters: it makes one
    // output texel exactly SS input samples, so the downsample is an exact block
    // average and the quad's size is exactly the region transformed.
    let pad = SPREAD as usize * SS;
    let slack = |n: usize| (SS - n % SS) % SS;
    let (mw, mh) = (
        m.width + 2 * pad + slack(m.width),
        m.height + 2 * pad + slack(m.height),
    );

    let mut inside = vec![false; mw * mh];
    for row in 0..m.height {
        for col in 0..m.width {
            // Threshold the antialiased coverage: the distance transform needs a
            // binary mask, and SS-times supersampling is what recovers the
            // sub-pixel edge position that thresholding throws away.
            if coverage[row * m.width + col] >= 128 {
                inside[(row + pad) * mw + (col + pad)] = true;
            }
        }
    }

    let signed = signed_distance(&inside, mw, mh);
    let (px_w, px_h) = (mw / SS, mh / SS);
    let mut field = vec![0u8; px_w * px_h];
    for oy in 0..px_h {
        for ox in 0..px_w {
            // Exact block average, then convert mask pixels to bake pixels.
            let mut sum = 0.0;
            for sy in 0..SS {
                for sx in 0..SS {
                    sum += signed[(oy * SS + sy) * mw + (ox * SS + sx)];
                }
            }
            let d = sum / (SS * SS) as f32 / SS as f32;
            // Encode so that 0.5 is the edge and larger is further inside, which
            // is what lets the shader threshold at 0.5 regardless of render size.
            let a = (0.5 - d / (2.0 * SPREAD)).clamp(0.0, 1.0);
            field[oy * px_w + ox] = (a * 255.0).round() as u8;
        }
    }

    Baked {
        ch,
        advance,
        // fontdue gives xmin as the left bearing and ymin as the bitmap's bottom
        // relative to the baseline, y **up**. The quad is the padded region, so
        // both move out by one spread.
        x: (m.xmin as f32 / SS as f32 - SPREAD) / BAKE_PPEM,
        y: (-(m.ymin as f32 + m.height as f32) / SS as f32 - SPREAD) / BAKE_PPEM,
        w: px_w as f32 / BAKE_PPEM,
        h: px_h as f32 / BAKE_PPEM,
        px_w,
        px_h,
        field,
        ax: 0,
        ay: 0,
    }
}

/// Every digit's advance, for the before/after report.
fn digit_advances(glyphs: &[Baked]) -> Vec<f32> {
    glyphs
        .iter()
        .filter(|g| g.ch.is_ascii_digit())
        .map(|g| g.advance)
        .collect()
}

/// Force every digit onto the widest digit's advance, re-centring each glyph in
/// its wider box. Returns the largest shift applied, in em units.
///
/// This is what stops a slider readout from wobbling: `1` is narrower than `0`
/// in Inter's proportional figures, so `0.50` and `1.11` are different widths
/// and a right-aligned readout's left edge jumps around while you drag.
fn tabularize(glyphs: &mut [Baked]) -> f32 {
    let widest = glyphs
        .iter()
        .filter(|g| g.ch.is_ascii_digit())
        .map(|g| g.advance)
        .fold(0.0f32, f32::max);
    let mut max_shift = 0.0f32;
    for g in glyphs.iter_mut().filter(|g| g.ch.is_ascii_digit()) {
        let shift = (widest - g.advance) / 2.0;
        g.x += shift;
        g.advance = widest;
        max_shift = max_shift.max(shift * 2.0);
    }
    max_shift
}

// --- Signed distance transform --------------------------------------------

/// Signed Euclidean distance to the glyph edge, in mask pixels: positive
/// outside, negative inside.
fn signed_distance(inside: &[bool], w: usize, h: usize) -> Vec<f32> {
    let to_inside = distance_to(inside, w, h, true);
    let to_outside = distance_to(inside, w, h, false);
    (0..w * h)
        .map(|i| {
            if inside[i] {
                -to_outside[i]
            } else {
                to_inside[i]
            }
        })
        .collect()
}

/// Euclidean distance from every cell to the nearest cell where
/// `mask == target`, by 8SSEDT: propagate the *offset vector* to the nearest
/// target in two raster sweeps.
///
/// Exact enough for glyph fields and O(w·h), which brute force is not — a
/// supersampled glyph mask is a few hundred thousand cells.
fn distance_to(mask: &[bool], w: usize, h: usize, target: bool) -> Vec<f32> {
    /// Larger than any real offset, small enough that its square fits an i64
    /// comfortably.
    const INF: i32 = 1 << 14;

    let mut dx = vec![INF; w * h];
    let mut dy = vec![INF; w * h];
    for i in 0..w * h {
        if mask[i] == target {
            dx[i] = 0;
            dy[i] = 0;
        }
    }

    let mag = |x: i32, y: i32| (x as i64) * (x as i64) + (y as i64) * (y as i64);
    // A cell's nearest target, seen from one step away, is that neighbour's
    // nearest target plus the step taken to reach it.
    let relax = |dx: &mut Vec<i32>, dy: &mut Vec<i32>, p: usize, q: usize, ox: i32, oy: i32| {
        let (cx, cy) = (dx[q] + ox, dy[q] + oy);
        if mag(cx, cy) < mag(dx[p], dy[p]) {
            dx[p] = cx;
            dy[p] = cy;
        }
    };

    // Forward sweep: everything above, and to the left on this row.
    for y in 0..h {
        for x in 0..w {
            let p = y * w + x;
            if x > 0 {
                relax(&mut dx, &mut dy, p, p - 1, -1, 0);
            }
            if y > 0 {
                relax(&mut dx, &mut dy, p, p - w, 0, -1);
                if x > 0 {
                    relax(&mut dx, &mut dy, p, p - w - 1, -1, -1);
                }
                if x + 1 < w {
                    relax(&mut dx, &mut dy, p, p - w + 1, 1, -1);
                }
            }
        }
        for x in (0..w.saturating_sub(1)).rev() {
            let p = y * w + x;
            relax(&mut dx, &mut dy, p, p + 1, 1, 0);
        }
    }
    // Backward sweep: everything below, and to the right on this row.
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let p = y * w + x;
            if x + 1 < w {
                relax(&mut dx, &mut dy, p, p + 1, 1, 0);
            }
            if y + 1 < h {
                relax(&mut dx, &mut dy, p, p + w, 0, 1);
                if x + 1 < w {
                    relax(&mut dx, &mut dy, p, p + w + 1, 1, 1);
                }
                if x > 0 {
                    relax(&mut dx, &mut dy, p, p + w - 1, -1, 1);
                }
            }
        }
        for x in 1..w {
            let p = y * w + x;
            relax(&mut dx, &mut dy, p, p - 1, -1, 0);
        }
    }

    (0..w * h)
        .map(|i| (mag(dx[i], dy[i]) as f32).sqrt())
        .collect()
}

// --- Atlas packing ---------------------------------------------------------

/// Shelf-pack every inked glyph into a [`ATLAS_W`]-wide atlas, tallest first.
/// Returns the atlas height, rounded up to a power of two.
fn pack(faces: &mut [(&str, Vec<Baked>)], all: &mut [(usize, usize)]) -> usize {
    /// Keeps neighbouring fields from bleeding into each other under linear
    /// filtering.
    const GUTTER: usize = 1;

    all.sort_by_key(|&(f, g)| std::cmp::Reverse(faces[f].1[g].px_h));

    let (mut x, mut y, mut shelf_h) = (GUTTER, GUTTER, 0usize);
    for &(f, g) in all.iter() {
        let (gw, gh) = (faces[f].1[g].px_w, faces[f].1[g].px_h);
        assert!(
            gw + 2 * GUTTER <= ATLAS_W,
            "glyph {:?} is {gw}px wide, atlas is only {ATLAS_W}",
            faces[f].1[g].ch
        );
        if x + gw + GUTTER > ATLAS_W {
            x = GUTTER;
            y += shelf_h + GUTTER;
            shelf_h = 0;
        }
        faces[f].1[g].ax = x;
        faces[f].1[g].ay = y;
        x += gw + GUTTER;
        shelf_h = shelf_h.max(gh);
    }
    // Deliberately *not* rounded to a power of two. WebGPU imposes no such
    // requirement on a non-mipmapped texture, and rounding up cost 288 blank
    // rows — 144 KiB of the committed artifact and of every wasm bundle. A
    // multiple of 4 is enough to keep the row stride tidy.
    (y + shelf_h + GUTTER).next_multiple_of(4)
}

/// Blit every packed field into one R8 buffer.
fn compose(faces: &[(&str, Vec<Baked>)], atlas_h: usize) -> Vec<u8> {
    let mut atlas = vec![0u8; ATLAS_W * atlas_h];
    for (_, glyphs) in faces {
        for g in glyphs.iter().filter(|g| g.px_w > 0) {
            for row in 0..g.px_h {
                let dst = (g.ay + row) * ATLAS_W + g.ax;
                atlas[dst..dst + g.px_w]
                    .copy_from_slice(&g.field[row * g.px_w..(row + 1) * g.px_w]);
            }
        }
    }
    atlas
}

// --- Emit ------------------------------------------------------------------

/// Render the generated `metrics.rs`.
fn emit_metrics(
    faces: &[(&str, Vec<Baked>)],
    charset: &[char],
    v: &Vertical,
    atlas_h: usize,
) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "//! Generated by `cargo run -p fontbake` from Inter {INTER_VERSION}. **Do not edit.**\n\
         //!\n\
         //! Every length is in **em units** — multiply by the render size in points to\n\
         //! get points. That is what makes one bake serve the whole type scale.\n\
         //!\n\
         //! Baked at {BAKE_PPEM}ppem with a {SPREAD}px spread, {SS}x supersampled.\n\
         //! See `fontbake/src/main.rs` for what each number means and how to change it.\n\n\
         use super::Glyph;\n\n\
         /// Atlas width in texels.\n\
         pub const ATLAS_W: u32 = {ATLAS_W};\n\
         /// Atlas height in texels.\n\
         pub const ATLAS_H: u32 = {atlas_h};\n\n\
         /// Pixels-per-em the field was baked at.\n\
         pub const BAKE_PPEM: f32 = {BAKE_PPEM:?};\n\
         /// Field falloff distance, in bake-ppem pixels.\n\
         pub const SPREAD_PX: f32 = {SPREAD:?};\n\n"
    ));
    s.push_str(&format!(
        "/// Baseline to line-box top, em units.\n\
         pub const ASCENT: f32 = {:?};\n\
         /// Baseline to line-box bottom, em units.\n\
         pub const DESCENT: f32 = {:?};\n\
         /// Distance between successive baselines, em units.\n\
         pub const LINE_HEIGHT: f32 = {:?};\n\
         /// Height of a capital, em units. What text is optically centred on.\n\
         pub const CAP_HEIGHT: f32 = {:?};\n\
         /// Height of a lowercase `x`, em units.\n\
         pub const X_HEIGHT: f32 = {:?};\n\n",
        round(v.ascent),
        round(v.descent),
        round(v.line_height),
        round(v.cap),
        round(v.x_height),
    ));

    s.push_str(&format!(
        "/// Every baked character, sorted, so a lookup is a binary search.\n\
         pub const CHARS: [char; {}] = [\n",
        charset.len()
    ));
    for chunk in charset.chunks(8) {
        s.push_str("    ");
        for &c in chunk {
            s.push_str(&format!("{:?}, ", c));
        }
        s.push('\n');
    }
    s.push_str("];\n\n");
    s.push_str(&format!(
        "/// Index of the tofu glyph in [`CHARS`], drawn for anything unbaked.\n\
         pub const TOFU: usize = {};\n\n",
        charset.iter().position(|&c| c == TOFU).expect("tofu baked")
    ));

    for (name, glyphs) in faces {
        s.push_str(&format!(
            "/// {} glyphs, indexed in step with [`CHARS`].\n\
             pub const {name}: [Glyph; {}] = [\n",
            if *name == "REGULAR" {
                "Regular"
            } else {
                "SemiBold"
            },
            glyphs.len()
        ));
        for g in glyphs.iter() {
            let (u0, v0, u1, v1) = if g.px_w == 0 {
                (0.0, 0.0, 0.0, 0.0)
            } else {
                (
                    g.ax as f32 / ATLAS_W as f32,
                    g.ay as f32 / atlas_h as f32,
                    (g.ax + g.px_w) as f32 / ATLAS_W as f32,
                    (g.ay + g.px_h) as f32 / atlas_h as f32,
                )
            };
            s.push_str(&format!(
                "    Glyph {{ advance: {:?}, x: {:?}, y: {:?}, w: {:?}, h: {:?}, \
                 uv: [{:?}, {:?}, {:?}, {:?}] }}, // {:?}\n",
                round(g.advance),
                round(g.x),
                round(g.y),
                round(g.w),
                round(g.h),
                round(u0),
                round(v0),
                round(u1),
                round(v1),
                g.ch,
            ));
        }
        s.push_str("];\n\n");
    }
    s
}

/// Trim emitted floats to five decimals — em units, so that is well under a
/// tenth of a pixel at any size the toolkit uses, and it keeps the generated
/// file diffable.
fn round(x: f32) -> f32 {
    (x * 100_000.0).round() / 100_000.0
}

// --- Odds and ends ---------------------------------------------------------

/// `--preview <path>`, if given.
fn parse_preview_arg() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        None => None,
        Some((flag, rest)) if flag == "--preview" => {
            Some(PathBuf::from(rest.first().expect("--preview needs a path")))
        }
        Some((other, _)) => panic!("unknown argument {other:?}; usage: [--preview <path.pgm>]"),
    }
}

/// The workspace root, found by walking up from this crate.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("fontbake lives in the workspace")
        .to_path_buf()
}

/// Dump the atlas as a binary PGM, which every image viewer opens. Purely a
/// debugging aid — nothing reads this back.
fn write_pgm(path: &Path, atlas: &[u8], w: usize, h: usize) {
    let mut out = format!("P5\n{w} {h}\n255\n").into_bytes();
    out.extend_from_slice(atlas);
    std::fs::write(path, out).expect("write preview");
}
