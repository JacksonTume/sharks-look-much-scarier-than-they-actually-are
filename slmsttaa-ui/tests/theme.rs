//! Theme tests: that the tokens are actually *read*, and that variants and
//! sizes reach the painter.
//!
//! The claim UI Slice 4 makes is auditable in a way most styling work is not:
//! **no widget names a literal color or metric**. A `RecordingPainter` can check
//! that directly — style a frame with a theme whose tokens are all distinct
//! sentinel values, and any color that comes out unaccounted for is a widget
//! that kept a hard-coded one.
//!
//! That is the test the crate could not have had before this slice, and it is
//! the one that will catch the regression: a widget added later with
//! `[0.26, 0.59, 0.98, 1.0]` typed into it looks perfect on the default theme
//! and wrong on every other, which is exactly the failure a screenshot misses.

use slmsttaa_ui::{
    Anchor, Color, DrawCmd, RecordingPainter, Rect, Size, Theme, Ui, UiInput, UiState, Variant,
};

/// Restated rather than imported, as elsewhere in this suite.
const PANEL_W: f32 = 340.0;
const ROW_H: f32 = 24.0;

/// Run one frame inside a default top-left panel, styled with `theme`.
fn frame<T>(theme: Theme, declare: impl FnOnce(&mut Ui) -> T) -> (RecordingPainter, T) {
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let result = {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.set_theme(theme);
        ui.panel(Anchor::TopLeft, PANEL_W, declare)
    };
    (painter, result)
}

/// Every color the frame drew with, fills and text alike.
fn colors(p: &RecordingPainter) -> Vec<Color> {
    p.cmds
        .iter()
        // Exhaustive on purpose. A primitive added to the seam without a case
        // here is a compile error rather than a hole in the rule this file
        // exists to enforce — which is exactly how the three UI Slice 10 shapes
        // arrived.
        .map(|c| match *c {
            DrawCmd::Rect { color, .. }
            | DrawCmd::Text { color, .. }
            | DrawCmd::Polyline { color, .. }
            | DrawCmd::Polygon { color, .. } => color,
            DrawCmd::Image { tint, .. } => tint,
        })
        .collect()
}

/// Whether `color` was used anywhere in the frame.
fn used(p: &RecordingPainter, color: Color) -> bool {
    colors(p).contains(&color)
}

/// A theme whose every color token is a distinct, recognizable sentinel.
///
/// The red channel is the token's index, so a color that turns up in the
/// recording can be traced back to the token it came from — and a color that is
/// *not* on this list came from somewhere it shouldn't have.
fn sentinel_theme() -> Theme {
    let mut theme = Theme::dark();
    let mut n = 0.0_f32;
    let mut next = || {
        n += 1.0;
        [n / 100.0, 0.5, 0.5, 1.0]
    };
    theme.color.background = next();
    theme.color.foreground = next();
    theme.color.muted = next();
    theme.color.surface = next();
    theme.color.border = next();
    theme.color.ring = next();
    theme.color.selection = next();
    theme.color.heading = next();
    theme.color.accent = next();
    theme.color.accent_hover = next();
    theme.color.primary = next();
    theme.color.primary_hover = next();
    theme.color.primary_foreground = next();
    theme.color.secondary = next();
    theme.color.secondary_hover = next();
    theme.color.secondary_foreground = next();
    theme.color.destructive = next();
    theme.color.destructive_hover = next();
    theme.color.destructive_foreground = next();
    theme
}

/// Declare one of everything, so a test sweeps the whole roster.
fn every_widget(ui: &mut Ui) {
    let (mut flag, mut value) = (true, 0.5_f32);
    let mut name = String::from("ridge");
    ui.title("Terrain");
    ui.label("plain");
    ui.label_muted("hint");
    ui.label_value("fps", "60");
    ui.separator();
    ui.section("Base shape", |ui| {
        ui.slider("frequency", &mut value, 0.0, 1.0).show();
    });
    ui.checkbox("wireframe", &mut flag);
    ui.text_field("name", &mut name).show();
    ui.button("new seed").show();
    ui.button("alps").variant(Variant::Secondary).show();
    ui.button("reset").variant(Variant::Destructive).show();
}

#[test]
fn no_widget_draws_a_color_the_theme_did_not_supply() {
    let theme = sentinel_theme();
    let (painter, _) = frame(theme, every_widget);

    let tokens: Vec<Color> = vec![
        theme.color.background,
        theme.color.foreground,
        theme.color.muted,
        theme.color.surface,
        theme.color.border,
        theme.color.ring,
        theme.color.selection,
        theme.color.heading,
        theme.color.accent,
        theme.color.accent_hover,
        theme.color.primary,
        theme.color.primary_hover,
        theme.color.primary_foreground,
        theme.color.secondary,
        theme.color.secondary_hover,
        theme.color.secondary_foreground,
        theme.color.destructive,
        theme.color.destructive_hover,
        theme.color.destructive_foreground,
    ];

    for color in colors(&painter) {
        assert!(
            tokens.contains(&color),
            "{color:?} is not one of the theme's tokens — some widget still \
             names a literal color",
        );
    }
}

#[test]
fn swapping_the_theme_restyles_every_widget() {
    let (dark, _) = frame(Theme::dark(), every_widget);
    let (light, _) = frame(Theme::light(), every_widget);

    // Same picture, different paint: the geometry is identical because only the
    // palette moved, and not one color survives the swap.
    assert_eq!(dark.cmds.len(), light.cmds.len());
    for (a, b) in colors(&dark).iter().zip(colors(&light).iter()) {
        assert_ne!(a, b, "a color came through the swap unchanged");
    }
}

#[test]
fn a_consumers_own_widget_restyles_with_the_built_in_ones() {
    // The unprivileged rule, checked against tokens: this reads exactly what a
    // built-in reads, so it has to move with the theme like one.
    fn meter(ui: &mut Ui, label: &str, t: f32) {
        let theme = *ui.theme();
        let id = ui.next_id(label);
        let row = ui.allocate([0.0, theme.control.row_h]);
        let _ = ui.interact(row, id);
        let filled = Rect::new(row.x, row.y, row.w * t.clamp(0.0, 1.0), row.h);
        ui.painter()
            .fill_rect(row, theme.radius.md, theme.color.surface);
        ui.painter()
            .fill_rect(filled, theme.radius.md, theme.color.accent);
    }

    let (dark, _) = frame(Theme::dark(), |ui| meter(ui, "carve", 0.4));
    let (light, _) = frame(Theme::light(), |ui| meter(ui, "carve", 0.4));

    assert!(used(&dark, Theme::dark().color.accent));
    assert!(!used(&light, Theme::dark().color.accent));
    assert!(used(&light, Theme::light().color.accent));
}

#[test]
fn a_variant_picks_its_own_fill_and_text_color() {
    let theme = sentinel_theme();

    for (variant, fill, text) in [
        (
            Variant::Primary,
            theme.color.primary,
            theme.color.primary_foreground,
        ),
        (
            Variant::Secondary,
            theme.color.secondary,
            theme.color.secondary_foreground,
        ),
        (
            Variant::Destructive,
            theme.color.destructive,
            theme.color.destructive_foreground,
        ),
    ] {
        let (painter, _) = frame(theme, |ui| ui.button("go").variant(variant).show());
        assert!(used(&painter, fill), "{variant:?} drew the wrong fill");
        assert!(used(&painter, text), "{variant:?} drew the wrong label");
    }
}

#[test]
fn a_hovered_button_takes_the_hover_token() {
    let theme = sentinel_theme();
    // The first row starts one margin plus one pad below the top.
    let input = UiInput {
        cursor: Some((100.0, 30.0)),
        ..UiInput::default()
    };

    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    {
        let mut ui = Ui::new(&mut painter, input, &mut state);
        ui.set_theme(theme);
        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.button("go").variant(Variant::Destructive).show()
        });
    }

    assert!(used(&painter, theme.color.destructive_hover));
    assert!(!used(&painter, theme.color.destructive));
}

#[test]
fn size_changes_the_row_a_button_consumes() {
    let theme = Theme::dark();
    let rows: Vec<_> = [Size::Sm, Size::Md, Size::Lg]
        .map(|size| {
            let (_, r) = frame(theme, |ui| ui.button("go").size(size).show());
            r.rect
        })
        .into_iter()
        .collect();

    assert!(rows[0].h < rows[1].h, "Sm should be shorter than Md");
    assert!(rows[1].h < rows[2].h, "Lg should be taller than Md");
    // Md is unchanged from every slice before this one: a 20-point face inside a
    // 24-point row. Sizes were added without moving the default.
    assert_eq!(rows[1].h, ROW_H - 4.0);
}

#[test]
fn metrics_are_read_from_the_theme_not_baked_in() {
    // Nothing in the roster hard-codes 24-point rows or 8-point gaps: double
    // them and the layout has to follow.
    let mut theme = Theme::dark();
    theme.control.row_h = 40.0;
    theme.space.indent = 32.0;

    let (_, (plain, indented)) = frame(theme, |ui| {
        let plain = ui.label("a").rect;
        let indented = ui.indent(|ui| ui.label("b").rect);
        (plain, indented)
    });

    assert_eq!(plain.h, 40.0);
    assert_eq!(indented.x, plain.x + 32.0);
    assert_eq!(indented.y, plain.y + 40.0);
}
