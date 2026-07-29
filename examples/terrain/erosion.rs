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
//! Iterate. More passes = a more deeply incised, mature landscape.
//!
//! ## The water
//!
//! Steps 1 and 2 compute, every single pass, exactly the two fields you need to
//! *draw* water — and this module used to throw both away when the pass ended:
//!
//! - **Lakes** are the Priority-Flood's own fill. `filled - z` is how far the
//!   flood had to raise a cell to make it drain, which is zero wherever the
//!   terrain already drains and positive inside every depression. That is a lake,
//!   and its depth, for free.
//! - **Rivers** are drainage area. `area` is the number of cells draining through
//!   this one, so thresholding it *is* the river network — the same quantity the
//!   incision is proportional to, which is why the rivers you see are exactly the
//!   ones doing the carving.
//!
//! Both now come back out of [`erode`] as a [`Water`], describing the terrain it
//! hands you rather than being dropped on the floor.
//!
//! Drawing them needed one change to the model itself: **lakes have to survive the
//! pass that finds them**. The implicit update raises a pit toward the rim it was
//! breached from, so a single pass used to pack every depression with rock — the
//! reason this demo had no lakes to draw. Skipping submerged cells fixes that and
//! is the honest reading anyway (no stream over a lake bed, so no incision), but
//! on its own it stalls the whole model: a fifth of the map ends up underwater and
//! inert. [`ErosionParams::deposition`] is the term that resolves it.
//!
//! Like the rest of the demo this lives **entirely in the consumer** (roadmap
//! principle 3): the engine never sees a heightmap, only the mesh built from one.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Tunable erosion parameters — the knobs the UI exposes.
#[derive(Debug, Clone, Copy)]
pub struct ErosionParams {
    /// Number of erosion timesteps. The headline "how eroded" control: more
    /// passes cut deeper, more mature valley networks.
    ///
    /// It also decides how much **water** survives, which is worth knowing before
    /// reaching for it. Lakes silt up as the landscape matures (see
    /// [`deposition`](Self::deposition)), so the default 60 leaves a landscape with
    /// both lakes and rivers in it; push much past 100 and the lakes are gone and
    /// only the river network is left.
    pub iterations: u32,
    /// Fluvial erodibility `K` (folds in the timestep): how strongly rivers pull
    /// the terrain down toward their outlet each step.
    pub erodibility: f32,
    /// Drainage-area exponent `m` in the stream power `K·Aᵐ` (geomorphology uses
    /// ~0.4–0.6). Higher values concentrate erosion into the big rivers.
    pub m: f32,

    /// Fraction of a lake's depth filled with sediment each pass — **siltation**.
    ///
    /// This term was added because leaving it out was measurably wrong, not for
    /// looks. Preserving depressions (so lakes exist at all) means submerged cells
    /// never incise — and with a fifth of the map underwater and inert, the whole
    /// model nearly stops: sixty passes moved the mean height 2.5% and the terrain
    /// read as flooded rather than eroded.
    ///
    /// Rivers carry sediment and drop it where they slow down, which is exactly
    /// where they meet standing water. So lakes silt up, spill over, and their
    /// basins join the network — *drainage integration*, the mechanism Cordonnier
    /// et al. build their whole model around, and the reason a mature landscape has
    /// long dendritic trunks rather than a thousand ponds. Turning this up buries
    /// the lakes fast; turning it to zero freezes them and stalls the erosion,
    /// which is worth doing once just to see the difference.
    pub deposition: f32,

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
            deposition: 0.05,
            // Off by default: a strong thermal pass rounds the dendritic detail
            // back into blobs. It's available as a finishing touch.
            thermal: false,
            talus: 1.5,
            thermal_rate: 0.3,
        }
    }
}

/// The standing water and river network riding on the terrain — the byproduct of
/// flow routing that the model has always computed and now hands back to be drawn.
///
/// Both fields are `n × n`, row-major, indexed exactly like the heightmap.
#[derive(Debug, Clone, Default)]
pub struct Water {
    /// Standing-water depth per cell: `filled - z`, how far the Priority-Flood had
    /// to raise this cell to give it somewhere to drain. Zero wherever the terrain
    /// drains freely; positive inside every depression, where it *is* the lake.
    pub depth: Vec<f32>,
    /// Drainage area per cell: how many cells' water passes through this one,
    /// itself included. `1.0` on a ridge top, thousands in a trunk river — so a
    /// threshold on this is the river network.
    pub area: Vec<f32>,
}

impl Water {
    /// A dry `count`-cell grid — what a degenerate (`n < 3`) grid reports.
    fn empty(count: usize) -> Self {
        Self {
            depth: vec![0.0; count],
            area: vec![1.0; count],
        }
    }
}

/// Erode `heights` (an `n × n` grid, modified in place) under `params`, and hand
/// back the [`Water`] left standing on the result.
///
/// The returned water is the *last* pass's — the lakes and rivers belonging to the
/// terrain you get back, which is exactly what the caller wants to draw. Earlier
/// passes' water is intermediate state and nobody sees it.
pub fn erode(heights: &mut [f32], n: usize, params: &ErosionParams) -> Water {
    if n < 3 {
        return Water::empty(heights.len());
    }
    let mut water = None;
    for _ in 0..params.iterations {
        water = Some(step(heights, n, params));
    }
    // Zero passes still has water on it: raw Perlin noise is full of depressions,
    // and they hold just as much water for never having been eroded.
    water.unwrap_or_else(|| analyze(heights, n).1)
}

/// Advance `heights` by **one** timestep of flow-routed stream-power incision,
/// siltation, and optional thermal relaxation, returning the [`Water`] that drove
/// it. See the module docs for the model.
fn step(heights: &mut [f32], n: usize, params: &ErosionParams) -> Water {
    if n < 3 {
        return Water::empty(heights.len());
    }
    let (flow, water) = analyze(heights, n);

    // Implicit stream-power incision, downstream-first so each receiver is
    // already at its new height when we solve the cell above it:
    //   z'[c] = (z[c] + f·z'[r]) / (1 + f),   f = K·Aᵐ / L.
    for &c in &flow.order {
        let r = flow.receiver[c];
        if r == c {
            continue; // outlet / fixed base level
        }
        // Two cells are left alone, and the reason is the same for both: they are
        // under water, or they are being asked to flow *uphill* through a breach.
        //
        // `receiver` is downhill on the **filled** surface, which inside a basin
        // means uphill in the real terrain — the flood reaches a pit from the rim
        // above it. Applying the implicit update there does not incise the pit, it
        // *raises* it toward the rim: one pass and every depression on the map has
        // been packed with rock and can never hold water again. That is why this
        // demo had no lakes to draw. Skipping those cells keeps depressions, and
        // "no fluvial incision under standing water" is the honest reading anyway
        // — a lake bed has no stream running over it to cut with.
        //
        // What still erodes is the lake's *outlet*, which is dry and drains
        // downhill like anything else — so spillways cut down over the run and
        // lakes partly drain themselves, without that being put in by hand.
        if water.depth[c] > MIN_POND || heights[r] >= heights[c] {
            continue;
        }
        let f = params.erodibility * water.area[c].powf(params.m) / flow.dist[c];
        let z = (heights[c] + f * heights[r]) / (1.0 + f);
        // Never incise below the receiver (keeps slopes downhill).
        heights[c] = z.max(heights[r]);
    }

    // Siltation: rivers drop their load where they meet standing water, so every
    // lake loses a fraction of its depth to sediment each pass. Lakes shrink from
    // the shallows inward, spill, and hand their basin to the drainage network.
    // See [`ErosionParams::deposition`] for why this term is load-bearing.
    let fill = params.deposition.clamp(0.0, 1.0);
    if fill > 0.0 {
        for (h, &d) in heights.iter_mut().zip(&water.depth) {
            if d > MIN_POND {
                *h += fill * d;
            }
        }
    }

    if params.thermal {
        thermal_sweep(heights, n, params);
    }

    water
}

/// The shared first half of a pass: route the flow, accumulate drainage area, and
/// read the standing water off the filled surface.
fn analyze(z: &[f32], n: usize) -> (Flow, Water) {
    let flow = flow_route(z, n);

    // Drainage area: every cell contributes one unit of area to itself, then
    // pushes its total down to its receiver (reverse topological order).
    let mut area = vec![1.0f32; z.len()];
    for &c in flow.order.iter().rev() {
        let r = flow.receiver[c];
        if r != c {
            area[r] += area[c];
        }
    }

    // How far the flood had to lift each cell to drain it — zero almost
    // everywhere, and a lake wherever it isn't.
    let depth = flow
        .filled
        .iter()
        .zip(z)
        .map(|(f, h)| (f - h).max(0.0))
        .collect();

    (flow, Water { depth, area })
}

/// Depth below which "standing water" is really just accumulated flood ε.
///
/// The Priority-Flood raises each cell a hair above the one it was reached from,
/// so a long dead-flat run picks up a nonzero depth that is an artifact of the
/// algorithm and not a pond. Heights are normalized to `[0, 1]`, so this is a
/// thousandth of the terrain's full relief — far below anything visible, and a
/// thousand cells of ε before a false positive.
pub const MIN_POND: f32 = 1.0e-3;

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
    /// The **filled** surface: the terrain with every depression flooded to its
    /// spill point. This used to be a local the flood dropped on the way out; it
    /// is kept now because `filled - z` is the standing water (see [`Water`]).
    filled: Vec<f32>,
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
        filled,
    }
}
