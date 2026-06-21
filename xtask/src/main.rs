//! Developer task runner for SLMSTTAA.
//!
//! One command builds a demo every way it ships and serves the web build:
//!
//! ```sh
//! cargo xtask serve [example] [--release] [--port <N>]
//! ```
//!
//! It (1) builds `<example>` (default `terrain`) as a native standalone, (2) builds
//! it for wasm and runs `wasm-bindgen` into `web/pkg/` (as `app.js`, so
//! `web/index.html` never has to change), then (3) serves `web/` from a tiny
//! built-in static file server — no Python, no extra crates.
//!
//! `wasm-bindgen` (the CLI) is the one external prerequisite:
//! `cargo install wasm-bindgen-cli`.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{exit, Command};
use std::thread;

fn main() {
    let mut release = false;
    let mut port: u16 = 8080;
    let mut positional: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--release" => release = true,
            "--port" => {
                port = args.next().and_then(|p| p.parse().ok()).unwrap_or_else(|| {
                    eprintln!("--port needs a number");
                    exit(2);
                });
            }
            other if other.starts_with("--port=") => {
                port = other
                    .trim_start_matches("--port=")
                    .parse()
                    .unwrap_or_else(|_| {
                        eprintln!("--port needs a number");
                        exit(2);
                    });
            }
            "--help" | "-h" => return print_help(),
            other => positional.push(other.to_string()),
        }
    }

    // Usage is `cargo xtask [serve] [example]`; `serve` is the only command and is
    // optional, so the first positional that isn't `serve` is the example name.
    let example = positional
        .into_iter()
        .find(|p| p != "serve")
        .unwrap_or_else(|| "terrain".to_string());

    serve(&example, release, port);
}

fn print_help() {
    println!("cargo xtask — SLMSTTAA dev tasks\n");
    println!("USAGE:");
    println!("  cargo xtask serve [example] [--release] [--port <N>]\n");
    println!("Builds <example> (default: terrain) natively and for the web, then");
    println!("serves web/ at http://localhost:<port> (default 8080).");
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
