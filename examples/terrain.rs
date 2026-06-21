//! The terrain vertical, rebuilt as **layered, iterative** terrain generation.
//!
//! Rather than one monolithic solver, the terrain is composed in clear layers,
//! each its own module and each independently visible:
//!
//! 1. **Base shape** — a fractal Perlin-noise heightmap ([`heightmap`]).
//! 2. **Erosion** — iterative hydro-thermal erosion carved on top
//!    ([`erosion`]), the layer that turns noise into something terrain-like.
//!
//! This is the payoff demo for the project's thesis: *a developer writes their
//! algorithm and a few engine calls, and never touches `wgpu`/`winit`.*
//! Everything physical here — the noise, the erosion, the shading — lives in this
//! consumer crate (it can only see `slmsttaa`'s public API). The engine just:
//!
//! - uploads the mesh we build ([`Renderer::set_meshes`]),
//! - draws it solid or as a wireframe on demand ([`Renderer::set_render_mode`]),
//! - lets us drive the orbit camera ([`Renderer::camera_mut`] + [`Renderer::input`]),
//! - draws our parameter panel and HUD ([`Renderer::ui`]), and
//! - hands us a frame delta ([`Renderer::dt`]) for the FPS readout.
//!
//! Controls: **drag the left mouse button** over the 3D view to orbit, **scroll**
//! to zoom, arrow keys also orbit. The panel on the left edits every parameter
//! live; moving a slider regenerates the terrain. Toggle **wireframe** to inspect
//! the underlying grid.
//!
//! Run it:
//!   native — `cargo run --example terrain`
//!   web    — `cargo xtask serve terrain`, then open the printed URL.

use slmsttaa::{run, Application, Key, Mesh, MouseButton, RenderMode, Renderer, Vertex};

#[path = "terrain/erosion.rs"]
mod erosion;
#[path = "terrain/heightmap.rs"]
mod heightmap;

use erosion::ErosionParams;
use heightmap::{Heightmap, NoiseParams};

/// Half-extent of the rendered terrain in world units (spans `[-HALF, HALF]`).
const HALF: f32 = 2.5;
/// Vertical scale: normalized `[0, 1]` heights map into `[0, VHEIGHT]` world units.
const VHEIGHT: f32 = 1.3;
/// Grid resolution bounds (cells per side); snapped to a multiple of 8.
const RES_MIN: f32 = 32.0;
const RES_MAX: f32 = 256.0;

/// The terrain consumer: owns the layer parameters, the heightmaps, and the
/// orbit-camera state.
struct TerrainDemo {
    /// Layer 1 (base shape) parameters.
    params: NoiseParams,
    /// Layer 2 (erosion) parameters.
    erosion: ErosionParams,
    /// The Perlin base heightmap, before erosion. Kept so the erosion sliders can
    /// re-erode without re-running the (separate) noise generation.
    base: Vec<f32>,
    /// The eroded heights actually rendered (`n * n`).
    heights: Vec<f32>,
    /// Grid side length.
    n: usize,
    /// Resolution slider value, snapped to a multiple of 8.
    res: f32,

    /// Draw the terrain as a wireframe instead of shaded triangles.
    wireframe: bool,

    /// Deferred-rebuild flags. Erosion costs ~100ms, so rather than recompute on
    /// every slider tick we mark what changed and apply it once the drag ends (the
    /// mouse button is released). `base` implies a full noise regen + re-erode;
    /// `erode` re-runs only the erosion layer on the cached base.
    pending_base: bool,
    pending_erode: bool,

    /// Orbit camera state (azimuth, elevation, range).
    yaw: f32,
    pitch: f32,
    distance: f32,

    /// Smoothed frames-per-second for the HUD.
    fps: f32,
}

impl TerrainDemo {
    fn new() -> Self {
        let n = 128usize;
        let params = NoiseParams::default();
        let erosion = ErosionParams::default();
        let hm = Heightmap::generate(n, &params);
        let mut demo = Self {
            params,
            erosion,
            base: hm.heights,
            heights: Vec::new(),
            n: hm.n,
            res: n as f32,
            wireframe: false,
            pending_base: false,
            pending_erode: false,
            yaw: 0.7,
            pitch: 0.62,
            distance: 6.5,
            fps: 60.0,
        };
        demo.apply_erosion();
        demo
    }

    /// Regenerate the Perlin base heightmap (layer 1) at the current parameters
    /// and resolution, then re-erode it. Called when a noise/grid control changes.
    fn regenerate_base(&mut self) {
        let hm = Heightmap::generate(self.n, &self.params);
        self.n = hm.n;
        self.base = hm.heights;
        self.apply_erosion();
    }

    /// Re-run the erosion layer (layer 2) on the cached base heightmap. Called when
    /// an erosion control changes — no need to regenerate the noise.
    fn apply_erosion(&mut self) {
        self.heights = self.base.clone();
        erosion::erode(&mut self.heights, self.n, &self.erosion);
    }

    /// Build the renderable mesh from the current heights: an `n × n` grid with a
    /// height/slope color palette and CPU-baked diffuse shading folded into the
    /// vertex color (the engine's pipeline is position+color only — lighting stays
    /// in the demo, KISS).
    fn build_mesh(&self) -> Mesh {
        let n = self.n;
        // Renormalize to [0, 1] before display: erosion shifts the overall range,
        // and this keeps the terrain framed and the palette stops meaningful.
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for &h in &self.heights {
            lo = lo.min(h);
            hi = hi.max(h);
        }
        let inv_range = 1.0 / (hi - lo).max(1e-6);
        // Displayed height per cell, in world units.
        let disp = |i: usize| (self.heights[i] - lo) * inv_range * VHEIGHT;

        let step = (2.0 * HALF) / (n as f32 - 1.0);
        let cell_world = step; // horizontal spacing for slope/normal estimates

        let mut vertices = Vec::with_capacity(n * n);
        let light = normalize3([0.45, 0.85, 0.35]);
        for y in 0..n {
            for x in 0..n {
                let i = y * n + x;
                let wx = -HALF + x as f32 * step;
                let wz = -HALF + y as f32 * step;
                let wy = disp(i);

                // Central-difference normal from displayed heights.
                let hl = disp(y * n + x.saturating_sub(1));
                let hr = disp(y * n + (x + 1).min(n - 1));
                let hd = disp(y.saturating_sub(1) * n + x);
                let hu = disp((y + 1).min(n - 1) * n + x);
                let normal = normalize3([
                    (hl - hr) / (2.0 * cell_world),
                    1.0,
                    (hd - hu) / (2.0 * cell_world),
                ]);
                let slope = 1.0 - normal[1].clamp(0.0, 1.0); // 0 flat → 1 vertical

                let t = (wy / VHEIGHT).clamp(0.0, 1.0);
                let base = palette(t, slope);

                // Simple diffuse + ambient, baked into the color.
                let diffuse = dot3(normal, light).clamp(0.0, 1.0);
                let shade = 0.35 + 0.65 * diffuse;
                let color = [base[0] * shade, base[1] * shade, base[2] * shade];

                vertices.push(Vertex {
                    position: [wx, wy, wz],
                    color,
                });
            }
        }

        // Two CCW triangles per cell (seen from +Y).
        let mut indices = Vec::with_capacity((n - 1) * (n - 1) * 6);
        let idx = |x: usize, y: usize| (y * n + x) as u32;
        for y in 0..n - 1 {
            for x in 0..n - 1 {
                let a = idx(x, y);
                let b = idx(x + 1, y);
                let c = idx(x + 1, y + 1);
                let d = idx(x, y + 1);
                indices.extend_from_slice(&[a, d, b, b, d, c]);
            }
        }
        Mesh::new(vertices, indices)
    }

    /// Lay out the parameter panel and HUD. Returns `(regen_base, reerode,
    /// wants_pointer)`: whether a base-shape/grid control changed (so we
    /// regenerate the noise *and* re-erode), whether only an erosion control
    /// changed (so we just re-erode the cached base), and whether the pointer is
    /// over the panel (so the camera ignores the drag).
    fn build_ui(&mut self, renderer: &mut Renderer) -> (bool, bool, bool) {
        let fps = self.fps;
        let n = self.n;

        let pending = self.pending_base || self.pending_erode;

        let mut ui = renderer.ui();
        ui.title("Terrain");
        ui.label_muted(&format!("{fps:.0} fps   {n}x{n} grid"));
        if pending {
            ui.label_muted("release to rebuild...");
        }
        ui.checkbox("wireframe", &mut self.wireframe);
        ui.separator();

        // --- Layer 1: the Perlin base shape ---
        ui.section("Base shape (Perlin)");
        let mut base = false;
        base |= ui.slider_fmt("frequency", &mut self.params.frequency, 0.5, 8.0, 2);
        let mut octaves = self.params.octaves as f32;
        if ui.slider_fmt("octaves", &mut octaves, 1.0, 8.0, 0) {
            self.params.octaves = octaves.round() as u32;
            base = true;
        }
        base |= ui.slider_fmt("lacunarity", &mut self.params.lacunarity, 1.5, 3.0, 2);
        base |= ui.slider_fmt("persistence", &mut self.params.persistence, 0.2, 0.8, 2);
        base |= ui.slider_fmt("ridge (peaks)", &mut self.params.ridge, 0.5, 3.0, 2);
        ui.separator();

        // --- Layer 2: erosion ---
        let mut erode = false;
        ui.section("Fluvial erosion (rivers)");
        let mut iters = self.erosion.iterations as f32;
        if ui.slider_fmt("passes", &mut iters, 0.0, 120.0, 0) {
            self.erosion.iterations = iters.round() as u32;
            erode = true;
        }
        erode |= ui.slider_fmt("erodibility", &mut self.erosion.erodibility, 0.0, 0.006, 4);
        erode |= ui.slider_fmt("area exponent m", &mut self.erosion.m, 0.2, 1.0, 2);

        ui.section("Thermal erosion");
        erode |= ui.checkbox("enable talus", &mut self.erosion.thermal);
        if self.erosion.thermal {
            erode |= ui.slider_fmt("  talus (slope)", &mut self.erosion.talus, 0.3, 4.0, 2);
            erode |= ui.slider_fmt("  rate", &mut self.erosion.thermal_rate, 0.0, 0.5, 2);
        }
        ui.separator();

        // --- Grid ---
        ui.section("Grid");
        ui.slider_fmt("resolution", &mut self.res, RES_MIN, RES_MAX, 0);
        let new_seed = ui.button("new seed");

        let wants_pointer = ui.wants_pointer();
        drop(ui);

        if new_seed {
            self.params.seed = self.params.seed.wrapping_add(1);
            base = true;
        }
        (base, erode, wants_pointer)
    }

    /// Orbit the camera from input (unless the pointer is over the UI panel).
    fn drive_camera(&mut self, renderer: &mut Renderer, wants_pointer: bool) {
        let input = renderer.input();
        let dragging = input.is_mouse_held(MouseButton::Left) && !wants_pointer;
        let (mdx, mdy) = input.mouse_delta();
        let scroll = if wants_pointer {
            0.0
        } else {
            input.scroll_delta()
        };
        let (left, right, up, down) = (
            input.is_key_held(Key::Left),
            input.is_key_held(Key::Right),
            input.is_key_held(Key::Up),
            input.is_key_held(Key::Down),
        );

        if dragging {
            self.yaw -= mdx * 0.005;
            self.pitch -= mdy * 0.005;
        }
        const KEY_STEP: f32 = 0.03;
        if left {
            self.yaw += KEY_STEP;
        }
        if right {
            self.yaw -= KEY_STEP;
        }
        if up {
            self.pitch += KEY_STEP;
        }
        if down {
            self.pitch -= KEY_STEP;
        }
        self.distance -= scroll * 0.5;
        self.pitch = self.pitch.clamp(0.08, 1.5);
        self.distance = self.distance.clamp(2.5, 18.0);

        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        let eye = [
            self.distance * cp * sy,
            self.distance * sp,
            self.distance * cp * cy,
        ];
        // Aim slightly above the base so the framed terrain sits centered.
        renderer
            .camera_mut()
            .look_from_to(eye, [0.0, VHEIGHT * 0.35, 0.0]);
    }
}

impl Application for TerrainDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        let mesh = self.build_mesh();
        renderer.set_meshes(&[mesh]);
    }

    fn update(&mut self, renderer: &mut Renderer) {
        // Smooth the FPS readout (exponential moving average).
        let dt = renderer.dt();
        if dt > 0.0 {
            self.fps = self.fps * 0.9 + (1.0 / dt) * 0.1;
        }

        let (regen_base, reerode, wants_pointer) = self.build_ui(renderer);
        self.pending_base |= regen_base;
        self.pending_erode |= reerode;

        // Snap the resolution slider; a resolution change needs a full rebuild.
        let target_n = ((self.res / 8.0).round() as usize * 8).clamp(32, 256);
        self.res = target_n as f32;
        if target_n != self.n {
            self.pending_base = true;
        }

        // Debounce: erosion costs ~100ms, so apply a pending rebuild only once the
        // user finishes dragging (left button up). A base/grid change regenerates
        // the noise and re-erodes; an erosion-only change re-runs just the erosion
        // layer on the cached base.
        let dragging = renderer.input().is_mouse_held(MouseButton::Left);
        if !dragging && (self.pending_base || self.pending_erode) {
            if self.pending_base {
                self.n = target_n;
                self.regenerate_base();
            } else {
                self.apply_erosion();
            }
            renderer.set_meshes(&[self.build_mesh()]);
            self.pending_base = false;
            self.pending_erode = false;
        }

        renderer.set_render_mode(if self.wireframe {
            RenderMode::Wireframe
        } else {
            RenderMode::Solid
        });

        self.drive_camera(renderer, wants_pointer);
    }
}

/// Height/slope color palette: green lowlands → tan slopes → gray rock → snow,
/// with steep faces biased toward bare rock regardless of altitude.
fn palette(t: f32, slope: f32) -> [f32; 3] {
    // Altitude stops.
    let stops = [
        (0.00, [0.20, 0.42, 0.24]), // valley green
        (0.30, [0.34, 0.50, 0.26]), // meadow
        (0.55, [0.52, 0.45, 0.32]), // tan slope
        (0.75, [0.48, 0.46, 0.45]), // rock
        (0.92, [0.92, 0.93, 0.96]), // snow
    ];
    let mut color = stops[stops.len() - 1].1;
    for w in stops.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            color = [
                c0[0] + (c1[0] - c0[0]) * f,
                c0[1] + (c1[1] - c0[1]) * f,
                c0[2] + (c1[2] - c0[2]) * f,
            ];
            break;
        }
    }
    // Blend toward bare rock on steep faces.
    let rock = [0.40, 0.38, 0.36];
    let s = (slope * 1.6).clamp(0.0, 0.7);
    [
        color[0] + (rock[0] - color[0]) * s,
        color[1] + (rock[1] - color[1]) * s,
        color[2] + (rock[2] - color[2]) * s,
    ]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / len, v[1] / len, v[2] / len]
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(err) = run(TerrainDemo::new()) {
        eprintln!("terrain example exited with an error: {err}");
        std::process::exit(1);
    }
}

/// WASM entry point. `wasm-bindgen` calls this once the module is instantiated.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    let _ = run(TerrainDemo::new());
}

#[cfg(target_arch = "wasm32")]
fn main() {}
