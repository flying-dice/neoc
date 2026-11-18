//! The JSON-RPC 2.0 envelope of the `/rpc` socket. The view protocol's methods and messages live in
//! [`crate::ui::protocol`]; the three typed params at the bottom are the spike's, kept until wave A4.

use crate::ui::{UiEvent, UiNode, UiPatch};
use crate::AppId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcId {
    Num(i64),
    Str(String),
}

/// Any inbound/outbound frame on the socket. Order matters for `untagged`: a Request has `method`+`id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcMessage {
    Request {
        jsonrpc: Version,
        id: RpcId,
        method: String,
        #[serde(default)]
        params: Value,
    },
    Notification {
        jsonrpc: Version,
        method: String,
        #[serde(default)]
        params: Value,
    },
    Response {
        jsonrpc: Version,
        id: RpcId,
        result: Value,
    },
    Error {
        jsonrpc: Version,
        id: Option<RpcId>,
        error: RpcError,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Version {
    #[serde(rename = "2.0")]
    V2,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcMessage {
    pub fn request(id: i64, method: &str, params: Value) -> Self {
        Self::Request { jsonrpc: Version::V2, id: RpcId::Num(id), method: method.into(), params }
    }
    pub fn notification(method: &str, params: Value) -> Self {
        Self::Notification { jsonrpc: Version::V2, method: method.into(), params }
    }
    pub fn ok(id: RpcId, result: Value) -> Self {
        Self::Response { jsonrpc: Version::V2, id, result }
    }
    pub fn err(id: Option<RpcId>, code: i32, message: &str) -> Self {
        Self::Error {
            jsonrpc: Version::V2,
            id,
            error: RpcError { code, message: message.into(), data: None },
        }
    }
}

// --- the spike's typed params (client <-> host), replaced by `ui::protocol` in wave A4 ---

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
pub struct DispatchUiEvent {
    pub app: AppId,
    pub event: UiEvent,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
pub struct UiTree {
    pub app: AppId,
    pub root: UiNode,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
pub struct UiPatches {
    pub app: AppId,
    pub patches: Vec<UiPatch>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frames_are_classified() {
        let p = |v: Value| serde_json::from_value::<RpcMessage>(v).unwrap();
        assert!(matches!(p(json!({"jsonrpc":"2.0","id":1,"method":"m"})), RpcMessage::Request { .. }));
        assert!(matches!(
            p(json!({"jsonrpc":"2.0","method":"m","params":[1]})),
            RpcMessage::Notification { .. }
        ));
        assert!(matches!(p(json!({"jsonrpc":"2.0","id":"a","result":1})), RpcMessage::Response { .. }));
        assert!(matches!(
            p(json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"x"}})),
            RpcMessage::Error { .. }
        ));
        assert!(serde_json::from_value::<RpcMessage>(json!({"jsonrpc":"1.0","id":1,"method":"m"})).is_err());
    }
}
