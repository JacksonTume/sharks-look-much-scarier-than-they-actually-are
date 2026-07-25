# slmsttaa-ui

The UI toolkit for [SLMSTTAA](../README.md) — an immediate-mode widget layer that
knows nothing about GPUs, windows, or terrain.

It is a **separate crate on purpose**. The engine renders 3D; this renders
controls. Keeping them apart is what stops a rendering engine from quietly
becoming a UI framework with a triangle demo attached.

## Status

Slices [0](ROADMAP.md#slice-0--extraction-the-move) (extraction) and
[1](ROADMAP.md#slice-1--interaction-core--draw-layers) (interaction core) are
done. The toolkit is a zero-dependency crate the engine re-exports as
`slmsttaa::ui`, and it now has the machinery that separates a toolkit from a
pile of sliders:

- **Ids** — `hash(scope, label)`, with `push_id`/`pop_id`. Never keyed by
  declaration order, so a row appearing above a widget can't steal its identity
  mid-drag.
- **Interaction** — `hot` / `active` / `focused`, and a `Response` from every
  widget.
- **Draw layers** — base / panel / popup / tooltip, flushed in order, still one
  draw call.
- **A public seam** — `allocate` / `interact` / `painter` / `theme`, so a widget
  written by a consumer is not second-class.

Widgets: `title` / `section` (collapsible) / `label` / `label_muted` /
`separator` / `button` / `checkbox` / `slider`.

Next is [Slice 2](ROADMAP.md#slice-2--painter-capabilities-and-the-scroll-region):
rounded rects, borders, and clipping — plus the scroll region that clipping
unblocks.

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
   ▲             the widgets, and its own UiInput snapshot type.
   │ path dep
   │
slmsttaa/        depends on slmsttaa-ui. impl Painter for Overlay;
                 translates engine Input → UiInput once per frame;
                 re-exports the toolkit as `slmsttaa::ui`.
```

This crate defines its *own* minimal input snapshot — `UiInput { cursor,
primary_held, primary_pressed }` — and the engine copies into it each frame in
`Renderer::ui()`. Three field assignments, nothing measurable, and it buys a leaf
crate with no `wgpu`, no `winit`, no `glam`, and therefore no reason to ever grow
a `#[cfg(target_arch = "wasm32")]` branch.

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
  can fill a rectangle and stamp a string can host this UI.
- **Upward, from the consumer** — widgets borrow the consumer's own `&mut f32` /
  `&mut bool`. The toolkit has no idea what it controls; erosion parameters live
  in the terrain demo, which is where root principle 3 puts them.

## What "shadcn, rolled our own" means here

shadcn/ui is the reference point for the *ambition* — a set of controls that read
as one designed system rather than a debug HUD. Three of its ideas port; the
third only in translation:

1. **Design tokens.** Semantic names (`background`, `foreground`, `muted`,
   `accent`, `border`, `ring`, `destructive`), a radius scale, a spacing scale, a
   type scale — held in a `Theme` struct. Widgets never name a literal color.
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
`tests/layout.rs` pins where widgets land; `tests/interaction.rs` pins press-edge
versus held semantics and drag capture. Both drive the crate through its public
API only, which doubles as a check that a consumer could do the same.

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
