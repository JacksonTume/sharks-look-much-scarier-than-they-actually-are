# SLMSTTAA

A small 3D engine with a UI toolkit beside it, built so that the code on top of it,
the consumer, never sees GPU or windowing plumbing.

## Process

**Slice**:
A numbered unit of work on the engine roadmap. Bare "Slice N" always means this one.
_Avoid_: milestone, phase

**UI Slice**:
A numbered unit of work on the `slmsttaa-ui` roadmap. Its numbering is separate from the
engine's, so it is always written "UI Slice N" and never shortened to "Slice N".

**Block**:
A section of the roadmap's forward plan. It groups the slices expected to land together.

**Vertical**:
A block led by one demo, aimed at making that demo's cool thing shockingly easy. Every
vertical is a block, but some blocks (Block 0, for one) are not verticals.
_Avoid_: feature, epic

## Consumers

**Consumer**:
Any code built on the engine's or the toolkit's public API alone, whether it lives in this
repo or not.
_Avoid_: client, user, app

**Demo**:
A consumer in this repo that hits roadblocks, and those roadblocks decide what the engine
gets next. Some consumers are not demos, but only a demo justifies engine work.
_Avoid_: example (that is only how Cargo packages a demo), sample

## Layout

**Window**:
The rectangle everything is drawn into: the OS window on native, the canvas on the web.
Its size in logical points is what layout is computed against.
_Avoid_: viewport, surface, canvas size

**Workspace**:
A window split into panes, with the scene in one of them. The alternative is a scene that
fills the window, with panels floating over it.
_Avoid_: split view, docking layout

**Pane**:
One of the separate areas of a split window (following the Microsoft Style Guide). It is
filled either by a panel or by the scene. A panel floating over a full-window scene is not
in a pane.
_Avoid_: column, tile, region

**Scene pane**:
The pane the scene is drawn into. When the window isn't split, there is no scene pane and
the scene fills the window.
_Avoid_: viewport, scene rect, inset

**Panel**:
A toolkit container of widgets with its own background, sized to fit its contents. It
either floats over the scene or fills a pane. The scene itself is never a panel.
_Avoid_: pane (for a floating panel), window

**Viewport**:
The visible part of a scroll area's content.
_Avoid_: using it for the window or for the scene pane
