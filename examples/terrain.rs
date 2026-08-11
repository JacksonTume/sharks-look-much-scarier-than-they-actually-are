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
//! ## A continent, baked once
//!
//! The demo does not compute *an* eroded landscape — it runs the erosion in front
//! of you and stops when the landscape is mature. One erosion pass is one fixed
//! simulation step ([`Application::fixed_update`]), so the run plays, pauses and
//! single-steps, and what you watch is a raw fractal island turning into a
//! continent with a drainage network on it.
//!
//! **There is a sea, and it is what makes the water go anywhere.** Every cell at
//! or below [`ErosionParams::sea_level`] is base level: the flood grows inland
//! from the coastline as well as from the map border, so a river ends by reaching
//! an ocean rather than by falling off the edge of the world. The landmass itself
//! is cut by [`NoiseParams::coast`], which drowns the rim; what is left is an
//! island with bays, headlands and offshore rocks, and lakes inland that silt up
//! and drain over the run while the sea does not.
//!
//! **The rewind is gone, and that was the trade that bought the size.** This demo
//! used to keep the landscape at *every* pass so the scrub slider could run
//! backwards by array index. That is `4·n²` bytes a pass: 9.6 MB at the old 128²
//! default and **2.4 GB** at 2048², which is simply not a thing a demo can
//! allocate. Erosion has no inverse, so a rewind is either stored or impossible —
//! and a map sixteen times the size turned out to be worth more than a slider that
//! ran backwards. What is left keeps one pass ahead ([`TerrainDemo::ahead`]) and
//! nothing else.
//!
//! ## Size is a control, and everything scales off it
//!
//! [`RESOLUTIONS`] runs from 32² to 2048² in octaves, and [`span`] is the one
//! number the rest is quoted against: the world's extent and vertical scale, the
//! number of noise octaves, the river drawing threshold and width, the camera's
//! orbit distance, and the length of the run. A 128² map is exactly the map this
//! demo has always had; every larger one holds proportionally more land *and*
//! more cells per unit of ground, and looks like the same landscape seen from
//! further away rather than a different one.
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
//! - paces the simulation ([`Renderer::time`]) — a fixed step and a pause.
//!
//! Controls: **drag the left mouse button** over the 3D view to orbit, **scroll**
//! to zoom, arrow keys also orbit. The panel's *Bake* section plays, pauses and
//! single-steps the erosion, reports how far along it is, and starts it again.
//! Everything below it edits the process rather than the position, and changing
//! any of it re-bakes from the base once the drag ends. The pass number doubles as
//! a wetness reading — inland lakes silt up as the landscape matures, so early
//! passes have lakes and late ones only rivers and the sea. Toggle
//! **wireframe** to inspect the underlying grid, **click a section heading** to
//! collapse it, and **reset all** at the bottom of the panel throws every
//! parameter back to its default.
//!
//! The panel also carries [`log_slider`] — a widget written *here*, in the demo,
//! from the toolkit's public API alone. That it can be is the point.
//!
//! **The *Plot* section is the same point made twice.** [`plot`] draws the
//! erosion's own history — mean lake depth and mean height moved, one reading per
//! pass — as two curves with the area under the first shaded and a marker on the
//! pass currently rendered, and the HUD calls the identical routine at a quarter
//! the height. Beneath it, [`TerrainDemo::shade_minimap`] shades a top-down
//! thumbnail and hands the engine the pixels. All of it is written here, out of
//! `allocate` + `painter` + three primitives ([`slmsttaa::ui::Painter::polyline`],
//! [`convex_polygon`](slmsttaa::ui::Painter::convex_polygon) and
//! [`image`](slmsttaa::ui::Painter::image)) and an [`ImageId`]. The
//! toolkit has no chart widget, no plot, and no minimap, and it does not need
//! one.
//!
//! The numbers were free. [`erosion::step`] has always returned the water
//! belonging to the state it was given, and this demo threw that return away on
//! every pass for three slices — so the series costs two sums over the grid and
//! no second flow routing.
//!
//! The HUD's **light** toggle swaps one [`Theme`] value and restyles the whole
//! UI — both panels, every built-in widget, and `log_slider` with them. That is
//! the demo's half of the toolkit's design-token claim: if any widget had kept a
//! hard-coded color, this is where it would stay dark.
//!
//! Run it:
//!   native — `cargo run --example terrain`
//!   web    — `cargo xtask serve terrain`, then open the printed URL.

use slmsttaa::ui::{font, Anchor, Color, Rect, Response, Size, Theme, Ui, Variant};
use slmsttaa::{
    run, Application, ImageId, Instance, Key, Material, Mesh, MeshHandle, MouseButton, RenderMode,
    Renderer, Vertex,
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

/// A bare progress bar, `0..=1`, drawn from the same tokens the built-in slider
/// uses.
///
/// Demo-side rather than a toolkit widget, for the reason the conventions file
/// gives: nothing in `slmsttaa-ui` needed it, and a widget with no roadblock
/// behind it is polish. It is the slider's track with the knob left off, which is
/// exactly what "reports but does not steer" should look like.
fn progress_bar(ui: &mut Ui, t: f32) {
    let theme = *ui.theme();
    let track_h = theme.control.track_h;
    let row = ui.allocate([0.0, track_h + 6.0]);
    let y = row.y + (row.h - track_h) * 0.5;
    let cap = track_h * 0.5;
    let painter = ui.painter();
    painter.fill_rect(
        Rect::new(row.x, y, row.w, track_h),
        cap,
        theme.color.surface,
    );
    painter.fill_rect(
        Rect::new(row.x, y, row.w * t.clamp(0.0, 1.0), track_h),
        cap,
        theme.color.accent,
    );
}

/// A plot of one or more series against the erosion time axis, written *here*
/// rather than in the toolkit — a chart is content, and content belongs in the
/// consumer.
///
/// `series` are sampled at pass 0, 1, 2, … and share one vertical scale, so two
/// quantities in the same units can be read against each other. `marker`, if
/// given, is a position along the axis in `0..=1` — the scrub, so the plot says
/// where on this curve the landscape currently on screen is sitting. `labels`
/// buys the axis annotations; the HUD copy runs without them.
///
/// Each series is scaled to **its own** peak, and each peak is labelled in that
/// series' color. One shared scale was the obvious thing and it was wrong here by
/// two orders of magnitude: standing water is a depth and per-pass movement is a
/// difference between consecutive passes, so the second is a couple of hundred
/// times smaller and a shared axis draws it as a flat line along the floor.
///
/// This is the demo's proof that the toolkit did not need a chart widget. It
/// uses [`Ui::allocate`] for space and [`Ui::painter`] to draw, and the three
/// primitives UI Slice 10 added — `polyline`, `convex_polygon` and (for the
/// minimap beneath it) `image` — are reached the same way `fill_rect` always was.
/// Nothing here is privileged.
fn plot(
    ui: &mut Ui,
    rect: Rect,
    series: &[&[f32]],
    colors: &[Color],
    axis: usize,
    marker: Option<f32>,
    labels: bool,
) {
    let theme = *ui.theme();
    let (px, weight) = theme.text.small.parts();

    // The horizontal scale is the **whole** axis, not the part computed so far.
    // Taking it from the data instead would let the plot stretch itself every
    // time the erosion ran a pass, and would put the marker — which is a
    // position on the whole axis — somewhere the data disagrees with.
    let span = axis.max(1) as f32;
    let step = rect.w / span;

    let painter = ui.painter();
    painter.fill_rect(rect, theme.radius.sm, theme.color.surface);

    // Gridlines at quarters. Horizontal, so a rect is still the right shape for
    // them and always was — it was the *diagonals* that could not be drawn.
    for i in 1..4 {
        let y = rect.y + rect.h * i as f32 / 4.0;
        painter.fill_rect(Rect::new(rect.x, y, rect.w, 1.0), 0.0, theme.color.border);
    }

    // 150 samples across a run at least 190 points wide, so there is always less
    // than one sample per pixel and nothing needs thinning. A plot with more
    // samples than pixels would have to decimate here: the painter does not.
    let mut pts: Vec<(f32, f32)> = Vec::with_capacity(axis + 1);

    for (index, (s, color)) in series.iter().zip(colors).enumerate() {
        let peak = s.iter().fold(0.0f32, |acc, v| acc.max(*v));
        if peak <= 0.0 || s.len() < 2 {
            continue;
        }
        pts.clear();
        pts.extend(s.iter().enumerate().map(|(i, v)| {
            let y = rect.max_y() - (v / peak).clamp(0.0, 1.0) * rect.h;
            (rect.x + i as f32 * step, y)
        }));

        // The area under the leading series, as one convex trapezoid per sample
        // — a curve's own area fill is concave in general, so it arrives in
        // pieces. Translucent both because that is what an area fill looks like
        // and because it is what makes the pieces join cleanly: two feathered
        // polygons sharing an edge composite to within a few percent of a solid
        // one at low alpha, and leave a visible light seam at high alpha.
        if index == 0 {
            let base = rect.max_y();
            let wash = [color[0], color[1], color[2], 0.22];
            for pair in pts.windows(2) {
                painter.convex_polygon(
                    &[pair[0], pair[1], (pair[1].0, base), (pair[0].0, base)],
                    wash,
                );
            }
        }

        painter.polyline(&pts, 1.5, *color);
    }

    if let Some(t) = marker {
        let x = rect.x + t.clamp(0.0, 1.0) * rect.w;
        painter.fill_rect(
            Rect::new(x, rect.y, 1.0, rect.h),
            0.0,
            theme.color.accent_hover,
        );
    }

    painter.stroke_rect(
        rect,
        theme.radius.sm,
        theme.control.border,
        theme.color.border,
    );

    if labels {
        // One peak per series, in that series' color, because each has its own
        // vertical scale and an unlabelled axis would be a lie about both.
        //
        // Right-aligned at the top, which is the one corner these particular
        // curves leave empty: both start at their maximum on the left and decay.
        // A rising series would want the other corner, and a general-purpose
        // chart would have to measure — this one is allowed to know its data.
        let mut y = rect.y + 2.0;
        for (s, color) in series.iter().zip(colors) {
            let peak = s.iter().fold(0.0f32, |acc, v| acc.max(*v));
            let text = format!("{peak:.4}");
            let w = font::text_width(&text, px, weight);
            painter.text(rect.max_x() - w - 4.0, y, &text, px, weight, *color);
            y += font::line_height(px);
        }
        let last = format!("{axis}");
        let w = font::text_width(&last, px, weight);
        painter.text(
            rect.max_x() - w - 4.0,
            rect.max_y() - font::line_height(px) - 2.0,
            &last,
            px,
            weight,
            theme.color.muted,
        );
    }
}

/// The grid the demo's world scale is quoted against. A `RES_REFERENCE²` map is
/// exactly the map this demo had before it grew: same extent, same feature size,
/// same timeline length.
const RES_REFERENCE: usize = 128;

/// How many reference map-widths across an `n`-cell map is.
///
/// **The one number the whole scaling story hangs off.** Cells go up as `n²` and
/// the world goes up as `span²`, so `span = √(n / RES_REFERENCE)` splits the
/// growth evenly between the two: at 2048² the map holds sixteen times the land
/// *and* four times the cells per unit of ground. More world and more detail,
/// rather than either one alone.
///
/// Everything that has to hold still as the map grows is quoted in terms of this:
/// the noise frequency (so a hill stays a hill), the vertical scale (so the
/// relief stays proportionate rather than flattening into a pancake), the river
/// drawing threshold and width (both measured in *cells*, and a river of a given
/// real size covers `span²` of them), and the length of the erosion run.
fn span(n: usize) -> f32 {
    (n as f32 / RES_REFERENCE as f32).sqrt()
}

/// Half-extent of the *reference* map in world units.
const HALF_BASE: f32 = 2.5;
/// Vertical scale of the reference map: normalized `[0, 1]` heights map into
/// `[0, VHEIGHT_BASE]` world units.
const VHEIGHT_BASE: f32 = 1.3;

/// Half-extent of an `n`-cell map in world units (it spans `[-half, half]`).
fn half(n: usize) -> f32 {
    HALF_BASE * span(n)
}

/// Vertical scale of an `n`-cell map. Grows with the map so a mountain keeps its
/// proportions — hold it fixed and a sixteen-times-wider continent reads as a
/// tidal flat.
fn vheight(n: usize) -> f32 {
    VHEIGHT_BASE * span(n)
}

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
fn disp(h: f32, vheight: f32) -> f32 {
    h.clamp(0.0, 1.0) * vheight
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

/// Fraction of the drainage-area threshold at which a channel *starts* to show,
/// fading to full strength as it reaches the threshold proper.
///
/// **A hard threshold was drawing dashes.** `area >= river_area` is a clean rule
/// and it cuts straight through a crowd: a headless count at pass 129 puts 212
/// cells just under the line and 192 just over it, so a fan of near-identical
/// parallel headwater threads has a few members switch on at full minimum width
/// while their neighbours stay invisible. Thirty of the seventy-six drawn threads
/// ran four cells or less before merging — stubs, and they read as a comb of
/// dashes lying across the valley floor rather than as streams.
///
/// Measured on how *abruptly* a drawn thread begins — its wetness minus the most
/// it has one cell upstream, where `1.0` is a dash out of dry ground — the hard
/// rule left 63 of those 76 threads starting at over `0.5`, mean `0.82`. At `0.4`
/// it is 12 threads and mean `0.21`, for 20% more drawn cells (3875 → 4666), all
/// of them faint tails. `0.3` buys two more percent of that and another 300 cells,
/// which is where it stops being worth it.
///
/// Nothing about which cells carry water changes here: this is the *drawing* rule,
/// and at the threshold and above it is exactly what it was. Below it a channel is
/// merely faint instead of absent, so a thread tapers out where it stops carrying
/// enough to see rather than ending on a grid cell.
const RIVER_FADE: f32 = 0.4;

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
/// Tint applied to what the surface reflects.
///
/// This used to be the *whole* reflection — a flat stand-in colour, because the
/// engine could not reflect anything. Slice 16 gave it a real sky and a
/// screen-space trace, so the field became a multiplier and this became nearly
/// white: the reflection is now an image, and tinting it hard would only throw
/// away the thing that was just built.
const WATER_REFLECTION_TINT: [f32; 3] = [0.96, 0.98, 1.0];
/// How far the surface displaces what is seen through it. Small: past about
/// `0.05` the distortion stops reading as water and starts reading as a broken
/// image, because a screen-space offset has no idea what it is dragging.
const WATER_REFRACTION: f32 = 0.018;
/// Beer-Lambert absorption per world unit of water crossed.
///
/// **Sized against the measurement, not the eye.** The lake-depth probe from
/// Slice 14 says the median lake shallows sixteen-fold between pass 0 and pass
/// 60, so a coefficient tuned to look right on a deep basin makes a mature lake
/// invisible. This is set so the *shallow* end still takes on colour, which
/// leaves the deep end saturating — the safe direction to be wrong in.
const WATER_ABSORPTION: f32 = 5.5;
/// How much of the reflection is traced from the scene rather than taken from
/// the sky. Full strength: where the trace finds a bank, that is what a mirror
/// would show, and where it does not it has already faded back to sky on its own.
const WATER_REFLECTION: f32 = 1.0;
/// How long the bake runs **at the reference grid**; [`max_pass`] scales it.
///
/// **Measured, not guessed.** A headless probe over 150 passes at 128² says the
/// landscape has three acts: per-pass movement decays six-fold over the first
/// forty passes, inland lake coverage falls from 22.6% to zero by about pass 110,
/// and past that the terrain lowers at a flat 0.019% of its relief per pass with
/// no standing fresh water left to change. So the run ends a little past the last
/// thing worth looking at, and has a genuine end rather than trailing off.
const MAX_PASS_BASE: usize = 150;

/// How many passes the bake runs at grid `n`.
///
/// **Fewer as the map grows, and that is the model's arithmetic rather than a
/// concession to the clock.** The stream power is `K·Aᵐ / L` with `A` counted in
/// *cells* and `L` a cell step, so quadrupling the cells per unit of ground
/// multiplies `A` by sixteen and, at the default `m = 0.5`, multiplies the cut per
/// pass by four. A pass at 2048² does four passes' worth of work, so the same
/// landscape arrives in a quarter of the passes. Scaling the count down keeps the
/// *amount* of erosion fixed across resolutions — which is the property that lets
/// the resolution control add detail instead of also winding the clock forward.
///
/// The floor keeps a small grid from finishing before anything has happened.
fn max_pass(n: usize) -> usize {
    ((MAX_PASS_BASE as f32 / span(n)).round() as usize).max(24)
}

/// How many passes a second the bake aims for.
///
/// The visible arc is the inland lakes draining, so this is really "how long is
/// the show": eight a second makes the reference run about fourteen seconds, long
/// enough to watch a basin silt up and short enough to sit through twice.
///
/// **A target, not a promise**, and increasingly so as the map grows. The bake
/// takes at most one pass per frame (see `fixed_update`), and a pass at 2048²
/// costs about 1.6 s — so past about 512² this stops being the thing that decides
/// the pace and the machine starts deciding it instead.
const PASS_HZ_DEFAULT: f32 = 8.0;
/// Bounds on that slider. The floor is a crawl for watching one basin; the ceiling
/// is past the point where a frame can keep up at any interesting size.
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

/// How tall the plot in the parameter panel is, in UI points.
///
/// Deep enough that two series at different magnitudes are separable, shallow
/// enough that the section it lives in does not push everything else off the
/// scroll. The panel is 340 points wide, so this is close to a 4:1 box.
const PLOT_H: f32 = 84.0;

/// How tall the HUD's sparkline copy of the same plot is.
const SPARK_H: f32 = 28.0;

/// Side length of the minimap thumbnail, in texels.
///
/// A **constant**, and deliberately not `self.n`. The resolution slider moves the
/// grid between 32 and 256, [`Renderer::update_image`] rewrites an image at the
/// size it was created with, and there is no way to free one — so a thumbnail
/// that tracked the grid would leak a texture per resolution change. The
/// heightmap is resampled into this instead, through the same [`bilinear`] the
/// mesh builder already uses.
const MINIMAP_N: usize = 160;

/// How big the minimap is drawn, in UI points. Smaller than the texture it
/// samples, so the thumbnail stays sharp on a HiDPI display.
const MINIMAP_VIEW: f32 = 132.0;
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

/// Grid resolutions the demo offers, in cells per side.
///
/// **A list, not a range, because the steps are octaves.** A slider that could
/// land on 1500² would offer a hundred sizes nobody wants between two that matter,
/// and each step here quadruples the cell count — which is the granularity the
/// costs actually come in. The slider snaps to an index into this.
///
/// The top of the list is where the demo stops being a thing you scrub and starts
/// being a world you bake once and look at; see [`TerrainDemo::bake`].
const RESOLUTIONS: [usize; 7] = [32, 64, 128, 256, 512, 1024, 2048];
/// The resolution the demo starts at, and the one "reset all" returns to.
const RES_DEFAULT: usize = 512;

/// Index into [`RESOLUTIONS`] of the nearest listed resolution to `n`.
fn res_index(n: usize) -> usize {
    RESOLUTIONS
        .iter()
        .position(|r| *r >= n)
        .unwrap_or(RESOLUTIONS.len() - 1)
}

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

    /// **The landscape one pass ahead of the one on screen.**
    ///
    /// This demo used to keep the *whole* time axis — every pass, indexed by pass
    /// number — so the scrub slider could run backwards by array index. That is
    /// `4·n²` bytes a pass, which was 9.6 MB at 128² and is **2.4 GB at 2048²**,
    /// and it is the reason the map could not grow. Trading the rewind for the
    /// world was the deal; see [`TerrainDemo::bake`].
    ///
    /// What is left is one buffer instead of a hundred and fifty, and it exists for
    /// a smaller reason: [`erosion::step`] returns the water belonging to the
    /// heights it was *given*, not the ones it produces. Keeping the next pass
    /// pre-computed means the pair on screen is always a state and its own water,
    /// at one flow routing per pass rather than two.
    ahead: Vec<f32>,
    /// **Two readings per baked pass**: mean standing depth and mean height moved.
    /// `series[_][k]` describes pass `k`.
    ///
    /// Two vectors rather than one of pairs, so each is a `&[f32]` the plot can
    /// be handed without copying it apart first.
    ///
    /// This costs a sum over the grid and **no extra flow routing**, which is the
    /// only reason a plot of the whole axis is affordable here. [`erosion::step`]
    /// already hands back the water belonging to the state it was *given*, and
    /// this demo threw that return away until there was something to draw with it
    /// — the numbers below have been computed on every pass since Slice 13 and
    /// discarded on every pass since Slice 13.
    ///
    /// Both are means rather than totals so the vertical scale means the same
    /// thing at 32² and at 2048², which matters because the resolution control
    /// throws the run away and starts it again.
    series: [Vec<f32>; 2],
    /// How many passes have been baked — the pass currently on screen.
    pass: usize,
    /// Whether the bake is running. Cleared when it reaches [`max_pass`], and by
    /// the pause button.
    baking: bool,
    /// Whether this frame has already run a pass — see `fixed_update`.
    baked_this_frame: bool,
    /// The pass the meshes were last built for. Skips the rebuild on every frame
    /// the world did not move, which is every frame once the bake has finished.
    resolved: Option<usize>,

    /// The landscape on screen (`n * n`), at pass [`TerrainDemo::pass`].
    heights: Vec<f32>,
    /// The water standing on exactly those heights.
    water: erosion::Water,

    /// Passes per second the bake aims for, when it can keep up.
    pass_hz: f32,
    /// Grid side length.
    n: usize,
    /// Resolution control value: an index into [`RESOLUTIONS`], as a float because
    /// the slider deals in floats.
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

    /// The top-down thumbnail in the parameter panel. `None` until `init` runs.
    ///
    /// The 3D view shows one corner of a basin from one angle; this shows the
    /// whole thing, including where the water is, which is the half of the
    /// erosion you cannot see from inside it.
    minimap: Option<ImageId>,
    /// Scratch RGBA for the thumbnail, kept so regenerating it does not allocate
    /// `MINIMAP_N²` texels every erosion pass.
    minimap_rgba: Vec<u8>,
    /// Which pass the thumbnail was last shaded for.
    ///
    /// The **pass**, not the `(pass, alpha)` pair the meshes are gated on. The
    /// meshes are re-blended every frame because a landscape sliding between two
    /// passes is the thing the transport exists to show; a 160² thumbnail is not,
    /// and reshading one every frame is 128,000 bilinear samples to move a lake
    /// edge by less than a texel. Once a pass is all it earns.
    minimap_for: Option<usize>,
}

impl TerrainDemo {
    fn new() -> Self {
        let n = RES_DEFAULT;
        let params = NoiseParams::default();
        let erosion = ErosionParams::default();
        let hm = Heightmap::generate(n, span(n), &params);
        let mut demo = Self {
            params,
            erosion,
            base: hm.heights,
            ahead: Vec::new(),
            series: [Vec::new(), Vec::new()],
            pass: 0,
            baking: true,
            baked_this_frame: false,
            resolved: None,
            heights: Vec::new(),
            water: erosion::Water::default(),
            pass_hz: PASS_HZ_DEFAULT,
            n: hm.n,
            res: res_index(n) as f32,
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
            distance: 6.5 * span(n),
            fps: 60.0,
            handles: None,
            minimap: None,
            minimap_rgba: vec![0; MINIMAP_N * MINIMAP_N * 4],
            minimap_for: None,
        };
        demo.restart_bake();
        demo
    }

    /// Regenerate the Perlin base heightmap (layer 1) at the current parameters
    /// and resolution, then start the bake again. Called when a noise/grid control
    /// changes.
    fn regenerate_base(&mut self) {
        let hm = Heightmap::generate(self.n, span(self.n), &self.params);
        self.n = hm.n;
        self.base = hm.heights;
        self.restart_bake();
    }

    /// Put the world back to pass zero and set the bake running.
    ///
    /// Every erosion parameter is fixed into the landscape the moment a pass is
    /// computed, so changing one invalidates everything computed so far — there is
    /// no partial update, and pretending otherwise would leave a landscape carved
    /// half under a `K` the panel no longer shows.
    fn restart_bake(&mut self) {
        self.series[0].clear();
        self.series[1].clear();
        self.pass = 0;
        self.baking = true;
        self.resolved = None;

        self.heights.clear();
        self.heights.extend_from_slice(&self.base);
        // Prime the pipeline: `ahead` becomes pass 1, and the water that falls out
        // is the water standing on pass 0 — which is what goes on screen with it.
        self.ahead.clear();
        self.ahead.extend_from_slice(&self.base);
        self.water = erosion::step(&mut self.ahead, self.n, &self.erosion);
    }

    /// Advance the bake by one pass, if it has not finished.
    ///
    /// The pair on screen is always a landscape and the water standing on *it*.
    /// [`erosion::step`] hands back the water belonging to the heights it was
    /// given, so the state one pass ahead is kept pre-computed and this is a swap
    /// plus one flow routing — never two, and never a re-analysis of a state
    /// already seen.
    fn bake_pass(&mut self) -> bool {
        if self.pass >= max_pass(self.n) {
            self.baking = false;
            return false;
        }
        let cells = (self.n * self.n).max(1) as f32;

        // The water on screen belongs to the pass being left behind, so both
        // readings describe it and `series[k]` stays aligned with pass `k`.
        let lake = self.water.depth.iter().sum::<f32>() / cells;
        // Halved because a sum of |dz| counts every grain twice: once where it was
        // cut and once where it landed.
        let moved = self
            .heights
            .iter()
            .zip(&self.ahead)
            .map(|(a, b)| (b - a).abs())
            .sum::<f32>()
            / (2.0 * cells);
        self.series[0].push(lake);
        self.series[1].push(moved);

        std::mem::swap(&mut self.heights, &mut self.ahead);
        self.ahead.clear();
        self.ahead.extend_from_slice(&self.heights);
        self.water = erosion::step(&mut self.ahead, self.n, &self.erosion);

        self.pass += 1;
        self.resolved = None;
        if self.pass >= max_pass(self.n) {
            self.baking = false;
        }
        true
    }

    /// Reshade the minimap from the heights and water currently on screen.
    ///
    /// Reads [`TerrainDemo::heights`] and [`TerrainDemo::water`] — the *blended*
    /// fields the meshes were built from — so the thumbnail and the landscape can
    /// never disagree about which pass they are showing.
    ///
    /// Shaded with the same [`palette`] the mesh uses, then darkened by a
    /// north-west hillshade so the relief reads without a light model, and
    /// finally tinted toward water wherever there is standing depth. The colors
    /// go out as `Rgba8Unorm` bytes, which is what makes them land in the same
    /// space as every [`Color`] the panel around them is drawn in.
    fn shade_minimap(&mut self) {
        let n = self.n;
        if n < 2 || self.heights.len() < n * n {
            return;
        }
        let sea = self.erosion.sea_level;
        let scale = (n - 1) as f32 / (MINIMAP_N - 1) as f32;
        let wet = !self.water.depth.is_empty();

        for ty in 0..MINIMAP_N {
            for tx in 0..MINIMAP_N {
                let (fx, fy) = (tx as f32 * scale, ty as f32 * scale);
                let h = bilinear(&self.heights, n, fx, fy);

                // A surface normal, built exactly the way the mesh builds one, so
                // the thumbnail and the landscape agree about what is rock and
                // what is meadow. Central differences over two cells, converted
                // into *world* units — a raw height difference is meaningless
                // here, because it shrinks as the grid gets finer.
                let dx = bilinear(&self.heights, n, fx + 1.0, fy)
                    - bilinear(&self.heights, n, fx - 1.0, fy);
                let dy = bilinear(&self.heights, n, fx, fy + 1.0)
                    - bilinear(&self.heights, n, fx, fy - 1.0);
                let cell = 2.0 * half(n) / (n - 1) as f32;
                let gx = dx * vheight(n) / (2.0 * cell);
                let gy = dy * vheight(n) / (2.0 * cell);
                let inv = 1.0 / (1.0 + gx * gx + gy * gy).sqrt();

                // `1 - normal.y`: 0 flat, 1 vertical. The same number
                // `terrain_mesh` hands the palette.
                let mut rgb = palette(h.clamp(0.0, 1.0), 1.0 - inv.clamp(0.0, 1.0), sea);

                // Relief shading from a north-west sun, which is the cartographic
                // convention and the reason a printed contour map reads as
                // terrain rather than as contours.
                let normal = [-gx * inv, inv, -gy * inv];
                let sun = normalize3([-0.55, 0.75, -0.35]);
                let lambert =
                    (normal[0] * sun[0] + normal[1] * sun[1] + normal[2] * sun[2]).clamp(0.0, 1.0);
                let shade = 0.5 + 0.6 * lambert;
                for c in &mut rgb {
                    *c = (*c * shade).clamp(0.0, 1.0);
                }

                if wet {
                    let depth = bilinear(&self.water.depth, n, fx, fy);
                    // Measured from `MIN_POND`, exactly as `wetness_field` does.
                    // Without the subtraction the two views of one field disagree
                    // at the end of the run: every drained basin keeps a hair of
                    // depth, far too little for the water surface to draw, and the
                    // minimap painted it as a lake — so the map still showed water
                    // over a landscape that plainly had none.
                    let t =
                        ((depth - erosion::MIN_POND) / LAKE_OPAQUE_DEPTH).clamp(0.0, 1.0) * 0.85;
                    let water = [0.24, 0.53, 0.66];
                    for (c, w) in rgb.iter_mut().zip(water) {
                        *c += (w - *c) * t;
                    }
                }

                let i = (ty * MINIMAP_N + tx) * 4;
                self.minimap_rgba[i] = (rgb[0] * 255.0) as u8;
                self.minimap_rgba[i + 1] = (rgb[1] * 255.0) as u8;
                self.minimap_rgba[i + 2] = (rgb[2] * 255.0) as u8;
                self.minimap_rgba[i + 3] = 255;
            }
        }
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
            // of it is water-specific engine code — every line is a `Material`
            // any instance can set:
            //
            //   - a tight sun glint off the ripples,
            //   - a Fresnel edge that turns the surface toward what it reflects
            //     and closes it up as the view flattens,
            //   - refraction, which displaces the lake bed seen through the
            //     surface and lets thickness decide how much of the water's own
            //     colour the light picked up on the way — the term that reads at
            //     *every* camera angle, where the reflection is only ~3% at this
            //     one,
            //   - and a traced reflection, which is what puts the far bank in
            //     the lake below it instead of a flat blue.
            //
            // `with_refraction` implies `blended()`, which is the one that used
            // to be easy to miss: the per-vertex shore fade is invisible to the
            // pipeline choice, so without it dragging opacity to 1.0 would drop
            // the whole surface into the opaque pass and the soft shoreline would
            // snap back to a hard line.
            instances.push(
                Instance::at(water_handle).with_material(
                    Material::OPAQUE
                        .with_alpha(self.water_alpha)
                        .with_specular(self.water_specular, WATER_SHININESS)
                        .with_fresnel(WATER_FRESNEL_F0, WATER_REFLECTION_TINT)
                        // Waves are quoted per world unit, so a map four times wider
                        // would carry four times as many of them across the same
                        // view. Dividing by `span` keeps a wave the size it looks
                        // at the reference scale, whatever the map grew to.
                        .with_ripples(self.ripple_strength, self.ripple_scale / span(self.n))
                        .with_refraction(WATER_REFRACTION, WATER_ABSORPTION)
                        .with_reflection(WATER_REFLECTION),
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
        let vheight = vheight(n);
        let half = half(n);
        let sea = self.erosion.sea_level;
        let disp = |i: usize| disp(self.heights[i], vheight);

        let step = (2.0 * half) / (n as f32 - 1.0);
        let cell_world = step; // horizontal spacing for slope/normal estimates

        let mut vertices = Vec::with_capacity(n * n);
        for y in 0..n {
            for x in 0..n {
                let i = y * n + x;
                let wx = -half + x as f32 * step;
                let wz = -half + y as f32 * step;
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

                let t = (wy / vheight).clamp(0.0, 1.0);

                vertices.push(Vertex {
                    position: [wx, wy, wz],
                    normal,
                    color: {
                        let c = palette(t, slope, sea);
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
        // load-bearing rather than tidy. A basin that has silted almost to its
        // spill point still holds a hair of water across its whole floor — a
        // fifteenth of the map, by the end of the run — and a *continuous* field
        // happily draws it. The result is a faint blue-green film over every
        // drained basin that reads as the terrain having changed colour rather
        // than as water being present. (It used to be worse and for a second
        // reason: the flood itself lifted each cell a hair above the one it was
        // reached from, strewing ε depths over dry ground. `erosion` no longer
        // does that — see `MIN_POND`.)
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
        // Both of the river's numbers are in **cells**, and a cell is not a fixed
        // amount of ground once the resolution moves, so both are quoted at the
        // reference grid and scaled here. A catchment of a given real size covers
        // `span²` cells, and a channel of a given real width spans `span` of them.
        // Without this the same landscape at 2048² would be threaded by rivers a
        // quarter as wide draining catchments a sixteenth as large — every gully
        // promoted to a river, and each one drawn as a hairline.
        let span = span(n);
        let river_area = self.river_area.max(1.0) * span * span;
        let narrow = RIVER_HALF_WIDTH * span;
        let widest = RIVER_HALF_WIDTH_MAX * span;
        for c in 0..n * n {
            // Deliberately *not* skipping cells that are already lake, which is
            // the obvious optimisation and punches holes in the rivers. A cell
            // whose depth sits just above `MIN_POND` would be dropped here while
            // contributing almost nothing as lake — it falls through the gap
            // between the two rules — and a channel crossing a silted-up basin
            // runs through exactly that band, so the network ends up riddled with
            // single-point dry spots. Each one shows up as a little diamond of
            // bare ground, because its four surrounding cells each contour around
            // it. Splatting regardless costs nothing: the combine below is a
            // `max`, and a channel drawn across a lake that is already fully wet
            // changes not one pixel.
            let ratio = self.water.area[c] / river_area;
            if ratio < RIVER_FADE {
                continue;
            }
            let r = self.water.receiver[c];
            if r == c {
                continue;
            }
            // How strongly this link draws. One at the threshold and above, so a
            // real river is untouched; fading to nothing over the approach to it,
            // so a headwater tapers out instead of starting on a grid cell. See
            // `RIVER_FADE`.
            let strength = smoothstep(((ratio - RIVER_FADE) / (1.0 - RIVER_FADE)).clamp(0.0, 1.0));
            // Physically a channel widens with the square root of its discharge,
            // which is also what reads correctly: doubling the catchment should be
            // visible but not dramatic. The floor is the narrowest ribbon the grid
            // can carry, so below the threshold the channel stops getting *fainter*
            // rather than thinner — a sub-cell ribbon would fall between samples and
            // break up, which is the artifact this is here to remove.
            let half = (narrow * ratio.sqrt()).clamp(narrow, widest);

            let (ax, ay) = ((c % n) as f32, (c / n) as f32);
            let (bx, by) = ((r % n) as f32, (r / n) as f32);

            let lo_x = (ax.min(bx) - half).floor().max(0.0) as usize;
            let hi_x = (ax.max(bx) + half).ceil().min(n as f32 - 1.0) as usize;
            let lo_y = (ay.min(by) - half).floor().max(0.0) as usize;
            let hi_y = (ay.max(by) + half).ceil().min(n as f32 - 1.0) as usize;

            for gy in lo_y..=hi_y {
                for gx in lo_x..=hi_x {
                    let d = point_segment_distance(gx as f32, gy as f32, ax, ay, bx, by);
                    let v = strength * (1.0 - d / half);
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
            .map(|(h, d)| disp(h + d, vheight(n)))
            .collect();

        let step = (2.0 * half(n)) / (n as f32 - 1.0);
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

        // A grid point's vertex, made once and shared by every cell that wants it.
        // `u32::MAX` means "not built yet"; the wet interior is the overwhelming
        // majority of a map with an ocean on it, so nearly every vertex is made
        // once and used six times.
        //
        // **This is what lets the map reach 2048².** Every clipped polygon carries
        // its own corners, which is unavoidable along a shoreline and pure waste in
        // open water: a fully submerged cell emits six vertices that its neighbours
        // emit again. An all-ocean 2048² map asked for a 780 MB vertex buffer and
        // wgpu refused it — the default `max_buffer_size` is 256 MB — so the biggest
        // world was not a memory problem to be tuned around, it simply would not
        // start. Sharing the interior takes the same geometry to 168 MB.
        let mut shared = vec![u32::MAX; n * n];

        for y in 0..n - 1 {
            let row = y * n;
            for x in 0..n - 1 {
                // Reject the whole cell before touching either triangle. On a
                // landlocked map water covers well under a fifth of the grid, so
                // this skips most of it on one comparison chain instead of six.
                let corners = [row + x, row + x + 1, row + n + x, row + n + x + 1];
                if corners.iter().all(|&i| wet[i] < WET_EPS) {
                    continue;
                }
                // The whole cell is under water: it needs no clipping, and its four
                // corners are grid points the neighbours will want too.
                if corners.iter().all(|&i| wet[i] >= WET_EPS) {
                    for tri in TRIS {
                        for (dx, dy) in tri {
                            let (gx, gy) = (x + dx, y + dy);
                            let slot = gy * n + gx;
                            if shared[slot] == u32::MAX {
                                shared[slot] = vertices.len() as u32;
                                vertices.push(
                                    self.water_vertex(gx as f32, gy as f32, step, &wet, &surface),
                                );
                            }
                            indices.push(shared[slot]);
                        }
                    }
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
            position: [
                -half(n) + fx * step,
                height + WATER_LIFT,
                -half(n) + fy * step,
            ],
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
        let mut pass_hz = self.pass_hz;
        let mut single_step = false;
        let mut rebake = false;
        let pass = self.pass;
        let last_pass = max_pass(self.n);

        // Borrowed before the UI takes the renderer, and read only inside the
        // panel closure. `self` and `renderer` are disjoint, so these live
        // happily alongside `ui`.
        let lake_series: &[f32] = &self.series[0];
        let moved_series: &[f32] = &self.series[1];
        // Where along the axis the landscape on screen is sitting, in `0..=1`.
        let scrub_t = pass as f32 / last_pass as f32;
        let minimap = self.minimap;

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
                    ui.section("Bake", |ui| {
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
                        // A readout and a bar, not a slider. There is no history to
                        // scrub back into any more — that memory is what bought the
                        // map its size — so this reports rather than steers.
                        ui.label_value("pass", &format!("{pass}/{last_pass}"));
                        progress_bar(ui, scrub_t);
                        // Every row here is unconditional, and that is a layout
                        // requirement rather than a simplification. The first shape
                        // this took swapped the rate slider for a button when the
                        // bake ended — and moved every row beneath it at the moment
                        // a *background process* finished, which reflowed the panel
                        // out from under a capture script's own coordinates. That is
                        // the trap `capture/editor-list.script` was written to
                        // record, arriving from a new direction: not a click that
                        // moves what it is about to click, but a panel that moves on
                        // its own. The rate stays live because "bake again" will use
                        // it.
                        ui.slider("passes/sec", &mut pass_hz, PASS_HZ_MIN, PASS_HZ_MAX)
                            .decimals(0)
                            .show();
                        rebake = ui
                            .button("bake again")
                            .variant(Variant::Secondary)
                            .show()
                            .clicked;
                    });
                    ui.separator();

                    // --- The history the demo has always measured and never shown ---
                    //
                    // Directly under Time on purpose: it reads as part of the same
                    // axis, and it keeps every coordinate a capture script clicks
                    // *above* it, so opening or closing this section reflows only
                    // the rows beneath and moves nothing scripted.
                    ui.section("Plot", |ui| {
                        let r = ui.allocate([0.0, PLOT_H]);
                        plot(
                            ui,
                            r,
                            &[lake_series, moved_series],
                            &[theme.color.accent, theme.color.heading],
                            last_pass,
                            Some(scrub_t),
                            true,
                        );
                        ui.label_muted("lake depth / moved, per pass");

                        // The whole basin from above, which is the half of the
                        // erosion the 3D view cannot show you: from inside a
                        // valley you can see that a lake drained, not where the
                        // water went. Square, centred, and drawn straight from
                        // pixels the demo shaded itself.
                        if let Some(id) = minimap {
                            let row = ui.allocate([0.0, MINIMAP_VIEW]);
                            let x = row.x + (row.w - MINIMAP_VIEW) * 0.5;
                            let box_ = Rect::new(x, row.y, MINIMAP_VIEW, MINIMAP_VIEW);
                            let painter = ui.painter();
                            painter.image_full(box_, id, [1.0, 1.0, 1.0, 1.0]);
                            painter.stroke_rect(
                                box_,
                                theme.radius.sm,
                                theme.control.border,
                                theme.color.border,
                            );
                        }
                    });
                    ui.separator();

                    // --- Layer 1: the Perlin base shape ---
                    let mut base = false;
                    let mut preset = None;
                    // Sea level is shaped like a base-shape control and behaves like
                    // an erosion one, so it is declared out here and folded into
                    // `erode` below.
                    let mut erode_sea = false;
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
                        // The two that decide there is a coastline at all. `coast`
                        // shapes the land and so regenerates the noise; `sea level`
                        // only moves the base level the erosion drains to, so it is
                        // a re-bake and not a re-roll — the same continent, drowned
                        // to a different line.
                        base |= ui
                            .slider("coast", &mut self.params.coast, 0.2, 1.0)
                            .show()
                            .changed;
                        erode_sea = ui
                            .slider("sea level", &mut self.erosion.sea_level, 0.0, 0.6)
                            .show()
                            .changed;
                    });
                    ui.separator();

                    // --- Layer 2: erosion ---
                    let mut erode = erode_sea;
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
                        // The slider runs over *indices* into `RESOLUTIONS`, so the
                        // steps are octaves and every one of them is a size worth
                        // having. The label carries the real number, because "4" is
                        // not a grid size anybody recognises.
                        let idx = (self.res.round().max(0.0) as usize).min(RESOLUTIONS.len() - 1);
                        let cells = RESOLUTIONS[idx];
                        ui.label_value("resolution", &format!("{cells}x{cells}"));
                        ui.slider("detail", &mut self.res, 0.0, (RESOLUTIONS.len() - 1) as f32)
                            .decimals(0)
                            .show();
                        // What that resolution buys, in the two units that matter
                        // and neither of which is cells: how much ground the map
                        // covers, and how long the bake will take.
                        ui.label_muted(&format!(
                            "{:.0}x the land, {} passes",
                            span(cells) * span(cells),
                            max_pass(cells)
                        ));
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
            ui.label_value("pass", &format!("{pass}/{last_pass}"));
            // The same routine as the panel's, at a quarter the height and with
            // the annotations off. Two call sites is the check that the demo's
            // chart code generalised rather than being fitted to one rectangle.
            let r = ui.allocate([0.0, SPARK_H]);
            plot(
                ui,
                r,
                &[lake_series],
                &[theme.color.accent],
                last_pass,
                Some(scrub_t),
                false,
            );
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
        if rebake {
            self.restart_bake();
            renderer.time_mut().seek(0.0);
        }

        if reset {
            self.params = NoiseParams::default();
            self.erosion = ErosionParams::default();
            self.res = res_index(RES_DEFAULT) as f32;
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
        // Everything the camera does is in world units, so all of it scales with
        // the map. Left fixed, the wheel moved a sixteenth as far per notch on the
        // big continent and the orbit's outer stop sat *inside* the mountains —
        // the 1024² world's first screenshot was taken from between two peaks.
        let span = span(self.n);
        self.distance -= scroll * 0.5 * span;
        self.pitch = self.pitch.clamp(0.08, 1.5);
        self.distance = self.distance.clamp(2.5 * span, 18.0 * span);

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
            .look_from_to(eye, [0.0, vheight(self.n) * 0.35, 0.0]);
    }
}

impl Application for TerrainDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        self.upload(renderer);
        // Created once, at a fixed size, and rewritten in place from then on.
        self.shade_minimap();
        self.minimap =
            Some(renderer.create_image(MINIMAP_N as u32, MINIMAP_N as u32, &self.minimap_rgba));
        self.minimap_for = self.resolved;
    }

    /// One erosion pass, and nothing else.
    ///
    /// **The entire simulation lives here**, which is what buys the transport
    /// controls above: the landscape advances only when the engine says a fixed
    /// step is due, so pausing genuinely stops geology rather than merely freezing
    /// a camera. Nothing in [`Application::update`] moves the terrain — it only
    /// decides how to *draw* whatever pass the bake has reached.
    ///
    /// `dt` is ignored, and that is honest rather than lazy. A pass is not defined
    /// in seconds: the model's timestep is folded into the erodibility `K` (see
    /// [`ErosionParams::erodibility`]), so a pass *is* the unit of simulation time.
    /// The step rate is set to the pass rate for exactly this reason, which makes
    /// `dt` a constant the hook has no use for.
    ///
    /// **At most one pass per fixed step, and the engine is told to stop asking
    /// for more.** A pass at 2048² takes over a second, so a clock that tries to
    /// make up the passes it owes would never catch up and would take the frame
    /// rate down with it. Capping the rate at what the machine actually delivered
    /// turns "too slow" into "bakes more slowly", which is the failure mode a
    /// person can sit through.
    fn fixed_update(&mut self, _renderer: &mut Renderer, _dt: f32) {
        // The engine will call this up to eight times in one frame when it is
        // behind. Take only the first: eight passes at 2048² is a twelve-second
        // frame, and the clock discards the backlog anyway, so obeying it would
        // buy nothing but a stall. One pass per frame makes "the machine cannot
        // keep up" mean "the bake runs slower", which is a thing to watch rather
        // than a hang.
        if self.baking && !self.baked_this_frame {
            self.baked_this_frame = true;
            self.bake_pass();
        }
    }

    fn update(&mut self, renderer: &mut Renderer) {
        // Smooth the FPS readout (exponential moving average).
        let dt = renderer.dt();
        if dt > 0.0 {
            self.fps = self.fps * 0.9 + (1.0 / dt) * 0.1;
        }

        // Before the UI, not inside it: `renderer.ui()` holds the renderer for as
        // long as the panels are being built, so an image cannot be uploaded from
        // inside a panel closure. Gated on the same `(pass, alpha)` the meshes
        // are, so a paused frame reshades nothing.
        let shaded_for = self.resolved;
        if self.minimap_for != shaded_for {
            self.shade_minimap();
            if let Some(id) = self.minimap {
                renderer.update_image(id, &self.minimap_rgba);
            }
            self.minimap_for = shaded_for;
        }

        let outcome = self.build_ui(renderer);
        self.pending_base |= outcome.regen_base;
        self.pending_erode |= outcome.reerode;

        // Snap the resolution control to a listed size; a resolution change needs
        // a full rebuild.
        let idx = (self.res.round().max(0.0) as usize).min(RESOLUTIONS.len() - 1);
        self.res = idx as f32;
        let target_n = RESOLUTIONS[idx];
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
                // Carry the camera across the change of scale: the viewer was
                // looking at the map from some fraction of its width away, and
                // should still be after it grows.
                self.distance *= span(target_n) / span(self.n);
                self.n = target_n;
                self.regenerate_base();
            } else {
                // An erosion constant changed, so everything carved so far was
                // carved under the old one. The world starts again from the base
                // rather than being patched — see `restart_bake`.
                self.restart_bake();
            }
            self.pending_base = false;
            self.pending_erode = false;
            dirty = true;
        }

        // Rebuild the meshes only for a pass they have not been built for. Once
        // the bake finishes that is never again, which is the point: a finished
        // world at 2048² is four million vertices, and rebuilding it every frame
        // to draw the identical picture would cost more than everything else in
        // the demo put together.
        if self.resolved != Some(self.pass) {
            self.resolved = Some(self.pass);
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
        self.baked_this_frame = false;
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

/// Height/slope color palette: seabed → green lowlands → tan slopes → gray rock →
/// snow, with steep faces biased toward bare rock regardless of altitude.
///
/// `t` is the normalized height and `sea` the level below which the ground is
/// under the ocean. **The seabed stops are not decoration.** The land palette
/// starts at valley green, so before they existed the whole drowned shelf — which
/// is most of the map once there is a coastline — was painted as meadow, showing
/// as a bright green ring glowing through the shallows and as a hard green line
/// wherever the ocean's edge and the terrain's edge did not project to the same
/// place. Ground under water is sand and then silt, and painting it that way is
/// both what it looks like and what stops it drawing attention to itself.
fn palette(t: f32, slope: f32, sea: f32) -> [f32; 3] {
    // Below the waterline: sand at the shore, darkening into the deep. Squeezed
    // into `[0, sea]` so the land ramp keeps the whole range above it and looks
    // exactly as it always did.
    if t < sea {
        let f = (t / sea.max(1e-4)).clamp(0.0, 1.0);
        let deep = [0.16, 0.22, 0.30];
        let sand = [0.55, 0.52, 0.40];
        return [
            deep[0] + (sand[0] - deep[0]) * f,
            deep[1] + (sand[1] - deep[1]) * f,
            deep[2] + (sand[2] - deep[2]) * f,
        ];
    }
    let t = ((t - sea) / (1.0 - sea).max(1e-4)).clamp(0.0, 1.0);
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
