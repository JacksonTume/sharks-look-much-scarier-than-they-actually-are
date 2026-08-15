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
//! `shoot` runs the example and photographs it at exact frame numbers,
//! optionally clicking, typing and scrolling in between. It exists because
//! `ROADMAP.md`'s Definition of Done ends with "runs and shows the new capability
//! on screen", and doing that by hand costs minutes per look — in a container
//! because there is no display, and on a desktop because a person has to sit
//! there doing it. It works on **Linux and Windows**; see [`harness`] for the two
//! very different ways that is arranged.
//!
//! **This crate has no dependencies and must keep it that way** — it is the thing
//! you run constantly, `fontbake` was split out to a separate member purely to
//! preserve that, and `.claude/CLAUDE.md` says so in as many words. Everything
//! here is `std` plus shelling out to tools that are already prerequisites.
//!
//! External prerequisites: `wasm-bindgen` (the CLI) for `serve`. On Linux,
//! `shoot` also needs `Xvfb`, ImageMagick's `import` and `xdotool`; on Windows it
//! needs nothing, because it talks to Win32 itself rather than shelling out.

mod harness;

use harness::Harness;
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
    println!("shoot  Runs <example> and photographs it at exact frame numbers,");
    println!("       writing PNGs to <DIR> (default: capture/). A --script drives");
    println!("       clicks, drags, keys and the wheel between shots. On Linux it");
    println!("       needs Xvfb, ImageMagick's `import` and xdotool; on Windows,");
    println!("       nothing.\n");
    println!("       Script verbs: shot [name] | move X Y | click | press | release");
    println!("                     | wheel <notches> | key <name>");
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
    /// Press and release the left button wherever the pointer is, both inside
    /// this one frozen frame.
    ///
    /// Fine for a button or a checkbox, which act on the press edge. **Not** for
    /// anything that drags — see [`Action::Press`].
    Click,
    /// Hold the left button down, and leave it down.
    ///
    /// The half of a click that was missing, and the reason a script could not
    /// move a slider: `click` never leaves the button down across a frame
    /// boundary, so `Response::held` — which every drag widget in the toolkit
    /// acts on — is never true for a single frame. A `press` on one checkpoint,
    /// a `move` on a later one and a `release` after that is a real drag, with
    /// real frames running in between for the widget to follow.
    Press,
    /// Let the left button back up.
    Release,
    /// Turn the wheel, in notches: negative scrolls down, the way a scroll area
    /// reads it.
    ///
    /// Added when a virtualized list needed checking and the harness could not
    /// scroll one — the demo could be clicked and typed at but not *read*, which
    /// left the one widget whose whole behaviour is scrolling unreachable.
    Wheel(i32),
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
    // "could not run `Xvfb`" wastes the time and buries the actual reason.
    Harness::require_tools();

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

    let mut stage = Harness::start(&root, w, h);

    let mut command = Command::new(&bin);
    command.current_dir(&root);
    for (key, value) in stage.env() {
        command.env(key, value);
    }
    let mut child = command
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
            stage.stop();
            eprintln!("error: could not run {}: {e}", bin.display());
            exit(1);
        });

    // Which process to photograph. X11 does not care — its display holds one
    // window — but a Windows desktop holds hundreds.
    stage.attach(child.id());

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
            stage.stop();
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
                    stage.shot(&file);
                    println!("    frame {frame}: {}", file.display());
                    shots += 1;
                }
                Action::Move(x, y) => stage.mouse_move(*x, *y),
                Action::Click => stage.click(),
                Action::Press => stage.press(),
                Action::Release => stage.release(),
                Action::Wheel(notches) => stage.wheel(*notches),
                Action::Key(key) => stage.key(key),
            }
        }

        // Release the freeze. A failed write means the child is already gone,
        // which the next read reports properly, so it is not worth a message.
        let _ = stdin.write_all(b"\n");
        let _ = stdin.flush();
    }

    // Everything wanted has been captured; the example has no other way to know.
    // Both the demo and whatever the harness started are ours, so both die here —
    // the reason this does not shell out to `xvfb-run`, which would have left the
    // example behind.
    let _ = child.kill();
    let _ = child.wait();
    stage.stop();
    println!("\n  {shots} shot(s) in {}", out_dir.display());
}

/// Parse a capture script.
///
/// One step per line, frame number first, `#` starts a comment:
///
/// ```text
/// 120  shot   inset
/// 120  move   414 368
/// 150  click
/// 150  wheel  -3
/// 150  shot   selected
/// ```
///
/// Deliberately not a config format anyone has to learn. The frame number
/// leading each line is the important part: it is what makes a click land on a
/// known frame rather than "about a second in".
///
/// A drag is the same idea spread over several of them — the button stays down
/// between the checkpoints, so the demo gets real frames with it held:
///
/// ```text
/// 200  move    120 300
/// 220  press
/// 240  move    260 300
/// 260  release
/// ```
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
            Some("press") => Action::Press,
            Some("release") => Action::Release,
            Some("wheel") => match words.next().map(str::parse) {
                Some(Ok(notches)) => Action::Wheel(notches),
                _ => bad("wheel needs a notch count, negative to scroll down"),
            },
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
