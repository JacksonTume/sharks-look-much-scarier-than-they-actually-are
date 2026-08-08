//! The X11 harness: an `Xvfb` display, ImageMagick's `import`, and `xdotool`.
//!
//! Unchanged in substance from the original single-file version — the code moved
//! behind [`Harness`](super::Harness) so a second platform could exist, and
//! nothing about how it works changed.

use std::path::Path;
use std::process::{exit, Command};
use std::thread;

pub struct Inner {
    server: std::process::Child,
    display: String,
}

/// Whether `tool` is runnable at all, asked with a flag that prints and exits
/// rather than doing any work — `Xvfb` with no arguments would start a server.
fn have_tool(tool: &str, probe: &str) -> bool {
    Command::new(tool)
        .arg(probe)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Fail early, once, naming **every** missing prerequisite.
///
/// The X11 harness owns an `Xvfb` display, and `import` and `xdotool` both talk
/// to one. That is a fine constraint — the point is a pinned, reproducible
/// frame, and a virtual display is how you get one — but a missing piece should
/// say so instead of surfacing as a spawn error partway through.
pub fn require_tools() {
    const TOOLS: [(&str, &str, &str); 3] = [
        ("Xvfb", "-help", "apt-get install xvfb"),
        ("import", "-version", "apt-get install imagemagick"),
        ("xdotool", "--version", "apt-get install xdotool"),
    ];

    let missing: Vec<&(&str, &str, &str)> = TOOLS
        .iter()
        .filter(|(tool, probe, _)| !have_tool(tool, probe))
        .collect();
    if missing.is_empty() {
        return;
    }

    eprintln!("error: `cargo xtask shoot` needs an X11 display and cannot find one.");
    eprintln!();
    for (tool, _, install) in &missing {
        eprintln!("  missing `{tool}`  ({install})");
    }
    eprintln!();
    eprintln!("The harness renders into an Xvfb display, photographs it with");
    eprintln!("ImageMagick's `import`, and clicks it with `xdotool`.");
    exit(1);
}

/// Start a virtual X server on the first free display, and wait for it.
///
/// # Why it starts `Xvfb` itself rather than using `xvfb-run`
///
/// `xvfb-run` is a shell script whose last line is
/// `DISPLAY=:$N XAUTHORITY=$AUTH "$@"`. The display it makes therefore exists
/// only inside *its* environment — so `import` and `xdotool`, which run from
/// here, get `unable to open X server`. It also does not `exec`, so killing it
/// leaves the example running. Owning the server directly fixes both and costs a
/// dozen lines.
pub fn start(w: u32, h: u32) -> Inner {
    for number in 99..120 {
        let socket = format!("/tmp/.X11-unix/X{number}");
        // X servers advertise themselves with a socket named after the display.
        // Skipping taken ones up front means two captures can run at once
        // without racing, which matters more than it sounds: losing that race
        // shows up as a screenshot of somebody else's window.
        if Path::new(&socket).exists() {
            continue;
        }

        let display = format!(":{number}");
        let spawned = Command::new("Xvfb")
            .arg(&display)
            .args(["-screen", "0", &format!("{w}x{h}x24")])
            .args(["-nolisten", "tcp"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                eprintln!("error: could not run `Xvfb`: {e}");
                eprintln!("install it with: apt-get install xvfb");
                exit(1);
            }
        };

        // Poll for the socket rather than sleeping a fixed amount: a cold
        // container takes noticeably longer than a warm one, so a fixed wait is
        // either too short (flaky) or too long (paid on every capture).
        for _ in 0..100 {
            if Path::new(&socket).exists() {
                return Inner {
                    server: child,
                    display,
                };
            }
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = child.kill();
    }

    eprintln!("error: no free X display in :99..:120");
    exit(1);
}

pub fn env(inner: &Inner) -> Vec<(String, String)> {
    vec![("DISPLAY".to_string(), inner.display.clone())]
}

/// Nothing to do: the display this harness made contains exactly one window.
pub fn attach(_inner: &mut Inner, _pid: u32) {}

/// Photograph the engine's window into `file`.
///
/// Targets the window by **name** rather than grabbing the root. The virtual
/// screen is usually larger than the window, so a root grab pads the image with
/// desktop background — which is invisible to a human and fatal to a diff.
pub fn shot(inner: &mut Inner, root: &Path, file: &Path) {
    // Two attempts: the window can be a moment behind the first checkpoint on a
    // cold software renderer, and a retry is cheaper than a wrong answer.
    for attempt in 0..2 {
        let status = Command::new("import")
            .current_dir(root)
            .env("DISPLAY", &inner.display)
            .args(["-window", "SLMSTTAA"])
            .arg(file)
            .status();
        match status {
            Ok(s) if s.success() => return,
            Ok(_) if attempt == 0 => thread::sleep(std::time::Duration::from_millis(400)),
            Ok(_) => {
                eprintln!("error: `import` could not find a window named SLMSTTAA");
                exit(1);
            }
            Err(e) => {
                eprintln!("error: could not run `import`: {e}");
                eprintln!("install it with: apt-get install imagemagick");
                exit(1);
            }
        }
    }
}

pub fn mouse_move(inner: &mut Inner, root: &Path, x: u32, y: u32) {
    xdo(
        root,
        &inner.display,
        &["mousemove", &x.to_string(), &y.to_string()],
    );
}

pub fn click(inner: &mut Inner, root: &Path) {
    xdo(root, &inner.display, &["click", "1"]);
}

/// X11 spells the wheel as buttons 4 (up) and 5 (down), one press per notch.
pub fn wheel(inner: &mut Inner, root: &Path, notches: i32) {
    let button = if notches < 0 { "5" } else { "4" };
    for _ in 0..notches.unsigned_abs() {
        xdo(root, &inner.display, &["click", button]);
    }
}

pub fn key(inner: &mut Inner, root: &Path, name: &str) {
    xdo(root, &inner.display, &["key", name]);
}

pub fn stop(inner: &mut Inner) {
    let _ = inner.server.kill();
    let _ = inner.server.wait();
}

/// Drive the pointer or keyboard with `xdotool`.
fn xdo(root: &Path, display: &str, args: &[&str]) {
    let status = Command::new("xdotool")
        .current_dir(root)
        .env("DISPLAY", display)
        .args(args)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(_) => eprintln!("warning: xdotool {args:?} failed"),
        Err(e) => {
            eprintln!("error: could not run `xdotool`: {e}");
            eprintln!("install it with: apt-get install xdotool");
            exit(1);
        }
    }
}
