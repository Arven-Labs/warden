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

/// Puts the catalog's model name in the `model` field of each `data:` line of a stream, so the caller never sees the
/// provider's own (the whole answer gets the same in `http`). It works a line at a time: a line split across two chunks waits
/// for its end. Whatever isn't a `data:` JSON line with a string `model` (`[DONE]`, comments, `event:`, noise) passes
/// byte for byte.
pub struct ModelRewriter {
    model: String,
    /// The tail of the last chunk that doesn't end a line yet.
    pending: Vec<u8>,
}

impl ModelRewriter {
    pub fn new(model: &str) -> ModelRewriter {
        ModelRewriter { model: model.to_string(), pending: Vec::new() }
    }

    /// The bytes to send on for `chunk`: every line it completed, rewritten where it applies.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        let mut out = Vec::with_capacity(chunk.len());
        while let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            out.extend_from_slice(&self.line(&line));
        }
        // A stream that never sends a newline must not grow without bound: past the limit the tail goes on as it is.
        if self.pending.len() > 1 << 20 {
            out.append(&mut self.pending);
        }
        out
    }

    /// What is left when the stream ends without a final newline.
    pub fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }

    fn line(&self, raw: &[u8]) -> Vec<u8> {
        let Ok(text) = std::str::from_utf8(raw) else { return raw.to_vec() };
        let body = text.trim_end_matches(['\r', '\n']);
        let ending = &text[body.len()..];
        let Some(data) = body.strip_prefix("data:") else { return raw.to_vec() };
        // Most lines carry no `model` (only the first chunk of some providers does): skip the parse for them.
        if !data.contains("\"model\"") {
            return raw.to_vec();
        }
        let Ok(mut value) = serde_json::from_str::<Value>(data.trim()) else { return raw.to_vec() };
        match value.get_mut("model") {
            Some(model) if model.is_string() => *model = Value::String(self.model.clone()),
            _ => return raw.to_vec(),
        }
        format!("data: {value}{ending}").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewrite_all(chunks: &[&[u8]]) -> String {
        let mut rewriter = ModelRewriter::new("fast");
        let mut out = Vec::new();
        for chunk in chunks {
            out.extend(rewriter.push(chunk));
        }
        out.extend(rewriter.finish());
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn the_model_of_a_data_line_becomes_the_catalogs() {
        let out = rewrite_all(&[b"data: {\"id\":\"x\",\"model\":\"real-fast\",\"choices\":[]}\n\n"]);
        let line = out.lines().next().unwrap().strip_prefix("data: ").unwrap();
        let value: Value = serde_json::from_str(line).unwrap();
        assert_eq!((value["model"].as_str(), value["id"].as_str()), (Some("fast"), Some("x")));
        assert!(!out.contains("real-fast") && out.ends_with("\n\n"));
    }

    #[test]
    fn what_is_not_a_model_line_passes_byte_for_byte() {
        let same: &[u8] = b": keep-alive\n\nevent: ping\ndata: [DONE]\n\ndata: not json \"model\"\n\ndata: {\"choices\":[]}\n\ndata: {\"model\":7}\n\n\xff\xfe\n";
        let mut rewriter = ModelRewriter::new("fast");
        let mut out = rewriter.push(same);
        out.extend(rewriter.finish());
        assert_eq!(out, same);
    }

    #[test]
    fn a_line_split_across_chunks_is_rewritten_whole() {
        let out = rewrite_all(&[b"data: {\"mod", b"el\":\"real-", b"fast\"}\n", b"\n"]);
        assert_eq!(out, "data: {\"model\":\"fast\"}\n\n");
    }

    #[test]
    fn nothing_is_sent_for_half_a_line_until_it_is_finished() {
        let mut rewriter = ModelRewriter::new("fast");
        assert!(rewriter.push(b"data: {\"model\":\"real").is_empty());
        assert_eq!(rewriter.push(b"-fast\"}\n"), b"data: {\"model\":\"fast\"}\n");
    }

    #[test]
    fn crlf_is_kept_and_several_lines_in_one_chunk_all_change() {
        let out = rewrite_all(&[b"data: {\"model\":\"a\"}\r\n\r\ndata: {\"model\":\"b\"}\r\n\r\n"]);
        assert_eq!(out, "data: {\"model\":\"fast\"}\r\n\r\ndata: {\"model\":\"fast\"}\r\n\r\n");
    }

    #[test]
    fn a_tail_without_a_newline_is_not_lost() {
        let mut rewriter = ModelRewriter::new("fast");
        assert!(rewriter.push(b"data: [DO").is_empty());
        assert_eq!(rewriter.finish(), b"data: [DO");
        assert!(rewriter.finish().is_empty());
    }

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
