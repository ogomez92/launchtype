//! Manual speech check: `cargo run -p prism --example speak -- "hello"`
//!
//! An example, not a `#[test]`, on purpose. The libtest harness runs every test
//! on a spawned thread, and the macOS backends `dispatch_sync` onto the main
//! queue during `initialize`, which deadlocks against the parked main thread.
//! Only a binary gets to run this code where it is meant to run.

fn main() {
    let text = std::env::args().nth(1).unwrap_or_else(|| "Prism speech test from Rust".into());

    let started = std::time::Instant::now();
    let speech = match prism::Speech::new() {
        Ok(speech) => speech,
        Err(e) => {
            eprintln!("init FAILED after {:?}: {e}", started.elapsed());
            std::process::exit(1);
        }
    };
    eprintln!("init: {:?}, backend: {:?}", started.elapsed(), speech.backend_name());

    let spoke = std::time::Instant::now();
    match speech.output(&text, true) {
        Ok(()) => eprintln!("output returned in {:?}", spoke.elapsed()),
        Err(e) => eprintln!("output FAILED in {:?}: {e}", spoke.elapsed()),
    }

    std::thread::sleep(std::time::Duration::from_secs(3));
}
