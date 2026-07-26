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

## Slice 2 — Painter capabilities (and the scroll region) ✅ done

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

**What shipped, and what it cost:**

- **The painter grew four methods**, and `rect` became a convenience default over
  `fill_rect(rect, 0.0, color)`: `fill_rect(rect, radius, color)`,
  `stroke_rect(rect, radius, width, color)`, and `push_clip` / `pop_clip`. Clip
  regions **intersect** rather than replace as they nest, so an inner region can
  only ever shrink what is visible — the alternative silently lets a nested
  widget escape its parent's bounds.
- **It cost one shader and 80 bytes a vertex, and no draw calls.** `Vertex2D`
  carries its rect (center + half-size), radius, border width, and clip rect;
  `overlay.wgsl` evaluates a rounded-box SDF, subtracts an inset SDF for strokes,
  and discards outside the clip. The prediction in the slice above held — the
  overlay is still a single `draw_indexed`, and the parity risk did not
  materialize.
- **`scroll_area(label, max_height, |ui| …)` takes a closure, not `begin`/`end`.**
  An unbalanced pair would desync the clip stack, and a closure makes that
  unrepresentable. It sizes its viewport from *last* frame's content height, so a
  brand-new scroll area takes its full height on frame one and settles on frame
  two; the slim scrollbar appears only when there is genuinely overflow.
- **Rounding was applied, not just enabled** — which is the difference between a
  capability and a look. A two-step radius scale (`RADIUS_LG` 8, `RADIUS` 4)
  covers panels versus controls; slider tracks and knobs are capsules (radius at
  half the shorter side); the panel gained a hairline border.
- **Focus rings are the first thing to read Slice 1's `focused` id**, which until
  now was tracked and ignored. Buttons, checkboxes, and slider knobs all ring.

**Soft shadows were declined, not forgotten.** They were in this slice's bullet
list. A hairline border at 10% white separates the panel from a busy 3D scene
just as legibly, for one stroke instead of a blurred fill, and without the
multi-tap blur or offscreen pass a real shadow wants. Recorded here so it isn't
re-litigated as an oversight; revisit only if a floating popup needs to read as
*above* the panel rather than merely distinct from it.

**On testing what a clip does.** Clipping is invisible in a recorded draw list —
a clipped-away widget is still *drawn*, it just doesn't survive the fragment
shader. So `RecordingPainter` records the clip in force on every primitive, and
`visible_texts()` reports what a viewer would actually read. That is the
assertion that a scrolled-away row is gone, and it is why the tests could catch
scroll behavior at all without a GPU.

*Verified on both targets:* native, and web under `BrowserWebGpu` in a Chromium
browser — rounded corners, hairline border, focus rings, and a scroll region
whose bottom heading is cut **mid-glyph** by the clip rect.

**What it exposed.** Clipping turned an invisible bug into a visible one: long
labels ("Fluvial erosion (rive…", "area exponent m: 0.5") are now truncated at
the viewport's right edge, where before they quietly overran the panel and drew
across the 3D scene. Both are wrong; only one is obvious. This is Slice 3's
roadblock arriving with evidence attached, and it is the argument for
right-aligned readouts rather than one long label string per row.

## Slice 3 — Layout ✅ done

*Roadblock:* the terrain panel wants a preset row (several buttons side by side)
and right-aligned value readouts. Neither is expressible when layout is
`cursor_y += h` down a hard-coded `PANEL_X` / `PANEL_W`.

- Allocate-from-available-rect instead of a bare vertical cursor.
- `horizontal()`, `columns(n)`, `indent()`, right-alignment.
- Panels anchored to any edge, so the HUD and the parameter panel stop being the
  same fixed rectangle.

*Proof:* the terrain demo shows a three-button preset row (`hills` / `alps` /
`peaks`), right-aligned values on every slider, thermal-erosion rows genuinely
indented rather than faked with leading spaces in the label, and the HUD living
in its own 210-point panel in the opposite corner. Verified on native and on web
under `BrowserWebGpu`.

**What shipped, and what it cost:**

- **Layout is a stack of regions.** `Layout { origin_y, cursor_y }` became
  `Region { avail, cursor, dir, line_h, origin }` and `Ui` holds a `Vec` of them.
  Every container — panel, row, column, indent, scroll area — is push, run a
  closure, pop, and advance the parent by what the child consumed. All the
  interesting arithmetic lives in one function, `Region::place`.
- **`allocate([0.0, h])` changed meaning without changing behavior.** It used to
  mean "the full content width"; it now means **"whatever is left"**, which is
  the same thing in a vertical region and the rest of the line inside a row. That
  is what lets one widget fill a panel on its own line and share a row when it is
  on one — and it is why `examples/terrain.rs`'s `log_slider` still compiles
  untouched, since it only ever derived geometry from the `Rect` it was handed.
- **`Ui` stopped being the panel and became the frame.** Panels are
  `ui.panel(anchor, width, |ui| …)`, and their width is a parameter rather than
  `PANEL_W`. There is still exactly one `Ui` per frame, which is what keeps hover
  and focus coherent across panels and lets `wants_pointer` union all of them —
  the alternative (a second `Ui`) would have had the second one clear the first's
  `hot` id on construction. `PANEL_X` / `PANEL_Y` / `CONTENT_X` / `CONTENT_W` are
  gone: they are per-panel now, and computed.
- **Edge anchoring cost one field at the seam.** `UiInput` gained
  `viewport: (f32, f32)`, filled by `Renderer::ui()` from the surface size over
  the scale factor — the same points conversion the cursor already went through.
  The toolkit still never learns what a window is.
- **Right alignment is a direction, not a special case.** `ui.right(|ui| …)` is a
  region that packs from the right edge inward, so it composes with `horizontal`
  instead of being a flag on every widget. `label_value(label, value)` is the row
  built on it, and it is what retires `format!("{label}: {value}")` — two runs
  anchored to opposite edges fit in 304 points where one run needed 336.
- **The slider grew a builder, pulling part of Slice 4 forward.** `slider_fmt` was
  about to become `slider_fmt_compact` and then a variant taking a closure, so it
  became `ui.slider(..).decimals(n).value_fmt(f).layout(l).show()` instead. This
  is Slice 4's `variant`/`size` pattern arriving early, deliberately: the two
  things a consumer wants to override on a slider are how the value reads and how
  the row is arranged, and both are presentation. It is a breaking change to every
  call site, which is the cost of having one surface rather than two.

**Top and bottom anchors are not symmetric, and that is load-bearing.** A
top-anchored panel knows where its first row goes before it lays anything out. A
bottom-anchored one cannot — it has to place its contents before its height
exists — so it positions from *last* frame's height and settles on the second
frame. This is the same one-frame lag Slice 1 retired for the panel background,
and it is not a regression of that fix: it is now confined to the one case that
genuinely cannot avoid it, documented on `Anchor`, and covered by a test that
asserts the settling. The demo uses only `TopLeft` and `TopRight`, so nothing
shipped depends on it.

**Containers deliberately do not scope ids.** It was tempting to have `columns`
push the cell index as an id scope. That is exactly the position-in-the-id bug
`interact::hash_id` exists to prevent, wearing a different hat, so `columns`
hands the caller its index and lets it `push_id` when the labels genuinely
repeat.

**What it exposed.** Deferring text fitting has a price, and the preset row is
where it came due: a third of the content width is 101 points, which is six
glyphs and change, so `"rolling"` overflowed its button face the first time the
demo ran. The fix was to pick shorter names — which is the *right* fix under this
slice's stated scope, and also the clearest possible argument that a `fit_text`
helper has now been asked for twice. It is still not scheduled; it is recorded
under *Waiting on a roadblock* with the evidence attached.

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
- **Text fitting (`fit_text` / ellipsis)** — asked for twice now and declined
  twice. Slice 2's clipping made long section headings truncate mid-glyph; Slice
  3's preset row made a button label overflow its cell. Both were answered by
  shortening the string, which works and is honest, but the third time will be
  the one where the caller can't shorten it. The shape is known and small: with
  monospace `Painter::text_size` and a real available rect, clamping a run to its
  width with a trailing `…` is a dozen lines. It waits for a consumer whose
  strings aren't its own to edit.
- **Draggable panel edges** — panel *width* is a parameter as of Slice 3, so a
  consumer can already resize one by passing a different number. A grab handle
  that lets the *user* do it at runtime is a separate thing, and stays under the
  "no" below until something wants it.
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
