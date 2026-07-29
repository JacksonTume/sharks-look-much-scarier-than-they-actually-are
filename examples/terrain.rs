//! The terrain vertical, rebuilt as **layered, iterative** terrain generation.
//!
//! Rather than one monolithic solver, the terrain is composed in clear layers,
//! each its own module and each independently visible:
//!
//! 1. **Base shape** — a fractal Perlin-noise heightmap ([`heightmap`]).
//! 2. **Erosion** — iterative hydro-thermal erosion carved on top
//!    ([`erosion`]), the layer that turns noise into something terrain-like.
//! 3. **Water** — the lakes and rivers the erosion leaves behind, drawn as a
//!    second mesh beside the terrain ([`erosion::Water`]).
//!
//! The water costs nothing to compute: flow routing already floods every
//! depression (so `filled - z` is a lake and its depth) and already accumulates
//! drainage area (so a threshold on it is the river network). Both used to be
//! discarded at the end of each pass. Now they come back out of the erosion and
//! get drawn, opaque, on a second mesh.
//!
//! This is the payoff demo for the project's thesis: *a developer writes their
//! algorithm and a few engine calls, and never touches `wgpu`/`winit`.*
//! Everything physical here — the noise, the erosion, the shading — lives in this
//! consumer crate (it can only see `slmsttaa`'s public API). The engine just:
//!
//! - uploads the mesh we build ([`Renderer::upload_mesh`]),
//! - draws it solid or as a wireframe on demand ([`Renderer::set_render_mode`]),
//! - lets us drive the orbit camera ([`Renderer::camera_mut`] + [`Renderer::input`]),
//! - draws our parameter panel and HUD ([`Renderer::ui`]), and
//! - hands us a frame delta ([`Renderer::dt`]) for the FPS readout.
//!
//! Controls: **drag the left mouse button** over the 3D view to orbit, **scroll**
//! to zoom, arrow keys also orbit. The panel on the left edits every parameter
//! live; releasing a slider regenerates the terrain. *passes* doubles as a wetness
//! control — lakes silt up as the landscape matures, so a low count leaves lakes
//! and a high one leaves only rivers. Toggle **wireframe** to inspect
//! the underlying grid, **click a section heading** to collapse it, and **reset
//! all** at the bottom of the panel throws every parameter back to its default.
//!
//! The panel also carries [`log_slider`] — a widget written *here*, in the demo,
//! from the toolkit's public API alone. That it can be is the point.
//!
//! The HUD's **light** toggle swaps one [`Theme`] value and restyles the whole
//! UI — both panels, every built-in widget, and `log_slider` with them. That is
//! the demo's half of the toolkit's design-token claim: if any widget had kept a
//! hard-coded color, this is where it would stay dark.
//!
//! Run it:
//!   native — `cargo run --example terrain`
//!   web    — `cargo xtask serve terrain`, then open the printed URL.

use slmsttaa::ui::{font, Anchor, Rect, Response, Size, Theme, Ui, Variant};
use slmsttaa::{
    run, Application, Instance, Key, Mesh, MeshHandle, MouseButton, RenderMode, Renderer, Vertex,
};

#[path = "terrain/erosion.rs"]
mod erosion;
#[path = "terrain/heightmap.rs"]
mod heightmap;

use erosion::ErosionParams;
use heightmap::{Heightmap, NoiseParams};

/// A **logarithmic** slider, written here in the demo rather than in the toolkit.
///
/// This exists twice over. It is genuinely what the erosion knob needed —
/// erodibility spans four orders of magnitude, so a linear track spends 90% of
/// its length on values that all look the same — and it is the demo's proof of
/// the toolkit's *unprivileged widget* rule: nothing below uses anything a
/// consumer can't reach. [`Ui::next_id`] for identity, [`Ui::allocate`] for
/// space, [`Ui::interact`] for hit-testing and drag capture, [`Ui::painter`] to
/// draw, and [`Ui::theme`] for the tokens that make it match the widgets that
/// ship with the crate.
///
/// That last one is what Slice 4 changed here, and it is worth reading as the
/// proof it is: this widget names **no literal color and no literal metric**, so
/// it restyles with the theme exactly like a built-in does. Toggle *light* in
/// the HUD and watch this track change with everything else — nothing in the
/// crate knows this widget exists.
///
/// Slice 5 is the second time the seam was tested from out here, and it moved:
/// `text_size` left the `Painter` trait for [`font`], text takes a
/// [`Weight`](slmsttaa::ui::Weight), and a run is no longer `px` tall. That this
/// widget needed four edits and no new access is the point — a consumer's widget
/// pays the same price a built-in one does, and no more.
///
/// If this needed private access, the seam would be wrong (UI roadmap Slice 1).
fn log_slider(ui: &mut Ui, label: &str, value: &mut f32, min: f32, max: f32) -> Response {
    let theme = *ui.theme();
    let (px, weight) = theme.text.body.parts();
    let track_h = theme.control.track_h;

    let id = ui.next_id(label);
    // Sized from the line box, not from `px`: an em size is not a text height.
    let text_h = font::line_height(px);
    let row = ui.allocate([0.0, text_h + track_h + 9.0]);

    // Work in log space: the knob position is linear in log10(value).
    let (lmin, lmax) = (min.max(1e-9).log10(), max.log10());
    let span = (lmax - lmin).max(f32::EPSILON);

    // Label left, value right — measured through the same public `font::text_width`
    // the built-in slider uses, so this row lines up with the ones above it.
    let readout = format!("{value:.1e}");
    let readout_w = font::text_width(&readout, px, weight);
    let painter = ui.painter();
    painter.text(row.x, row.y, label, px, weight, theme.color.foreground);
    painter.text(
        row.max_x() - readout_w,
        row.y,
        &readout,
        px,
        weight,
        theme.color.muted,
    );

    let track_y = row.y + text_h + 2.0;
    let band = Rect::new(row.x, track_y - 6.0, row.w, track_h + 12.0);
    let mut response = ui.interact(band, id);

    if response.held {
        if let Some((cx, _)) = ui.input().cursor {
            let t = ((cx - row.x) / row.w).clamp(0.0, 1.0);
            let new_val = 10f32.powf(lmin + t * span);
            if (new_val - *value).abs() > f32::EPSILON {
                *value = new_val;
                response.changed = true;
                ui.mark_changed();
            }
        }
    }

    let t = ((value.max(1e-9).log10() - lmin) / span).clamp(0.0, 1.0);
    let knob_col = if response.held || response.hovered {
        theme.color.accent_hover
    } else {
        theme.color.foreground
    };
    // Capsule track and knob, matching the built-in slider — which the demo can
    // do because the tokens and the rounded-rect painter are both public.
    let cap = track_h * 0.5;
    let knob_w = theme.control.knob_w;
    let knob_x = (row.x + row.w * t - knob_w * 0.5).clamp(row.x, row.max_x() - knob_w);
    let painter = ui.painter();
    painter.fill_rect(
        Rect::new(row.x, track_y, row.w, track_h),
        cap,
        theme.color.surface,
    );
    painter.fill_rect(
        Rect::new(row.x, track_y, row.w * t, track_h),
        cap,
        theme.color.accent,
    );
    painter.fill_rect(
        Rect::new(knob_x, track_y - 4.0, knob_w, track_h + 8.0),
        knob_w * 0.5,
        knob_col,
    );

    response
}

/// Half-extent of the rendered terrain in world units (spans `[-HALF, HALF]`).
const HALF: f32 = 2.5;
/// Vertical scale: normalized `[0, 1]` heights map into `[0, VHEIGHT]` world units.
const VHEIGHT: f32 = 1.3;

/// Map a normalized height to world units.
///
/// **Fixed**, not refitted to the current min/max the way it used to be, because
/// there are two meshes now and they have to agree. A lake surface is a terrain
/// height plus a depth, so if the mapping were derived from the terrain's own
/// range, the water would have to be told what that range came out as — and any
/// disagreement puts the lakes through the ground. A constant scale makes them
/// agree by construction. It is also more honest: refitting the range to the
/// eroded terrain quietly cancels out the fact that erosion *lowered* it.
///
/// The base heightmap arrives normalized to `[0, 1]`, so nothing needs fitting.
fn disp(h: f32) -> f32 {
    h.clamp(0.0, 1.0) * VHEIGHT
}

/// How far the water surface floats above the terrain it covers, in world units.
/// Enough to beat depth-buffer precision, far below the ~0.04 grid spacing.
const WATER_LIFT: f32 = 0.0025;
/// Where the pass-count slider tops out.
///
/// Worth knowing while dragging it: lakes silt up as the landscape matures, so
/// this doubles as a wetness control. The default 60 leaves lakes *and* rivers;
/// past about 100 the lakes are gone and only the network remains.
const PASSES_MAX: f32 = 150.0;
/// Default drainage area (in cells) at which a channel is drawn as a river.
const RIVER_AREA_DEFAULT: f32 = 60.0;
/// How tall the panel's scrolling body is allowed to get, in UI points.
///
/// A constant rather than "whatever fits the window": a panel is anchored to a
/// corner and sized by its caller, not stretched to the viewport, so the demo
/// picks a budget that leaves the 3D view usable at the default 1280x720.
const PANEL_SCROLL_MAX: f32 = 420.0;
/// Width of the HUD panel in the opposite corner. Narrow on purpose — it holds
/// two readouts and a toggle, and a 340-point slab for that would be absurd.
///
/// Not arbitrary: `label_value` puts the label at the left edge and the value at
/// the right, so the panel has to be wider than the two together or they meet in
/// the middle. `"grid"` + `"128x128"` is 11 glyphs at 16 points = 176, and this
/// leaves a comfortable gap.
const HUD_W: f32 = 210.0;

/// Base-shape presets: `(name, frequency, octaves, ridge)`.
///
/// The row of buttons these drive is what pulled horizontal layout into
/// existence — three of them side by side is not expressible with a cursor that
/// only moves down.
///
/// The names are short because a third of the content width is 101 points: the
/// toolkit does not fit or ellipsize text (still deliberately out of scope), so
/// a caller putting a button in a column has to know what will fit in one. Slice
/// 4 bought some slack without changing that — these are drawn at [`Size::Sm`],
/// whose 13-point text fits seven glyphs where the standard 16 fits six.
const SHAPE_PRESETS: [(&str, f32, u32, f32); 3] = [
    ("hills", 2.5, 4, 1.0),
    ("alps", 3.5, 5, 1.4),
    ("peaks", 5.0, 7, 2.2),
];

/// Grid resolution bounds (cells per side); snapped to a multiple of 8.
const RES_MIN: f32 = 32.0;
const RES_MAX: f32 = 256.0;
/// The resolution the demo starts at, and the one "reset all" returns to.
const RES_DEFAULT: usize = 128;

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
    /// The water standing on `heights` — lakes and rivers, ready to draw.
    water: erosion::Water,
    /// Grid side length.
    n: usize,
    /// Resolution slider value, snapped to a multiple of 8.
    res: f32,

    /// Draw the water surface at all.
    show_water: bool,
    /// Drainage area (in cells) above which a channel is drawn as a river. The one
    /// purely *visual* water knob: it changes what counts as a river, never what
    /// the erosion does.
    river_area: f32,

    /// Draw the terrain as a wireframe instead of shaded triangles.
    wireframe: bool,
    /// Which theme the UI is drawn with, re-applied at the top of every frame.
    ///
    /// The consumer owns this, not the toolkit — immediate mode all the way
    /// down. It is one value, and swapping it restyles every widget in both
    /// panels, `log_slider` included.
    theme: Theme,

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

    /// Handles to the uploaded terrain and water meshes, claimed on the first
    /// upload and refilled in place afterwards. `None` until `init` runs.
    handles: Option<(MeshHandle, MeshHandle)>,
}

impl TerrainDemo {
    fn new() -> Self {
        let n = RES_DEFAULT;
        let params = NoiseParams::default();
        let erosion = ErosionParams::default();
        let hm = Heightmap::generate(n, &params);
        let mut demo = Self {
            params,
            erosion,
            base: hm.heights,
            heights: Vec::new(),
            water: erosion::Water::default(),
            n: hm.n,
            res: n as f32,
            show_water: true,
            river_area: RIVER_AREA_DEFAULT,
            wireframe: false,
            theme: Theme::dark(),
            pending_base: false,
            pending_erode: false,
            yaw: 0.7,
            pitch: 0.62,
            distance: 6.5,
            fps: 60.0,
            handles: None,
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

    /// Re-run the erosion layer (layer 2) on the cached base heightmap, keeping the
    /// water it leaves behind. Called when an erosion control changes — no need to
    /// regenerate the noise.
    fn apply_erosion(&mut self) {
        self.heights = self.base.clone();
        self.water = erosion::erode(&mut self.heights, self.n, &self.erosion);
    }

    /// Upload the current terrain, and the water on it, as the engine's draw-list.
    ///
    /// Two meshes rather than one: the landscape and its lakes and rivers. Both
    /// sit at the origin in the same world space, so both are identity instances —
    /// this demo is the case a transform *can't* help with. Erosion changes the
    /// terrain's shape, not its placement, so the geometry behind each handle is
    /// refilled with [`Renderer::update_mesh`] and the handles themselves never
    /// change.
    fn upload(&mut self, renderer: &mut Renderer) {
        let terrain = self.build_mesh();
        let water = self.build_water_mesh();

        let (terrain_handle, water_handle) = match self.handles {
            Some(handles) => {
                renderer.update_mesh(handles.0, &terrain);
                if let Some(water) = &water {
                    renderer.update_mesh(handles.1, water);
                }
                handles
            }
            None => {
                // First upload: claim a handle for each. The water gets one even
                // if the landscape is currently dry, so it has somewhere to go
                // when a parameter change floods a basin.
                let empty = Mesh::default();
                let handles = (
                    renderer.upload_mesh(&terrain),
                    renderer.upload_mesh(water.as_ref().unwrap_or(&empty)),
                );
                self.handles = Some(handles);
                handles
            }
        };

        let mut instances = vec![Instance::at(terrain_handle)];
        if water.is_some() {
            instances.push(Instance::at(water_handle));
        }
        renderer.set_instances(&instances);
    }

    /// Build the renderable mesh from the current heights: an `n × n` grid with a
    /// height/slope color palette and CPU-baked diffuse shading folded into the
    /// vertex color (the engine's pipeline is position+color only — lighting stays
    /// in the demo, KISS).
    fn build_mesh(&self) -> Mesh {
        let n = self.n;
        // Displayed height per cell, in world units, on the fixed scale — see
        // `disp`. Erosion lowering the terrain is now something you can *see*
        // rather than something a refitted range quietly cancels out.
        let disp = |i: usize| disp(self.heights[i]);

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

    /// Build the water surface: lakes where the flood is standing, rivers where
    /// enough drainage area has collected. `None` when there is nothing wet to
    /// draw (or the toggle is off), so the draw-list just holds the terrain.
    ///
    /// Both fields come straight out of the erosion pass — see [`erosion::Water`].
    /// Nothing here simulates anything; this function only decides what the model
    /// already computed should *look* like.
    fn build_water_mesh(&self) -> Option<Mesh> {
        let n = self.n;
        if !self.show_water || self.water.depth.len() != n * n {
            return None;
        }

        // Classify every grid point once: dry, river, or lake (and how deep).
        let wet = |i: usize| -> Option<(f32, bool)> {
            let depth = self.water.depth[i];
            if depth > erosion::MIN_POND {
                // A lake sits at the flooded surface: terrain plus its own depth.
                Some((disp(self.heights[i] + depth), true))
            } else if self.water.area[i] >= self.river_area {
                // A river is a skin on the terrain — it has no depth in this model,
                // it is just where the water is.
                Some((disp(self.heights[i]), false))
            } else {
                None
            }
        };

        let step = (2.0 * HALF) / (n as f32 - 1.0);
        // A flat surface with an up normal, lit by the same light as the terrain,
        // so water and land agree about where the sun is.
        let shade = 0.35 + 0.65 * dot3([0.0, 1.0, 0.0], normalize3([0.45, 0.85, 0.35]));

        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for y in 0..n - 1 {
            for x in 0..n - 1 {
                let corners = [(x, y), (x, y + 1), (x + 1, y + 1), (x + 1, y)];
                let states = corners.map(|(cx, cy)| wet(cy * n + cx));
                // Draw a cell if *any* corner is wet, not all four. A river is one
                // cell wide, so an all-wet test would find no complete quad along
                // it and render nothing at all — the whole network would vanish and
                // only lakes would show. Spilling onto the dry corners costs half a
                // cell of width and tucks the bank into the ground.
                if states.iter().all(Option::is_none) {
                    continue;
                }

                let base = vertices.len() as u32;
                for (k, (cx, cy)) in corners.iter().enumerate() {
                    let i = cy * n + cx;
                    let (h, lake) = match states[k] {
                        // Dry corner: sit on the terrain so the edge meets the bank.
                        None => (disp(self.heights[i]), false),
                        Some(state) => state,
                    };
                    let color = water_color(if lake { self.water.depth[i] } else { 0.0 }, shade);
                    vertices.push(Vertex {
                        position: [
                            -HALF + *cx as f32 * step,
                            h + WATER_LIFT,
                            -HALF + *cy as f32 * step,
                        ],
                        color,
                    });
                }
                // Same counter-clockwise-from-above winding as the terrain.
                indices.extend_from_slice(&[
                    base,
                    base + 1,
                    base + 3,
                    base + 3,
                    base + 1,
                    base + 2,
                ]);
            }
        }

        (!indices.is_empty()).then(|| Mesh::new(vertices, indices))
    }

    /// Lay out the parameter panel and HUD, returning what it asked for.
    fn build_ui(&mut self, renderer: &mut Renderer) -> UiOutcome {
        let fps = self.fps;
        let n = self.n;
        let theme = self.theme;
        let pending = self.pending_base || self.pending_erode;

        let mut ui = renderer.ui();
        // One line, at the top of the frame, and the whole UI is styled. Nothing
        // style-shaped is retained by the toolkit between frames.
        ui.set_theme(theme);

        // The parameter panel, top-left.
        let (mut base, erode, rebuild, new_seed, preset, reset) =
            ui.panel(Anchor::TopLeft, theme.panel_w, |ui| {
                ui.title("Terrain");
                if pending {
                    ui.label_muted("release to rebuild...");
                }
                ui.separator();

                // Everything below the header scrolls. Sections collapse too
                // (click a heading) — between them the panel stays on screen
                // however many knobs it grows.
                ui.scroll_area("params", PANEL_SCROLL_MAX, |ui| {
                    // --- Layer 1: the Perlin base shape ---
                    let mut base = false;
                    let mut preset = None;
                    ui.section("Base shape", |ui| {
                        // The button row. Each cell is its own column, so the
                        // three hit-test to their own thirds of the width.
                        // Secondary and small: three equivalent choices, none of
                        // which is *the* action of the panel.
                        ui.columns(SHAPE_PRESETS.len(), |ui, i| {
                            if ui
                                .button(SHAPE_PRESETS[i].0)
                                .variant(Variant::Secondary)
                                .size(Size::Sm)
                                .show()
                                .clicked
                            {
                                preset = Some(i);
                            }
                        });
                        base |= ui
                            .slider("frequency", &mut self.params.frequency, 0.5, 8.0)
                            .show()
                            .changed;
                        let mut octaves = self.params.octaves as f32;
                        if ui
                            .slider("octaves", &mut octaves, 1.0, 8.0)
                            .decimals(0)
                            .show()
                            .changed
                        {
                            self.params.octaves = octaves.round() as u32;
                            base = true;
                        }
                        base |= ui
                            .slider("lacunarity", &mut self.params.lacunarity, 1.5, 3.0)
                            .show()
                            .changed;
                        base |= ui
                            .slider("persistence", &mut self.params.persistence, 0.2, 0.8)
                            .show()
                            .changed;
                        base |= ui
                            .slider("ridge (peaks)", &mut self.params.ridge, 0.5, 3.0)
                            .show()
                            .changed;
                    });
                    ui.separator();

                    // --- Layer 2: erosion ---
                    let mut erode = false;
                    ui.section("Fluvial erosion", |ui| {
                        let mut iters = self.erosion.iterations as f32;
                        if ui
                            .slider("passes", &mut iters, 0.0, PASSES_MAX)
                            .decimals(0)
                            .show()
                            .changed
                        {
                            self.erosion.iterations = iters.round() as u32;
                            erode = true;
                        }
                        // The demo's own widget: erodibility is only tunable on
                        // a log track.
                        erode |= log_slider(
                            ui,
                            "erodibility",
                            &mut self.erosion.erodibility,
                            1.0e-5,
                            6.0e-3,
                        )
                        .changed;
                        erode |= ui
                            .slider("area exponent m", &mut self.erosion.m, 0.2, 1.0)
                            .show()
                            .changed;
                        // Zero this and the landscape stalls with a fifth of it
                        // underwater — see `ErosionParams::deposition`.
                        erode |= ui
                            .slider("lake siltation", &mut self.erosion.deposition, 0.0, 0.3)
                            .show()
                            .changed;
                    });

                    ui.section("Thermal erosion", |ui| {
                        erode |= ui
                            .checkbox("enable talus", &mut self.erosion.thermal)
                            .changed;
                        if self.erosion.thermal {
                            // Indented because these belong to the toggle above
                            // them — which used to be faked with two leading
                            // spaces inside the label string.
                            erode |= ui.indent(|ui| {
                                let talus = ui
                                    .slider("talus (slope)", &mut self.erosion.talus, 0.3, 4.0)
                                    .show()
                                    .changed;
                                let rate = ui
                                    .slider("rate", &mut self.erosion.thermal_rate, 0.0, 0.5)
                                    .show()
                                    .changed;
                                talus || rate
                            });
                        }
                    });
                    ui.separator();

                    // --- The water riding on it ---
                    //
                    // Display only. Both controls change what is drawn, never what
                    // is simulated — the lakes and rivers exist in the model
                    // whether or not this section is showing them.
                    let mut rebuild = false;
                    ui.section("Water", |ui| {
                        rebuild |= ui.checkbox("show water", &mut self.show_water).changed;
                        if self.show_water {
                            rebuild |= ui.indent(|ui| {
                                log_slider(ui, "river threshold", &mut self.river_area, 4.0, 4000.0)
                                    .changed
                            });
                        }
                    });
                    ui.separator();

                    // --- Grid ---
                    let mut new_seed = false;
                    ui.section("Grid", |ui| {
                        ui.slider("resolution", &mut self.res, RES_MIN, RES_MAX)
                            .decimals(0)
                            .show();
                        new_seed = ui.button("new seed").show().clicked;
                    });
                    ui.separator();

                    // The one control that throws work away. Before variants this
                    // would have been an ordinary blue button beside "new seed",
                    // indistinguishable from it right up until it was clicked — or
                    // a hand-colored rectangle, which is the thing tokens exist to
                    // stop. `Variant::Destructive` says what it *is*, and the theme
                    // decides what that looks like.
                    let reset = ui
                        .button("reset all")
                        .variant(Variant::Destructive)
                        .show()
                        .clicked;

                    (base, erode, rebuild, new_seed, preset, reset)
                })
            });

        // The HUD, top-right — its own panel now, not the first three rows of
        // the parameter panel. Right-aligned readouts keep the numbers in a
        // column and stop the line growing past the border.
        // `light` is the theme proof, and it lives here rather than in the
        // toolkit because a theme is the consumer's to choose. It takes effect
        // on the next frame — the UI for *this* frame was styled before the
        // checkbox existed — which is invisible at 60fps and is just immediate
        // mode being consistent.
        let mut light = self.theme == Theme::light();
        ui.panel(Anchor::TopRight, HUD_W, |ui| {
            ui.label_value("fps", &format!("{fps:.0}"));
            ui.label_value("grid", &format!("{n}x{n}"));
            ui.checkbox("wireframe", &mut self.wireframe);
            ui.checkbox("light", &mut light);
        });

        let wants_pointer = ui.wants_pointer();
        drop(ui);

        self.theme = if light { Theme::light() } else { Theme::dark() };

        if reset {
            self.params = NoiseParams::default();
            self.erosion = ErosionParams::default();
            self.res = RES_DEFAULT as f32;
            self.show_water = true;
            self.river_area = RIVER_AREA_DEFAULT;
            base = true;
        }
        if let Some(i) = preset {
            let (_, frequency, octaves, ridge) = SHAPE_PRESETS[i];
            self.params.frequency = frequency;
            self.params.octaves = octaves;
            self.params.ridge = ridge;
            base = true;
        }
        if new_seed {
            self.params.seed = self.params.seed.wrapping_add(1);
            base = true;
        }
        UiOutcome {
            regen_base: base,
            reerode: erode,
            rebuild,
            wants_pointer,
        }
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
        self.upload(renderer);
    }

    fn update(&mut self, renderer: &mut Renderer) {
        // Smooth the FPS readout (exponential moving average).
        let dt = renderer.dt();
        if dt > 0.0 {
            self.fps = self.fps * 0.9 + (1.0 / dt) * 0.1;
        }

        let outcome = self.build_ui(renderer);
        self.pending_base |= outcome.regen_base;
        self.pending_erode |= outcome.reerode;

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
        //
        // The water controls are exempt — they only change what the water *mesh*
        // looks like, so they rebuild immediately and stay live under the cursor.
        let dragging = renderer.input().is_mouse_held(MouseButton::Left);
        let mut dirty = outcome.rebuild;
        if !dragging && (self.pending_base || self.pending_erode) {
            if self.pending_base {
                self.n = target_n;
                self.regenerate_base();
            } else {
                self.apply_erosion();
            }
            self.pending_base = false;
            self.pending_erode = false;
            dirty = true;
        }

        if dirty {
            self.upload(renderer);
        }

        renderer.set_render_mode(if self.wireframe {
            RenderMode::Wireframe
        } else {
            RenderMode::Solid
        });

        self.drive_camera(renderer, outcome.wants_pointer);
    }
}

/// What one frame of UI asked the demo to do.
///
/// A struct rather than the tuple this used to be: at three booleans it was
/// already hard to read at the call site, and the fourth would have made it a
/// puzzle.
struct UiOutcome {
    /// A base-shape or grid control changed — regenerate the noise and re-erode.
    regen_base: bool,
    /// Only an erosion control changed — re-erode the cached base heightmap.
    reerode: bool,
    /// A display-only control changed, so the meshes need rebuilding even though
    /// the simulation itself hasn't moved.
    rebuild: bool,
    /// The pointer is over a panel, so the camera should ignore this drag.
    wants_pointer: bool,
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

/// Water color: a river shallow and bright, a lake darkening with its own depth.
///
/// Opaque, which is the ceiling on this slice — the engine's `Vertex` is
/// position + RGB and the scene pipeline is `BlendState::REPLACE`, so there is
/// nowhere to put an alpha even if we wanted one. Depth-darkening is the honest
/// substitute available today: it reads as *water*, and it makes a deep lake look
/// deep, without anything see-through. Translucency arrives with Slice 10's
/// per-instance material, and this is the function that will lose most of its job
/// when it does.
fn water_color(depth: f32, shade: f32) -> [f32; 3] {
    const SHALLOW: [f32; 3] = [0.29, 0.55, 0.72];
    const DEEP: [f32; 3] = [0.06, 0.17, 0.36];
    // Saturates around a tenth of the terrain's full relief — past that a lake is
    // simply "deep" and gets no darker.
    let t = (depth / 0.1).clamp(0.0, 1.0);
    [
        (SHALLOW[0] + (DEEP[0] - SHALLOW[0]) * t) * shade,
        (SHALLOW[1] + (DEEP[1] - SHALLOW[1]) * t) * shade,
        (SHALLOW[2] + (DEEP[2] - SHALLOW[2]) * t) * shade,
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
