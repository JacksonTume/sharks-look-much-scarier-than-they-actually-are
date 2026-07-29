# UI Wishlist — the second consumer

What a **data-dense application** would demand of this toolkit, recorded so the
demands are recognized when they arrive.

This is not a schedule. [`ROADMAP.md`](ROADMAP.md) holds the sequenced slices and
the things already waiting on a roadblock; this file holds a *different* kind of
list — the capabilities a second, non-terrain consumer will need, written down
before it starts hitting walls.

## Why this file exists at all

Root principle 2 says build only what a real consumer demands, and the honest
reading of that is: a wishlist is usually a trap. The reason this one isn't
speculation is that the second consumer is real and already exists.

**The Matchmaker** is a deterministic MMA fight-simulation and management game in
Rust — a parity-tested simulation kernel, a procedural fighter generator, and no
front end. It is choosing its UI stack now, and this toolkit is a candidate. Its
screens are the opposite of the terrain demo's: where terrain needs a parameter
panel over a 3D scene, a management sim is mostly **tables, dossiers, charts, and
tickers**, with the rendered scene as one panel among many.

That inversion is the value of this list. The terrain demo will never demand a
virtualized table, so demand-driven development against a single consumer will
never surface one. Knowing where the *second* consumer's walls are does not mean
building them early — it means not making choices now that are expensive to undo
when it arrives.

**The stopping rule still applies.** Nothing here is scheduled. Nothing here gets
built until something actually hits the wall. Items move from this file into
`ROADMAP.md` when a consumer names the roadblock, not before.

## Already covered — no action needed

Most of the substrate a dense UI needs is already sequenced. Recorded here only so
this list isn't mistaken for a gap analysis:

| Need | Where it already lives |
|---|---|
| Hot/active/focused, `Response`, id stack | [Slice 1](ROADMAP.md#slice-1--interaction-core--draw-layers) |
| Draw layers (popovers above their spawner) | [Slice 1](ROADMAP.md#slice-1--interaction-core--draw-layers) |
| DPI / `scale_factor` correctness | [Slice 1](ROADMAP.md#slice-1--interaction-core--draw-layers) |
| Clipping, rounded rects, borders, focus rings | [Slice 2](ROADMAP.md#slice-2--painter-capabilities-and-the-scroll-region) |
| Scroll regions | [Slice 2](ROADMAP.md#slice-2--painter-capabilities-and-the-scroll-region) |
| Horizontal/columns/indent layout, edge-anchored panels | [Slice 3](ROADMAP.md#slice-3--layout) |
| Semantic theme tokens, variants, type scale | [Slice 4](ROADMAP.md#slice-4--theme-tokens--variants) |
| Proportional text at multiple sizes | [Slice 5](ROADMAP.md#slice-5--typography-polish-labeled) |
| Deterministic, testable layout | `RecordingPainter` (Slice 0 + the DoD) |

That last row is worth calling out: a consumer whose entire engineering culture is
golden-file testing gets snapshot-testable UI layout **for free** from a decision
this crate already made. Very few toolkits offer it.

---

## New demands

Each names the screen that would pull it into existence.

### Tables — the keystone

Nothing in the current roadmap approaches this, and it is the single
highest-value widget for a data-dense consumer. Not a styled grid of labels: a
real table with columns, sorting, and selection.

*Roadblock:* a roster screen — ~450 rows × ~25 columns, sortable by any column,
filterable, row-selectable, with the selection driving a detail pane beside it.
There is no way to express this today, and composing it from `label` calls in a
vertical cursor produces neither the layout nor the interaction.

- Column model: widths, alignment, resize, reorder, sort indicators, multi-sort.
- Sticky header row; frozen leading column when scrolled horizontally.
- Row selection (single and range), hover highlight, alternating row backgrounds.
- **Cell renderers.** A cell is often not text — it's a bar, a colored chip, a
  sparkline, an icon. This is where the unprivileged-widget rule earns out: if
  `allocate` / `interact` / `painter` are public, a consumer writes its own cell
  renderers and the toolkit only supplies the grid.

### Virtualization

*Roadblock:* the same roster at world scale — thousands of fighters across
divisions and history. Slice 1/2's scroll region clips what is drawn, but layout
still walks every child. At a few thousand rows that is wasted work every frame.

Only build and hit-test the visible row range. In immediate mode this is a
`for row in visible_range` loop plus knowing the row height in advance — cheap,
but only if the scroll region is designed to allow it. Retrofitting virtualization
into a scroll container that assumes it lays out all children is the expensive
version.

### Typography, beyond proportional

[Slice 5](ROADMAP.md#slice-5--typography-polish-labeled) already prefers an
offline-baked SDF atlas, which is the right call and preserves the
zero-dependency shape. Two additions a table-heavy consumer needs that a
parameter panel never will:

- **Tabular figures** — digits on a single fixed advance width. Without them,
  every number column reflows on each re-sort and the decimal points don't line
  up. This is the most-missed requirement in custom UI and is nearly free when the
  atlas is baked offline: bake the digits to a common advance.
- **Latin-Extended coverage.** A generated world's fighters carry nationalities,
  so names contain `ł ñ ö ç ø š å`. ASCII-only is a visible correctness bug, not
  a polish issue. Cost is atlas size in a `const [u8]` — worth measuring before
  committing to a glyph range.
- Two weights (regular + semibold) so a header can differ from a body row without
  a color change carrying all the emphasis.
- Ellipsis/truncation with measurement, for names that overflow a column.

### Input and navigation

Text input is [already on the waiting list](ROADMAP.md#waiting-on-a-roadblock)
with no driver. This consumer supplies one, plus two more:

- **Text input** — a search/filter box over a roster. Same underlying need the
  roadmap already identified (typed characters, modifiers, Tab/Enter/Esc beyond
  the engine's 8-variant `Key` enum).
- **Keyboard navigation** — tab-order traversal, arrow-key movement within a
  table, Enter to activate. Power users of this genre (Football Manager, Out of
  the Park Baseball) navigate almost entirely by keyboard; mouse-only is a
  reviewable flaw, not a missing nicety.
- **Text selection + clipboard copy** — a player copying a stat line out of the
  game to paste elsewhere. Small, and consistently forgotten.

### Painter additions for data visualization

Charts themselves should **not** live in this crate — they are content, and the
unprivileged-widget rule says a consumer writes them from `allocate` +
`interact` + `painter`. But the painter cannot currently draw them at all.

*Roadblock:* career-progression curves, ranking history, attribute profiles, a
style-matchup heatmap, and sparklines inside table cells.

- Stroked polylines with joins and caps (line and area charts, axes, gridlines).
- Filled convex polygons (radar/spider plots, area fills).
- Textured quads from a consumer-supplied atlas — icons, flags, portraits. The
  overlay already samples a glyph atlas, so the shader work is adjacent.

If those three land, every chart above is writable in the consumer with no widget
roster growth here at all. That is the cheapest possible answer to a large demand,
and it is a direct dividend of the seam design.

### Runtime behavior — reactive repaint

*Roadblock:* a management sim sits on a static screen while the player reads.
Redrawing 450 table rows at display refresh to show nothing new drains laptop
batteries and reads badly on a Steam Deck. The terrain demo, which animates every
frame by nature, will never surface this.

Immediate mode's structural weakness. The fix is damage tracking — repaint only
when input, animation, or consumer state changed — and it is much cheaper to
design in near Slice 1 than to retrofit once widgets assume a frame is free.

### Engine-side, not this crate

One item that belongs in [`../ROADMAP.md`](../ROADMAP.md), noted here because the
same consumer pulls it: **an offscreen render target composited into a UI rect**,
so a 3D/2D scene is a *panel among panels* rather than a fullscreen background
with UI floating over it. The engine roadmap already lists a render graph as a
seam awaiting demand; this is what would demand it. It is now named explicitly in
[engine *Beyond*](../ROADMAP.md#beyond-seams-not-commitments) — still a seam, still
unscheduled.

Since this file was written, the same consumer has asked the engine for a
**renderer** for its simulation, which produced [engine Slices
8–12](../ROADMAP.md#the-second-vertical--a-scene-demo-slices-812). Two knock-on
notes for this crate, neither of them scheduled work:

- **Transport controls** for the engine's new fixed-step clock (play / pause /
  step / scrub). Composes from today's `button` + `slider`; a dedicated timeline
  widget is on [*Waiting on a roadblock*](ROADMAP.md#waiting-on-a-roadblock).
- **Textured quads got a second demander.** The painter addition requested above
  for icons/flags/portraits is the same public texture-upload API a 3D consumer
  would want for surface detail. Two independent demanders is normally what
  promotes an item — recorded here so that argument is available when either side
  actually hits the wall.

---

## Open conflicts with current scope

Recorded honestly rather than quietly reversed. Each needs a decision if this
consumer is taken seriously — and "no" remains a legitimate answer, with the
consequence being that the consumer composes it themselves or goes elsewhere.

- **Dockable / resizable panels are currently "no"** ([Waiting on a
  roadblock](ROADMAP.md#waiting-on-a-roadblock)). A multi-pane workspace — dossier
  beside roster beside card builder, panes the player sizes — is the native idiom
  of this genre, not a luxury. This is the sharpest conflict on the list.
- **A retained-mode tree stays out**, and should. Virtualized tables and keyboard
  focus are both achievable in immediate mode (egui is the existence proof); none
  of the above requires reversing that decision. Noted so the table entry isn't
  read as an argument for one.
- **Zero dependencies vs. real text.** The offline-baked SDF atlas satisfies
  both, which is why Slice 5 already prefers it. Latin-Extended plus two weights
  plus tabular digits grows the baked constant — measure before committing.
- **"Accessibility beyond keyboard navigation" is out of scope.** Reasonable for
  a demo engine; a commercially released game may want more. Flagged, not decided.

## The honest scope note

Tables, virtualization, text input, keyboard navigation, and the painter
additions above are — together — a substantially larger body of work than Slices
1 through 6 combined. Taking this consumer on is a real commitment, and the
failure mode is well understood: the widget roster becomes the project, which is
the exact outcome [`README.md § Scope`](README.md#scope) exists to prevent.

The mitigation is the rule already written down. Build Slices 1–3, let the
consumer build **one real screen**, and let that screen name its own roadblocks.
A roster table that hurts to build is worth more information than any amount of
planning in this file.
