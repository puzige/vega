use crate::Error;
use serde_json::{Number, Value};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RequestId {
    String(String),
    Number(String),
}

impl RequestId {
    pub(crate) fn from_value(value: &Value) -> Result<Self, Error> {
        match value {
            Value::String(value) => Ok(Self::String(value.clone())),
            Value::Number(value) if value.is_i64() || value.is_u64() => {
                Ok(Self::Number(value.to_string()))
            }
            _ => Err(Error::InvalidRequestId),
        }
    }

    pub(crate) fn to_value(&self) -> Value {
        match self {
            Self::String(value) => Value::String(value.clone()),
            Self::Number(value) => value
                .parse::<Number>()
                .map(Value::Number)
                .unwrap_or(Value::Null),
        }
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        match self {
            Self::String(value) | Self::Number(value) => value.len(),
        }
    }
}

#[derive(Debug)]
pub(crate) enum RpcMessage {
    Request {
        id: RequestId,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Response {
        id: RequestId,
        result: Result<Value, i64>,
    },
}

pub(crate) fn parse_message(value: Value) -> Result<RpcMessage, Error> {
    let object = value.as_object().ok_or(Error::MalformedMessage)?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(Error::MalformedMessage);
    }
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    if let Some(method) = object.get("method") {
        if object.contains_key("result")
            || object.contains_key("error")
            || object
                .get("params")
                .is_some_and(|params| !params.is_object() && !params.is_array())
        {
            return Err(Error::MalformedMessage);
        }
        let method = method.as_str().ok_or(Error::MalformedMessage)?.to_owned();
        if let Some(id) = object.get("id") {
            let id = RequestId::from_value(id)?;
            Ok(RpcMessage::Request { id, method, params })
        } else {
            Ok(RpcMessage::Notification { method, params })
        }
    } else {
        if object.contains_key("params") {
            return Err(Error::MalformedMessage);
        }
        let id = RequestId::from_value(object.get("id").ok_or(Error::MalformedMessage)?)?;
        let result = match (object.get("result"), object.get("error")) {
            (Some(result), None) => Ok(result.clone()),
            (None, Some(error)) => {
                let code = error
                    .as_object()
                    .and_then(|error| error.get("code"))
                    .and_then(Value::as_i64)
                    .ok_or(Error::MalformedMessage)?;
                if error
                    .as_object()
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .is_none()
                {
                    return Err(Error::MalformedMessage);
                }
                Err(code)
            }
            _ => return Err(Error::MalformedMessage),
        };
        Ok(RpcMessage::Response { id, result })
    }
}

pub(crate) fn response(id: &RequestId, result: Value) -> Value {
    serde_json::json!({"jsonrpc":"2.0","id":id.to_value(),"result":result})
}

pub(crate) fn error_response(id: &RequestId, code: i64, message: &str) -> Value {
    serde_json::json!({"jsonrpc":"2.0","id":id.to_value(),"error":{"code":code,"message":message}})
}

pub(crate) fn request(id: &RequestId, method: &str, params: Value) -> Value {
    serde_json::json!({"jsonrpc":"2.0","id":id.to_value(),"method":method,"params":params})
}

pub(crate) fn notification(method: &str, params: Value) -> Value {
    serde_json::json!({"jsonrpc":"2.0","method":method,"params":params})
}
