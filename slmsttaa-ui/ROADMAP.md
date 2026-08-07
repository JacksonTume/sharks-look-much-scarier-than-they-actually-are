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

## Slice 4 — Theme tokens + variants ✅ done

*Roadblock:* by here there are enough widgets that nine `const Color`s no longer
hold the look together, and the demo wants a destructive-styled "reset" action
that shouldn't be a hand-colored rectangle.

- A `Theme` struct of semantic tokens (background / foreground / muted / accent /
  border / ring / destructive, radius scale, spacing scale, type scale).
- `variant` + `size` on widgets via a small builder.
- Widgets stop naming literal colors — this is what makes ten widgets look like
  one system rather than ten decisions.

*Proof:* the terrain panel restyles end-to-end by swapping one `Theme` value —
there is a **light** toggle in the HUD, and both panels, every widget, and the
demo's own `log_slider` follow it. The Grid section now ends in a red **reset
all** beneath a blue **new seed**, and the shape presets are secondary and small,
so one panel shows all three variants at once. Verified on native and on web
under `BrowserWebGpu`.

**What shipped, and what it cost:**

- **`theme` stopped being a wall of constants and became a value.** `Theme` holds
  a `Palette` of 18 semantic colors plus four scales — `Radii { sm, md, lg }`,
  `Space { margin, pad, gap, indent }`, `TypeScale { small, body, section, title }`,
  `Control { row_h, track_h, knob_w, scrollbar_w, scroll_speed, border, ring }`.
  `Theme::dark()` restates the exact numbers the toolkit shipped with through
  Slice 3, so adopting tokens changed the vocabulary without changing the picture.
- **It is `Copy`, and that is load-bearing.** Widgets open with
  `let theme = *ui.theme();` and then borrow the painter mutably — which is
  exactly what a snapshot buys and a `&Theme` would have forbidden. About 300
  bytes a widget a frame; unmeasurable next to a `format!` in every slider.
- **The theme is re-applied every frame, deliberately.** `ui.set_theme(t)` at the
  top of the frame, and nothing style-shaped is retained in `UiState`. The
  alternative — persisting it in host state — would have needed a
  `Renderer::ui_theme_mut()` on the engine side to be reachable at all, and it
  would have made the one part of the design that is *purely* declarative the one
  part that accumulates. The consumer already owns the value.
- **Variants are three, not shadcn's six.** `Primary` / `Secondary` /
  `Destructive`, because the terrain panel wanted exactly those three in one
  panel: a plain action, a row of equivalent choices, and one that throws work
  away. `Ghost` is not here. That is the roadmap's stopping rule applied to
  *styling* rather than to widgets, and it is the harder place to apply it —
  variants are cheap to add and each one is a token pair forever.
- **Pressed is a scrim, not a fourth color per variant.** A held control draws its
  fill and then `surface` over it. `surface` is a light wash on a dark theme and a
  dark one on a light theme, so one existing token gives every variant a pressed
  state that reads correctly in both directions. Three more tokens per variant
  would have said the same thing at three times the price — and this is the shape
  the rule "a widget never names a literal color" pushes you into once you take
  it seriously.
- **The button became a builder**, which is a breaking change to every call site
  (`ui.button("x").clicked` → `ui.button("x").show().clicked`). This is the same
  trade Slice 3 made for the slider and for the same reason: `button_destructive`
  beside `button` is two surfaces for one control, and the third variant would
  have made it three. `checkbox` did **not** get one — nothing has asked a
  checkbox for a variant, and a builder with nothing to configure is ceremony.
- **`Size` earned its place by fixing something.** Slice 3 recorded a button label
  overflowing its third-width cell, answered by shortening the string. `Size::Sm`
  draws at 13 points instead of 16, which fits seven glyphs in a preset cell where
  the standard size fits six. It does not retire `fit_text` — the caller still has
  to know what fits — but it is the first thing to make that budget bigger rather
  than the string shorter.

**This is the first UI slice since the split that cost the engine nothing.**
Slices 1 and 2 both landed work in `renderer/overlay.rs` and `overlay.wgsl`;
tokens are pure toolkit, because the `Painter` seam speaks in colors and
rectangles and does not care where a color came from. That the seam absorbed a
whole styling system without moving is the strongest evidence so far that it is
drawn in the right place.

**The claim this slice makes is testable, which is unusual for styling.** "No
widget names a literal color" is not a code-review convention here — `tests/theme.rs`
styles a frame with a theme whose every token is a distinct sentinel value and
asserts that *every color reaching the painter* is one of them. A widget added
later with `[0.26, 0.59, 0.98, 1.0]` typed into it looks perfect on the default
theme and wrong on every other, which is precisely the failure a screenshot
misses and this test doesn't. A second test drives the whole roster through
`dark()` and `light()` and asserts no color survives the swap.

**What it exposed.** `Theme::light()` was written to be the proof, and writing it
is what forced `primary_foreground` / `secondary_foreground` /
`destructive_foreground` to exist. On a dark theme every filled control can share
one near-white label color and nobody notices the shortcut; on a light theme a
blue-filled button needs white text while a faint-wash button needs near-black,
and a single `on_fill` token cannot be both. The second theme is not decoration —
it is the thing that finds the tokens a one-theme system lets you skip.

## Slice 5 — Typography *(polish, labeled)* ✅ done

*Roadblock:* honestly, none — this is the "it still looks like a debug HUD"
slice, and it is named as polish rather than dressed up as infrastructure.

The embedded 8×8 bitmap font is the loudest remaining tell after square corners.
Three options, in preference order: bake an SDF atlas offline from a real face
into a `const [u8]` (crisp at any size, zero runtime dependencies, identical
native/web — keeps this crate's leaf shape); add proportional advance widths to
the existing bitmap; or take a `fontdue`/`ab_glyph` dependency with a dynamic
atlas (rejected by default — it would break the zero-dependency rule).

*Proof:* readable text at multiple sizes with correct proportional metrics.

**Labeled polish, and it behaved like anything but.** This is the slice with the
widest gap between how it reads on the roadmap and what it cost. The rendering was
the easy half. The dangerous half was that *measuring* text turned out to be load
bearing for every alignment in the crate, and it was implemented twice.

**What shipped, and what it cost:**

- **`text_size` left the `Painter` trait, which is the whole point.** It had been
  implemented once on the engine's `Overlay` and once on `RecordingPainter`, both
  as `chars().count() * px`. Those agreed *only* because the bitmap font was a
  monospace grid. Proportional advances break the tie — and the failure mode is
  not a wrong number, it is that **the tests measure a different font than the
  screen draws**. `tests/regions.rs` asserts right-alignment against the recording
  painter, so the suite would have stayed green while every readout in the demo
  drifted off the panel. Metrics now live in [`font`](src/font.rs), both painters
  read them, and the divergence is unrepresentable rather than merely discouraged.
- **The font lives in this crate, and `cargo tree -p slmsttaa-ui` still prints one
  line.** That was the test of whether "zero dependencies" meant anything precise:
  an atlas is *data*, so `include_bytes!` costs nothing in the dependency graph.
  The rasterizer went into a new `fontbake` workspace member — quarantined there so
  that `xtask`, which gets run constantly, stays dependency-free too.
- **It cost the seam some width, deliberately.** A `Painter` no longer chooses its
  font: it is handed a run, a size, a `Weight`, and glyph geometry from
  `font::glyph`. This is the first slice to make the downward seam *narrower*, and
  the trade is stated in `font`'s module docs — a seam wide enough for two fonts is
  a seam wide enough for two disagreeing fonts, and nothing ever wanted the second.
- **`px` changed meaning, and the type scale was re-tuned to hide it.** It used to
  be a square glyph cell, which *was* the cap height; it is now an em size, the
  conventional meaning. Inter's capitals are `0.729em`, so the scale went up by
  about 1.2× (13/16/15/20 → 15/19/18/24) to keep text the same visual size. Matching
  cap height rather than nominal size is why a real face didn't arrive looking
  abruptly smaller than the bitmap it replaced.
- **Every hand-tuned vertical offset in the crate was wrong and had to be
  rederived.** `row.y + 2.0`, `(face.h - px) * 0.5`, `row.y + px + 3.0` — all
  correct only while a run was exactly `px` tall. A real face has a line box
  *larger* than its em and ink *smaller*, so there is now one function,
  `font::centered_top`, and it centres **cap height, not the line box**: a line box
  reserves descender space no capital occupies, and centring it leaves text sitting
  visibly high. The checkbox well was re-derived the same way and now sits on the
  label's cap band instead of being an em tall and top-aligned.
- **Digits are tabular, synthesized in the bake.** `fontdue` applies no OpenType
  features, so `tnum` was unavailable and the widest digit's advance is forced onto
  all ten with each glyph re-centred. This is not fussiness: Inter's proportional
  `1` is **37% narrower** than its `0` (0.4067em vs 0.6460em), so a dragged slider's
  readout visibly shuffles sideways as digits change. Right-aligning it pins the
  right edge and makes the wobble *more* obvious, not less.
- **Two weights, because the docs were already lying.** `widgets/text.rs` described
  `title` as "a bold heading row" while drawing it in the same weight as a label.
  A `TypeStep` now pairs a size *with* a weight — which is what a type-scale step
  actually is — and headings are genuinely semibold. It costs a second atlas page,
  and `tests/typography.rs` asserts the default scale uses both, so the page can't
  become dead weight in every wasm bundle unnoticed.
- **Antialiasing is computed on the CPU, not with `fwidth`.** `overlay.wgsl`
  deliberately avoids derivatives so the WebGL2 fallback matches WebGPU, so the
  usual SDF trick was unavailable. `font::aa_band(physical_px)` is called by the
  painter — it takes *physical* pixels, which is why it can't be a widget's job —
  and rides along in the vertex attribute slot Slice 2 left unused. This is
  strictly better than `fwidth`: the CPU knows the exact render size where the
  shader could only estimate it.

**The SDF was the wrong call to be confident about, and it worked anyway.** Plain
SDF is known to go mushy at small ppem, and 15pt body text on a 1× display was
exactly the case to worry about. It reads crisply. Two things bought that: the type
scale went *up* rather than down, and the bake supersamples 4× before the distance
transform so edge positions resolve to a quarter of a bake pixel instead of a whole
one. Recorded because the reasoning generalizes — if a future scale wants 11pt text,
this is the decision to revisit first, and per-size coverage bakes are the fallback.

**What it exposed, and this is the real find.** Reserving no space for the
scrollbar was a **live bug the whole time**, and the bitmap font was hiding it. The
scroll area laid its contents out across the full content width and then painted
the bar *over* the right 4 points. `font8x8` leaves roughly a quarter of every
glyph cell blank on the right, so a right-aligned readout's box ran under the bar
while its *ink* cleared it. Inter's `0` has about a point of side bearing. The
moment the font became real, the bar started eating the last digit of every value
in the panel — `3.50` read as `3.5`, and the `peaks` preset lost its `s`.

The fix reserves a gutter unconditionally rather than only when the bar is showing,
because a conditional one reflows every row the moment one more row tips the area
into overflow — and once anything here wraps, a narrower region could *grow* the
content height and toggle the bar on and off forever. There is now a test
(`clipping.rs`) asserting every run ends left of the bar.

This is the third time the pattern has repeated: Slice 1's id bug, Slice 3's
overflowing button label, and now this. **Every test passed.** What found it was
running the demo and looking at the panel — and the reason it was *findable* is
that a proportional font removes slack that a monospace grid silently donates.
Changing the font was, accidentally, a fuzz test for every layout assumption in
the crate.

## Slice 6 — Animation *(polish, labeled)* ✅ done

*Roadblock:* also none. Cheap, though — roughly forty lines once Slice 1's ids
exist and given `Renderer::dt`, which the engine already provides.

Per-id animated floats easing toward a target: hover fades, accordion
expand/collapse, knob springs. Disproportionate perceived-quality return, which
is exactly why it needs the polish label to stay honest.

*Proof:* the terrain panel's headings warm under the pointer, its buttons and
checkbox ticks fade rather than snap, its slider knobs swell when grabbable, a
wheel notch glides its 28 points instead of teleporting them, and a section
**collapses and expands over ~250 ms** — its rows clipped to a shrinking height
while everything below slides up to meet them. Verified on native and on web
under `BrowserWebGpu`, where a screenshot taken one frame after the click catches
a slider with its track cut halfway through.

**What shipped, and what it cost:**

- **The core is one function and one map.** [`anim::approach`](src/anim.rs)
  integrates exponential decay over the *real* elapsed time — `1 - e^(-rate·dt)`
  — and `UiState` keeps a float per `(widget id, property name)`. Everything else
  in this slice is a call to `ui.animate(id, "hover", target)`.
- **The forty-line estimate was right about the core and wrong about the slice.**
  Easing a number is trivial. What it cost was the section's API (below), a sweep,
  a snap rule, and a decision about what `dt = 0` means — none of which is
  arithmetic.
- **Frame-rate independence is the whole point, and it is the part that would
  have been got wrong.** The tempting `value += (target - value) * 0.2` converges
  in a fixed number of *frames*: the same fade takes 130 ms at 144 Hz and 330 ms
  at 60 Hz, and a stuttering machine changes its character. That failure is
  invisible on the machine it was written on, which is why `tests/animation.rs`
  asserts one 100 ms step and ten 10 ms steps land in the same place.
- **`dt = 0` snaps rather than freezes, and that is deliberate.** "No time has
  passed" argues for freezing, and freezing would be a trap: a host that never
  fills the new field would get a UI whose sections can never finish collapsing.
  Snapping instead makes `dt` genuinely *optional* at the seam — every widget
  behaves exactly as it did through Slice 5 — which is why the other seven test
  files never had to learn that animation exists.
- **Values snap onto their target once they are close enough.** Exponential decay
  never arrives, and "never arrives" is load-bearing here: a fully open section
  takes a fast path that skips clipping entirely, and at `t == 0.99999` it would
  clip forever. `anim::SNAP` ends the animation for real.
- **The slot map is swept every frame**, unlike the four maps already in
  `UiState`. Those are keyed by containers, of which a panel has a handful; this
  one gets an entry per animated property of every widget declared, so a consumer
  generating rows from changing labels would grow it without bound. The sweep also
  buys the right *behavior*: a row that leaves the screen comes back settled
  rather than resuming a fade from a hover the user has long since forgotten.
- **Motion is a theme token, and turning it off is a value rather than a flag.**
  `Motion { fade, expand }` sits beside the radius and spacing scales, because
  "how long does a hover take" is a property of the look. `Motion::none()` is an
  *infinite rate*, so a reduced-motion preference is one assignment and there is
  no `if animating` for a widget to forget.

**The section had to change shape, and it is the interesting part of this slice.**

`if ui.section("x").open { … }` cannot be animated. The toolkit sees a heading and
then, some rows later, unrelated widgets — it never learns where the section
*ends*, so it cannot clip a collapse to a partial height. The height was not a
number it owned. So `section` takes its contents as a closure, joining
`panel` / `horizontal` / `columns` / `indent` / `scroll_area`, all of which took
closures already and for a related reason.

That is a breaking change to every call site, which is the same trade the slider
builder made in Slice 3 and the button builder in Slice 4. Four call sites in
`examples/terrain.rs`, five in the tests.

Three things fell out of it that were not the point:

- **Contents are now scoped under the section's id**, so two sections may each
  hold a `"strength"` slider without sharing a drag. `Ui::push_id`'s docs had
  claimed section did this since Slice 1; it never had. The claim is now true
  rather than corrected.
- **`Response::open` became informational.** Nothing has to branch on it, because
  the section decides for itself what to show. It stays because a consumer may
  want to mirror the state elsewhere.
- **The scroll area's content-height hack retired.** It stored its measurement in
  the scroll-offset map under an XOR'd key, because there was one map and it
  needed two things from it. The section needs the identical "how tall were the
  contents last frame" value, and two callers is what justified `UiState.measured`
  existing properly.

**A live edge, recorded rather than fixed.** While a section is mid-collapse its
rows are clipped but still hit-testable, so a click landing on a half-hidden
button registers for those ~250 ms. This is not new — a row scrolled out of a
`scroll_area` has always behaved this way, because clipping is something the
*painter* does and hit-testing knows nothing about it. It is worth fixing when
something can actually be clicked by accident; a section the user is watching
collapse is not that.

**What it did not expose, for once.** Slices 1, 3 and 5 each shipped with a bug
that every test passed and running the demo found. This one didn't — the first
run of the demo showed the collapse working. The honest reading is not that the
process improved: it is that this slice added a *new* capability on top of the
layout rather than changing an assumption underneath it, and the three earlier
bugs were all assumptions (ids are ordered, labels fit, a glyph cell is its cap
height). Adding is safer than re-deriving, and that says nothing about the next
slice that re-derives something.

## Slice 7 — Keyboard, focus, and text entry ✅ done

*Roadblock:* the first one this crate has taken from a consumer other than the
terrain demo, and the first thing that consumer could not build around.
[`WISHLIST.md` § Input and navigation](WISHLIST.md#input-and-navigation) records
it: a client with a route stack and three screens where **back has to be a
button** (no Esc, no Backspace, no mouse-4), **a table cannot be walked** (no
arrows, no Enter, no Tab) and **nothing can be typed** (no filter over a roster).

The diagnosis is what made it a slice rather than a wish. That consumer had
written its own table, its own cell alignment, its own truncation and its own
navigation against the public seam — but *it cannot inject an input the snapshot
has no field for*. `UiInput` carried `cursor`, `primary_held`, `primary_pressed`,
`scroll_delta`, `viewport` and `dt`, and all three needs wanted a field that was
not there. The engine half was on the critical path too: `Key` had eight variants
chosen to fly a camera, and there were no modifiers, no typed characters and no
key press edge at all.

*Proof:* `examples/editor.rs` gives its objects **names**, typed into a field;
its scene list is **filtered by typing** and **walked with the arrows**; Escape
backs out of the field and then out of the selection; Delete removes the
selection and mouse-4 deselects; Tab walks the whole panel and Enter activates
what it lands on. The camera stands down while any of that is happening.
Verified on native and on web under `BrowserWebGPU`, where the same keystrokes
produce the same picture and Tab stays inside the canvas rather than walking the
browser's focus ring.

**What shipped, and what it cost:**

- **The seam grew an ordered event log, and that is the whole design.**
  Everything `UiInput` carried before is a *level* — what is true at the end of
  the frame — and a text field cannot be built on levels: typing `ab` then
  Backspace inside one frame leaves `a`, the other order leaves `ab`, and a set
  of flags has already thrown the difference away. So `UiInput.events` is a
  `&[Event]` the host lends for the frame. Borrowing rather than owning keeps the
  struct `Copy` and allocation-free; the price is the lifetime parameter, which
  is the only reason `UiInput<'a>` has one and which cost ~15 mechanical edits
  across the test suite.
- **Typed characters are a separate channel from `Key`, and must stay one.**
  `Key` is physical positions, for bindings; `Event::Text` is what the platform
  *produced*, layout and shift and dead keys already applied. Rebuilding `'A'`
  from `Key::A` plus a shift flag is the classic way to ship something that only
  works on a US layout.
- **Focus became drivable, and Tab is resolved before any widget is declared.**
  `focusable` / `focused` / `set_focus` are public — a consumer's own list is a
  first-class member of the tab ring — and `Ui::new` walks the ring *last* frame
  recorded, so tabbing onto a button rings it on the same frame rather than one
  later. That is the trick `scroll_area` already used on the wheel.
- **The tab ring is the one place position is load-bearing**, and it is not the
  order-keyed-id bug in a hat. Ids are still `hash(scope, label)`, so a row
  appearing above a widget cannot change its identity; all that shifts is where
  it sits in the ring, which is what a ring *is*. `tests/keyboard.rs` asserts
  exactly that.
- **Every existing widget became keyboard-operable without changing its call
  sites.** Enter or Space on a focused button or checkbox sets
  `Response::clicked`, so every `if ui.button("x").show().clicked` in the
  codebase gained keyboard operation for free. A focused slider takes the
  Windows contract: arrows nudge 1%, Page jumps 10%, Home and End pin the ends.
- **`wants_keyboard` is coarse, exactly like `wants_pointer` — and it had to be
  *state* rather than an event.** A camera reads *held* keys, so a focused text
  field has to suppress it on every frame, not only the frames a key went down;
  otherwise holding `W` both types `w` and flies. A button is the other way
  round: all it binds is Enter and Space, so it claims the keyboard only on the
  frame it consumes one, and clicking a button does not silently kill WASD.
- **`text_field` is the first widget that reads the log in order**, and the only
  place in the crate that would notice it is a log. Caret, click-to-place,
  drag-select, shift-selection, Home/End, Ctrl+A, and a horizontal scroll that
  chases the caret. Offsets are **byte** indices moved through `char_indices`,
  because character counts and byte indices agree right up until a name contains
  `ō` — which the wishlist had already measured at one generated name in six.
- **The crate still has no idea what a clipboard is.** Copy and cut leave their
  text in `UiState` for the host to collect (`take_clipboard`); paste never
  reaches this crate at all, because a host delivers it as ordinary
  `Event::Text`. A zero-dependency crate has nothing to talk to an operating
  system with, and this is what that constraint looks like taken seriously rather
  than worked around.
- **One new theme token, `selection`** — translucent by convention, so the glyphs
  read through it and a second "text on selection" token is not needed. Same
  trade `surface` already makes for the pressed scrim.

**A scroll area now chases keyboard focus, and that was not in the plan.** The
demo's list is six rows tall inside a `scroll_area`, so walking past the sixth
row focused something nobody could see — which makes "walk a long list from the
keyboard" quietly not work, and a list is exactly what a scroll area is *for*.
The offset belongs to the container, so the container has to do it: `interact`
records the focused widget's rect, and a scroll area whose contents claimed focus
this frame nudges its target to bring it into view. It fires only when focus
moved *without* the pointer — a click already proves the widget was visible, and
chasing every frame would drag the view back the instant the wheel moved it.

**What it exposed, and it is the fourth time this pattern has repeated.** Every
test passed; running the demo found it. Pressing Ctrl+A in the name field
inserted an `a`, and Ctrl+C a `c`: **Windows reports `text: Some("a")` for
Ctrl+A**, so the platform hands you a shortcut and a keystroke at once. The fix
is a filter on the engine's text channel — a keystroke under a shortcut modifier
is not typing — with `Ctrl+Alt` deliberately exempt, because that is AltGr on a
European layout and it types real characters. No test in this crate could have
caught it: the toolkit believed what the host told it, and the host was wrong.

---

## Nothing is scheduled

Slice 7 answered the only roadblock a second consumer has actually hit. **That is
the correct state for this crate to be in**, not a gap to fill: every slice above
was pulled into existence by something a demo couldn't do, and the list below is
what a *future* consumer would have to ask for first.

The next UI work should therefore arrive from a demo, not from this file. The
nearest candidates are recorded in [`WISHLIST.md`](WISHLIST.md) — virtualization,
the sticky-header gutter, and the painter additions for charts — and none of them
has a driver yet.

## Waiting on a roadblock

Recognized but **not** scheduled — listed so they're identified when a demo
finally demands one, not as a to-build list:

- ~~**Text input / numeric entry**~~ — **shipped in [Slice
  7](#slice-7--keyboard-focus-and-text-entry-done)**. The predicted driver
  (wanting to type an exact erosion constant instead of dragging for it) is *not*
  what pulled it: a second consumer's route stack did, and the engine's 8-variant
  `Key` enum was on the critical path exactly as this entry said it would be.
  Terrain still drags for its constants, and is welcome to.
- **Numeric entry proper** — a field that parses, clamps, and rejects. `text_field`
  plus the consumer's own `parse::<f32>()` covers it today; a widget waits until
  something wants the validation, the increment gestures, and the "what does a
  half-typed `-` mean" answers as one piece.
- **Select / dropdown, popover, tooltip, context menu** — unblocked by Slice 1's
  layers, but each still waits for something to actually need it. A terrain
  preset picker is the likely first.
- **Tabs, accordion, card, badge, modal** — the shadcn roster proper. None has a
  roadblock yet, and the roster is the part most likely to become the project.
- **Text fitting (`fit_text` / ellipsis)** — asked for twice, declined twice, and
  now *fully unblocked*, which is a different status than before. Slice 2's
  clipping made long section headings truncate mid-glyph; Slice 3's preset row made
  a button label overflow its cell. Both were answered by shortening the string,
  which works and is honest, but the third time will be the one where the caller
  can't. Slice 4's `Size::Sm` bought slack rather than solving anything. Slice 5
  removed the last two excuses: `…` is now in the atlas by name, and
  `font::text_width` is exact per-glyph rather than a monospace estimate, so
  clamping a run to an available width is a `take_while` over advances plus one
  fallback glyph — genuinely a dozen lines. Still waiting on a consumer whose
  strings aren't its own to edit, because the *policy* (truncate where? middle-
  ellipsis? wrap instead?) is the part that needs a real caller to answer.
- **Kerning** — deliberately skipped in Slice 5, recorded so it reads as a decision
  rather than an omission. `font::text_width` is a plain sum of advances, which is
  what makes it exactly reproducible on both sides of the seam and trivially
  testable. Kerning would make a run narrower than the sum of its parts, so every
  measurement would have to replicate the pair table — a real cost for a
  barely-visible gain at 15 to 24 points. Revisit if a display size (a 48pt
  heading, a title screen) ever makes `AV` and `To` look loose.
- **A third type weight, or italics** — Slice 5 baked Regular and SemiBold because
  the type scale distinguishes headings from body text. Nothing has asked for
  Medium, Bold, or an italic, and each is another atlas page in every wasm bundle
  (~185 KiB), so each waits for something that actually needs it.
- **Draggable panel edges** — panel *width* is a parameter as of Slice 3, so a
  consumer can already resize one by passing a different number. A grab handle
  that lets the *user* do it at runtime is a separate thing, and stays under the
  "no" below until something wants it.
- **A transport / timeline scrubber** — play, pause, single-step, and seek along a
  time axis, with tick marks or event markers. **The driver arrived, and the
  prediction held.** Engine [Slice
  12](../ROADMAP.md#slice-12--fixed-timestep-clock--time-control) shipped the
  fixed-step clock, and `examples/scene.rs` drives it from a `Time` section built
  out of `button` + `slider` and nothing else — a play/pause button whose label
  swaps, a secondary `step`, and two sliders for speed and scrub. This crate cost
  the engine nothing and gained nothing, which is the third slice running
  (4, 6, and now this) where the seam absorbed a demand without moving.
  One thing the demo *did* find, and it belongs here rather than in the engine
  roadmap: two buttons in a `horizontal` row do not share it. A button allocates
  "whatever is left of the line", so the first takes the whole width and the
  second is clipped off the panel edge. `columns(2)` is the answer and the docs
  now say which to reach for. A dedicated widget still waits until this
  composition is demonstrably not enough — markers along the track remain the
  likely breaking point.

  **A second consumer arrived and changed nothing, which is the useful part.**
  Engine [Slice
  13](../ROADMAP.md#slice-13--erosion-as-a-scrubbable-time-axis-done) gave
  `examples/terrain.rs` its own transport — play/pause, single-step, a passes-per-
  second slider and a scrub that genuinely rewinds — and it is the same
  `columns(2)` + `button` + `slider` composition `scene.rs` uses, written
  independently in a different demo. Two consumers building the identical control
  out of primitives is normally the moment a widget graduates; here it is the
  argument *against* one, because neither needed anything the roster lacks and a
  `transport()` widget would only be those four calls with their arrangement
  frozen. It stays on this list. What would actually move it is the thing already
  named above — markers along the track — and terrain has a natural candidate
  (where the lakes finish draining) that it has not asked for.
- **Golden-file layout snapshots.** `RecordingPainter` already records every
  primitive a frame draws, with the clip in force — which is a serialisable
  description of a screen, and `WISHLIST.md` already advertises exactly this to a
  consumer whose engineering culture is golden files. Writing one to disk and
  diffing it would catch a layout regression with no GPU, no window and no image,
  and would say *which widget* moved rather than which pixels did. The engine
  grew image capture in `cargo xtask shoot`
  ([engine *The harness*](../ROADMAP.md#the-harness)); this is the half of the
  same idea that belongs up here, and is plausibly the higher-value half per line
  written. Nothing has demanded it: the existing tests assert against the
  recorder directly, and until a screen is too big to assert by hand that is
  enough.
- **A `RecordingPainter` that a capture script can reach.** The harness drives the
  toolkit only through the pointer, so a widget with no visible effect on a
  screenshot is invisible to it. Nothing has needed more yet, and the honest note
  is that three of the four bugs this crate found by running the demo *were*
  visible ones.
- **A content region — `Ui::remaining()`** — the space a layout has left after its
  panels. **Asked for once, declined once, and the reason is that the demo's own
  arithmetic is better.** Engine [Slice
  18](../ROADMAP.md#slice-18--the-scene-as-a-panel-among-panels-done) let a
  consumer put the 3D scene in a rectangle, and `examples/workspace.rs` needs to
  know which rectangle. It computes one from `Theme::space.margin` and its own two
  panel widths — four lines, exact, no lag. A generic version would be worse in
  three specific ways: panels here are corner-anchored floaters that reserve
  nothing, so "what is left" is only a bounding-box guess; it could only answer
  after every panel had closed; and a bottom-anchored panel's height carries the
  one-frame measurement lag documented under `Anchor`, which for a *scene* rect
  means a texture re-allocation on the settling frame. A second consumer wanting
  it — one whose panel set is dynamic enough that the arithmetic stops being
  writable — is what would move this.
- **Draggable / resizable / dockable panels** — still no, and Slice 19 is the
  closest anything has come to arguing otherwise. A workspace demo makes splitter
  handles look like the obvious next thing, and `set_scene_rect` takes a new
  rectangle every frame for free, so the engine half is already there. The
  toolkit half is not, and this stays a "no" until a consumer is blocked rather
  than tempted. Recorded because the temptation is now concrete instead of
  theoretical.
- **A retained-mode widget tree** — explicitly not the destination (root
  principle 2). The toolkit stays immediate-mode with minimal persistent state.

## A second consumer

A data-dense application (tables, dossiers, charts) demands a different substrate
than a parameter panel over a 3D scene, and the terrain demo will never surface
those walls. They are catalogued in [`WISHLIST.md`](WISHLIST.md) — recognized, not
scheduled. Items graduate from there into the slices above only when a consumer
names the roadblock.

One item from that file has since **landed on the engine side**: a scene rendered
into a UI rectangle, as [engine Slice
18](../ROADMAP.md#slice-18--the-scene-as-a-panel-among-panels-done). It cost this
crate nothing at all — no `Painter` method, no `UiInput` field, no widget — and
nothing in the slices above moves. What it did surface is one new entry on
*Waiting on a roadblock*, `Ui::remaining()`, filed there as declined with its
reason rather than as a gap.

That consumer has since asked for a **renderer** as well as a UI, and the engine
half of the answer is now sequenced: [engine Slices
8–12](../ROADMAP.md#the-second-vertical--a-scene-demo-slices-812) (per-object
transforms, lighting, per-instance material, primitives, a fixed-step clock),
driven by an engine demo of our own rather than by the request list. **It changes
nothing here.** No slice below moves, and nothing graduates out of
`WISHLIST.md` — the request was explicitly scoped to exclude UI, and the parts of
it that touch this crate (a scene rendered as a *panel among panels*, transport
controls for the new clock) were already recorded there and in *Waiting on a
roadblock* above.
