# UI Roadmap

Where the UI toolkit is going, and what has to demand each step before it gets
built. This is the UI counterpart to the engine's [`../ROADMAP.md`](../ROADMAP.md)
— that one sequences 3D capability, this one sequences controls.

**UI work is tracked here, not in the engine roadmap.** The engine roadmap keeps
a one-line pointer and moves on.

## The goal

Controls that read as **one designed system** — the way a good component library
does — while staying an immediate-mode toolkit small enough for one person to
hold in their head.

The litmus test mirrors the engine's: a demo author should expose a parameter,
compose a panel, or write a *custom widget the toolkit never shipped* using only
the public API, and never think about vertices, atlases, or draw order.

## Inherited principles

All six root principles apply unchanged. Three bind hardest here, plus one this
layer adds:

1. **Demand-driven** (root principle 2). Every slice below names the demo
   roadblock that pulls it. A widget with no roadblock is polish, and gets
   labeled as such.
2. **Push only generic plumbing down** (root principle 3). Painter capabilities,
   interaction, layout, and tokens are substrate — they belong here. The widget
   *roster* is content, and content grows only when a second consumer wants the
   same thing.
3. **Smallest surface that holds the boundary** (root principle 4). The toolkit
   depends on nothing. See [README § Dependency direction](README.md#dependency-direction).
4. **The widget layer is unprivileged.** Anything this crate ships must be
   re-implementable by a consumer from public API alone. If a widget needs
   private access, the public seam is wrong — fix the seam, don't grant the
   privilege.

## Definition of done (every UI slice)

- Builds on native (`cargo build`) **and** wasm
  (`cargo build --target wasm32-unknown-unknown --lib`).
- `cargo clippy --all-targets` clean, `cargo fmt --all` run.
- The driving demo shows the new capability on screen, on both targets.
- New public API has rustdoc; this file and
  [`../ARCHITECTURE.md`](../ARCHITECTURE.md) updated if a seam moved.
- **`slmsttaa-ui` still has zero dependencies** and still contains no
  consumer-specific content.
- Any new layout or hit-testing logic has a test against the recording painter.

---

## Slice 0 — Extraction (the move) ✅ done

*Roadblock:* none — this is the prerequisite. Everything below assumes the crate
exists, and doing the move *during* the Slice 1 rewrite is cheaper than a second
migration afterward.

- New workspace member `slmsttaa-ui`, `publish = false`, **zero dependencies**.
- `src/ui.rs` moves here, split into `theme.rs` / `interact.rs` / `layout.rs` /
  `widgets/` so later slices have somewhere to land.
- Invert the input dependency: this crate defines its own `UiInput` snapshot;
  the engine translates its `Input` into one per frame.
- Engine keeps `impl Painter for Overlay`, adds the path dependency, and
  re-exports the toolkit as `slmsttaa::ui` so consumers see no change.
- Add the `RecordingPainter` test double and the first tests.

*Proof:* `cargo tree -p slmsttaa-ui` prints exactly one line — the crate has no
dependencies, so `wgpu` and `winit` are now unreachable from it by construction,
and `slmsttaa::ui` still resolves for consumers (`examples/terrain.rs` is
unchanged). The 371-line `src/ui.rs` became `painter.rs` (the `Painter` seam plus
the `RecordingPainter` double), `interact.rs` (`UiInput` / `UiState` / id
hashing), `layout.rs` (`Rect` + the vertical cursor), `theme.rs` (every metric
and color), and `widgets/{text,button,slider}.rs`.

The input inversion landed as designed: `UiInput { cursor, primary_held,
primary_pressed }` is filled in by `Renderer::ui()` from the engine's `Input`
each frame, which is what breaks the would-be cycle. It is deliberately narrower
than the engine's input — one pointer, one button, no keys — because that is all
any widget reads today.

**Behavior is unchanged**, and there are now 11 tests plus a doctest to say so:
layout (row stacking, the panel's one-frame height lag, slider fill) and
hit-testing (press-edge vs. held, whole-row checkbox targets, drags surviving the
cursor leaving the track, duplicate labels not sharing a drag, `wants_pointer`).
These are the project's first tests, and they exist *because* of the split — none
of them need a GPU, a window, or an event loop.

*Also fixed along the way:* nothing. That is the point of a mechanical slice.

## Slice 1 — Interaction core + draw layers ✅ done

*Roadblock:* the terrain panel now runs off the bottom of the window at modest
heights, and the obvious fixes — collapsible sections, a scroll region — are both
blocked on the same missing thing: the toolkit has no notion of *which widget is
being interacted with*. `next_id()` exists but `button` and `checkbox` bind the
result to `_id` purely to keep the sequence in step, so there is no
hot/active/focused state, no press-inside-release-inside click semantics, and no
way to keep a popup alive across frames.

This is the keystone slice — the difference between "some sliders" and a toolkit.

- **Id stack**: parent-scoped ids, not a flat sequence counter.
- **Interaction state**: `hot` / `active` / `focused`, and a `Response { rect,
  hovered, clicked, changed, held }` returned by *every* widget.
- **Draw layers**: base / panel / popup / tooltip as separate accumulation
  buckets, flushed in order. Side win — this retires the "size the panel
  background from last frame's height" hack, since contents can be laid out
  first and the background drawn into a lower layer at the correct size.
- **The unprivileged seam**: `allocate` / `interact` / `painter` go public
  together, so a demo can build its own widget.
- **Engine-side:** `Overlay::flush` flushes ordered buckets instead of one
  buffer. Also fix DPI here — nothing reads `scale_factor` today, so the UI
  renders at half size on a 2× display, and the fix touches the same input
  translation this slice is already rewriting.

*Proof:* the terrain panel's sections collapse — click a heading and it folds to
a `+` caret, which is what puts the panel back inside the window — and the demo
ships **`log_slider`, written in `examples/terrain.rs`** from public API alone,
proving the unprivileged rule. It exists for a real reason, not as a
demonstration: erodibility spans four orders of magnitude, so the linear track it
used to have spent most of its length on indistinguishable values.

**What shipped, and what it cost:**

- **Ids** are `hash(enclosing scope, label)`, with `push_id`/`pop_id` for
  explicit scoping. Pointedly **not** keyed by declaration order — see *The bug
  that rewrote this* below, which is how that was settled. Two widgets sharing a
  label in one scope are re-hashed apart; `push_id` is the durable fix.
- **`Response { id, rect, hovered, held, clicked, changed, open }`** is returned
  by every widget, labels included — a label's rectangle is how a consumer will
  hang a tooltip on one. `open` is only ever `false` for a section.
- **Layers** landed as one index bucket per `Layer`, sharing a single vertex
  vector, concatenated in order at flush. The overlay is **still one draw call**.
  The promised side win is real and tested: the panel background is now emitted
  *last* (the only point at which its height is known) and painted *first*, so
  it is correct on frame one instead of lagging a frame.
- **DPI** is fixed by making the toolkit speak **logical points** and converting
  at both ends of the seam — `Renderer::ui()` divides the cursor by the scale
  factor, `Overlay` multiplies coordinates on the way to vertices. The toolkit
  never learns the scale factor, which is why `theme`'s numbers still mean one
  thing on every display.
- **The seam went public** as `allocate` / `interact` / `painter` / `next_id` /
  `input` / `mark_changed` — *and `theme` with it*. That last one
  was not in the plan: a consumer's widget cannot look like a built-in one while
  the metrics and colors are private, so the unprivileged rule forced the
  constants public. Slice 4 replaces them with a `Theme` of semantic tokens.

**The bug that rewrote this.** Ids originally mixed in a per-scope *sequence
number*, so a widget's identity depended on how many widgets were declared above
it. Every test passed. Then the demo was driven by hand and the sliders wouldn't
slide: clicking snapped the value, dragging did nothing.

The cause is worth recording, because it will look tempting again. The terrain
panel shows a `"release to rebuild..."` row only while a rebuild is pending — and
pending becomes true the instant a slider *first moves*. So the row appears
between the press frame and the next one, every widget below it renumbers, the
dragged slider's `active` claim stops matching anything, and the drag dies after
exactly one frame. A conditional row is an entirely ordinary thing to write; any
order-keyed id scheme is broken by it.

So ids are keyed on the label and scope alone. The lesson is not about hashing:
**every test passed because every test declared a fixed set of widgets.** The
demo is the only place a conditional row existed, which is precisely the case
root principle 2 exists to catch — the toolkit's blind spots are the ones the
driving demo is shaped to find, and only running it finds them.

**Deliberately not shipped: the scroll region.** It was in this slice's proof,
and it is moved to Slice 2 rather than quietly dropped. Scrolling without
clipping means content bleeding out over the 3D scene — visibly broken, not
merely unpolished — and clipping is precisely what Slice 2 adds. Collapsible
sections solve the stated roadblock (the panel running off the bottom) on their
own, so nothing is blocked by waiting.

## Slice 2 — Painter capabilities (and the scroll region)

*Roadblock:* the scroll region deferred out of Slice 1 needs clipping, which the
painter cannot do — it draws axis-aligned solid rectangles and nothing else. The
same gap is why everything has square corners and no borders.

Collapsible sections bought the panel enough room that scrolling is no longer
urgent, but it is still the thing clipping is *for*, so the two ship together:
add the capability, then the widget that needed it.

The highest visual return per line in the whole sequence, and cheaper than it
looks: rounded rects **and** clipping both fit in the shader without splitting
draw calls. Add per-vertex rect parameters (center, half-size, radius) and a clip
rectangle, evaluate a rounded-box SDF in the fragment shader for shapes, discard
outside the clip. One extra attribute pair; the single-draw-call design survives.

- `Painter` grows: rounded rects, strokes/borders, clip rects, soft shadows.
- A scrollable panel body built on the clip rect, which is the widget Slice 1
  stopped short of.
- **Engine-side:** `overlay.wgsl` and the `Vertex2D` layout. This is the one
  piece of the sequence with real native/web parity risk, which is an argument
  for doing it while the surface is still small.

*Proof:* rounded panels, 1px borders, focus rings, and a genuinely clipped scroll
region — identical on native and web. The focus ring is also the first thing to
*read* the `focused` id Slice 1 already tracks.

## Slice 3 — Layout

*Roadblock:* the terrain panel wants a preset row (several buttons side by side)
and right-aligned value readouts. Neither is expressible when layout is
`cursor_y += h` down a hard-coded `PANEL_X` / `PANEL_W`.

- Allocate-from-available-rect instead of a bare vertical cursor.
- `horizontal()`, `columns(n)`, `indent()`, right-alignment.
- Panels anchored to any edge, so the HUD and the parameter panel stop being the
  same fixed rectangle.

*Proof:* the terrain demo shows a button row, right-aligned values, and a second
panel anchored opposite the first.

## Slice 4 — Theme tokens + variants

*Roadblock:* by here there are enough widgets that nine `const Color`s no longer
hold the look together, and the demo wants a destructive-styled "reset" action
that shouldn't be a hand-colored rectangle.

- A `Theme` struct of semantic tokens (background / foreground / muted / accent /
  border / ring / destructive, radius scale, spacing scale, type scale).
- `variant` + `size` on widgets via a small builder.
- Widgets stop naming literal colors — this is what makes ten widgets look like
  one system rather than ten decisions.

*Proof:* the terrain panel restyles end-to-end by swapping one `Theme` value.

## Slice 5 — Typography *(polish, labeled)*

*Roadblock:* honestly, none — this is the "it still looks like a debug HUD"
slice, and it is named as polish rather than dressed up as infrastructure.

The embedded 8×8 bitmap font is the loudest remaining tell after square corners.
Three options, in preference order: bake an SDF atlas offline from a real face
into a `const [u8]` (crisp at any size, zero runtime dependencies, identical
native/web — keeps this crate's leaf shape); add proportional advance widths to
the existing bitmap; or take a `fontdue`/`ab_glyph` dependency with a dynamic
atlas (rejected by default — it would break the zero-dependency rule).

*Proof:* readable text at multiple sizes with correct proportional metrics.

## Slice 6 — Animation *(polish, labeled)*

*Roadblock:* also none. Cheap, though — roughly forty lines once Slice 1's ids
exist and given `Renderer::dt`, which the engine already provides.

Per-id animated floats easing toward a target: hover fades, accordion
expand/collapse, knob springs. Disproportionate perceived-quality return, which
is exactly why it needs the polish label to stay honest.

---

## Waiting on a roadblock

Recognized but **not** scheduled — listed so they're identified when a demo
finally demands one, not as a to-build list:

- **Text input / numeric entry** — needs typed characters, modifiers, and
  Tab/Enter/Esc, none of which the engine's 8-variant `Key` enum carries. The
  driver would be wanting to type an exact erosion constant instead of dragging
  for it.
- **Select / dropdown, popover, tooltip, context menu** — unblocked by Slice 1's
  layers, but each still waits for something to actually need it. A terrain
  preset picker is the likely first.
- **Tabs, accordion, card, badge, modal** — the shadcn roster proper. None has a
  roadblock yet, and the roster is the part most likely to become the project.
- **A transport / timeline scrubber** — play, pause, single-step, and seek along a
  time axis, with tick marks or event markers. The driver is real and close: the
  engine's [Slice 11](../ROADMAP.md#slice-11--fixed-timestep-clock--time-control)
  gives `scene.rs` a fixed-step clock the demo has to *drive from somewhere*. Not
  scheduled anyway, because the crude version composes from today's `button` +
  `slider` — which is the correct first move. A dedicated widget waits until that
  composition is demonstrably not enough (markers along the track are the likely
  breaking point).
- **Draggable / resizable / dockable panels** — no.
- **A retained-mode widget tree** — explicitly not the destination (root
  principle 2). The toolkit stays immediate-mode with minimal persistent state.

## A second consumer

A data-dense application (tables, dossiers, charts) demands a different substrate
than a parameter panel over a 3D scene, and the terrain demo will never surface
those walls. They are catalogued in [`WISHLIST.md`](WISHLIST.md) — recognized, not
scheduled. Items graduate from there into the slices above only when a consumer
names the roadblock.

That consumer has since asked for a **renderer** as well as a UI, and the engine
half of the answer is now sequenced: [engine Slices
7–11](../ROADMAP.md#the-second-vertical--a-scene-demo-slices-711) (per-object
transforms, lighting, per-instance material, primitives, a fixed-step clock),
driven by an engine demo of our own rather than by the request list. **It changes
nothing here.** No slice below moves, and nothing graduates out of
`WISHLIST.md` — the request was explicitly scoped to exclude UI, and the parts of
it that touch this crate (a scene rendered as a *panel among panels*, transport
controls for the new clock) were already recorded there and in *Waiting on a
roadblock* above.
