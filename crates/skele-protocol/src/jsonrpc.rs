//! JSON-RPC 2.0 envelope types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: &str = "2.0";

pub mod codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    Num(i64),
    Str(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), data: None }
    }
    pub fn method_not_found(method: &str) -> Self {
        Self::new(codes::METHOD_NOT_FOUND, format!("method not found: {method}"))
    }
    pub fn invalid_params(err: impl std::fmt::Display) -> Self {
        Self::new(codes::INVALID_PARAMS, format!("invalid params: {err}"))
    }
    pub fn internal(err: impl std::fmt::Display) -> Self {
        Self::new(codes::INTERNAL_ERROR, err.to_string())
    }
}

/// A decoded frame. Requests carry an `id`; notifications do not.
#[derive(Debug, Clone)]
pub enum Message {
    Request { id: Id, method: String, params: Value },
    Notification { method: String, params: Value },
    Response { id: Id, result: Result<Value, RpcError> },
}

#[derive(Serialize, Deserialize)]
struct Raw {
    jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

impl Message {
    pub fn decode(bytes: &[u8]) -> Result<Self, RpcError> {
        let raw: Raw = serde_json::from_slice(bytes).map_err(|e| RpcError::new(codes::PARSE_ERROR, e.to_string()))?;
        if raw.jsonrpc != VERSION {
            return Err(RpcError::new(codes::INVALID_REQUEST, "jsonrpc must be \"2.0\""));
        }
        let params = raw.params.unwrap_or(Value::Null);
        match (raw.method, raw.id) {
            (Some(method), Some(id)) => Ok(Message::Request { id, method, params }),
            (Some(method), None) => Ok(Message::Notification { method, params }),
            (None, Some(id)) => Ok(Message::Response {
                id,
                result: match raw.error {
                    Some(err) => Err(err),
                    None => Ok(raw.result.unwrap_or(Value::Null)),
                },
            }),
            (None, None) => Err(RpcError::new(codes::INVALID_REQUEST, "neither method nor id")),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut raw = Raw { jsonrpc: VERSION.into(), id: None, method: None, params: None, result: None, error: None };
        match self.clone() {
            Message::Request { id, method, params } => {
                raw.id = Some(id);
                raw.method = Some(method);
                raw.params = Some(params);
            }
            Message::Notification { method, params } => {
                raw.method = Some(method);
                raw.params = Some(params);
            }
            Message::Response { id, result } => {
                raw.id = Some(id);
                match result {
                    // `result` must be present on success, even when null.
                    Ok(value) => raw.result = Some(value),
                    Err(err) => raw.error = Some(err),
                }
            }
        }
        serde_json::to_vec(&raw).expect("JSON-RPC envelope serializes")
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (code {})", self.message, self.code)
    }
}

impl std::error::Error for RpcError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_frames() {
        let req = Message::decode(br#"{"jsonrpc":"2.0","id":1,"method":"a/b","params":{"x":1}}"#).unwrap();
        assert!(matches!(req, Message::Request { id: Id::Num(1), .. }));
        let note = Message::decode(br#"{"jsonrpc":"2.0","method":"a/b"}"#).unwrap();
        assert!(matches!(note, Message::Notification { .. }));
        let ok = Message::decode(br#"{"jsonrpc":"2.0","id":"x","result":null}"#).unwrap();
        assert!(matches!(ok, Message::Response { result: Ok(Value::Null), .. }));
        let err = Message::decode(br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"nope"}}"#).unwrap();
        assert!(matches!(err, Message::Response { result: Err(_), .. }));
        assert!(Message::decode(br#"{"jsonrpc":"1.0","id":2}"#).is_err());
    }

    #[test]
    fn null_result_is_serialized() {
        let bytes = Message::Response { id: Id::Num(7), result: Ok(Value::Null) }.encode();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value, json!({"jsonrpc": "2.0", "id": 7, "result": null}));
    }
}
