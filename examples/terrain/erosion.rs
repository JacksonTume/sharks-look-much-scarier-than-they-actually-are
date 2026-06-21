//! Layer 2 of the terrain vertical: iterative **stream-power fluvial erosion**
//! plus **thermal relaxation** — a small landscape-evolution model (LEM) carved
//! on top of the Perlin base shape ([`super::heightmap`]).
//!
//! ## Why this model (and not droplets)
//!
//! The branching, tree-like ("dendritic") valley networks that make eroded
//! mountains read as *real* come from **flow accumulation**: water from every
//! cell is routed to its lowest neighbor, and *drainage area* `A` accumulates
//! down that network so trunk rivers carry far more water than their tributaries.
//! Erosion driven by that area cuts deep shared valleys while the low-area ridges
//! between them are barely touched — which is exactly the look. Independent
//! water *droplets* can't reproduce it: they each carve in isolation and never
//! pool into a connected network.
//!
//! Both of the project's terrain references are built on this — Cordonnier et al.
//! 2016 (`reference/2016_cordonnier.pdf`) and the analytical Tzathas et al. 2024
//! (`reference/Analytical_Terrains_EG.pdf`). The grid pipe-model of Mei et al.
//! 2007 (`reference/download.pdf`) is an alternative *hydraulic* scheme (rainfall
//! on an existing terrain); it is great for surface detail but does not build
//! ranges, so it isn't what we use here.
//!
//! ## The algorithm (one timestep)
//!
//! 1. **Flow routing** — a Priority-Flood (Barnes 2014) over the 8-neighborhood
//!    assigns every cell a downstream *receiver* even across pits (depressions are
//!    filled with an ε slope so nothing dead-ends), and yields a downstream-first
//!    processing order.
//! 2. **Drainage area** — accumulate cell areas up the receiver tree (each cell
//!    adds its area to its receiver), processing the order in reverse.
//! 3. **Stream-power incision** — pull each cell toward its receiver by the stream
//!    power `K·Aᵐ`, using the unconditionally stable *implicit* update of
//!    Braun & Willett 2013 (FastScape). Processing downstream-first means a cell's
//!    receiver is already updated, so the linear-time implicit solve is exact.
//! 4. **Thermal relaxation** — shed any slope above the talus angle to its lower
//!    neighbors (Musgrave 1989), rounding the spiky divides into natural ridges.
//!
//! Iterate. More iterations = a more deeply incised, mature landscape.
//!
//! Like the rest of the demo this lives **entirely in the consumer** (roadmap
//! principle 3): the engine never sees a heightmap, only the mesh built from one.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Tunable erosion parameters — the knobs the UI exposes.
#[derive(Debug, Clone, Copy)]
pub struct ErosionParams {
    /// Number of erosion timesteps. The headline "how eroded" control: more
    /// iterations cut deeper, more mature valley networks.
    pub iterations: u32,
    /// Fluvial erodibility `K` (folds in the timestep): how strongly rivers pull
    /// the terrain down toward their outlet each step.
    pub erodibility: f32,
    /// Drainage-area exponent `m` in the stream power `K·Aᵐ` (geomorphology uses
    /// ~0.4–0.6). Higher values concentrate erosion into the big rivers.
    pub m: f32,

    /// Whether the thermal (talus relaxation) pass runs each timestep.
    pub thermal: bool,
    /// Critical slope (talus angle, rise/run) above which material slides.
    pub talus: f32,
    /// Fraction of the over-talus excess moved per sweep (kept ≤ 0.5 for
    /// stability).
    pub thermal_rate: f32,
}

impl Default for ErosionParams {
    fn default() -> Self {
        Self {
            iterations: 60,
            erodibility: 0.004,
            m: 0.5,
            // Off by default: a strong thermal pass rounds the dendritic detail
            // back into blobs. It's available as a finishing touch.
            thermal: false,
            talus: 1.5,
            thermal_rate: 0.3,
        }
    }
}

/// Erode `heights` (an `n × n` grid, modified in place) under `params`: a small
/// landscape-evolution loop of flow-routed stream-power incision plus thermal
/// relaxation. See the module docs for the model.
pub fn erode(heights: &mut [f32], n: usize, params: &ErosionParams) {
    if n < 3 {
        return;
    }
    let count = n * n;
    let mut area = vec![0.0f32; count];

    for _ in 0..params.iterations {
        let flow = flow_route(heights, n);

        // Drainage area: every cell contributes one unit of area to itself, then
        // pushes its total down to its receiver (reverse topological order).
        area.iter_mut().for_each(|a| *a = 1.0);
        for &c in flow.order.iter().rev() {
            let r = flow.receiver[c];
            if r != c {
                area[r] += area[c];
            }
        }

        // Implicit stream-power incision, downstream-first so each receiver is
        // already at its new height when we solve the cell above it:
        //   z'[c] = (z[c] + f·z'[r]) / (1 + f),   f = K·Aᵐ / L.
        for &c in &flow.order {
            let r = flow.receiver[c];
            if r == c {
                continue; // outlet / fixed base level
            }
            let f = params.erodibility * area[c].powf(params.m) / flow.dist[c];
            let z = (heights[c] + f * heights[r]) / (1.0 + f);
            // Never incise below the receiver (keeps slopes downhill).
            heights[c] = z.max(heights[r]);
        }

        if params.thermal {
            thermal_sweep(heights, n, params);
        }
    }
}

// --- Thermal erosion -------------------------------------------------------

/// One talus-relaxation sweep: move a fraction of each over-critical-slope excess
/// from a cell to its lower 4-neighbors. Double-buffered through a delta grid so
/// the whole sweep reads one consistent state.
fn thermal_sweep(h: &mut [f32], n: usize, p: &ErosionParams) {
    let dx = 1.0 / n as f32;
    let talus = (p.talus * dx).max(0.0);
    let rate = p.thermal_rate.clamp(0.0, 0.5);
    let mut delta = vec![0.0f32; h.len()];
    const NEIGHBORS: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

    for y in 0..n {
        for x in 0..n {
            let c = y * n + x;
            let hc = h[c];
            for (ox, oy) in NEIGHBORS {
                let nx = x as isize + ox;
                let ny = y as isize + oy;
                if nx < 0 || ny < 0 || nx >= n as isize || ny >= n as isize {
                    continue;
                }
                let nb = ny as usize * n + nx as usize;
                let diff = hc - h[nb];
                if diff > talus {
                    // Only the higher cell of a pair sees `diff > talus`, so each
                    // transfer is counted once; the 0.5 keeps it gentle.
                    let m = 0.5 * rate * (diff - talus);
                    delta[c] -= m;
                    delta[nb] += m;
                }
            }
        }
    }
    for (hi, d) in h.iter_mut().zip(&delta) {
        *hi += *d;
    }
}

// --- Flow routing (Priority-Flood) -----------------------------------------

/// Per-cell downstream receiver, a downstream-first processing order, and the
/// distance from each cell to its receiver.
struct Flow {
    /// Downstream receiver per cell; boundary/outlet cells receive themselves.
    receiver: Vec<usize>,
    /// Cells by increasing filled elevation (outlets first) — a valid topological
    /// order over the receiver forest.
    order: Vec<usize>,
    /// Distance from each cell to its receiver (1 orthogonal, √2 diagonal).
    dist: Vec<f32>,
}

/// A min-heap node ordered by (filled) elevation, with a deterministic index
/// tie-break so the flood is reproducible.
#[derive(PartialEq)]
struct HeapNode {
    elev: f32,
    idx: u32,
}
impl Eq for HeapNode {}
impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed so `BinaryHeap` (a max-heap) pops the *lowest* elevation.
        other
            .elev
            .total_cmp(&self.elev)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}
impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// D8 neighbor offsets paired with their step distance.
const D8: [(isize, isize, f32); 8] = [
    (-1, 0, 1.0),
    (1, 0, 1.0),
    (0, -1, 1.0),
    (0, 1, 1.0),
    (-1, -1, std::f32::consts::SQRT_2),
    (1, -1, std::f32::consts::SQRT_2),
    (-1, 1, std::f32::consts::SQRT_2),
    (1, 1, std::f32::consts::SQRT_2),
];

/// Route flow with a Priority-Flood + ε (Barnes 2014): grow inward from the
/// boundary outlets in elevation order, carving an ε-downhill path out of every
/// depression so the whole grid drains. Each cell's receiver is the already-
/// processed (lower, on the filled surface) neighbor it was reached from.
fn flow_route(z: &[f32], n: usize) -> Flow {
    let count = n * n;
    let mut receiver = vec![usize::MAX; count];
    let mut dist = vec![1.0f32; count];
    let mut filled = vec![0.0f32; count];
    let mut visited = vec![false; count];
    let mut order = Vec::with_capacity(count);
    let mut heap = BinaryHeap::new();

    // Seed every boundary cell as an outlet (drains to itself, base level).
    for y in 0..n {
        for x in 0..n {
            if x == 0 || y == 0 || x == n - 1 || y == n - 1 {
                let c = y * n + x;
                receiver[c] = c;
                filled[c] = z[c];
                visited[c] = true;
                heap.push(HeapNode {
                    elev: z[c],
                    idx: c as u32,
                });
            }
        }
    }

    // A tiny increment so breached paths slope strictly downhill (no flat pits).
    let epsilon = 1e-6;

    while let Some(node) = heap.pop() {
        let c = node.idx as usize;
        order.push(c);
        let cx = (c % n) as isize;
        let cy = (c / n) as isize;
        for (ox, oy, step) in D8 {
            let nx = cx + ox;
            let ny = cy + oy;
            if nx < 0 || ny < 0 || nx >= n as isize || ny >= n as isize {
                continue;
            }
            let nb = ny as usize * n + nx as usize;
            if visited[nb] {
                continue;
            }
            visited[nb] = true;
            receiver[nb] = c;
            dist[nb] = step;
            // Fill/breach: the neighbor sits at least ε above its receiver,
            // guaranteeing a downhill route even across a basin.
            filled[nb] = z[nb].max(filled[c] + epsilon);
            heap.push(HeapNode {
                elev: filled[nb],
                idx: nb as u32,
            });
        }
    }

    Flow {
        receiver,
        order,
        dist,
    }
}
