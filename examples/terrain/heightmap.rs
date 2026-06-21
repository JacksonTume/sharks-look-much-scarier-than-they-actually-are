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
    /// unit square, then normalizing the result to `[0, 1]`.
    pub fn generate(n: usize, params: &NoiseParams) -> Self {
        let mut heights = vec![0.0f32; n * n];
        let inv = 1.0 / n as f32;

        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for y in 0..n {
            for x in 0..n {
                let fx = x as f32 * inv;
                let fy = y as f32 * inv;
                let h = fbm(fx, fy, params);
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

        Self { n, heights }
    }
}

// --- Fractal Perlin noise --------------------------------------------------

/// Fractional Brownian motion: octaves of Perlin noise at rising frequency and
/// falling amplitude, summed. Returns roughly `[-1, 1]` before normalization.
fn fbm(x: f32, y: f32, p: &NoiseParams) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut freq = p.frequency.max(0.01);
    for o in 0..p.octaves.max(1) {
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
