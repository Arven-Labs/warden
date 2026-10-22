//! Reading the usage out of what a provider sends back.
//!
//! A non-streaming answer carries a `usage` object. A stream carries it in one of its last `data:` lines, and only if
//! the request asked for it (`stream_options.include_usage`, which the gateway always sets). `UsageScanner` watches the
//! bytes of a stream as they are forwarded, without holding them back.

use serde_json::Value;

use crate::catalog::Tokens;

/// `usage.prompt_tokens` and `usage.completion_tokens` of an answer or a stream chunk, if both are there as numbers.
pub fn usage_of(body: &Value) -> Option<Tokens> {
    let usage = body.get("usage")?;
    Some(Tokens { prompt: usage.get("prompt_tokens")?.as_u64()?, completion: usage.get("completion_tokens")?.as_u64()? })
}

/// Feed it the chunks of a server-sent-events stream, in order; it remembers the last usage it saw and whether `[DONE]` came.
#[derive(Default)]
pub struct UsageScanner {
    /// The tail of the last chunk that doesn't end a line yet.
    pending: Vec<u8>,
    usage: Option<Tokens>,
    done: bool,
}

impl UsageScanner {
    pub fn push(&mut self, chunk: &[u8]) {
        self.pending.extend_from_slice(chunk);
        while let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            self.line(&line);
        }
        // A stream that never sends a newline must not grow without bound.
        if self.pending.len() > 1 << 20 {
            self.pending.clear();
        }
    }

    fn line(&mut self, raw: &[u8]) {
        let Ok(text) = std::str::from_utf8(raw) else { return };
        let Some(data) = text.trim_end_matches(['\r', '\n']).strip_prefix("data:") else { return };
        let data = data.trim();
        if data == "[DONE]" {
            self.done = true;
        } else if let Ok(value) = serde_json::from_str::<Value>(data) {
            if let Some(usage) = usage_of(&value) {
                self.usage = Some(usage);
            }
        }
    }

    pub fn usage(&self) -> Option<Tokens> {
        self.usage
    }

    pub fn done(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usage_of_an_answer_needs_both_counts() {
        let body: Value = serde_json::json!({"usage": {"prompt_tokens": 12, "completion_tokens": 34, "total_tokens": 46}});
        assert_eq!(usage_of(&body), Some(Tokens { prompt: 12, completion: 34 }));
        assert_eq!(usage_of(&serde_json::json!({"usage": {"prompt_tokens": 12}})), None);
        assert_eq!(usage_of(&serde_json::json!({"usage": null})), None);
        assert_eq!(usage_of(&serde_json::json!({})), None);
    }

    #[test]
    fn a_stream_gives_its_usage_and_its_end() {
        let mut scan = UsageScanner::default();
        scan.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}],\"usage\":null}\n\n");
        assert_eq!(scan.usage(), None);
        scan.push(b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n");
        assert_eq!(scan.usage(), Some(Tokens { prompt: 5, completion: 2 }));
        assert!(scan.done());
    }

    #[test]
    fn a_line_split_across_chunks_is_still_read() {
        let mut scan = UsageScanner::default();
        scan.push(b"data: {\"usage\":{\"prompt_tokens\":7,");
        assert_eq!(scan.usage(), None);
        scan.push(b"\"completion_tokens\":8}}\r\n");
        assert_eq!(scan.usage(), Some(Tokens { prompt: 7, completion: 8 }));
    }

    #[test]
    fn noise_is_ignored() {
        let mut scan = UsageScanner::default();
        scan.push(b": keep-alive\n\nevent: ping\ndata: not json\n\n\xff\xfe\n");
        assert_eq!(scan.usage(), None);
        assert!(!scan.done());
    }
}
