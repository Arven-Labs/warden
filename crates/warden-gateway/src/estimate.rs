//! A guess at how many tokens a request's prompt has, before the provider has counted them (P168 f).
//!
//! The gateway needs it to size the hold of a call: the prompt's tokens plus the answer ceiling, priced. The real charge
//! comes from the provider's `usage`, and the hold is released when the call settles, so the guess only has to be safe:
//! too low and a call can cost more than was reserved; too high only ties up credit for a moment (and, for a call that
//! comes back with no `usage`, the hold is what is charged). It leans high, but it counts what a tokenizer would see:
//! the text, not the JSON around it and not the bytes of an image sent inline.

use serde_json::Value;

/// ASCII characters in a token, for the worst case (code and JSON; English prose is nearer 4).
const ASCII_PER_TOKEN: u64 = 3;
/// A character outside ASCII: CJK is about one token each, accented Latin less. Emoji (four bytes) count double.
const OTHER_TOKENS: u64 = 1;
const WIDE_TOKENS: u64 = 2;
/// What a message costs beyond its content (role and separators), as chat formats count it.
const PER_MESSAGE: u64 = 4;
/// What the reply starts with.
const REPLY_PRIMER: u64 = 3;
/// An image, audio clip or file sent with the prompt. Providers charge from a few hundred to a couple of thousand tokens
/// for one; its size in bytes says nothing about it, so the (base64) data is never counted.
const MEDIA_TOKENS: u64 = 2000;

/// Content parts that are not text.
const MEDIA_PARTS: [&str; 4] = ["image_url", "input_audio", "file", "video_url"];

fn text_tokens(text: &str) -> u64 {
    let (mut ascii, mut other) = (0u64, 0u64);
    for c in text.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else if c.len_utf8() == 4 {
            other += WIDE_TOKENS;
        } else {
            other += OTHER_TOKENS;
        }
    }
    ascii.div_ceil(ASCII_PER_TOKEN) + other
}

/// Any JSON value: its text and its keys (a tool's schema is tokenized with them), a flat token for a number, a boolean or null,
/// and a flat `MEDIA_TOKENS` for a `data:` URL wherever it turns up.
fn value_tokens(value: &Value) -> u64 {
    match value {
        Value::String(s) if s.starts_with("data:") => MEDIA_TOKENS,
        Value::String(s) => text_tokens(s),
        Value::Array(items) => items.iter().map(value_tokens).sum(),
        Value::Object(map) => map.iter().map(|(key, v)| text_tokens(key) + value_tokens(v)).sum(),
        Value::Number(_) | Value::Bool(_) | Value::Null => 1,
    }
}

fn part_tokens(part: &Value) -> u64 {
    let is_media = part.get("type").and_then(Value::as_str).is_some_and(|t| MEDIA_PARTS.contains(&t))
        || MEDIA_PARTS.iter().any(|key| part.get(*key).is_some());
    if is_media {
        return MEDIA_TOKENS;
    }
    match part.get("text").and_then(Value::as_str) {
        Some(text) => text_tokens(text),
        None => value_tokens(part),
    }
}

fn content_tokens(content: &Value) -> u64 {
    match content {
        Value::String(s) => text_tokens(s),
        Value::Array(parts) => parts.iter().map(part_tokens).sum(),
        other => value_tokens(other),
    }
}

fn message_tokens(message: &Value) -> u64 {
    let Some(fields) = message.as_object() else { return PER_MESSAGE + value_tokens(message) };
    let rest: u64 = fields
        .iter()
        // The role is in the per-message cost already.
        .filter(|(key, _)| key.as_str() != "role")
        .map(|(key, v)| if key == "content" { content_tokens(v) } else { text_tokens(key) + value_tokens(v) })
        .sum();
    PER_MESSAGE + rest
}

/// The prompt's tokens, from the body of a chat request: the messages, and the tools and formats that go with them.
pub fn estimate_prompt_tokens(body: &Value) -> u64 {
    let messages = match body.get("messages") {
        Some(Value::Array(messages)) => messages.iter().map(message_tokens).sum(),
        Some(other) => value_tokens(other),
        None => 0,
    };
    let extras: u64 = ["tools", "functions", "response_format", "tool_choice"].iter().filter_map(|key| body.get(*key)).map(value_tokens).sum();
    REPLY_PRIMER + messages + extras
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_plain_message_is_its_text_plus_the_frame() {
        let body = json!({"model": "m", "messages": [{"role": "user", "content": "hello there"}]});
        // 11 ASCII characters = 4 tokens, plus 4 for the message and 3 for the reply primer.
        assert_eq!(estimate_prompt_tokens(&body), 4 + PER_MESSAGE + REPLY_PRIMER);
    }

    #[test]
    fn it_leans_high_against_a_real_tokenizer() {
        // About 9 tokens for a real tokenizer; the guess must not be under.
        let body = json!({"messages": [{"role": "user", "content": "What is the capital of France?"}]});
        assert!(estimate_prompt_tokens(&body) >= 9);
    }

    #[test]
    fn characters_outside_ascii_cost_a_token_each_and_emoji_two() {
        assert_eq!(text_tokens("日本語"), 3);
        assert_eq!(text_tokens("é"), 1);
        assert_eq!(text_tokens("😀"), 2);
        assert_eq!(text_tokens(""), 0);
        assert_eq!(text_tokens("abc"), 1);
        assert_eq!(text_tokens("abcd"), 2);
    }

    #[test]
    fn an_inline_image_costs_a_flat_amount_whatever_its_size() {
        let huge = format!("data:image/png;base64,{}", "A".repeat(1_000_000));
        let body = json!({"messages": [{"role": "user", "content": [
            {"type": "text", "text": "what is this?"},
            {"type": "image_url", "image_url": {"url": huge}},
        ]}]});
        let tokens = estimate_prompt_tokens(&body);
        assert!(tokens >= MEDIA_TOKENS && tokens < MEDIA_TOKENS + 100, "{tokens}");
    }

    #[test]
    fn a_data_url_anywhere_else_is_flat_too() {
        let body = json!({"messages": [{"role": "user", "content": "x", "attachment": format!("data:text/plain;base64,{}", "B".repeat(500_000))}]});
        assert!(estimate_prompt_tokens(&body) < MEDIA_TOKENS + 100);
    }

    #[test]
    fn text_parts_add_up_and_the_audio_and_file_parts_are_media() {
        let body = json!({"messages": [{"role": "user", "content": [
            {"type": "text", "text": "abcdef"},
            {"type": "text", "text": "ghi"},
            {"type": "input_audio", "input_audio": {"data": "zzz", "format": "wav"}},
            {"type": "file", "file": {"file_data": "data:application/pdf;base64,AAAA"}},
        ]}]});
        assert_eq!(estimate_prompt_tokens(&body), REPLY_PRIMER + PER_MESSAGE + 2 + 1 + 2 * MEDIA_TOKENS);
    }

    #[test]
    fn tools_count_their_schema_and_a_bigger_schema_costs_more() {
        let small = json!({"messages": [], "tools": [{"type": "function", "function": {"name": "f", "parameters": {"type": "object"}}}]});
        let big = json!({"messages": [], "tools": [{"type": "function", "function": {"name": "f", "description": "Looks up the weather for a city and returns a short report.",
            "parameters": {"type": "object", "properties": {"city": {"type": "string", "description": "The city to look up"}}, "required": ["city"]}}}]});
        assert!(estimate_prompt_tokens(&small) > REPLY_PRIMER);
        assert!(estimate_prompt_tokens(&big) > estimate_prompt_tokens(&small) + 20);
    }

    #[test]
    fn tool_calls_and_tool_results_in_the_history_are_counted() {
        let body = json!({"messages": [
            {"role": "assistant", "content": null, "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{\"city\":\"Paris\"}"}}]},
            {"role": "tool", "tool_call_id": "call_1", "content": "sunny"},
        ]});
        assert!(estimate_prompt_tokens(&body) > REPLY_PRIMER + 2 * PER_MESSAGE + 10);
    }

    #[test]
    fn a_malformed_body_is_never_estimated_as_nothing() {
        let loose = json!({"messages": "just a string"});
        assert!(estimate_prompt_tokens(&loose) > REPLY_PRIMER);
        let odd = json!({"messages": ["not an object", 7]});
        assert!(estimate_prompt_tokens(&odd) > REPLY_PRIMER + 2 * PER_MESSAGE);
        assert_eq!(estimate_prompt_tokens(&json!({})), REPLY_PRIMER);
    }
}
