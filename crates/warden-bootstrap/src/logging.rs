//! Where the hub's and the other binaries' diagnostics go (P143 d): `tracing` events written to stderr.
//!
//! Library code (`warden-core`, `warden-bootstrap`, `warden-server`) only emits events; each binary installs one
//! subscriber at start-up. An event with no subscriber is dropped silently, so **every binary that uses those crates
//! must call one of these** (`warden-server serve` calls [`init`], the rest [`init_compact`]).
//!
//! `RUST_LOG` filters as usual (`RUST_LOG=warn`, `RUST_LOG=warden_core=debug`); without it, `info` and above show.

use std::io::IsTerminal;

use tracing_subscriber::EnvFilter;

fn filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

/// The hub's log: time, level and module on each line, or with `json` one object per line for a collector.
/// A second call is a no-op instead of a panic.
pub fn init(json: bool) {
    let builder = tracing_subscriber::fmt().with_env_filter(filter()).with_writer(std::io::stderr);
    let _ = if json { builder.json().try_init() } else { builder.with_ansi(std::io::stderr().is_terminal()).try_init() };
}

/// For the binaries a person runs at a terminal (the CLI, the bots, the desktop app): just the level and the message,
/// no time and no module, so a warning reads like the plain line it replaced. A second call is a no-op.
pub fn init_compact() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter())
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .without_time()
        .with_target(false)
        .try_init();
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;

    /// A writer that keeps what the subscriber prints, so a test can read it.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Capture {
        type Writer = Capture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// The same builders `init` and `init_compact` use, writing to a buffer instead of stderr.
    fn captured(run: impl FnOnce(), style: &str) -> String {
        let out = Capture::default();
        let builder = tracing_subscriber::fmt().with_writer(out.clone()).with_ansi(false);
        let subscriber: Box<dyn tracing::Subscriber + Send + Sync> = match style {
            "json" => Box::new(builder.json().finish()),
            "compact" => Box::new(builder.without_time().with_target(false).finish()),
            _ => Box::new(builder.finish()),
        };
        tracing::subscriber::with_default(subscriber, run);
        let bytes = out.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn json_is_one_object_per_line_with_level_message_and_fields() {
        let out = captured(|| tracing::warn!(peer = %"1.2.3.4:5", device_id = "phone", "wrong username or password"), "json");
        let line: serde_json::Value = serde_json::from_str(out.trim()).expect("one JSON object");
        assert_eq!(line["level"], "WARN");
        assert_eq!(line["fields"]["message"], "wrong username or password");
        assert_eq!(line["fields"]["device_id"], "phone");
        assert_eq!(line["fields"]["peer"], "1.2.3.4:5");
    }

    #[test]
    fn text_has_the_level_the_message_and_the_fields_without_color_codes() {
        let out = captured(|| tracing::info!(task = "daily", "task done"), "text");
        assert!(out.contains("INFO") && out.contains("task done") && out.contains("task=\"daily\""), "got: {out}");
        assert!(!out.contains('\u{1b}'), "no ANSI in a captured stream: {out:?}");
    }

    #[test]
    fn compact_is_the_level_and_the_message_only() {
        let out = captured(|| tracing::warn!("could not save the permissions"), "compact");
        assert_eq!(out.trim(), "WARN could not save the permissions", "got: {out:?}");
    }
}
