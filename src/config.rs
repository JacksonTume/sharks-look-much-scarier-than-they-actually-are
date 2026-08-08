//! How a consumer describes the window it wants.
//!
//! Everything here used to be hard-coded in [`app`](crate::app)'s window
//! creation, which meant a shipped game's title bar said "SLMSTTAA" and there
//! was nothing a consumer could do about it. [`Config`] is the seam that fixes
//! that, and it arrives through the same inversion of control everything else
//! does: a defaulted [`Application::config`] the engine calls once, rather than
//! a second `run_with` entry point. A consumer that doesn't care writes nothing.
//!
//! [`Application::config`]: crate::Application::config

/// The window a consumer wants opened.
///
/// [`Config::default`] is exactly what the engine hard-coded before this
/// existed, so adopting it changes nothing until a field is set.
///
/// ```
/// use slmsttaa::{Application, Config, Renderer};
///
/// struct Demo;
///
/// impl Application for Demo {
///     fn config(&self) -> Config {
///         Config::default()
///             .with_title("The Matchmaker")
///             .with_size(1600.0, 900.0)
///     }
///
///     fn init(&mut self, _renderer: &mut Renderer) {}
/// }
/// ```
///
/// # Scope, recorded honestly
///
/// Only [`title`](Config::title) had a consumer behind it. `size` came along
/// because it was hard-coded on the adjacent line and splitting them would mean
/// touching the same code twice; the three flags below it are **speculative** by
/// the roadmap's own stopping rule — added while the struct was being introduced
/// because that is the cheapest moment, not because anything asked. They are
/// labeled rather than quietly filed as infrastructure.
///
/// # On the web
///
/// A tab is not a window, so most of this does not cross over:
///
/// - [`size`](Config::size) is ignored twice over — winit drops
///   `with_inner_size` on the web outright, and the canvas is sized to the
///   browser viewport and resynced on resize (see `ARCHITECTURE.md`).
/// - The three flags describe a decoration the page does not have, and are
///   no-ops.
/// - [`title`](Config::title) **does** name the tab, but only when you set one.
///   winit alone would not do this: its web backend puts the title on the
///   canvas's `alt` attribute, which is accessibility text and not a caption,
///   so the engine sets `document.title` itself. It does that *only* for
///   `Some` — an unset title leaves the page's own `<title>` alone rather than
///   overwriting a caption the consumer wrote in their HTML with an engine
///   default they never chose.
///
/// This block originally claimed the title crossed over on its own. Checked
/// against winit 0.30's source rather than assumed, it did not — which is what
/// turned a documentation fix into the three lines that make the claim true.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// What to call the window, or `None` for "no opinion".
    ///
    /// `None` is not the same as `Some("SLMSTTAA")`, and the difference only
    /// shows on the web. Natively both open a window captioned
    /// [`Config::DEFAULT_TITLE`]. On the web, `None` leaves `document.title`
    /// alone so the page's own `<title>` survives, while `Some` overwrites it —
    /// so an engine default can never clobber a caption a consumer wrote in
    /// their own HTML. Read it through [`Config::window_title`] when you want
    /// the resolved string.
    pub title: Option<String>,
    /// The initial inner size in **logical** points — the same units the UI
    /// toolkit lays out in, so the display's scale factor is not your problem.
    /// Defaults to `(1280.0, 720.0)`. Ignored on the web.
    pub size: (f32, f32),
    /// Whether the user may resize the window. Defaults to `true`.
    pub resizable: bool,
    /// Whether the OS draws a title bar and border. Defaults to `true`.
    pub decorations: bool,
    /// Whether to open maximized. Defaults to `false`.
    pub maximized: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            title: None,
            size: (1280.0, 720.0),
            resizable: true,
            decorations: true,
            maximized: false,
        }
    }
}

impl Config {
    /// What a window is called when the consumer has not said.
    pub const DEFAULT_TITLE: &'static str = "SLMSTTAA";

    /// The title to actually put on the window: the consumer's, or
    /// [`Config::DEFAULT_TITLE`].
    pub fn window_title(&self) -> &str {
        self.title.as_deref().unwrap_or(Self::DEFAULT_TITLE)
    }

    /// Set the window title. On the web this also names the browser tab, which
    /// leaving it unset deliberately does not.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the initial inner size, in logical points.
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = (width, height);
        self
    }

    /// Set whether the user may resize the window.
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set whether the OS draws a title bar and border.
    pub fn decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    /// Set whether the window opens maximized.
    pub fn maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Config;
    use crate::application::Application;
    use crate::renderer::Renderer;

    /// A consumer that says nothing about its window.
    struct Silent;
    impl Application for Silent {
        fn init(&mut self, _renderer: &mut Renderer) {}
    }

    /// A consumer that does.
    struct Named;
    impl Application for Named {
        fn config(&self) -> Config {
            Config::default()
                .with_title("The Matchmaker")
                .with_size(1600.0, 900.0)
        }
        fn init(&mut self, _renderer: &mut Renderer) {}
    }

    #[test]
    fn the_default_is_what_the_engine_used_to_hard_code() {
        // `app::resumed` had these four numbers and this string written into it.
        // If this test ever has to change, every existing consumer's window
        // changed size or name without asking.
        let config = Config::default();
        // Unset rather than "SLMSTTAA": the engine supplies the caption, so the
        // web half can tell "no opinion" from "called the default on purpose".
        assert_eq!(config.title, None);
        assert_eq!(config.window_title(), "SLMSTTAA");
        assert_eq!(config.size, (1280.0, 720.0));
        assert!(config.resizable);
        assert!(config.decorations);
        assert!(!config.maximized);
    }

    #[test]
    fn a_consumer_that_says_nothing_gets_the_default() {
        assert_eq!(Silent.config(), Config::default());
    }

    #[test]
    fn an_unset_title_stays_distinguishable_from_the_default_one() {
        // The whole web design rests on this. `app::resumed` writes
        // `document.title` for `Some` and skips it for `None`, so the two must
        // not collapse into one value — if they did, every page that never
        // asked to be renamed would have its own `<title>` replaced by a
        // generic engine string. They agree on the *caption* and differ on
        // whether one was asked for, which is exactly the distinction needed.
        let unset = Config::default();
        let explicit = Config::default().with_title(Config::DEFAULT_TITLE);
        assert_eq!(unset.window_title(), explicit.window_title());
        assert_ne!(unset.title, explicit.title);
    }

    #[test]
    fn a_consumer_that_names_its_window_is_read_through_the_trait() {
        // The point of the whole item: the engine asks `dyn Application`, so a
        // consumer it has never heard of can answer.
        let app: &dyn Application = &Named;
        let config = app.config();
        assert_eq!(config.title.as_deref(), Some("The Matchmaker"));
        assert_eq!(config.window_title(), "The Matchmaker");
        assert_eq!(config.size, (1600.0, 900.0));
        // Untouched fields keep the default.
        assert!(config.resizable);
    }

    #[test]
    fn the_builders_set_only_what_they_name() {
        let config = Config::default()
            .resizable(false)
            .decorations(false)
            .maximized(true);
        assert_eq!(config.title, Config::default().title);
        assert_eq!(config.size, Config::default().size);
        assert!(!config.resizable);
        assert!(!config.decorations);
        assert!(config.maximized);
    }
}
