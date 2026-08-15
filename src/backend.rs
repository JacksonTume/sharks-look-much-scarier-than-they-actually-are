//! Choosing a backend and a limit set **on purpose**, so parity claims can be
//! checked instead of reasoned about.
//!
//! # The problem this exists for
//!
//! The engine targets two very different GPU stacks and says so constantly:
//! `ARCHITECTURE.md` and the roadmap between them record that WebGL2 has no
//! non-zero `first_instance`, no storage buffers, sixteen vertex attributes, no
//! re-viewable surface texture, and no derivatives worth relying on. Every one of
//! those constraints is honored in the code.
//!
//! Almost none of them has ever been *executed*. Chrome serves the WebGPU backend
//! whenever it can, so five slices running were verified in a browser that never
//! once took the fallback path — and Slice 8's `first_instance` bug is the proof
//! that this matters, because it compiled, ran perfectly on native, and was broken
//! only in the branch nobody had entered.
//!
//! # Why it is environment-driven and not API
//!
//! `src/capture.rs` set the precedent and the reasoning is identical: a
//! `Renderer::set_backend` that only a parity check calls would widen the public
//! surface `examples/triangle.rs` exists to measure. A developer forcing a
//! fallback is not a consumer configuring an application, so this is reachable
//! from outside the process and from nowhere inside it.
//!
//! # Using it
//!
//! Native, where `gl` gives you a real OpenGL adapter:
//!
//! ```sh
//! SLMSTTAA_BACKEND=gl cargo run --example scene
//! SLMSTTAA_LIMITS=webgl2 cargo run --example terrain
//! ```
//!
//! Web, as query parameters on the page the demo is served from:
//!
//! ```text
//! http://localhost:8080/?backend=gl
//! http://localhost:8080/?backend=gl&limits=webgl2
//! ```
//!
//! **`limits` is the more interesting of the two, and it is the one that works
//! everywhere.** The backend override needs a GL driver to be present; the limit
//! override needs nothing, and asking a desktop Vulkan adapter for
//! `downlevel_webgl2_defaults` reproduces the *ceilings* the browser fallback
//! imposes — sixteen vertex attributes among them, of which the instance buffer
//! currently spends fourteen. A demo that exceeds one now fails on the machine it
//! was written on rather than in a browser somebody else opened.

/// Which backends the instance should consider.
///
/// Defaults to the best primary backend on native, and WebGPU-with-a-GL-fallback
/// on the web — the behaviour that shipped from Slice 0 to Slice 22.
pub(crate) fn backends() -> wgpu::Backends {
    #[cfg(not(target_arch = "wasm32"))]
    let default = wgpu::Backends::PRIMARY;
    // Prefer WebGPU, but allow the GL (WebGL2) fallback so browsers without
    // WebGPU still run. `PRIMARY` alone excludes GL, which is why a WebGPU-less
    // browser would otherwise find no adapter.
    #[cfg(target_arch = "wasm32")]
    let default = wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL;

    let Some(name) = setting("backend") else {
        return default;
    };
    let chosen = match name.to_ascii_lowercase().as_str() {
        "gl" | "webgl" | "webgl2" | "opengl" => wgpu::Backends::GL,
        "webgpu" | "browser_webgpu" => wgpu::Backends::BROWSER_WEBGPU,
        "vulkan" => wgpu::Backends::VULKAN,
        "dx12" | "d3d12" => wgpu::Backends::DX12,
        "metal" => wgpu::Backends::METAL,
        "primary" | "default" => default,
        other => {
            log::warn!("unknown backend override `{other}`, using the default");
            return default;
        }
    };
    log::info!("backend override: {name} -> {chosen:?}");
    chosen
}

/// Which limits the device request should require.
///
/// `using_resolution` keeps the adapter's own texture-size ceiling rather than
/// the limit set's, which is what stops a large map failing to allocate on a
/// machine that could have held it.
pub(crate) fn limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    // On the web, fall back to the WebGL2 limit set so a GL adapter can satisfy
    // the request; native uses the broader downlevel defaults.
    #[cfg(target_arch = "wasm32")]
    let default = wgpu::Limits::downlevel_webgl2_defaults();
    #[cfg(not(target_arch = "wasm32"))]
    let default = wgpu::Limits::downlevel_defaults();

    let chosen = match setting("limits").as_deref() {
        None => default,
        Some(name) => match name.to_ascii_lowercase().as_str() {
            "webgl2" | "webgl" | "gl" => {
                log::info!("limit override: webgl2 downlevel defaults");
                wgpu::Limits::downlevel_webgl2_defaults()
            }
            "downlevel" => wgpu::Limits::downlevel_defaults(),
            "default" | "full" => wgpu::Limits::default(),
            other => {
                log::warn!("unknown limits override `{other}`, using the default");
                default
            }
        },
    };
    chosen.using_resolution(adapter.limits())
}

/// One setting, from the environment on native and the query string on the web.
///
/// Returns `None` when unset, which is the overwhelmingly common case and the
/// one that must cost nothing.
fn setting(name: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let var = format!("SLMSTTAA_{}", name.to_ascii_uppercase());
        std::env::var(var).ok().filter(|v| !v.is_empty())
    }

    // `location.search` is `?backend=gl&limits=webgl2` or empty. Split it by
    // hand rather than taking a URL dependency for two lookups that run once.
    #[cfg(target_arch = "wasm32")]
    {
        let search = web_sys::window()?.location().search().ok()?;
        search
            .trim_start_matches('?')
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string())
            .filter(|v| !v.is_empty())
    }
}
