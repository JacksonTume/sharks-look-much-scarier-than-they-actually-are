//! Developer task runner for SLMSTTAA.
//!
//! Two commands, both about seeing a demo rather than merely compiling one:
//!
//! ```sh
//! cargo xtask serve [example] [--release] [--port <N>]
//! cargo xtask shoot [example] [--frames <N,..>] [--script <FILE>] [--size <WxH>]
//! ```
//!
//! `serve` (1) builds `<example>` (default `terrain`) as a native standalone,
//! (2) builds it for wasm and runs `wasm-bindgen` into `web/pkg/` (as `app.js`,
//! so `web/index.html` never has to change), then (3) serves `web/` from a tiny
//! built-in static file server — no Python, no extra crates.
//!
//! `shoot` runs the example under a virtual X server and photographs it at exact
//! frame numbers, optionally clicking things in between. It exists because
//! `ROADMAP.md`'s Definition of Done ends with "runs and shows the new capability
//! on screen", and doing that by hand in a container costs minutes per look. See
//! [`shoot`] for what it needs installed.
//!
//! **This crate has no dependencies and must keep it that way** — it is the thing
//! you run constantly, `fontbake` was split out to a separate member purely to
//! preserve that, and `.claude/CLAUDE.md` says so in as many words. Everything
//! here is `std` plus shelling out to tools that are already prerequisites.
//!
//! External prerequisites: `wasm-bindgen` (the CLI) for `serve`; `Xvfb`,
//! ImageMagick's `import`, and `xdotool` for `shoot`.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{exit, Command};
use std::thread;

fn main() {
    let mut release = false;
    let mut port: u16 = 8080;
    let mut frames: Option<String> = None;
    let mut script: Option<String> = None;
    let mut size = (1280u32, 800u32);
    let mut out: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--release" => release = true,
            "--port" => port = value(&mut args, "--port", "a number"),
            other if other.starts_with("--port=") => {
                port = parse_value(other.trim_start_matches("--port="), "--port", "a number");
            }
            "--frames" => frames = Some(next_raw(&mut args, "--frames")),
            other if other.starts_with("--frames=") => {
                frames = Some(other.trim_start_matches("--frames=").to_string());
            }
            "--script" => script = Some(next_raw(&mut args, "--script")),
            other if other.starts_with("--script=") => {
                script = Some(other.trim_start_matches("--script=").to_string());
            }
            "--out" => out = Some(next_raw(&mut args, "--out")),
            other if other.starts_with("--out=") => {
                out = Some(other.trim_start_matches("--out=").to_string());
            }
            "--size" => size = parse_size(&next_raw(&mut args, "--size")),
            other if other.starts_with("--size=") => {
                size = parse_size(other.trim_start_matches("--size="));
            }
            "--help" | "-h" => return print_help(),
            other => positional.push(other.to_string()),
        }
    }

    // `serve` was the only command for a long time and was optional, so
    // `cargo xtask cube` has always meant "serve cube". Keeping that working is
    // why this peels a *known* command name off the front rather than assuming
    // the first positional is one — otherwise `cargo xtask cube` would look for a
    // command called `cube`.
    let (command, rest) = match positional.split_first() {
        Some((first, rest)) if first == "serve" || first == "shoot" => (first.as_str(), rest),
        _ => ("serve", &positional[..]),
    };
    let example = rest.first().cloned().unwrap_or_else(|| "terrain".into());

    match command {
        "serve" => serve(&example, release, port),
        "shoot" => shoot(
            &example,
            release,
            frames.as_deref(),
            script.as_deref(),
            out.as_deref(),
            size,
        ),
        other => {
            eprintln!("unknown command `{other}`");
            exit(2);
        }
    }
}

/// The next argument, or a usage error naming the flag that wanted it.
fn next_raw(args: &mut impl Iterator<Item = String>, flag: &str) -> String {
    args.next().unwrap_or_else(|| {
        eprintln!("{flag} needs a value");
        exit(2);
    })
}

/// The next argument, parsed, or a usage error.
fn value<T: std::str::FromStr>(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
    what: &str,
) -> T {
    parse_value(&next_raw(args, flag), flag, what)
}

/// Parse one argument, or a usage error naming what was expected.
fn parse_value<T: std::str::FromStr>(raw: &str, flag: &str, what: &str) -> T {
    raw.parse().unwrap_or_else(|_| {
        eprintln!("{flag} needs {what}");
        exit(2);
    })
}

/// `WxH` into a pair, e.g. `1280x800`.
fn parse_size(raw: &str) -> (u32, u32) {
    let mut parts = raw.split(['x', 'X']);
    let w = parts.next().and_then(|p| p.parse().ok());
    let h = parts.next().and_then(|p| p.parse().ok());
    match (w, h, parts.next()) {
        (Some(w), Some(h), None) if w > 0 && h > 0 => (w, h),
        _ => {
            eprintln!("--size needs WxH, e.g. 1280x800");
            exit(2);
        }
    }
}

fn print_help() {
    println!("cargo xtask — SLMSTTAA dev tasks\n");
    println!("USAGE:");
    println!("  cargo xtask serve [example] [--release] [--port <N>]");
    println!("  cargo xtask shoot [example] [--frames <N,..>] [--script <FILE>]");
    println!("                    [--out <DIR>] [--size <WxH>] [--release]\n");
    println!("serve  Builds <example> (default: terrain) natively and for the web,");
    println!("       then serves web/ at http://localhost:<port> (default 8080).\n");
    println!("shoot  Runs <example> under a virtual X server and photographs it at");
    println!("       exact frame numbers, writing PNGs to <DIR> (default: capture/).");
    println!("       A --script drives clicks between shots. Needs Xvfb, the");
    println!("       ImageMagick `import` tool, and xdotool.");
}

/// Build the example natively and for the web, then serve `web/`.
fn serve(example: &str, release: bool, port: u16) {
    let root = workspace_root();
    let profile = if release { "release" } else { "debug" };

    println!("==> building native standalone `{example}`");
    cargo(&root, &build_args(example, release, false));

    println!("==> building web (wasm) `{example}`");
    cargo(&root, &build_args(example, release, true));

    let wasm = root
        .join("target/wasm32-unknown-unknown")
        .join(profile)
        .join("examples")
        .join(format!("{example}.wasm"));
    if !wasm.exists() {
        eprintln!("error: expected wasm at {}", wasm.display());
        exit(1);
    }

    println!("==> generating web/pkg via wasm-bindgen");
    run_bindgen(&root, &wasm);

    let web = root.join("web");
    let release_flag = if release { " --release" } else { "" };
    println!("\n  SLMSTTAA `{example}` is ready.");
    println!("  native standalone:  cargo run --example {example}{release_flag}");
    println!("  web (serving now):  http://localhost:{port}");
    println!("  press Ctrl+C to stop.\n");
    http_serve(&web, port);
}

/// One step of a capture script: what to do, and on which frame.
#[derive(Debug)]
struct Step {
    /// The engine freezes on this frame; the action runs while it is held.
    frame: u64,
    action: Action,
}

/// What a script step does while the engine is parked.
#[derive(Debug)]
enum Action {
    /// Photograph the window into `capture/<example>-<name>.png`.
    Shot(String),
    /// Warp the pointer, in physical pixels from the window's top-left.
    Move(u32, u32),
    /// Press and release the left button wherever the pointer is.
    Click,
    /// Tap a key by `xdotool` name (`space`, `Escape`, `w`).
    Key(String),
}

/// Run the example under a virtual X server and photograph it at exact frames.
///
/// # Why this is not just a `sleep` and a screenshot
///
/// Because that is what it replaces, and it was bad in two specific ways. A
/// wall-clock wait cannot say *which* frame it caught, and the engine's
/// wall-clock `elapsed` drives the ripple field — so two runs of the same commit
/// were never in phase and a diff was 0.6% noise before any real change. The
/// engine's capture mode (see `src/capture.rs`) fixes both: it pins the frame
/// delta so a picture is a pure function of frame index, announces each
/// checkpoint on stdout, and holds there until this function says go.
///
/// So the protocol is: launch with the checkpoints in the environment, read
/// stdout until a checkpoint is announced, do that frame's steps, write a
/// newline to release it, repeat.
///
/// # Why it starts `Xvfb` itself rather than using `xvfb-run`
///
/// `xvfb-run` is a shell script whose last line is
/// `DISPLAY=:$N XAUTHORITY=$AUTH "$@"`. The display it makes therefore exists
/// only inside *its* environment — so `import` and `xdotool`, which run from
/// here, get `unable to open X server`. It also does not `exec`, so killing it
/// leaves the example running. Owning the server directly fixes both and costs a
/// dozen lines.
///
/// # Prerequisites
///
/// `Xvfb`, ImageMagick's `import`, and `xdotool` — all installed by
/// `.claude/hooks/session-start.sh` in a cloud session. On a machine with a real
/// display this still works; it renders into a virtual one anyway, which is what
/// keeps a capture the same size everywhere.
fn shoot(
    example: &str,
    release: bool,
    frames: Option<&str>,
    script: Option<&str>,
    out: Option<&str>,
    size: (u32, u32),
) {
    let root = workspace_root();
    let profile = if release { "release" } else { "debug" };

    let steps = match script {
        Some(path) => read_script(&root.join(path)),
        // No script is the common case: one shot, named after the example.
        None => {
            let frame = frames
                .and_then(|f| f.split(',').next())
                .and_then(|f| f.trim().parse().ok())
                .unwrap_or(120);
            vec![Step {
                frame,
                action: Action::Shot(String::new()),
            }]
        }
    };
    if steps.is_empty() {
        eprintln!("error: no steps to run");
        exit(1);
    }

    // Checkpoints the engine must stop on: every frame any step mentions.
    let mut checkpoints: Vec<u64> = steps.iter().map(|s| s.frame).collect();
    checkpoints.sort_unstable();
    checkpoints.dedup();

    // Checked before the build, because a three-minute compile followed by
    // "could not run `Xvfb`" wastes the time and buries the actual reason. The
    // harness needs all three: an X server to render into, `import` to
    // photograph it, and `xdotool` to click it.
    require_capture_tools();

    println!("==> building `{example}`");
    cargo(&root, &build_args(example, release, false));

    // `EXE_SUFFIX` is "" on Unix and ".exe" on Windows. Without it this looked
    // for `examples/terrain` on a machine that had just built `terrain.exe`,
    // and reported a missing binary that was sitting right there.
    let bin = root
        .join("target")
        .join(profile)
        .join("examples")
        .join(format!("{example}{}", std::env::consts::EXE_SUFFIX));
    if !bin.exists() {
        eprintln!("error: expected binary at {}", bin.display());
        exit(1);
    }

    let out_dir = root.join(out.unwrap_or("capture"));
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("error: cannot create {}: {e}", out_dir.display());
        exit(1);
    }

    let frame_list: Vec<String> = checkpoints.iter().map(u64::to_string).collect();
    let (w, h) = size;
    println!(
        "==> shooting `{example}` at {w}x{h}, frames {}",
        frame_list.join(",")
    );

    let (mut xvfb, display) = start_xvfb(w, h);

    let mut child = Command::new(&bin)
        .current_dir(&root)
        .env("DISPLAY", &display)
        // A pinned delta is the whole reason two runs can be compared. 1/60 s is
        // arbitrary but it is what the demos are tuned to look right at.
        .env("SLMSTTAA_CAPTURE_DT", "0.0166666")
        .env("SLMSTTAA_CAPTURE_FRAMES", frame_list.join(","))
        // Markers go to stdout, so keep the log on stderr and out of the way.
        .env(
            "RUST_LOG",
            std::env::var("RUST_LOG").as_deref().unwrap_or("warn"),
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| {
            let _ = xvfb.kill();
            eprintln!("error: could not run {}: {e}", bin.display());
            exit(1);
        });

    let stdout = child.stdout.take().expect("stdout was piped");
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let mut lines = BufReader::new(stdout).lines();
    let mut shots = 0usize;

    for frame in &checkpoints {
        // Wait for this checkpoint. Anything else on stdout is a demo's own
        // output and is passed through rather than swallowed.
        let mut reached = false;
        for line in lines.by_ref() {
            let Ok(line) = line else { break };
            match line.strip_prefix("slmsttaa: capture ") {
                Some(n) if n.trim().parse::<u64>().ok() == Some(*frame) => {
                    reached = true;
                    break;
                }
                _ => println!("{line}"),
            }
        }
        if !reached {
            eprintln!("error: the example exited before frame {frame}");
            eprintln!("hint: run it directly to see why, or lower --frames");
            let _ = child.kill();
            let _ = xvfb.kill();
            exit(1);
        }

        for step in steps.iter().filter(|s| s.frame == *frame) {
            match &step.action {
                Action::Shot(name) => {
                    let file = if name.is_empty() {
                        out_dir.join(format!("{example}.png"))
                    } else {
                        out_dir.join(format!("{example}-{name}.png"))
                    };
                    grab(&root, &display, &file);
                    println!("    frame {frame}: {}", file.display());
                    shots += 1;
                }
                Action::Move(x, y) => xdo(
                    &root,
                    &display,
                    &["mousemove", &x.to_string(), &y.to_string()],
                ),
                Action::Click => xdo(&root, &display, &["click", "1"]),
                Action::Key(key) => xdo(&root, &display, &["key", key]),
            }
        }

        // Release the freeze. A failed write means the child is already gone,
        // which the next read reports properly, so it is not worth a message.
        let _ = stdin.write_all(b"\n");
        let _ = stdin.flush();
    }

    // Everything wanted has been captured; the example has no other way to know.
    // Both children are ours, so both die here — the reason this does not shell
    // out to `xvfb-run`, which would have left the example behind.
    let _ = child.kill();
    let _ = child.wait();
    let _ = xvfb.kill();
    let _ = xvfb.wait();
    println!("\n  {shots} shot(s) in {}", out_dir.display());
}

/// Start a virtual X server on the first free display, and wait for it.
///
/// Returns the handle to kill later and the `DISPLAY` value everything else must
/// be given — the example, `import` and `xdotool` alike. That sharing is the
/// whole point; see [`shoot`] for what happens without it.
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
/// The harness is X11-only by construction: it owns an `Xvfb` display, and
/// `import` and `xdotool` both talk to one. That is a fine constraint — the
/// point is a pinned, reproducible frame, and a virtual display is how you get
/// one — but it means `shoot` cannot work on a bare Windows or macOS box, and
/// the failure should say so instead of surfacing as a missing binary or a
/// spawn error partway through.
fn require_capture_tools() {
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

    eprintln!("error: `cargo xtask shoot` cannot run here — it needs an X11 display.");
    eprintln!();
    for (tool, _, install) in &missing {
        eprintln!("  missing `{tool}`  ({install})");
    }
    eprintln!();
    eprintln!("The harness renders into an Xvfb display, photographs it with");
    eprintln!("ImageMagick's `import`, and clicks it with `xdotool`, so it is");
    eprintln!("Linux-only. On Windows or macOS, run the demo and look at it:");
    eprintln!();
    eprintln!("  cargo run --example <name>");
    exit(1);
}

fn start_xvfb(w: u32, h: u32) -> (std::process::Child, String) {
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
                return (child, display);
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

/// Photograph the engine's window into `file`.
///
/// Targets the window by **name** rather than grabbing the root. The virtual
/// screen is usually larger than the window, so a root grab pads the image with
/// desktop background — which is invisible to a human and fatal to a diff.
fn grab(root: &Path, display: &str, file: &Path) {
    // Two attempts: the window can be a moment behind the first checkpoint on a
    // cold software renderer, and a retry is cheaper than a wrong answer.
    for attempt in 0..2 {
        let status = Command::new("import")
            .current_dir(root)
            .env("DISPLAY", display)
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

/// Parse a capture script.
///
/// One step per line, frame number first, `#` starts a comment:
///
/// ```text
/// 120  shot   inset
/// 120  move   414 368
/// 150  click
/// 150  shot   selected
/// ```
///
/// Deliberately not a config format anyone has to learn. The frame number
/// leading each line is the important part: it is what makes a click land on a
/// known frame rather than "about a second in".
fn read_script(path: &Path) -> Vec<Step> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        exit(1);
    });

    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let bad = |what: &str| -> ! {
            eprintln!("{}:{}: {what}", path.display(), n + 1);
            exit(2);
        };

        let Some(Ok(frame)) = words.next().map(str::parse::<u64>) else {
            bad("expected a frame number first");
        };
        let action = match words.next() {
            Some("shot") => Action::Shot(words.next().unwrap_or("shot").to_string()),
            Some("move") => match (words.next().map(str::parse), words.next().map(str::parse)) {
                (Some(Ok(x)), Some(Ok(y))) => Action::Move(x, y),
                _ => bad("move needs X and Y"),
            },
            Some("click") => Action::Click,
            Some("key") => match words.next() {
                Some(key) => Action::Key(key.to_string()),
                None => bad("key needs a key name"),
            },
            Some(other) => bad(&format!("unknown action `{other}`")),
            None => bad("expected an action after the frame number"),
        };
        steps.push(Step { frame, action });
    }
    steps
}

/// The workspace root — `xtask`'s manifest dir is `<root>/xtask`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask should live under the workspace root")
        .to_path_buf()
}

/// `cargo build` args for the engine example, native or wasm.
fn build_args(example: &str, release: bool, wasm: bool) -> Vec<String> {
    let mut v = vec![
        "build".into(),
        "--package".into(),
        "slmsttaa".into(),
        "--example".into(),
        example.into(),
    ];
    if wasm {
        v.push("--target".into());
        v.push("wasm32-unknown-unknown".into());
    }
    if release {
        v.push("--release".into());
    }
    v
}

/// Run `cargo` with `args` in `dir`, aborting on failure.
fn cargo(dir: &Path, args: &[String]) {
    // `CARGO` is set by the cargo that invoked us; fall back to PATH lookup.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("failed to run cargo: {e}");
            exit(1);
        });
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

/// Emit JS/wasm bindings into `web/pkg/` under the stable name `app`.
fn run_bindgen(root: &Path, wasm: &Path) {
    let out = root.join("web/pkg");
    let result = Command::new("wasm-bindgen")
        .current_dir(root)
        .arg(wasm)
        .args(["--out-dir".as_ref(), out.as_os_str()])
        .args(["--target", "web", "--out-name", "app"])
        .status();
    match result {
        Ok(status) if status.success() => {}
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(_) => {
            eprintln!("error: `wasm-bindgen` was not found on PATH.");
            eprintln!("install it with: cargo install wasm-bindgen-cli");
            exit(1);
        }
    }
}

/// A minimal, dependency-free static file server for the web demo.
fn http_serve(dir: &Path, port: u16) -> ! {
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|e| {
        eprintln!("failed to bind 127.0.0.1:{port}: {e}");
        exit(1);
    });
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let dir = dir.to_path_buf();
                thread::spawn(move || {
                    if let Err(e) = handle(stream, &dir) {
                        eprintln!("connection error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
    exit(0)
}

/// Serve one request: GET a file under `dir`, or 404/403/405.
fn handle(mut stream: TcpStream, dir: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    // Drain the remaining request headers; we don't need them.
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");

    if method != "GET" {
        return respond(&mut stream, 405, "Method Not Allowed", "text/plain", b"405");
    }

    let path = target.split('?').next().unwrap_or("/");
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };

    // Refuse path traversal outright.
    if rel.split('/').any(|c| c == "..") {
        return respond(&mut stream, 403, "Forbidden", "text/plain", b"403");
    }

    match std::fs::read(dir.join(rel)) {
        Ok(bytes) => {
            println!("GET /{rel} -> 200 ({} bytes)", bytes.len());
            respond(&mut stream, 200, "OK", content_type(rel), &bytes)
        }
        Err(_) => {
            println!("GET /{rel} -> 404");
            respond(
                &mut stream,
                404,
                "Not Found",
                "text/plain",
                b"404 Not Found",
            )
        }
    }
}

/// Write a single HTTP/1.1 response and close the connection.
fn respond(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {code} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// Map a file name to a Content-Type. `application/wasm` matters: browsers need it
/// to stream-compile the module.
fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("ts") => "text/plain; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}
