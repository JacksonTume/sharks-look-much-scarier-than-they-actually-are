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
//! get drawn, translucent, on a second mesh.
//!
//! ## Erosion is a time axis you can scrub
//!
//! The demo does not compute *an* eroded landscape any more — it walks a timeline
//! and draws where it is standing. One erosion pass is one fixed simulation step
//! ([`Application::fixed_update`]), so the whole run plays, pauses, single-steps,
//! and **rewinds**, and what you watch is a flooded landscape draining into a
//! river network over about fourteen seconds.
//!
//! Rewinding is the part that needed a decision. Erosion has no inverse — you
//! cannot un-cut a valley — so the landscape at every pass is simply *kept*
//! ([`TerrainDemo::history`]). Recomputing instead was measured and rejected: it
//! costs 2.7 s in a debug build, which is the build this demo is usually run
//! under, and every drag of the slider would have been a freeze.
//!
//! This is the payoff demo for the project's thesis: *a developer writes their
//! algorithm and a few engine calls, and never touches `wgpu`/`winit`.*
//! Everything physical here — the noise, the erosion, the shading — lives in this
//! consumer crate (it can only see `slmsttaa`'s public API). The engine just:
//!
//! - uploads the mesh we build ([`Renderer::upload_mesh`]),
//! - draws it solid or as a wireframe on demand ([`Renderer::set_render_mode`]),
//! - lets us drive the orbit camera ([`Renderer::camera_mut`] + [`Renderer::input`]),
//! - draws our parameter panel and HUD ([`Renderer::ui`]),
//! - hands us a frame delta ([`Renderer::dt`]) for the FPS readout, and
//! - paces the simulation ([`Renderer::time`]) — a fixed step, a pause, and the
//!   sub-step fraction the two stored passes are blended by.
//!
//! Controls: **drag the left mouse button** over the 3D view to orbit, **scroll**
//! to zoom, arrow keys also orbit. The panel's *Time* section plays, pauses,
//! single-steps and scrubs the erosion; dragging *pass* backwards is a real
//! rewind. Everything below it edits the process rather than the position, and
//! changing any of it rebuilds the axis from the base once the drag ends. The
//! pass number doubles as a wetness reading — lakes silt up as the landscape
//! matures, so early passes have lakes and late ones only rivers. Toggle
//! **wireframe** to inspect the underlying grid, **click a section heading** to
//! collapse it, and **reset all** at the bottom of the panel throws every
//! parameter back to its default.
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
    run, Application, Instance, Key, Material, Mesh, MeshHandle, MouseButton, RenderMode, Renderer,
    Vertex,
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

/// Standing depth at which a lake reaches full opacity, in normalized height
/// units — the width of the shallows, and the number that decides how soft a
/// shoreline looks.
///
/// **Measured, after guessing wrong twice.** The obvious reasoning — "a lake is
/// order 1% of the relief deep, so fade over that" — gives about `0.012` and is
/// badly wrong, because lake depth is not a constant of the model. A headless
/// probe over the run says the median lake shallows from `0.064` at pass 0 to
/// `0.0041` by pass 60, a **sixteen-fold** drop, as siltation fills the basins.
/// At `0.012` that leaves *zero* percent of the lake area at full opacity by
/// pass 60: the water goes faint and mottled exactly where the demo spends most
/// of its time, and the lakes appear to evaporate rather than drain.
///
/// So this is sized against the *shallowest* water worth seeing rather than the
/// typical, which keeps a lake solid at both ends of the timeline and leaves the
/// fade to the genuinely shallow fringe.
const LAKE_OPAQUE_DEPTH: f32 = 0.0015;

/// Wetness below which nothing is drawn. The contour is taken at this value, so
/// it wants to be barely above zero — it is "where the water ends", not a
/// threshold with an opinion.
const WET_EPS: f32 = 0.02;

/// Half-width, in grid cells, of the narrowest drawn river — one exactly at the
/// drainage-area threshold.
const RIVER_HALF_WIDTH: f32 = 0.9;
/// Half-width of the widest. A trunk carrying the whole map would otherwise grow
/// without bound and read as a lake with straight sides.
const RIVER_HALF_WIDTH_MAX: f32 = 2.4;

/// Defaults for the two ripple knobs. Strength is a normal tilt, not a height;
/// chop is the spatial frequency of the largest wave in world units.
const RIPPLE_STRENGTH_DEFAULT: f32 = 0.45;
const RIPPLE_SCALE_DEFAULT: f32 = 9.0;

/// Strength of the sun glint. Water is a mirror at heart, so this is high
/// compared with anything else the engine draws.
const WATER_SPECULAR_DEFAULT: f32 = 0.5;
/// Blinn-Phong exponent for the glint: tight, because still water throws a small
/// hard highlight rather than a broad sheen.
const WATER_SHININESS: f32 = 90.0;
/// Schlick reflectance of water at normal incidence. The real physical value —
/// water really is only 2% reflective face-on, and almost a mirror edge-on.
const WATER_FRESNEL_F0: f32 = 0.02;
/// What a Fresnel edge tends toward, standing in for a sky the engine cannot
/// reflect. Matched to the horizon so distant water reads as reflecting *this*
/// scene rather than an unrelated blue.
const WATER_SKY: [f32; 3] = [0.42, 0.56, 0.72];
/// The last pass on the time axis — where the scrub slider tops out, and where
/// the simulation stops advancing.
///
/// **Measured, not guessed.** A headless probe over 150 passes at 128² says the
/// landscape has three acts: per-pass movement decays six-fold over the first
/// forty passes, lake coverage falls from 22.6% to zero by about pass 110, and
/// past that the terrain lowers at a flat 0.019% of its relief per pass with no
/// standing water left to change. So the axis ends a little past the last thing
/// worth looking at, and the run has a genuine end rather than trailing off.
const MAX_PASS: usize = 150;

/// How many passes a second the timeline pays out by default.
///
/// The whole visible arc is the lakes draining over ~110 passes, so this is
/// really "how long is the show": eight a second makes it about fourteen seconds,
/// which is long enough to watch a basin silt up and short enough to sit through
/// twice. It is a slider because the honest answer depends on the grid size.
const PASS_HZ_DEFAULT: f32 = 8.0;
/// Bounds on that slider. The floor is a crawl for watching one basin; the ceiling
/// is past the point where blending has anything left to smooth.
const PASS_HZ_MIN: f32 = 1.0;
const PASS_HZ_MAX: f32 = 30.0;
/// Default drainage area (in cells) at which a channel is drawn as a river.
const RIVER_AREA_DEFAULT: f32 = 60.0;
/// Default water opacity. Low enough that the riverbed reads through a channel,
/// high enough that a deep lake still looks like water rather than a stain.
const WATER_ALPHA_DEFAULT: f32 = 0.72;
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

    /// **The time axis: the landscape at every pass, indexed by pass number.**
    /// `history[0]` is the un-eroded base.
    ///
    /// This is the whole trick behind a scrub that runs *backwards*. Erosion is
    /// irreversible — there is no inverse pass — so the only way to show pass 30
    /// after reaching pass 90 is to have pass 30 written down. Recomputing it
    /// instead was measured and rejected: 150 passes at 128² costs 336 ms in a
    /// release build and **2.7 s in a debug one**, which is the build the demo is
    /// normally run under, so every drag of the slider would have been a freeze.
    ///
    /// Storing it costs `4·n²` bytes a pass — 9.6 MB across the whole axis at the
    /// default 128², 39 MB at the 256² maximum. That is a lot of memory to spend
    /// on a demo and it is the right trade by a wide margin: it turns a rewind
    /// into an array index, which is instant in *any* build.
    ///
    /// Water is deliberately **not** stored alongside. It is another `8·n²` bytes
    /// a pass (three times the total), and it is only ever needed for the two
    /// passes currently on screen — see [`TerrainDemo::snap_water`].
    history: Vec<Vec<f32>>,
    /// Which pass the blend starts from; the head of the time axis.
    pass: usize,
    /// The water at `pass` and at `pass + 1` — the two ends of the blend.
    ///
    /// Kept as a pair rather than recomputed because flow routing is the expensive
    /// half of a pass. Walking forward, the far end becomes the near end and only
    /// one new routing is needed; a scrub, which can land anywhere, pays for two.
    snap_water: [erosion::Water; 2],
    /// The `(pass, alpha)` the render buffers below were last filled for. Skips the
    /// per-frame rebuild when neither has moved — which is every frame while paused.
    resolved: Option<(usize, f32)>,

    /// The heights actually rendered (`n * n`): the two bracketing passes lerped
    /// by [`Timeline::alpha`]. Reused between frames rather than reallocated.
    heights: Vec<f32>,
    /// The water actually rendered, blended from [`TerrainDemo::snap_water`] the
    /// same way.
    water: erosion::Water,

    /// Passes per second the timeline pays out — the fixed-step rate.
    pass_hz: f32,
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
    /// How opaque the water surface is. `1.0` is the solid blue this demo shipped
    /// with; anything less is the engine's blended pass, and the riverbed shows
    /// through. Purely visual, like `river_area`.
    ///
    /// This scales the *whole* surface. The per-vertex alpha that fades the
    /// shallows is separate and multiplies underneath it, so a lake still fades
    /// into its shore at any setting of this.
    water_alpha: f32,

    /// How hard the ripples tilt the surface normal — the knob that actually
    /// changes how the water reads, because the specular highlight is a function
    /// of the normal alone.
    ///
    /// The engine animates this against its own clock, so the water keeps moving
    /// while the erosion is paused. That is the same wall-versus-simulation split
    /// the engine's time docs draw, and pausing the timeline to watch the surface
    /// carry on is the clearest demonstration of it the demo has.
    ripple_strength: f32,
    /// Spatial frequency of the largest ripple, in world units.
    ripple_scale: f32,
    /// Last frame's water vertex/index counts, so the next build allocates once
    /// at the right size instead of doubling its way up. A `Cell` because the
    /// build only needs `&self` otherwise and this is pure bookkeeping.
    water_capacity: std::cell::Cell<(usize, usize)>,
    /// Strength of the specular sun glint on the water.
    water_specular: f32,

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
            history: Vec::new(),
            pass: 0,
            snap_water: [erosion::Water::default(), erosion::Water::default()],
            resolved: None,
            heights: Vec::new(),
            water: erosion::Water::default(),
            pass_hz: PASS_HZ_DEFAULT,
            n: hm.n,
            res: n as f32,
            show_water: true,
            river_area: RIVER_AREA_DEFAULT,
            water_alpha: WATER_ALPHA_DEFAULT,
            ripple_strength: RIPPLE_STRENGTH_DEFAULT,
            ripple_scale: RIPPLE_SCALE_DEFAULT,
            water_capacity: std::cell::Cell::new((0, 0)),
            water_specular: WATER_SPECULAR_DEFAULT,
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
        demo.reset_history();
        demo.resolve(0.0);
        demo
    }

    /// Regenerate the Perlin base heightmap (layer 1) at the current parameters
    /// and resolution, then re-erode it. Called when a noise/grid control changes.
    fn regenerate_base(&mut self) {
        let hm = Heightmap::generate(self.n, &self.params);
        self.n = hm.n;
        self.base = hm.heights;
        self.reset_history();
    }

    /// Throw the time axis away and rebuild it up to the current pass.
    ///
    /// Every erosion parameter is baked into the history the moment a pass is
    /// computed, so changing one invalidates all of it — there is no partial
    /// update, and pretending otherwise would leave passes computed under a `K` the
    /// panel no longer shows. This is the expensive path (60 passes is ~120 ms
    /// release, ~1 s debug), which is why the panel debounces it to mouse-release
    /// exactly as it did when it was one batch `erode` call.
    fn reset_history(&mut self) {
        self.history.clear();
        self.history.push(self.base.clone());
        let pass = self.pass;
        self.pass = 0;
        self.seek_pass(pass);
    }

    /// Grow the history until pass `target` exists, capped at the end of the axis.
    ///
    /// Each iteration steps a clone of the newest state, which is the cheap way
    /// round: [`erosion::step`] hands back the water belonging to the state it was
    /// *given*, so walking forward never routes the same flow twice.
    fn extend_to(&mut self, target: usize) {
        let target = target.min(MAX_PASS);
        while self.history.len() <= target {
            let mut next = self.history[self.history.len() - 1].clone();
            erosion::step(&mut next, self.n, &self.erosion);
            self.history.push(next);
        }
    }

    /// The water on a stored pass, clamped to the end of the axis.
    fn water_at(&self, index: usize) -> erosion::Water {
        let index = index.min(self.history.len().saturating_sub(1));
        erosion::water_of(&self.history[index], self.n)
    }

    /// Move the head to `pass`, computing whatever the axis and the blend pair need.
    ///
    /// The jump case: both ends of the blend are unknown, so both are routed. Used
    /// by the scrub slider, which can land anywhere including behind us.
    fn seek_pass(&mut self, pass: usize) {
        self.pass = pass.min(MAX_PASS);
        self.extend_to(self.pass + 1);
        self.snap_water = [self.water_at(self.pass), self.water_at(self.pass + 1)];
        self.resolved = None;
    }

    /// Step one pass along the axis — the common case, and the cheap one.
    ///
    /// The state being moved onto was the *far* end of last frame's blend, so its
    /// water has already been routed; only the new far end is unknown. That is what
    /// keeps a playing timeline at one flow routing per pass instead of two.
    fn advance_pass(&mut self) {
        if self.pass >= MAX_PASS {
            return;
        }
        self.pass += 1;
        self.extend_to(self.pass + 1);
        self.snap_water.swap(0, 1);
        self.snap_water[1] = self.water_at(self.pass + 1);
        self.resolved = None;
    }

    /// Fill the render buffers by blending the bracketing passes.
    ///
    /// **This is the half of [`Timeline::alpha`] no consumer had exercised.**
    /// `scene.rs` renders between steps by evaluating a pose function at a sub-step
    /// instant, which a landscape cannot do — there is no closed form for "pass
    /// 43.6". So this is the other case the engine's docs name: a consumer holding
    /// two snapshots and interpolating them.
    ///
    /// It is load-bearing rather than polish. The probe says a single cell moves up
    /// to 0.0147 of the height range in the first pass — about half a grid cell —
    /// so at eight passes a second the early landscape visibly jumps without it.
    ///
    /// The water blends the same way, and that turned out to be enough on its own:
    /// only 0.2–0.6% of cells change wet/dry in a pass (rivers usually under 0.1%),
    /// so lerping depth and area retreats a lake edge smoothly instead of popping
    /// it. A soft threshold was planned for the river network and never needed.
    fn resolve(&mut self, alpha: f32) {
        // At the end of the axis there is nothing ahead to blend toward.
        let t = if self.pass >= MAX_PASS {
            0.0
        } else {
            alpha.clamp(0.0, 1.0)
        };
        if self.resolved == Some((self.pass, t)) {
            return;
        }
        self.resolved = Some((self.pass, t));

        let far = (self.pass + 1).min(self.history.len().saturating_sub(1));
        let (a, b) = (&self.history[self.pass], &self.history[far]);
        let lerp = |x: &f32, y: &f32| x + (y - x) * t;

        self.heights.clear();
        self.heights
            .extend(a.iter().zip(b).map(|(x, y)| lerp(x, y)));

        let (wa, wb) = (&self.snap_water[0], &self.snap_water[1]);
        self.water.depth.clear();
        self.water
            .depth
            .extend(wa.depth.iter().zip(&wb.depth).map(|(x, y)| lerp(x, y)));
        self.water.area.clear();
        self.water
            .area
            .extend(wa.area.iter().zip(&wb.area).map(|(x, y)| lerp(x, y)));
        // The receiver tree is taken from the near pass rather than blended,
        // because a link is an *index* and there is no halfway between flowing
        // north and flowing east. Nothing is lost: the network's topology changes
        // in well under a tenth of a percent of cells per pass, so the channel a
        // river is drawn along is the same one at both ends of the blend.
        self.water.receiver.clear();
        self.water.receiver.extend_from_slice(&wa.receiver);
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
            // The one line that makes the rivers see-through. No water-specific
            // engine code exists: it is the same `Material` any instance can set,
            // and the engine sorts the blended run after the opaque terrain and
            // draws it without writing depth.
            // What makes it read as water rather than as blue plastic, and none
            // of it is water-specific engine code: a tight sun glint off the
            // ripples, and a Fresnel edge that turns the surface toward the sky
            // colour and closes it up as the view flattens. `blended()` is the
            // one that is easy to miss — the per-vertex shore fade is invisible
            // to the pipeline choice, so without it dragging opacity to 1.0 would
            // drop the whole surface into the opaque pass and the soft shoreline
            // would snap back to a hard line.
            instances.push(
                Instance::at(water_handle).with_material(
                    Material::OPAQUE
                        .with_alpha(self.water_alpha)
                        .with_specular(self.water_specular, WATER_SHININESS)
                        .with_fresnel(WATER_FRESNEL_F0, WATER_SKY)
                        .with_ripples(self.ripple_strength, self.ripple_scale)
                        .blended(),
                ),
            );
        }
        renderer.set_instances(&instances);
    }

    /// Build the renderable mesh from the current heights: an `n × n` grid with a
    /// height/slope color palette, and a real surface normal per vertex.
    ///
    /// This used to fold diffuse shading into the vertex color, because the
    /// engine's pipeline was position+color only. That bake is **gone**: the
    /// normal goes to the engine instead, which lights it in world space. The
    /// arithmetic didn't move so much as get deleted — the normal was already
    /// computed here to drive the slope term of the palette, so handing it over
    /// costs nothing and the shading is now correct for geometry that moves.
    fn build_mesh(&self) -> Mesh {
        let n = self.n;
        // Displayed height per cell, in world units, on the fixed scale — see
        // `disp`. Erosion lowering the terrain is now something you can *see*
        // rather than something a refitted range quietly cancels out.
        let disp = |i: usize| disp(self.heights[i]);

        let step = (2.0 * HALF) / (n as f32 - 1.0);
        let cell_world = step; // horizontal spacing for slope/normal estimates

        let mut vertices = Vec::with_capacity(n * n);
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

                vertices.push(Vertex {
                    position: [wx, wy, wz],
                    normal,
                    color: {
                        let c = palette(t, slope);
                        [c[0], c[1], c[2], 1.0]
                    },
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

    /// The **wetness field**: how much water covers each grid point, `0` dry to
    /// `1` fully submerged. Lakes and rivers both write into it, and everything
    /// downstream reads only this.
    ///
    /// This exists because the old water mesh classified whole *cells* as wet or
    /// dry and drew their four corners. That quantises every shoreline to the
    /// grid, which is why lakes had axis-aligned staircase edges and a river was a
    /// chain of squares — the boundary could only ever land on a grid line. A
    /// continuous field can be contoured *between* samples instead, so the
    /// shoreline goes where the water actually stops.
    ///
    /// It also does a second job that turns out to matter as much: the value is
    /// the surface's **opacity**, so water fades out as it shallows instead of
    /// ending on a hard line.
    fn wetness_field(&self) -> Vec<f32> {
        let n = self.n;
        let mut wet = vec![0.0f32; n * n];

        // --- Lakes: straight off the flood depth ---
        //
        // Saturating within a hair of the shore keeps the drawn edge at the true
        // waterline while still giving the shallows a few cells of fade.
        //
        // Measured from `MIN_POND`, not from zero, and that subtraction is
        // load-bearing rather than tidy. The Priority-Flood lifts each cell a
        // hair above the one it was reached from, so depths of a few ε are strewn
        // across the map — the old per-cell test discarded them by construction,
        // but a *continuous* field happily draws them, and the result is a faint
        // blue-green film over half the landscape that reads as the terrain
        // having changed colour rather than as water being present.
        for (w, &d) in wet.iter_mut().zip(&self.water.depth) {
            *w = ((d - erosion::MIN_POND) / LAKE_OPAQUE_DEPTH).clamp(0.0, 1.0);
        }

        // --- Rivers: splat each flow link as a segment with a width ---
        //
        // A river is drawn from the network's *edges*, not its cells. Each
        // `c -> receiver[c]` link is a segment, and every grid point within the
        // channel's half-width of it gets wet, tapering to nothing at the bank.
        // That is what makes a river follow its own diagonal instead of
        // staircasing along the grid, and what lets a trunk be wider than the
        // tributaries feeding it.
        let river_area = self.river_area.max(1.0);
        for c in 0..n * n {
            // Deliberately *not* skipping cells that are already lake, which is
            // the obvious optimisation and punches holes in the rivers. A cell
            // whose depth sits just above `MIN_POND` would be dropped here while
            // contributing almost nothing as lake — it falls through the gap
            // between the two rules — and since the flood's ε leaves plenty of
            // river cells in exactly that band, the network ends up riddled with
            // single-point dry spots. Each one shows up as a little diamond of
            // bare ground, because its four surrounding cells each contour around
            // it. Splatting regardless costs nothing: the combine below is a
            // `max`, and a channel drawn across a lake that is already fully wet
            // changes not one pixel.
            if self.water.area[c] < river_area {
                continue;
            }
            let r = self.water.receiver[c];
            if r == c {
                continue;
            }
            // Physically a channel widens with the square root of its discharge,
            // which is also what reads correctly: doubling the catchment should be
            // visible but not dramatic.
            let half = (RIVER_HALF_WIDTH * (self.water.area[c] / river_area).sqrt())
                .clamp(RIVER_HALF_WIDTH, RIVER_HALF_WIDTH_MAX);

            let (ax, ay) = ((c % n) as f32, (c / n) as f32);
            let (bx, by) = ((r % n) as f32, (r / n) as f32);

            let lo_x = (ax.min(bx) - half).floor().max(0.0) as usize;
            let hi_x = (ax.max(bx) + half).ceil().min(n as f32 - 1.0) as usize;
            let lo_y = (ay.min(by) - half).floor().max(0.0) as usize;
            let hi_y = (ay.max(by) + half).ceil().min(n as f32 - 1.0) as usize;

            for gy in lo_y..=hi_y {
                for gx in lo_x..=hi_x {
                    let d = point_segment_distance(gx as f32, gy as f32, ax, ay, bx, by);
                    let v = 1.0 - d / half;
                    if v > 0.0 {
                        let slot = &mut wet[gy * n + gx];
                        // `max`, not `+`: two links overlapping at a confluence
                        // must not read as twice as wet, or every junction shows
                        // up as a bright blob.
                        *slot = slot.max(v);
                    }
                }
            }
        }
        wet
    }

    /// Build the water surface by **contouring** the wetness field.
    ///
    /// `None` when there is nothing wet to draw (or the toggle is off), so the
    /// draw-list just holds the terrain.
    ///
    /// # How the hard edges went away
    ///
    /// Marching squares, with the interior filled rather than just the contour
    /// line traced. Each grid cell looks at the wetness at its four corners and
    /// emits a polygon made of the corners that are wet plus the points where the
    /// field crosses [`WET_EPS`] along the edges between a wet corner and a dry
    /// one. That crossing is found by interpolation, so it lands *between* grid
    /// samples — which is the entire difference between a coastline and a
    /// staircase.
    ///
    /// Three separate things then stop the water meeting the land in a visible
    /// seam, and all three matter:
    ///
    /// - The surface height is sampled from `terrain + depth`, and depth is zero
    ///   at the waterline, so the edge of the water sits exactly on the ground it
    ///   is lapping against rather than hovering over it.
    /// - Opacity is the wetness, which is zero at that same boundary, so the
    ///   surface fades out instead of stopping.
    /// - The lift that keeps water off the terrain is scaled by wetness too, so
    ///   it tapers away at the shore rather than leaving a rim standing proud.
    fn build_water_mesh(&self) -> Option<Mesh> {
        let n = self.n;
        if !self.show_water || self.water.depth.len() != n * n || n < 2 {
            return None;
        }

        let wet = self.wetness_field();
        // Sized from last frame's result rather than grown from empty. A water
        // surface is tens of thousands of vertices, and letting a `Vec` double its
        // way there re-allocates and memcpys about seventeen times per frame for
        // a count that barely changes between frames.
        let (cap_v, cap_i) = self.water_capacity.get();
        // One height field for both kinds of water: a lake sits at the flooded
        // level and a river sits on the ground, and `depth` is what tells them
        // apart — it is the flood's own fill, and it is zero on a river.
        //
        // Stored already in **world units**, with `disp` applied per grid point
        // exactly as the terrain mesh applies it to its own vertices. Displaying
        // after interpolating instead would reintroduce a mismatch at the height
        // clamp, and the whole point of this field is that it agrees with the
        // ground wherever the depth is zero.
        let surface: Vec<f32> = self
            .heights
            .iter()
            .zip(&self.water.depth)
            .map(|(h, d)| disp(h + d))
            .collect();

        let step = (2.0 * HALF) / (n as f32 - 1.0);
        let mut vertices: Vec<Vertex> = Vec::with_capacity(cap_v);
        let mut indices: Vec<u32> = Vec::with_capacity(cap_i);
        let mut poly: Vec<(f32, f32)> = Vec::with_capacity(4);

        // **The terrain's own two triangles, not the cell.** Contouring the quad
        // and fan-splitting the result is the obvious way and it is subtly wrong:
        // a fan from corner `a` divides the cell along `a–c`, while the terrain
        // divides it along `d–b`. The four corner heights agree either way, but
        // the two surfaces interpolate across *different diagonals*, so they
        // cross somewhere inside every cell. Half of each cell's water then sits
        // below the ground and is eaten by the depth test — which is why the
        // rivers came out as chains of little triangular holes, each one exactly
        // half a grid cell. No lift fixes it, because the error scales with how
        // twisted the quad is and not with any constant.
        //
        // Clipping the same triangles the terrain draws makes the water
        // piecewise-planar on the identical partition, so the two agree
        // everywhere and a hair of lift is enough. Counter-clockwise from above,
        // matching the terrain's winding.
        const TRIS: [[(usize, usize); 3]; 2] = [
            [(0, 0), (0, 1), (1, 0)], // a, d, b
            [(1, 0), (0, 1), (1, 1)], // b, d, c
        ];

        for y in 0..n - 1 {
            let row = y * n;
            for x in 0..n - 1 {
                // Reject the whole cell before touching either triangle. Water
                // covers well under a fifth of the map, so this skips most of the
                // grid on one comparison chain instead of six.
                if wet[row + x] < WET_EPS
                    && wet[row + x + 1] < WET_EPS
                    && wet[row + n + x] < WET_EPS
                    && wet[row + n + x + 1] < WET_EPS
                {
                    continue;
                }
                for tri in TRIS {
                    let point = |k: usize| ((x + tri[k].0) as f32, (y + tri[k].1) as f32);
                    let value = |k: usize| wet[(y + tri[k].1) * n + (x + tri[k].0)];

                    poly.clear();
                    for k in 0..3 {
                        let (v0, v1) = (value(k), value((k + 1) % 3));
                        let (p0, p1) = (point(k), point((k + 1) % 3));
                        if v0 >= WET_EPS {
                            poly.push(p0);
                        }
                        // Exactly one end wet: the boundary crosses this edge, so
                        // put a vertex where it actually crosses.
                        if (v0 >= WET_EPS) != (v1 >= WET_EPS) {
                            let t = ((WET_EPS - v0) / (v1 - v0)).clamp(0.0, 1.0);
                            poly.push((p0.0 + (p1.0 - p0.0) * t, p0.1 + (p1.1 - p0.1) * t));
                        }
                    }
                    if poly.len() < 3 {
                        continue;
                    }

                    let base = vertices.len() as u32;
                    for &(fx, fy) in &poly {
                        vertices.push(self.water_vertex(fx, fy, step, &wet, &surface));
                    }
                    // A convex polygon walked in order: a fan is safe, and it is
                    // at most two triangles because clipping a triangle by one
                    // half-plane yields three or four points.
                    for k in 1..poly.len() as u32 - 1 {
                        indices.extend_from_slice(&[base, base + k, base + k + 1]);
                    }
                }
            }
        }

        self.water_capacity.set((vertices.len(), indices.len()));
        (!indices.is_empty()).then(|| Mesh::new(vertices, indices))
    }

    /// One water vertex at fractional grid position `(fx, fy)`: where it sits,
    /// which way it faces once the ripples have moved it, and how blue and how
    /// see-through it is.
    fn water_vertex(&self, fx: f32, fy: f32, step: f32, wet: &[f32], surface: &[f32]) -> Vertex {
        let n = self.n;
        let w = bilinear(wet, n, fx, fy);
        let depth = bilinear(&self.water.depth, n, fx, fy);
        // Height matches the terrain's own triangulation — see `sample_triangulated`.
        let height = sample_triangulated(surface, n, fx, fy);

        Vertex {
            position: [-HALF + fx * step, height + WATER_LIFT, -HALF + fy * step],
            // Flat. **The ripples are not here any more** — they are a per-fragment
            // normal perturbation on the material, which is both cheaper and
            // better: this function used to evaluate four `sin_cos` per vertex and
            // that alone was measured at ~4 ms a frame, and the result was
            // detail no finer than the tessellation, which read as stripes.
            normal: Vertex::UP,
            color: {
                let c = water_color(depth);
                // Opacity is the wetness curve, shaped so the very edge goes to
                // nothing quickly and the body of a lake reaches full strength.
                [c[0], c[1], c[2], smoothstep(w)]
            },
        }
    }

    /// Lay out the parameter panel and HUD, returning what it asked for.
    fn build_ui(&mut self, renderer: &mut Renderer) -> UiOutcome {
        let fps = self.fps;
        let n = self.n;
        let theme = self.theme;
        let pending = self.pending_base || self.pending_erode;

        // Transport state is read off the engine's clock *before* the UI borrows
        // the renderer, and written back after it is dropped — the same shape
        // `scene.rs` uses, for the same borrow reason.
        let mut paused = renderer.time().is_paused();
        let mut scrub = self.pass as f32;
        let mut pass_hz = self.pass_hz;
        let mut single_step = false;
        let mut scrubbed = false;
        let pass = self.pass;

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
                    // --- The time axis ---
                    //
                    // First in the panel because it is what the demo now *is*: the
                    // landscape is a position on this axis, and everything below
                    // describes the process that generated it.
                    ui.section("Time", |ui| {
                        // Two buttons need `columns`, not `horizontal` — a button
                        // allocates whatever is left of the line, so in a row the
                        // first takes all of it and the second is clipped off the
                        // panel edge. Slice 12 shipped that bug and a screenshot
                        // found it; this is the fix arriving pre-applied.
                        ui.columns(2, |ui, i| {
                            if i == 0 {
                                let label = if paused { "play" } else { "pause" };
                                if ui.button(label).show().clicked {
                                    paused = !paused;
                                }
                            } else {
                                single_step =
                                    ui.button("step").variant(Variant::Secondary).show().clicked;
                            }
                        });
                        // The scrub, and the reason this slice picked a stored
                        // history over a recomputed one: dragging this backwards is
                        // an array index, so it is as cheap as dragging it forwards.
                        scrubbed = ui
                            .slider("pass", &mut scrub, 0.0, MAX_PASS as f32)
                            .decimals(0)
                            .show()
                            .changed;
                        ui.slider("passes/sec", &mut pass_hz, PASS_HZ_MIN, PASS_HZ_MAX)
                            .decimals(0)
                            .show();
                    });
                    ui.separator();

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
                        // "passes" used to live here, as the headline knob. It is
                        // gone from this section on purpose: a pass count is a
                        // position in *time*, not a property of the model, and it
                        // now drives the Time section above rather than sitting
                        // among the constants it is not one of.
                        //
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
                                let mut changed = log_slider(
                                    ui,
                                    "river threshold",
                                    &mut self.river_area,
                                    4.0,
                                    4000.0,
                                )
                                .changed;
                                // Opacity is the one water knob that needs no
                                // rebuild — it rides the instance's material, so
                                // the mesh it tints is untouched.
                                changed |= ui
                                    .slider("opacity", &mut self.water_alpha, 0.15, 1.0)
                                    .show()
                                    .changed;
                                // Nor does the glint: it is a material field too.
                                ui.slider("sun glint", &mut self.water_specular, 0.0, 2.5)
                                    .show();
                                // Both are material fields evaluated in the
                                // shader, so neither rebuilds the mesh either.
                                ui.slider("ripple", &mut self.ripple_strength, 0.0, 1.2)
                                    .show();
                                ui.slider("chop", &mut self.ripple_scale, 2.0, 30.0).show();
                                changed
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
            ui.label_value("pass", &format!("{pass}/{MAX_PASS}"));
            ui.checkbox("wireframe", &mut self.wireframe);
            ui.checkbox("light", &mut light);
        });

        let wants_pointer = ui.wants_pointer();
        drop(ui);

        self.theme = if light { Theme::light() } else { Theme::dark() };

        // --- Transport, written back to the engine's clock ---
        //
        // The rate is the pass rate: **one fixed step is one erosion pass**, which
        // is what makes `alpha` mean "how far between two passes are we" and lets
        // the blend fall straight out of it. Driving a 60 Hz clock and counting
        // passes separately would have worked and would have thrown that away.
        self.pass_hz = pass_hz;
        renderer.time_mut().set_rate(pass_hz);
        renderer.time_mut().set_paused(paused);
        if single_step {
            renderer.time_mut().step_once();
        }
        if scrubbed {
            // A seek the engine explicitly says it cannot do for you: its clock
            // moves, and rewinding the *consumer* is the consumer's problem. This
            // demo can solve it only because it wrote every pass down — which is
            // the whole design, stated as one line of code.
            self.seek_pass(scrub.round().max(0.0) as usize);
            renderer
                .time_mut()
                .seek(self.pass as f32 / pass_hz.max(1.0));
        }

        if reset {
            self.params = NoiseParams::default();
            self.erosion = ErosionParams::default();
            self.res = RES_DEFAULT as f32;
            self.show_water = true;
            self.river_area = RIVER_AREA_DEFAULT;
            self.water_alpha = WATER_ALPHA_DEFAULT;
            self.ripple_strength = RIPPLE_STRENGTH_DEFAULT;
            self.ripple_scale = RIPPLE_SCALE_DEFAULT;
            self.water_specular = WATER_SPECULAR_DEFAULT;
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

    /// One erosion pass, and nothing else.
    ///
    /// **The entire simulation lives here**, which is what buys the transport
    /// controls above: the landscape advances only when the engine says a fixed
    /// step is due, so pausing genuinely stops geology rather than merely freezing
    /// a camera. Nothing in [`Application::update`] moves the terrain — it only
    /// decides how to *draw* whatever pass the axis is currently sitting on.
    ///
    /// `dt` is ignored, and that is honest rather than lazy. A pass is not defined
    /// in seconds: the model's timestep is folded into the erodibility `K` (see
    /// [`ErosionParams::erodibility`]), so a pass *is* the unit of simulation time.
    /// The step rate is set to the pass rate for exactly this reason, which makes
    /// `dt` a constant the hook has no use for.
    fn fixed_update(&mut self, _renderer: &mut Renderer, _dt: f32) {
        self.advance_pass();
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
                // An erosion constant changed, so every stored pass was computed
                // under the old one. The axis is rebuilt from the base rather than
                // patched — see `reset_history`.
                self.reset_history();
            }
            self.pending_base = false;
            self.pending_erode = false;
            dirty = true;
        }

        // Fill the render buffers for the instant this frame lands on. `resolve`
        // is a no-op unless the pass or the sub-pass fraction actually moved, so a
        // paused demo costs no mesh work at all — and a playing one at 75 fps
        // against 8 passes a second rebuilds for nine frames it would otherwise
        // have drawn identically, which is the point.
        let alpha = renderer.time().alpha();
        let was = self.resolved;
        self.resolve(alpha);
        if self.resolved != was {
            dirty = true;
        }

        // Nothing here advances the waves any more. They are a material property
        // evaluated per fragment against the engine's own clock, so they keep
        // moving while the erosion is paused *and* cost no mesh work — which is
        // the difference between a 10 ms per-frame rebuild and none.

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
/// It **kept** its job when translucency arrived, which was not the expectation.
/// The prediction was that depth-darkening was a substitute for see-through water
/// and would mostly go away; in practice the two do different things. Alpha shows
/// you the riverbed, and depth-darkening is what still distinguishes a deep lake
/// from a shallow one once you can see through both. What did go is the `shade`
/// argument — the engine lights this surface now.
fn water_color(depth: f32) -> [f32; 3] {
    const SHALLOW: [f32; 3] = [0.17, 0.44, 0.58];
    const DEEP: [f32; 3] = [0.02, 0.10, 0.25];
    // Saturates well before the deepest water the model produces, because the
    // depths themselves shrink sixteen-fold over the run (see
    // `LAKE_OPAQUE_DEPTH`). Scaled to a tenth of the relief, every late lake sits
    // at the shallow end of the ramp and the whole landscape's water is one flat
    // colour for the second half of the timeline. This way the young flooded
    // basins read as genuinely deep and the mature ponds as shallow, which is a
    // real thing the simulation is doing and was previously invisible.
    let t = (depth / 0.05).clamp(0.0, 1.0);
    [
        SHALLOW[0] + (DEEP[0] - SHALLOW[0]) * t,
        SHALLOW[1] + (DEEP[1] - SHALLOW[1]) * t,
        SHALLOW[2] + (DEEP[2] - SHALLOW[2]) * t,
    ]
}

/// Distance from a grid point to the segment `a -> b`, all in cell units.
fn point_segment_distance(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (ex, ey) = (bx - ax, by - ay);
    let len_sq = ex * ex + ey * ey;
    // A degenerate link is a point; the clamp below would divide by zero.
    let t = if len_sq > 1e-12 {
        (((px - ax) * ex + (py - ay) * ey) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dy) = (px - (ax + ex * t), py - (ay + ey * t));
    (dx * dx + dy * dy).sqrt()
}

/// Sample a height field the way the **terrain mesh** interpolates it: linearly
/// across the two triangles each grid cell is split into.
///
/// This is not a refinement of [`bilinear`], it is a correctness requirement, and
/// getting it wrong is visible. A quad split into triangles is *not* a bilinear
/// patch — the two agree at the four corners and nowhere else, differing most
/// along the diagonal. So a water surface sampled bilinearly sits below a terrain
/// drawn as triangles across half of every cell, by far more than any sane lift
/// can cover, and the water gets eaten by the depth test in a pattern that
/// follows the triangulation: rivers break into strings of beads and lakes grow
/// holes.
///
/// Matching the split makes the two surfaces agree *exactly* wherever the water
/// depth is zero, which is precisely the river network and every shoreline — the
/// places a disagreement would show. The terrain builds each cell as `(a, d, b)`
/// and `(b, d, c)`, so the diagonal runs from `d` to `b` and the halves are
/// `u + v <= 1` and `u + v >= 1`.
fn sample_triangulated(field: &[f32], n: usize, fx: f32, fy: f32) -> f32 {
    let max = n as f32 - 1.0;
    let (fx, fy) = (fx.clamp(0.0, max), fy.clamp(0.0, max));
    // The last row and column have no cell of their own to sit in.
    let x0 = (fx.floor() as usize).min(n - 2);
    let y0 = (fy.floor() as usize).min(n - 2);
    let (u, v) = (fx - x0 as f32, fy - y0 as f32);

    let at = |dx: usize, dy: usize| field[(y0 + dy) * n + (x0 + dx)];
    let (a, b, c, d) = (at(0, 0), at(1, 0), at(1, 1), at(0, 1));

    if u + v <= 1.0 {
        a + u * (b - a) + v * (d - a)
    } else {
        c + (1.0 - u) * (d - c) + (1.0 - v) * (b - c)
    }
}

/// Bilinear sample of an `n × n` field at fractional grid coordinates.
///
/// Contour vertices land between grid samples by construction, so every field the
/// surface reads has to be readable there too — sampling at the nearest cell
/// would put the smooth outline back on the grid it was just taken off.
///
/// Fine for the quantities that only shade the surface (wetness, depth). The
/// *height* must not use this — see [`sample_triangulated`].
fn bilinear(field: &[f32], n: usize, fx: f32, fy: f32) -> f32 {
    let max = n as f32 - 1.0;
    let (fx, fy) = (fx.clamp(0.0, max), fy.clamp(0.0, max));
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(n - 1), (y0 + 1).min(n - 1));
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let top = field[y0 * n + x0] * (1.0 - tx) + field[y0 * n + x1] * tx;
    let bottom = field[y1 * n + x0] * (1.0 - tx) + field[y1 * n + x1] * tx;
    top * (1.0 - ty) + bottom * ty
}

/// Hermite smoothstep on `[0, 1]` — an ease with zero slope at both ends, which
/// is what stops a fade having a visible start and finish.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
