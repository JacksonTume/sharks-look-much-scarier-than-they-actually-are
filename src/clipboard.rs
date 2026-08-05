//! The system clipboard, behind two functions and one `#[cfg]`.
//!
//! This is engine-internal on purpose. The UI toolkit has **no dependencies**, so
//! it cannot talk to an operating system at all; what it does instead is ask, via
//! [`UiState::take_clipboard`](slmsttaa_ui::UiState::take_clipboard), and the
//! engine carries the text the rest of the way. Inbound there is no seam to speak
//! of — a paste is delivered as ordinary
//! [`Event::Text`](crate::input::Event::Text) characters, so nothing downstream
//! of the event loop has to know a clipboard was involved.
//!
//! # Native and web are not equally good at this, and the difference is `winit`'s
//!
//! On the desktop this is `arboard` and it is synchronous and complete.
//!
//! On the web the obvious route is closed. The browser delivers clipboard content
//! through its own `paste` event — but `winit`'s web backend calls
//! `event.prevent_default()` on **every** keydown
//! (`platform_impl/web/web_sys/canvas.rs`), which cancels the default action that
//! would have produced it. Turning that off is not an option either: it is what
//! stops Tab from walking the page's focus ring and Space from scrolling it, both
//! of which this toolkit binds.
//!
//! So the web path uses the async `navigator.clipboard` API, and it is honestly
//! asymmetric:
//!
//! - **Copying out works everywhere.** `writeText` needs no permission when it
//!   runs inside a user gesture, which a Ctrl+C always is.
//! - **Pasting in is best-effort.** `readText` is permission-gated on Chromium
//!   and is not available to web content in Firefox at all. It is also a
//!   *promise*, so its answer cannot arrive on the frame the key was pressed.
//!
//! [`get`] therefore answers immediately from a cache and refreshes it in the
//! background: pasting text this page copied always works, and pasting text from
//! outside the page works on the second attempt on browsers that allow it. That
//! is worse than native, it is written down rather than papered over, and the
//! thing that would fix it is upstream.

/// Put `text` on the system clipboard.
pub(crate) fn set(text: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_owned())) {
            Ok(()) => log::debug!("copied {} bytes to the clipboard", text.len()),
            Err(err) => log::warn!("could not write the clipboard: {err}"),
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        web::cache(text.to_owned());
        web::write(text);
    }
}

/// Read the system clipboard, if there is anything readable there.
pub(crate) fn get() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
            Ok(text) => Some(text),
            Err(err) => {
                log::debug!("nothing readable on the clipboard: {err}");
                None
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        // Kick off a refresh for *next* time, then answer from what we already
        // have — see the module docs for why this cannot be synchronous.
        web::refresh();
        web::cached()
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;

    thread_local! {
        /// The last text we either wrote or successfully read back. This is what
        /// makes in-page copy/paste reliable on a browser that refuses
        /// `readText`.
        static CACHE: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    /// What [`super::get`] answers with.
    pub(super) fn cached() -> Option<String> {
        CACHE.with(|cache| cache.borrow().clone())
    }

    /// Remember `text` as the clipboard's contents.
    pub(super) fn cache(text: String) {
        CACHE.with(|cache| *cache.borrow_mut() = Some(text));
    }

    /// The browser's clipboard object, if this context has one at all. Absent on
    /// an insecure origin, which is worth saying out loud rather than logging a
    /// promise rejection.
    fn clipboard() -> Option<web_sys::Clipboard> {
        web_sys::window().map(|window| window.navigator().clipboard())
    }

    /// Write `text` out. Fire-and-forget: the promise's answer arrives after the
    /// frame and there is nothing useful to do with it but log a failure.
    pub(super) fn write(text: &str) {
        let Some(clipboard) = clipboard() else {
            return;
        };
        let promise = clipboard.write_text(text);
        wasm_bindgen_futures::spawn_local(async move {
            if wasm_bindgen_futures::JsFuture::from(promise).await.is_err() {
                log::warn!("the browser refused a clipboard write");
            }
        });
    }

    /// Ask the browser for the clipboard's contents and update the cache when the
    /// answer comes back — a frame or two later, and never at all on a browser
    /// that does not offer `readText` to web content.
    pub(super) fn refresh() {
        let Some(clipboard) = clipboard() else {
            return;
        };
        let promise = clipboard.read_text();
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(value) => {
                    if let Some(text) = value.as_string() {
                        cache(text);
                    }
                }
                // Firefox does not implement this for web content, and Chromium
                // gates it behind a permission prompt. Either way the cache keeps
                // whatever this page last copied, which is the common case.
                Err(_) => log::debug!("the browser would not hand over the clipboard"),
            }
        });
    }
}
