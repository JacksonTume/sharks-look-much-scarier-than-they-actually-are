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

> **Mostly answered, and not the way this entry assumed.** The consumer built
> the table **in the consumer**, from `allocate` / `interact` / `painter` /
> `theme` / `next_id` plus `font::text_width` — proportional column widths,
> per-column alignment, ellipsis truncation measured with the same function that
> lays the glyphs out, row hover, and row clicks driving navigation. **Neither
> this crate nor the engine was modified.** Three screens now reuse that table,
> which is the evidence it generalised rather than being shaped around the first
> one.
>
> So the grid is content, by the same argument this file already applies to cell
> renderers and charts, and the "keystone" framing below overstates what the
> toolkit owes. What it did owe is the gutter fix directly beneath this entry,
> and that has since shipped. Sorting and selection remain unbuilt, and look
> consumer-shaped for the same reason.

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

### A sticky header has to shed the scrollbar gutter by hand

Found by building the above, and the one piece of it that is squarely this
crate's.

A header row drawn *outside* a `scroll_area` spans the region's full width; the
body inside it is inset by `scrollbar_w + gap`. Nothing warns you, so every
column sits a few points out of line with its own heading — invisible in a
screenshot, obvious on a real screen, and wrong in a way that reads as sloppiness
rather than as a bug.

The consumer can correct it, because both numbers are public on the theme, and
does. But **every consumer that writes a table will rediscover it**, which is the
usual sign that the toolkit should own it — either a scroll area that can report
its own gutter, or a way to lay a header out in the same region as the body.

> **Done, as the second of the two options.** [UI Slice
> 8](ROADMAP.md#slice-8--a-header-that-lines-up-with-its-body--done) added
> `scroll_area_headed`, which lays a header out at the body's width from a
> single measurement handed to both. The first option — reporting the gutter —
> was deliberately **declined**: it is the smaller change, and it would leave a
> consumer subtracting the number by hand, which is the thing that goes wrong.
> The gutter is still private. That is this file's own unprivileged-widget
> argument pointed at a *number* rather than a widget, and it is the fourth time
> the answer has been "the toolkit owns the arithmetic, the consumer never
> learns it exists".
>
> One thing came out of it that this entry did not anticipate: the wheel had to
> learn to cover the header too, because a sticky header is part of the same
> scrollable thing to a reader. The first test written for the feature failed on
> exactly that.

### `columns` is not a grid primitive, and that is worth saying out loud

Not a defect — `columns` is documented as the button-row primitive and does that
job. But it is the first thing a consumer reaches for when building a table, and
it cannot work: it splits the width **equally**, so a rank column and a name
column come out the same size, and it lays out **column-major**, which means a
*row* is not a thing and row hover and row selection are inexpressible through
it.

The consumer hit this twice — once building the rankings table, and again on a
bout screen's stat comparison, where equal thirds put each number some 300 points
from its own label. Worth one line in the docs so the next author does not spend
the same afternoon.

> **Done**, and it came to rather more than one line — `columns` now names both
> properties under a heading of their own, and points at building the row from
> `allocate` / `interact` / `painter` instead. Shipped alongside the gutter fix
> in [UI Slice 8](ROADMAP.md#slice-8--a-header-that-lines-up-with-its-body--done),
> because they are the same afternoon.

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

  **Now measured, and larger than "a few diacritics":** of 1,377 generated
  fighters, **218 — one name in six — contain a character `fontbake` does not
  bake**, across **47 distinct codepoints** in four blocks: Latin-1 Supplement
  (`é ñ ç ö ã ü`), Latin Extended-A (`ō ū š ć ż ğ ę ř`), Latin Extended-B
  (`Ș ț`, Romanian comma-below) and Latin Extended Additional (`ḥ`, dot-below
  from Arabic transliteration). The macrons matter most — Japanese romanisation
  puts `ō` and `ū` through a large share of one nation's roster, so this is not
  a long tail. They render as tofu boxes today.

  Two things make it cheap: an unbaked character draws a visible `□` rather than
  vanishing, so it self-reports on the first frame, and `fontbake` runs by hand
  with committed output, so widening the range costs a rebake and some atlas
  bytes.

  **Done — and the cost is the number this entry asked for.** `fontbake` gained
  `LATIN_EXTENDED`, **134 codepoints** taken as the exact closure over that
  consumer's 150,502 name-pool entries rather than as the four whole blocks they
  come from (512 codepoints, most of which nothing can draw). The atlas went
  **370 KiB → 918 KiB** at 512×1836 and `metrics.rs` 40 KiB → 64 KiB, for 240
  glyphs × 2 weights at 90% packing. That is the measurement this entry said to
  take before committing to a range, and it is why the range was not taken: full
  blocks would have roughly doubled the growth again for glyphs no data reaches.
  Inter 4.1 covers all 134, so the bake's own missing-glyph assert passed
  untouched.

  The closure is a property of the *consumer's data*, which this crate cannot
  see — so the guard lives over there: its suite walks every pool entry and
  fails if one carries a codepoint the atlas lacks. That is the seam working
  as intended; nothing here has to learn what a name pool is.

### Combining marks cannot render, and the data that needs them is correct

Found by doing the above, and it is the one part of it a re-bake cannot fix.

Four codepoints in that consumer's pools are combining marks — `U+0300`,
`U+0301`, `U+030B`, `U+0361` — appearing in six entries where **Unicode has no
precomposed form**: Yoruba `ọ́` (`U+1ECD` + `U+0301`), Thai stacked tone marks,
and an ALA-LC tie bar in a Russian transliteration. NFC cannot compose them
because there is nothing to compose them *to*; the strings are already normal
form.

A bake is one glyph per codepoint at a positive advance, and `text_width` is a
plain sum of those advances. So a combining mark cannot render *as* a mark here:
it lands beside its base letter instead of over it. Baking these would replace a
visible tofu with a plausible-looking wrong word, which is strictly worse — so
they are deliberately left out and still draw `□`.

The correct fix is **mark-to-base positioning**: zero-advance glyphs plus an
anchor per base glyph, which is a `GPOS`-shaped feature the bake has no concept
of today. That is real work for six entries in 150,502, so it is recorded rather
than scheduled — but recorded *here* rather than solved in the consumer, because
the alternative was for the consumer to strip the marks from its own data, and
that is a renderer's limit rewriting a world's content. A name is not the
renderer's to correct.
- Two weights (regular + semibold) so a header can differ from a body row without
  a color change carrying all the emphasis.
- Ellipsis/truncation with measurement, for names that overflow a column.

### Input and navigation

> **The roadblock has arrived.** The Matchmaker's client now has a route stack
> and three screens — a division table, a fighter dossier, and a scrubbable past
> bout — and navigation is where mouse-only stops being survivable. Concretely,
> today: **back has to be a button**, because there is no Esc, no Backspace and
> no mouse-4; **a table cannot be walked**, because there are no arrows, no Enter
> and no Tab; and **nothing can be typed**, so there is no filter over a roster.
>
> This is the first thing that consumer has been unable to build around. It built
> its own table, its own cell alignment, its own truncation and its own
> navigation from the public API — but it cannot inject an input the snapshot has
> no field for. `UiInput` carries `cursor`, `primary_held`, `primary_pressed`,
> `scroll_delta`, `viewport` and `dt`, and every one of the three needs above
> wants a field that is not there.
>
> Per this file's own rule, that promotes it: it is no longer a recognized demand
> but a named roadblock, and it belongs in [`ROADMAP.md`](ROADMAP.md) as a slice.
> Note the engine half — the 8-variant `Key` enum, typed characters and modifiers
> — is on the critical path for it and is not this crate's to widen alone.
>
> **Built, and this entry is closed.** [UI Slice
> 7](ROADMAP.md#slice-7--keyboard-focus-and-text-entry-done) and [engine Slice
> 18](../ROADMAP.md#slice-18--a-keyboard-that-reaches-the-consumer-done) landed
> together: an ordered key/text event log on `UiInput`, Tab and Shift-Tab focus
> traversal with `focusable`/`set_focus` public, Enter and Space activating every
> existing button and checkbox, arrows nudging a focused slider, `wants_keyboard`
> so a camera and a text field can coexist, and a `text_field` with a caret,
> selection and clipboard. The engine half widened `Key` to fifty variants, added
> `Modifiers` and `MouseButton::Back`/`Forward`, and made Escape the consumer's to
> claim.
>
> **The prediction in the last paragraph held exactly.** The engine half *was* on
> the critical path, and it was the larger half — and the thing that actually bit
> was in it: Windows reports `text: Some("a")` for `Ctrl+A`, so "select all"
> typed an `a` until the engine learned to tell a shortcut from a keystroke.
>
> Two of the three needs below are answered outright. **Text selection and
> clipboard copy** — the third bullet, filed as "small, and consistently
> forgotten" — shipped with the field rather than waiting, because a selection is
> most of what a caret is for. What did *not* ship is anything table-shaped: the
> demo's walkable, filterable list was written **in the demo** from
> `next_id`/`focusable`/`allocate`/`interact`/`painter`, which is the same result
> this file's *Tables* entry already recorded, arrived at independently a second
> time.

Text input was [already on the waiting list](ROADMAP.md#waiting-on-a-roadblock)
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

One item that belonged in [`../ROADMAP.md`](../ROADMAP.md), noted here because the
same consumer pulls it: **an offscreen render target composited into a UI rect**,
so a 3D/2D scene is a *panel among panels* rather than a fullscreen background
with UI floating over it.

**It landed, as engine Slice 19** — `Renderer::set_scene_rect` takes a rectangle
in the same logical points this toolkit lays out in, and the scene renders into
it. Two things about how it arrived are worth recording here, because both bear
on the rest of this file:

- **It cost this crate nothing.** No `Painter` method, no `UiInput` field, no
  widget. The engine reads a rectangle the consumer computed and the toolkit
  never learns that a scene exists. That is the fifth demand in a row absorbed
  without the seam moving.
- **It was not driven by this consumer.** The engine wrote its own demo
  (`examples/workspace.rs`) and let that hit the wall, exactly as the stopping
  rule says. The wishlist entry did not cause the work; it was sitting under the
  demo when the demo arrived, and it turned out to have badly underestimated the
  job. Recognitions are not estimates.

Two more the same consumer has now hit, both small and both squarely the
engine's:

- ~~**A consumer cannot name its own window.**~~ **Done, as [engine Slice
  20](../ROADMAP.md#slice-20--a-consumers-own-window--done).** `run(app)` still
  takes no configuration; the answer went on the trait instead, as a defaulted
  `Application::config()` returning a `Config` — the same inversion
  `quit_on_escape` already used, and a consumer that doesn't care writes
  nothing. It carries the initial size and three window flags as well as the
  title, of which only the title had this consumer behind it; the rest are
  labeled speculative in `Config`'s own docs rather than filed as
  infrastructure.
  
  This entry called it trivial, and **it was trivial on native and not on the
  web** — which is the one thing worth keeping. winit puts a web window's title
  on the canvas's `alt` attribute rather than in `document.title`, so the first
  version of this shipped a config field that named a title bar and left the
  *tab* saying whatever the page's HTML said. That is the same complaint this
  entry filed, surviving its own fix. `title` is now `Option<String>` and the
  engine sets `document.title` itself, but only when a consumer actually asked —
  writing the default there would have overwritten every page's own caption.
- **A per-frame failure is logged every frame.** A surface that fails validation
  logs one line per attempt: the consumer's first run of its table screen
  produced **170,897 lines and 512 KiB in about eighteen minutes** and never
  presented a frame. The volume is the trivial cost; the real one is that it made
  the failure *harder* to spot, because half a megabyte of one repeated line
  reads as noise to skim past. An error that shouts continuously communicates
  less than one that speaks twice — edge-trigger it, with a count. Relatedly,
  `Renderer` exposes no surface-health signal, so a consumer cannot detect or
  assert the state either.

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
  read as an argument for one. **Half of this is now settled rather than
  asserted:** keyboard focus shipped in Slice 7 and cost `UiState` one `Vec<u64>`
  of declaration order per frame. Virtualization is still the open half.
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

> **The consumer did that, and the estimate above was too pessimistic — for a
> specific and reusable reason.** The table did not hurt, because it turned out
> not to be toolkit work at all: it was written in the consumer against the
> public seam, and the toolkit grew nothing. The largest item on the list moved
> rather than getting built.
>
> What is left is genuinely smaller and better shaped: **keyboard and text
> input** (now a named roadblock, and the only thing that has actually blocked
> the consumer), **virtualization** (still real, still needs designing *into* the
> scroll region), the **gutter fix** above, and the **painter additions** for
> charts. That is a slice or three, not a second project.
>
> The general lesson is the one this file was built to test, and it held: the
> unprivileged-widget rule is the scope control. It was a claim with no second
> consumer to check it; it now has one, and it paid for itself the first time
> something asked for a widget this crate never shipped.

> **Keyboard and text input is now built, and the estimate was wrong in the other
> direction.** It was named above as one of four remaining items, roughly a
> quarter of "a slice or three". It came to one slice on each side of the seam —
> and the *engine's* half was the larger one, which nothing in this file
> predicted. The toolkit's share was an ordered field on `UiInput`, a `Vec<u64>`
> of focus order, four widgets learning to read a key, and one new widget.
>
> What is left is **virtualization**, the **gutter fix**, the **painter
> additions** for charts, and table **sorting and selection** — and the last of
> those still looks consumer-shaped, because the demo built its walkable filtered
> list without the toolkit growing a list. That is the third time the
> unprivileged-widget rule has answered a question this file expected to cost a
> widget.

> **The gutter fix has since landed too**, along with the `columns` note and the
> window title — the three small, already-demanded items, taken together as [UI
> Slice 8](ROADMAP.md#slice-8--a-header-that-lines-up-with-its-body--done) and
> [engine Slice 20](../ROADMAP.md#slice-20--a-consumers-own-window--done). Two
> things about how they went are worth keeping:
>
> - **The gutter was answered by taking a number away, not by adding one.** The
>   obvious fix was to publish `scrollbar_w + gap`; what shipped keeps it
>   private and moves the arithmetic into the toolkit. A public number a
>   consumer must remember to subtract is not a fix, it is the same bug with
>   documentation.
> - **Slice 8 is the first thing this file has pulled directly**, and it exposed
>   a weakness in doing so. The roadblock was found in a consumer this project
>   cannot run, so the demo was written *after* the fix, to check it rather than
>   to discover the need. That is the reverse of the usual order and it produced
>   a smaller, more dutiful demo. A wishlist entry can say what to build; it
>   still cannot supply the thing a demo supplies, which is the surprise.
>
> What remains is **virtualization**, the **painter additions** for charts, table
> **sorting and selection**, **reactive repaint**, and the engine's
> **per-frame surface log**. Nothing on that list has a driver.
