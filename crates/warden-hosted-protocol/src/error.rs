//! The error answer: the same shape the gateway already uses on its model endpoint, `{"error": {"code", "message"}}`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorDetail {
    /// A short stable word the caller can match on (`invalid_signature`, `account_not_found`...).
    pub code: String,
    pub message: String,
}

impl ErrorBody {
    pub fn new(code: &str, message: &str) -> ErrorBody {
        ErrorBody { error: ErrorDetail { code: code.to_string(), message: message.to_string() } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_has_the_nested_shape() {
        let json = serde_json::to_string(&ErrorBody::new("account_not_found", "no such account")).unwrap();
        assert_eq!(json, r#"{"error":{"code":"account_not_found","message":"no such account"}}"#);
    }
}
