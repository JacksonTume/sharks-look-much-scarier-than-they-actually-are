//! Layer 1 of the terrain vertical: the **base shape**, a fractal Perlin-noise
//! heightmap.
//!
//! This is the foundation the erosion layer ([`super::erosion`]) later carves
//! into. On its own it already gives recognizable rolling hills and ridges: a few
//! octaves of gradient (Perlin) noise summed as fractional Brownian motion.
//!
//! Like everything in this demo it lives **entirely in the consumer** (roadmap
//! principle 3): the engine never sees a heightmap, only the [`Mesh`] the demo
//! builds from one.
//!
//! [`Mesh`]: slmsttaa::Mesh

/// The tunable knobs for the base heightmap — exactly the sliders the UI exposes.
#[derive(Debug, Clone, Copy)]
pub struct NoiseParams {
    /// Seed for the gradient lattice; changing it gives an entirely new terrain.
    pub seed: u32,
    /// Number of fBm octaves summed. More octaves add finer detail.
    pub octaves: u32,
    /// Base frequency: roughly how many noise features span the terrain.
    pub frequency: f32,
    /// Frequency multiplier per octave (classic fBm uses ~2.0).
    pub lacunarity: f32,
    /// Amplitude multiplier per octave (a.k.a. gain; classic fBm uses ~0.5).
    pub persistence: f32,
    /// Exponent applied to the normalized height. `>1` flattens valleys and
    /// sharpens peaks (a cheap way to get plains + mountains rather than uniform
    /// bumpiness); `1.0` leaves the noise untouched.
    pub ridge: f32,

    /// How far out the land holds its height before falling away to the sea, as a
    /// fraction of the distance from the centre to the edge of the map.
    ///
    /// This is what turns a square of noise into a **continent**. Without it the
    /// landmass runs to all four edges and the map border is the only base level,
    /// so every river ends by falling off the world. With it the rim is pulled
    /// down under [`ErosionParams::sea_level`], the coastline lands where the
    /// eroded terrain happens to cross that level — ragged, because the noise is
    /// still there where the falloff is partial — and rivers reach an actual sea.
    ///
    /// Measured from the centre in units where `1.0` is the middle of an edge, so
    /// the corners (at `√2`) drown first and the landmass reads as round rather
    /// than as a square with soft edges. `1.0` disables it.
    pub coast: f32,
}

impl Default for NoiseParams {
    fn default() -> Self {
        Self {
            seed: 1,
            octaves: 5,
            frequency: 3.5,
            lacunarity: 2.0,
            persistence: 0.5,
            ridge: 1.4,
            coast: 0.45,
        }
    }
}

/// A square `n × n` grid of normalized heights in `[0, 1]`, row-major
/// (`index = y * n + x`).
#[derive(Clone)]
pub struct Heightmap {
    /// Grid side length.
    pub n: usize,
    /// Height per cell, normalized to `[0, 1]`.
    pub heights: Vec<f32>,
}

impl Heightmap {
    /// Generate an `n × n` heightmap by sampling fractal Perlin noise across the
    /// map, then normalizing the result to `[0, 1]` and cutting a coastline into
    /// it.
    ///
    /// `span` is **how many reference map-widths across this map is**, and a bigger
    /// map answers it by widening the *spectrum* rather than by tiling more of the
    /// same noise.
    ///
    /// The obvious reading — sample `span` times as much noise, so a hill keeps its
    /// size and the map holds more of them — was built first and was wrong on
    /// screen. Sixteen times the land came out as sixteen times as many hills of
    /// exactly the same size, with nothing larger than a hill anywhere on it: from
    /// the one view that can see the whole map it read as fur. A continent is not a
    /// lot of hills, it is *structure at every scale* — belts and basins spanning
    /// the whole landmass, ranges inside those, hills inside those.
    ///
    /// So the lowest octave always spans the map, whatever the map is, and growing
    /// it adds octaves at the **fine** end: two per doubling of `span`, which is
    /// what keeps the smallest landform the same handful of cells across that it is
    /// at the reference size. The result is self-similar — a 2048² continent has
    /// the same texture as the 128² one, with four times the range of feature sizes
    /// between its biggest and smallest — and `span == 1.0` reproduces the original
    /// map exactly.
    pub fn generate(n: usize, span: f32, params: &NoiseParams) -> Self {
        let mut heights = vec![0.0f32; n * n];
        let inv = 1.0 / n as f32;
        // Two octaves per doubling: one to cover the ground the map gained, one to
        // spend the cells it gained on detail finer than it could hold before.
        let extra = (2.0 * span.max(1.0).log2()).round() as u32;

        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for y in 0..n {
            for x in 0..n {
                let fx = x as f32 * inv;
                let fy = y as f32 * inv;
                let h = fbm(fx, fy, params, extra);
                heights[y * n + x] = h;
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }

        // Normalize to [0, 1], then apply the ridge exponent for a flatter-base,
        // sharper-peak profile.
        let inv_range = 1.0 / (hi - lo).max(1e-6);
        for h in &mut heights {
            let t = (*h - lo) * inv_range;
            *h = t.powf(params.ridge.max(0.05));
        }

        // Drown the rim: scale the land toward zero as it approaches the edge, so
        // what is left is a continent with a sea around it. See `NoiseParams::coast`.
        let inner = params.coast.clamp(0.0, 0.999);
        if inner < 1.0 {
            let mid = (n as f32 - 1.0).max(1.0) / 2.0;
            for y in 0..n {
                for x in 0..n {
                    let (nx, ny) = ((x as f32 - mid) / mid, (y as f32 - mid) / mid);
                    let r = (nx * nx + ny * ny).sqrt();
                    let t = ((r - inner) / (1.0 - inner)).clamp(0.0, 1.0);
                    heights[y * n + x] *= 1.0 - fade(t);
                }
            }
        }

        Self { n, heights }
    }
}

// --- Fractal Perlin noise --------------------------------------------------

/// Fractional Brownian motion: octaves of Perlin noise at rising frequency and
/// falling amplitude, summed. Returns roughly `[-1, 1]` before normalization.
///
/// `extra` octaves are appended past the ones the caller asked for — the finer
/// detail a bigger map has the cells to carry. See [`Heightmap::generate`].
fn fbm(x: f32, y: f32, p: &NoiseParams, extra: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut freq = p.frequency.max(0.01);
    for o in 0..p.octaves.max(1) + extra {
        sum += amp
            * perlin(
                x * freq,
                y * freq,
                p.seed.wrapping_add(o.wrapping_mul(1013)),
            );
        freq *= p.lacunarity.max(1.0);
        amp *= p.persistence.clamp(0.0, 1.0);
    }
    sum
}

/// 2D Perlin (gradient) noise at `(x, y)`, in roughly `[-1, 1]`.
///
/// Standard construction: interpolate the dot products of pseudo-random gradient
/// vectors at the four surrounding lattice corners, using the quintic fade so the
/// result is C2-continuous (no visible grid creasing).
fn perlin(x: f32, y: f32, seed: u32) -> f32 {
    let xi = x.floor();
    let yi = y.floor();
    let xf = x - xi;
    let yf = y - yi;

    let u = fade(xf);
    let v = fade(yf);

    let n00 = grad(hash_lattice(xi, yi, seed), xf, yf);
    let n10 = grad(hash_lattice(xi + 1.0, yi, seed), xf - 1.0, yf);
    let n01 = grad(hash_lattice(xi, yi + 1.0, seed), xf, yf - 1.0);
    let n11 = grad(hash_lattice(xi + 1.0, yi + 1.0, seed), xf - 1.0, yf - 1.0);

    let x1 = lerp(n00, n10, u);
    let x2 = lerp(n01, n11, u);
    lerp(x1, x2, v)
}

/// Quintic fade `6t⁵ − 15t⁴ + 10t³` (Perlin's improved interpolant).
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Dot of the gradient selected by `hash` with the offset `(x, y)`. The eight
/// gradients point at the edge/diagonal midpoints — Perlin's reduced set, which
/// avoids the directional bias of fully random gradients.
fn grad(hash: u32, x: f32, y: f32) -> f32 {
    match hash & 7 {
        0 => x + y,
        1 => x - y,
        2 => -x + y,
        3 => -x - y,
        4 => x,
        5 => -x,
        6 => y,
        _ => -y,
    }
}

/// Hash integer lattice coordinates + a seed into a `u32` (used to pick a
/// gradient). Deterministic and cheap; no permutation table needed.
fn hash_lattice(x: f32, y: f32, seed: u32) -> u32 {
    let mut h = seed
        .wrapping_add((x as i32 as u32).wrapping_mul(374761393))
        .wrapping_add((y as i32 as u32).wrapping_mul(668265263));
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^ (h >> 16)
}
