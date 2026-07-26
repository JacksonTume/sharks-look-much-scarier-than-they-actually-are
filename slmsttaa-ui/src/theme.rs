//! The look: **semantic tokens**, in one [`Theme`] value.
//!
//! Slices 0–3 kept the look in a flat wall of `const`s — `COL_BTN`,
//! `COL_ACCENT_HOT`, `RADIUS_LG`. That held while there were six widgets and one
//! panel. It stopped holding the moment a second kind of button was wanted: a
//! destructive "reset" has no constant to reach for, so it becomes a hand-colored
//! rectangle, and the next one after that becomes a second hand-colored
//! rectangle. Ten widgets then look like ten decisions.
//!
//! Tokens are the fix, and the rule they enforce is short: **a widget never names
//! a literal color**. It asks for [`Palette::accent`] or
//! [`Theme::fill`]`(`[`Variant::Destructive`]`, …)`, and the *theme* decides what
//! that is. Restyling is then swapping one value rather than auditing every
//! `fill_rect` call in the crate.
//!
//! ```
//! # use slmsttaa_ui::{theme::Theme, RecordingPainter, Ui, UiInput, UiState};
//! # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
//! let mut ui = Ui::new(&mut p, UiInput::default(), &mut s);
//! ui.set_theme(Theme::light()); // the whole UI, restyled
//! ```
//!
//! ## What is a token and what is not
//!
//! Everything here is either a **color with a job** (`accent` is "the highlight",
//! not "blue") or a **step on a scale** ([`Radii`], [`Space`], [`TypeScale`],
//! [`Control`]). Nothing here is a widget's private business: a slider's knob is
//! `control.knob_w` wide because every slider agrees on that, but *where* the knob
//! goes is the slider's own arithmetic and stays in `widgets/slider.rs`.
//!
//! ## Why this module is public
//!
//! Same reason `allocate` / `interact` / `painter` are: a widget written by a
//! consumer has to be able to look like the ones that ship here, and it cannot if
//! the tokens are private. That is the unprivileged-widget rule, and this module
//! is part of its cost. A consumer's widget reads [`Ui::theme`](crate::Ui::theme)
//! exactly as a built-in one does.
//!
//! All metrics are in **logical points**, so they mean the same thing on a 1× and
//! a 2× display.

use crate::Color;

/// The emphasis level a control is drawn at.
///
/// Three, because the terrain demo asked for three in one panel: a plain action
/// (`new seed`), a row of low-emphasis choices (the shape presets), and one
/// action that destroys work (`reset`). A fourth — shadcn's `ghost` — is not here
/// because nothing has needed it, which is the roadmap's stopping rule applied to
/// styling rather than to widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Variant {
    /// The default: a filled control that reads as the row's main action.
    #[default]
    Primary,
    /// Low emphasis — a faint wash, for one of several equivalent choices.
    Secondary,
    /// Destroys something. Colored so that it is not clicked by accident.
    Destructive,
}

/// How large a control is drawn.
///
/// A scale rather than a height in points: the whole argument for tokens is that
/// a caller says *what it means* and the theme says how big that is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Size {
    /// Compact — for controls packed several to a row.
    Sm,
    /// The standard row control. The default.
    #[default]
    Md,
    /// Emphasized, for a control that should be easy to hit.
    Lg,
}

impl Size {
    /// The height of the control's drawn face, in points.
    ///
    /// A row is this plus a 4-point trailing gap, which is the spacing every
    /// button has had since Slice 0.
    pub fn face_height(self, theme: &Theme) -> f32 {
        match self {
            Size::Sm => theme.control.row_h - 6.0,
            Size::Md => theme.control.row_h - 4.0,
            Size::Lg => theme.control.row_h + 6.0,
        }
    }

    /// The cell size of text drawn inside the control, in points.
    pub fn text_px(self, theme: &Theme) -> f32 {
        match self {
            Size::Sm => theme.text.small,
            Size::Md | Size::Lg => theme.text.body,
        }
    }
}

/// Every color the toolkit draws, named by **job** rather than by hue.
///
/// The names are the contract. `accent` means "the highlight the eye should land
/// on"; it happens to be blue in [`Theme::dark`] and a different blue in
/// [`Theme::light`], and a consumer is free to make it orange. A widget that
/// wrote `[0.26, 0.59, 0.98, 1.0]` instead would survive neither.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Panel fill. Translucent by convention, so the 3D scene reads through.
    pub background: Color,
    /// Primary text, on [`Palette::background`].
    pub foreground: Color,
    /// Secondary text: readouts, hints, the scroll thumb.
    pub muted: Color,
    /// A faint wash for inert surfaces — slider tracks, checkbox wells,
    /// separators — and the scrim laid over a control while it is pressed.
    pub surface: Color,
    /// Hairline outlines. Does the job a drop shadow would, for one stroke.
    pub border: Color,
    /// The focus ring around whatever was last clicked.
    pub ring: Color,
    /// Section headings.
    pub heading: Color,
    /// The highlight: slider fill, checkbox tick, the rule under a title.
    pub accent: Color,
    /// The highlight, hovered or being dragged.
    pub accent_hover: Color,
    /// Fill for a [`Variant::Primary`] control.
    pub primary: Color,
    /// Fill for a hovered [`Variant::Primary`] control.
    pub primary_hover: Color,
    /// Text drawn on [`Palette::primary`].
    pub primary_foreground: Color,
    /// Fill for a [`Variant::Secondary`] control.
    pub secondary: Color,
    /// Fill for a hovered [`Variant::Secondary`] control.
    pub secondary_hover: Color,
    /// Text drawn on [`Palette::secondary`].
    pub secondary_foreground: Color,
    /// Fill for a [`Variant::Destructive`] control.
    pub destructive: Color,
    /// Fill for a hovered [`Variant::Destructive`] control.
    pub destructive_hover: Color,
    /// Text drawn on [`Palette::destructive`].
    pub destructive_foreground: Color,
}

/// The corner-radius scale.
///
/// Three steps, because a panel, a control, and the tick inside a control are the
/// three sizes of thing this toolkit rounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radii {
    /// Small details nested inside a control — a checkbox tick.
    pub sm: f32,
    /// Controls: buttons, checkbox wells, the scroll indicator.
    pub md: f32,
    /// Panels and other large surfaces.
    pub lg: f32,
}

/// The spacing scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Space {
    /// Between a panel and the window edge it is anchored to.
    pub margin: f32,
    /// Between a panel's edge and its contents.
    pub pad: f32,
    /// Between two widgets sharing a row.
    pub gap: f32,
    /// How far [`Ui::indent`](crate::Ui::indent) steps in.
    pub indent: f32,
}

/// The type scale, in glyph cell size (points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeScale {
    /// Compact text, inside a [`Size::Sm`] control.
    pub small: f32,
    /// Body text — labels, values, button faces.
    pub body: f32,
    /// Section headings.
    pub section: f32,
    /// Panel titles.
    pub title: f32,
}

/// Metrics shared by the controls themselves.
///
/// These are the numbers every widget has to agree on for a panel to look like
/// one thing: rows the same height, tracks the same thickness, hairlines the same
/// weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Control {
    /// The height of one standard widget row.
    pub row_h: f32,
    /// Slider track thickness.
    pub track_h: f32,
    /// Slider knob width.
    pub knob_w: f32,
    /// Width of the indicator beside an overflowing scroll area.
    pub scrollbar_w: f32,
    /// How many points one wheel notch scrolls.
    pub scroll_speed: f32,
    /// Standard hairline stroke width.
    pub border: f32,
    /// Focus ring thickness.
    pub ring: f32,
}

/// Everything the toolkit needs to know about how it should look.
///
/// One [`Ui`](crate::Ui) frame holds one of these. It is [`Copy`] and about 300
/// bytes, which is what lets a widget take a snapshot (`let t = self.theme;`) and
/// then borrow the painter mutably without a fight.
///
/// Set it with [`Ui::set_theme`](crate::Ui::set_theme) at the top of each frame.
/// That it must be re-applied every frame is immediate mode being consistent
/// rather than an oversight: the consumer owns the value, the toolkit borrows it
/// for the frame, and nothing style-shaped accumulates in
/// [`UiState`](crate::UiState).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Colors, named by job.
    pub color: Palette,
    /// The corner-radius scale.
    pub radius: Radii,
    /// The spacing scale.
    pub space: Space,
    /// The type scale.
    pub text: TypeScale,
    /// Metrics shared by controls.
    pub control: Control,
    /// The panel width a caller with no opinion gets.
    ///
    /// Only a default: [`Ui::panel`](crate::Ui::panel) takes the width it should
    /// use, because a HUD showing "60 fps" and a parameter panel full of sliders
    /// want very different rectangles.
    pub panel_w: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    /// The dark theme, and the default — a translucent slab over a 3D scene.
    ///
    /// These are the exact colors and metrics the toolkit shipped with through
    /// Slice 3, so adopting tokens changed the vocabulary without changing the
    /// picture.
    pub fn dark() -> Self {
        Self::with_palette(Palette {
            background: [0.04, 0.06, 0.09, 0.78],
            foreground: [0.86, 0.90, 0.95, 1.0],
            muted: [0.55, 0.60, 0.68, 1.0],
            surface: [1.0, 1.0, 1.0, 0.14],
            border: [1.0, 1.0, 1.0, 0.10],
            ring: [0.42, 0.72, 1.0, 0.85],
            heading: [0.45, 0.66, 0.92, 1.0],
            accent: [0.26, 0.59, 0.98, 1.0],
            accent_hover: [0.42, 0.72, 1.0, 1.0],
            primary: [0.18, 0.32, 0.55, 1.0],
            primary_hover: [0.26, 0.46, 0.78, 1.0],
            primary_foreground: [0.86, 0.90, 0.95, 1.0],
            secondary: [1.0, 1.0, 1.0, 0.10],
            secondary_hover: [1.0, 1.0, 1.0, 0.18],
            secondary_foreground: [0.86, 0.90, 0.95, 1.0],
            destructive: [0.60, 0.20, 0.22, 1.0],
            destructive_hover: [0.78, 0.28, 0.30, 1.0],
            destructive_foreground: [0.99, 0.93, 0.93, 1.0],
        })
    }

    /// A light theme, for a scene bright enough that a dark slab fights it.
    ///
    /// It exists to be the *proof* that tokens work: every widget in the terrain
    /// demo restyles when this is swapped in, and not one of them was edited to
    /// make that happen. If some control had kept a literal color, this is where
    /// it would show up as the one thing that stayed dark.
    pub fn light() -> Self {
        Self::with_palette(Palette {
            background: [0.96, 0.97, 0.98, 0.88],
            foreground: [0.10, 0.12, 0.16, 1.0],
            muted: [0.38, 0.42, 0.48, 1.0],
            surface: [0.05, 0.07, 0.12, 0.12],
            border: [0.05, 0.07, 0.12, 0.18],
            ring: [0.15, 0.45, 0.90, 0.85],
            heading: [0.13, 0.35, 0.68, 1.0],
            accent: [0.15, 0.45, 0.90, 1.0],
            accent_hover: [0.25, 0.56, 0.98, 1.0],
            primary: [0.20, 0.48, 0.88, 1.0],
            primary_hover: [0.28, 0.56, 0.96, 1.0],
            primary_foreground: [0.98, 0.99, 1.0, 1.0],
            secondary: [0.05, 0.07, 0.12, 0.08],
            secondary_hover: [0.05, 0.07, 0.12, 0.16],
            secondary_foreground: [0.10, 0.12, 0.16, 1.0],
            destructive: [0.78, 0.20, 0.22, 1.0],
            destructive_hover: [0.88, 0.30, 0.32, 1.0],
            destructive_foreground: [1.0, 0.96, 0.96, 1.0],
        })
    }

    /// `color` over the standard metrics, which is what a new theme usually is.
    ///
    /// The scales are what make a panel *this* toolkit's panel — a restyle
    /// should not have to restate that rows are 24 points tall. A consumer that
    /// genuinely wants different metrics edits the fields afterward; they are all
    /// public.
    ///
    /// ```
    /// # use slmsttaa_ui::theme::Theme;
    /// let mut theme = Theme::dark();
    /// theme.color.accent = [0.95, 0.55, 0.15, 1.0]; // orange highlights
    /// theme.radius.lg = 0.0; // and square panels
    /// ```
    pub fn with_palette(color: Palette) -> Self {
        Self {
            color,
            radius: Radii {
                sm: 3.0,
                md: 4.0,
                lg: 8.0,
            },
            space: Space {
                margin: 12.0,
                pad: 10.0,
                gap: 8.0,
                indent: 16.0,
            },
            text: TypeScale {
                small: 13.0,
                body: 16.0,
                section: 15.0,
                title: 20.0,
            },
            control: Control {
                row_h: 24.0,
                track_h: 8.0,
                knob_w: 10.0,
                scrollbar_w: 4.0,
                scroll_speed: 28.0,
                border: 1.0,
                ring: 2.0,
            },
            panel_w: 340.0,
        }
    }

    /// The fill for a `variant` control, hovered or not.
    ///
    /// Pressed is deliberately **not** a third color here. A control that is held
    /// draws this fill and then a [`Palette::surface`] scrim over it, which gives
    /// every variant a pressed state from tokens that already exist — and works
    /// in both directions, since `surface` is a light wash on a dark theme and a
    /// dark one on a light theme. The alternative was three more tokens per
    /// variant to say the same thing.
    pub fn fill(&self, variant: Variant, hovered: bool) -> Color {
        match (variant, hovered) {
            (Variant::Primary, false) => self.color.primary,
            (Variant::Primary, true) => self.color.primary_hover,
            (Variant::Secondary, false) => self.color.secondary,
            (Variant::Secondary, true) => self.color.secondary_hover,
            (Variant::Destructive, false) => self.color.destructive,
            (Variant::Destructive, true) => self.color.destructive_hover,
        }
    }

    /// The text color to draw on top of [`Theme::fill`] for a `variant`.
    pub fn on_fill(&self, variant: Variant) -> Color {
        match variant {
            Variant::Primary => self.color.primary_foreground,
            Variant::Secondary => self.color.secondary_foreground,
            Variant::Destructive => self.color.destructive_foreground,
        }
    }
}
