# slmsttaa-ui

The UI toolkit for [SLMSTTAA](../README.md) — an immediate-mode widget layer that
knows nothing about GPUs, windows, or terrain.

It is a **separate crate on purpose**. The engine renders 3D; this renders
controls. Keeping them apart is what stops a rendering engine from quietly
becoming a UI framework with a triangle demo attached.

## Status

Slices [0](ROADMAP.md#slice-0--extraction-the-move) (extraction),
[1](ROADMAP.md#slice-1--interaction-core--draw-layers) (interaction core),
[2](ROADMAP.md#slice-2--painter-capabilities-and-the-scroll-region) (painter
capabilities), [3](ROADMAP.md#slice-3--layout) (layout),
[4](ROADMAP.md#slice-4--theme-tokens--variants) (theme tokens),
[5](ROADMAP.md#slice-5--typography-polish-labeled) (typography) and
[6](ROADMAP.md#slice-6--animation-polish-labeled) (animation) are done. The
toolkit is a zero-dependency crate the engine re-exports as `slmsttaa::ui`, and it
now has the machinery that separates a toolkit from a pile of sliders:

- **Ids** — `hash(scope, label)`, with `push_id`/`pop_id`. Never keyed by
  declaration order, so a row appearing above a widget can't steal its identity
  mid-drag.
- **Interaction** — `hot` / `active` / `focused`, and a `Response` from every
  widget.
- **Draw layers** — base / panel / popup / tooltip, flushed in order, still one
  draw call.
- **Painter capabilities** — rounded rects, strokes, and nesting clip regions
  that intersect rather than replace.
- **Layout** — a stack of regions rather than a `y` cursor. Panels anchored to a
  corner and sized by their caller, plus `horizontal` / `right` / `columns` /
  `indent` / `sized`, all closure-scoped so the stack cannot desync.
- **Theme tokens** — one `Theme` of semantic colors and four scales, plus
  `Variant` and `Size`. No widget anywhere names a literal color, and there is a
  test that says so.
- **Typography** — Inter as a baked signed distance field, proportional advances,
  two weights, and tabular figures so a live readout doesn't shuffle sideways as
  its digits change. Metrics live in [`font`](src/font.rs) rather than on the
  painter, which is the whole point — see *The two seams* below.
- **Animation** — one eased float per `(widget, property)`, converging in
  wall-clock time rather than in frames, so a fade is the same length at 60 and
  144 Hz. Hover and press fades, focus rings, a growing slider knob, smooth
  scrolling, and collapsing sections. Turning it off is a theme value
  (`Motion::none()`), not a flag anything checks.
- **A public seam** — `allocate` / `interact` / `painter` / `theme` / `animate`,
  so a widget written by a consumer is not second-class.

Widgets: `title` / `section` (collapsible) / `label` / `label_muted` /
`label_value` / `separator` / `button` / `checkbox` / `slider` / `scroll_area`.

**Nothing is scheduled next, and that is the intended state.** Every slice above
was pulled into existence by something the terrain demo could not do; it can now
do all of it. The next piece of UI work should arrive from a demo hitting a wall,
not from the roadmap — see [*Waiting on a
roadblock*](ROADMAP.md#waiting-on-a-roadblock) for what is recognized but
deliberately unbuilt.

**New UI code and UI docs belong here, not in the engine.**

## Why its own crate

Not for reuse, and not to insulate the engine from change. For **enforcement**.

The old `src/ui.rs` claimed, in its module doc, that "the UI never sees `wgpu`".
That was a convention — nothing stopped a late-night edit from reaching into
`Renderer` internals because it was convenient. A crate boundary turns the claim
into a compile error, exactly the way `examples/` turns the engine/consumer
boundary into a build failure (root roadmap principle 1).

What it does **not** do is eliminate engine churn. Growing the toolkit means
growing what a painter can draw — rounded corners, clipping, draw layers — and
each of those lands in `renderer/overlay.rs`, `overlay.wgsl`, and the `Vertex2D`
layout. The crate boundary doesn't prevent that work; it forces it to arrive as a
deliberate widening of the [`Painter`](#the-two-seams) trait rather than as a
private reach-through. That is the whole benefit, and it is worth the split.

## Dependency direction

The one decision that is expensive to get wrong. The pre-split `src/ui.rs` did
`use crate::input::{Input, MouseButton}` — if this crate imported `Input` from
the engine while the engine imported `Painter` from here, that's a dependency
cycle.

So the rule is: **`slmsttaa-ui` depends on nothing.**

```
slmsttaa-ui/     zero dependencies. Owns Painter, Color, Rect, Theme, Ui,
   ▲             the widgets, the font (as data), and its own UiInput snapshot.
   │ path dep
   │
slmsttaa/        depends on slmsttaa-ui. impl Painter for Overlay;
                 translates engine Input + surface size → UiInput once
                 per frame; re-exports the toolkit as `slmsttaa::ui`.
```

This crate defines its *own* minimal host snapshot — `UiInput { cursor,
primary_held, primary_pressed, scroll_delta, viewport }` — and the engine copies
into it each frame in `Renderer::ui()`. Four field assignments, nothing
measurable, and it buys a leaf crate with no `wgpu`, no `winit`, no `glam`, and
therefore no reason to ever grow a `#[cfg(target_arch = "wasm32")]` branch.

The font does not change that, which was worth checking rather than assuming: the
atlas is `include_bytes!`d and its metrics are generated Rust, so `cargo tree -p
slmsttaa-ui` still prints exactly one line. **Data is not a dependency.** The
rasterizer that produced both lives in the `fontbake` workspace member and runs by
hand — quarantined there so that even `xtask` stays dependency-free.

`viewport` is the whole of what edge anchoring needs: a `TopRight` panel has to
know where the right edge is, and this is how it finds out without the toolkit
ever learning what a window is. Like the cursor, it arrives already divided by
the scale factor, so everything in `theme` keeps meaning one thing on every
display.

It is narrower than the engine's `Input` on purpose: one pointer, one button, no
keys, because that is all any widget reads today. Typed characters and modifiers
arrive with text input, which is [waiting on a
roadblock](ROADMAP.md#waiting-on-a-roadblock) — not built ahead of one.

The rejected alternatives, recorded so they aren't relitigated: moving
`src/input.rs` into this crate (input isn't UI-specific — the camera reads it
too), and adding a third `slmsttaa-core` crate for shared primitives (more
ceremony than one consumer earns; root principle 4).

## The two seams

Both are inherited from the current design and both survive the split:

- **Downward, from the renderer** — the toolkit draws through the `Painter`
  trait and nothing else. The engine's `renderer::overlay::Overlay` is one
  implementation; a headless recorder used by the tests is another. Anything that
  can fill a rectangle and stamp a glyph can host this UI.

  Slice 5 **narrowed** this seam, the only time that has happened. `text_size`
  used to be on the trait, which meant every painter had its own idea of how wide
  a string is — and the two agreed only because the old bitmap font was a
  monospace grid. Proportional advances would have made them disagree *silently*:
  the tests measure through the recorder, so the suite would have stayed green
  while the demo's readouts slid off the edge of the panel. Measuring now lives in
  [`font`](src/font.rs), and a painter no longer chooses its font. A seam wide
  enough for two fonts is a seam wide enough for two disagreeing fonts.
- **Upward, from the consumer** — widgets borrow the consumer's own `&mut f32` /
  `&mut bool`. The toolkit has no idea what it controls; erosion parameters live
  in the terrain demo, which is where root principle 3 puts them.

## What "shadcn, rolled our own" means here

shadcn/ui is the reference point for the *ambition* — a set of controls that read
as one designed system rather than a debug HUD. Three of its ideas port; the
third only in translation:

1. **Design tokens.** Semantic names (`background`, `foreground`, `muted`,
   `accent`, `border`, `ring`, `destructive`), a radius scale, a spacing scale, and
   a type scale whose steps pair a size *with* a weight — held in a `Theme` struct.
   Widgets never name a literal color.
   **Shipped in Slice 4**, along with `Variant` and `Size`. The rule is enforced
   rather than asserted: `tests/theme.rs` styles a frame with sentinel tokens and
   fails if any color reaches the painter that the theme didn't supply.
2. **Headless behavior underneath styling.** shadcn gets this from Radix; here it
   is the interaction core — id stack, hot/active/focused, a `Response` returned
   by every widget, and ordered draw layers so popovers land on top of what
   spawned them. Without this layer, Select/Popover/Tooltip/Tabs are not
   implementable at all.
3. **You own the components.** There is no `npx shadcn add` for a Rust crate, but
   the transferable half is stronger: **the widget layer must be unprivileged.**
   If `Ui` exposes `allocate(size) -> Rect`, `interact(rect, id) -> Response`, and
   `painter()`, then a consumer can write a widget this crate never shipped, using
   only the public API.

That last one is a testable property, not an aspiration. The terrain demo wants a
curve editor for the erosion falloff; it gets written **in the demo**, from public
API. If a second consumer wants the same widget, it has earned its way in here.
That is the same demand-driven rule the root roadmap applies to the engine, one
layer down — and it is what keeps the widget roster from becoming the project.

## Scope

**In:** the substrate — painter capabilities, interaction core, layout, theme
tokens, animation — plus the widgets a real demo hit a wall without.

**Out:** a general-purpose GUI framework, a component roster grown for its own
sake, accessibility beyond keyboard navigation, i18n/RTL, and anything published
to crates.io. This crate is `publish = false` and built for this project.

The stopping rule: **if a UI change can't name the demo roadblock that demanded
it, it's polish.** Polish is allowed — it just gets labeled as its own slice and
time-boxed, rather than smuggled in as infrastructure.

## Testing

The zero-dependency leaf shape makes this crate the one place in the repo that is
genuinely easy to test: no GPU, no window, no async. [`RecordingPainter`] collects
draw commands into a `Vec`, so layout math and hit-testing are asserted directly —
`cargo test -p slmsttaa-ui`.

[`RecordingPainter`]: src/painter.rs

These are the project's first and only tests, which is fair: layout and
hit-testing were simultaneously the most testable and least verified code here.
`tests/layout.rs` pins where panels and widgets land; `tests/regions.rs` pins the
row/column/indent/right-align arithmetic; `tests/interaction.rs` pins press-edge
versus held semantics and drag capture; `tests/ids.rs` pins that a widget's
identity survives rows appearing above it; `tests/clipping.rs` pins that a
scrolled-away row is genuinely invisible *and* that no run strays under the
scrollbar; `tests/theme.rs` pins that no widget draws a color or reads a metric the
theme didn't supply; `tests/typography.rs` pins proportional advances, tabular
digits, em-linear metrics, and that an unbaked character draws a visible box rather
than vanishing; `tests/virtual_rows.rs` pins that a virtualized list places only
the rows its viewport covers *and* puts them exactly where a real one would. All of
them drive the crate through its public API only, which doubles as a check that a
consumer could do the same.

That last file is the one place the recorder does more than constrain. Everywhere
else it checks arithmetic a person could also have checked by looking; there, the
draw list **is** the claim — "these rows were built and no others" is not something
a screenshot can report, because a virtualized list and a real one are supposed to
look identical.

They constrain, but they do not replace running the demo. Slice 1's id bug,
Slice 3's overflowing button label, and Slice 5's readouts running under the
scrollbar all passed every test and were found the first time a human looked at
the screen — because a test declares a fixed set of widgets with strings it chose
itself, and the demo does not.

The scrollbar one is the sharpest of the three, because the suite was not merely
silent about it: the bug had been live since Slice 2, and the *font* was hiding it.
`font8x8` left roughly a quarter of every glyph cell blank on the right, so a
right-aligned readout's box ran under the bar while its ink cleared it. Replacing
the font removed that donated slack and the bug surfaced on the first frame — which
makes changing a font an accidental fuzz test for every layout assumption in the
crate.

## See also

- [`ROADMAP.md`](ROADMAP.md) — the UI slice sequence, and what each one is
  waiting on.
- [`WISHLIST.md`](WISHLIST.md) — what a second, data-dense consumer would demand.
  Recognized, not scheduled.
- [`../ROADMAP.md`](../ROADMAP.md) — the engine roadmap and the six guiding
  principles this crate inherits.
- [`../ARCHITECTURE.md`](../ARCHITECTURE.md) — how the overlay pass (the engine
  half of the `Painter` seam) actually works, and the cross-platform gotchas
  behind it.
