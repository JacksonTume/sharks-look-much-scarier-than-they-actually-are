//! Binary entry point for the SLMSTTAA demo.
//!
//! All the interesting code lives in the library crate; this just kicks off the
//! event loop.

fn main() {
    if let Err(err) = slmsttaa::run() {
        eprintln!("slmsttaa exited with an error: {err}");
        std::process::exit(1);
    }
}
