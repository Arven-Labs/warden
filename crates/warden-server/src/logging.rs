//! The hub's diagnostics (P143 d): `tracing` events with a level, a time and fields, written to stderr.
//!
//! `text` is for a person at a terminal or in `journalctl`; `json` is one object per line, for a collector (a
//! container's log driver, Loki, CloudWatch). `RUST_LOG` filters as usual (`RUST_LOG=warn`, `RUST_LOG=warden_server=debug`);
//! without it, everything from `info` up is shown.

use std::io::IsTerminal;

use clap::ValueEnum;
use tracing_subscriber::EnvFilter;

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

fn filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

/// Installs the global subscriber. Called once, by `serve` — the commands that print for a person (`api-keys create`,
/// `webhooks token`) don't log, so they never call it. A second call is a no-op instead of a panic.
pub fn init(format: LogFormat) {
    let builder = tracing_subscriber::fmt().with_env_filter(filter()).with_writer(std::io::stderr);
    let _ = match format {
        LogFormat::Json => builder.json().try_init(),
        LogFormat::Text => builder.with_ansi(std::io::stderr().is_terminal()).try_init(),
    };
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

    fn captured(run: impl FnOnce(), json: bool) -> String {
        let out = Capture::default();
        let builder = tracing_subscriber::fmt().with_writer(out.clone()).with_ansi(false);
        let subscriber: Box<dyn tracing::Subscriber + Send + Sync> =
            if json { Box::new(builder.json().finish()) } else { Box::new(builder.finish()) };
        tracing::subscriber::with_default(subscriber, run);
        let bytes = out.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn json_is_one_object_per_line_with_level_message_and_fields() {
        let out = captured(|| tracing::warn!(peer = %"1.2.3.4:5", device_id = "phone", "wrong username or password"), true);
        let line: serde_json::Value = serde_json::from_str(out.trim()).expect("one JSON object");
        assert_eq!(line["level"], "WARN");
        assert_eq!(line["fields"]["message"], "wrong username or password");
        assert_eq!(line["fields"]["device_id"], "phone");
        assert_eq!(line["fields"]["peer"], "1.2.3.4:5");
    }

    #[test]
    fn text_has_the_level_the_message_and_the_fields_without_color_codes() {
        let out = captured(|| tracing::info!(task = "daily", "task done"), false);
        assert!(out.contains("INFO") && out.contains("task done") && out.contains("task=\"daily\""), "got: {out}");
        assert!(!out.contains('\u{1b}'), "no ANSI in a captured stream: {out:?}");
    }
}
