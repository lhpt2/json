//! Hot reload: watch a `.cson` file and re-parse it when it changes on
//! disk, so a running app picks up config edits without restarting.
//!
//! The watching lives here, in an example, rather than in the library,
//! for two reasons. `notify` is a std-only, platform-specific
//! dependency tree, and `cson_edit`'s only dependency being `serde`
//! (and working in `no_std`) is a deliberate property -- so it's a
//! **dev**-dependency here, costing nothing to anyone who depends on
//! the crate. And everything interesting about reloading is *policy*:
//! how long to debounce, what to do when the file is caught mid-write
//! and doesn't parse, whether your own writes should trigger your own
//! watcher. Every app wants those slightly differently; this file is
//! meant to be copied and adjusted, not called.
//!
//! What the library does provide is the piece you can't write yourself
//! from outside: [`cson_edit::Document::into_owned`]. A `Document<'a>`
//! borrows from the text it was parsed from, so without it a freshly
//! parsed document couldn't outlive the `String` read off disk, let
//! alone be stored in shared state or moved to another thread.
//!
//! Run with: `cargo run --example 08_hot_reload`
//!
//! It drives itself: a background thread watches a scratch file while
//! the main thread edits that file a few times, including one write of
//! deliberately broken CSON to show the last-good config being kept.

use cson_edit::{parse, Document, Value};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{fs, thread};

/// How long to wait for the event stream to go quiet before reloading.
/// Editors emit several events per save (truncate, write, chmod,
/// rename), and a file caught between them may not even be valid CSON.
const DEBOUNCE: Duration = Duration::from_millis(120);

/// What the watcher thread reports back after each settled change.
#[derive(Debug)]
enum Reload {
    /// Parsed cleanly; the shared document has been swapped.
    Applied,
    /// Didn't parse. The shared document is untouched -- still the last
    /// known-good config.
    Rejected(String),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join("cson_edit_hot_reload_demo.csn");
    fs::write(
        &path,
        "# Device configuration\nname: \"grid-controller\"\n\nserver: {\n  host: \"localhost\"\n  port: 8080  # ops override\n}\n",
    )?;

    // The shared, always-parseable config. `into_owned()` is what lets a
    // Document live here at all: the String it was parsed from is gone
    // by the time load() returns.
    let config: Arc<Mutex<Document<'static>>> = Arc::new(Mutex::new(load(&path)?));
    println!("initial:\n{}", config.lock().unwrap().to_cson_string());

    let (reloads_tx, reloads) = mpsc::channel();
    // The watcher must outlive the loop, so the thread owns it.
    let _watcher = spawn_watcher(path.clone(), Arc::clone(&config), reloads_tx)?;

    // --- pretend to be a person editing the file while the app runs ---

    println!("--- edit 1: change the port ---");
    fs::write(
        &path,
        "# Device configuration\nname: \"grid-controller\"\n\nserver: {\n  host: \"localhost\"\n  port: 9090  # ops override\n}\n",
    )?;
    expect(&reloads, "edit 1");
    let text = config.lock().unwrap().to_cson_string();
    assert!(text.contains("9090"));
    assert!(text.contains("# ops override")); // comments survive the reload
    println!("port is now {}\n", port_of(&config));

    println!("--- edit 2: save a half-written file (broken CSON) ---");
    fs::write(&path, "name: \"grid-controller\"\nserver: {\n  host:")?;
    match expect(&reloads, "edit 2") {
        Reload::Rejected(msg) => println!("rejected, keeping last good config: {msg}"),
        other => panic!("expected a parse rejection, got {other:?}"),
    }
    // The app kept running on the last good config rather than losing it.
    assert_eq!(port_of(&config), "9090");
    println!("port is still {}\n", port_of(&config));

    println!("--- edit 3: fix it up again ---");
    fs::write(
        &path,
        "# Device configuration\nname: \"grid-controller\"\n\nserver: {\n  host: \"localhost\"\n  port: 7000  # back to the default\n}\n",
    )?;
    expect(&reloads, "edit 3");
    assert_eq!(port_of(&config), "7000");
    println!("{}", config.lock().unwrap().to_cson_string());

    fs::remove_file(&path)?;
    println!("(hot reload OK: two applied, one rejected without losing the good config)");
    Ok(())
}

/// Reads and parses `path` into a document that owns its text.
///
/// `parse` borrows from `text`, which is a local here -- `into_owned()`
/// detaches the document so it can be returned, stored, and shared.
fn load(path: &Path) -> Result<Document<'static>, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(path)?;
    Ok(parse(&text)?.into_owned())
}

/// Watches `path`'s directory and swaps `config` whenever the file
/// changes and still parses. Returns the watcher, which must be kept
/// alive: dropping it stops the watch.
fn spawn_watcher(
    path: PathBuf,
    config: Arc<Mutex<Document<'static>>>,
    reloads: Sender<Reload>,
) -> Result<RecommendedWatcher, Box<dyn std::error::Error>> {
    let (tx, events) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;

    // Watch the *directory*, not the file. Editors (and `fs::write` on
    // some platforms) save by writing a temp file and renaming it over
    // the target, which replaces the inode -- a watch on the file itself
    // would silently stop firing after the first such save.
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    watcher.watch(&dir, RecursiveMode::NonRecursive)?;

    thread::spawn(move || {
        while let Ok(first) = events.recv() {
            if !concerns(&first, &path) {
                continue;
            }
            // Debounce: drain whatever else arrives for this one save,
            // so we parse once, after the writing has settled.
            while events.recv_timeout(DEBOUNCE).is_ok() {}

            let outcome = match load(&path) {
                Ok(fresh) => {
                    *config.lock().unwrap() = fresh;
                    Reload::Applied
                }
                Err(e) => Reload::Rejected(e.to_string()),
            };
            if reloads.send(outcome).is_err() {
                break; // the app went away
            }
        }
    });

    Ok(watcher)
}

/// Is this event about the file we care about? The directory watch sees
/// everything in it, including editors' temp files.
fn concerns(event: &notify::Result<Event>, path: &Path) -> bool {
    match event {
        Ok(e) => e.paths.iter().any(|p| p == path),
        Err(_) => false,
    }
}

fn port_of(config: &Arc<Mutex<Document<'static>>>) -> String {
    let doc = config.lock().unwrap();
    let port = doc.root().value().get("server").and_then(|s| s.value().get("port"));
    match port.map(|n| n.value()) {
        Some(Value::Number(n)) => n.as_str().to_string(),
        _ => String::new(),
    }
}

fn expect(reloads: &Receiver<Reload>, what: &str) -> Reload {
    reloads
        .recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| panic!("timed out waiting for a reload after {what}"))
}
