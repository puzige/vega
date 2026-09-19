use std::collections::HashSet;

use base64::Engine;
use serde_json::{Map, Value, json};

use crate::{Catalog, LEGACY_VERSION, MODERN_VERSION, McpError, Tool, ToolResult};

pub(crate) const MAX_TOOLS: usize = 64;
pub(crate) const MAX_SCHEMAS: usize = 256 * 1024;
pub(crate) const MAX_ARGUMENTS: usize = 256 * 1024;
pub(crate) const MAX_RESULT: usize = 256 * 1024;
pub(crate) const MAX_LINE_OR_EVENT: usize = 1024 * 1024;
pub(crate) const MAX_RESPONSE: usize = 8 * 1024 * 1024;
pub(crate) const MAX_CATALOG: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct HeaderParam {
    name: String,
    path: Vec<String>,
    kind: HeaderKind,
}

#[derive(Clone, Copy, Debug)]
enum HeaderKind {
    String,
    Integer,
    Boolean,
}

pub(crate) fn modern_request(id: u64, method: &str, mut params: Value) -> Value {
    if let Some(object) = params.as_object_mut() {
        object.insert(
            "_meta".into(),
            json!({
                "io.modelcontextprotocol/protocolVersion": MODERN_VERSION,
                "io.modelcontextprotocol/clientInfo": {"name": "Vega", "version": env!("CARGO_PKG_VERSION")},
                "io.modelcontextprotocol/clientCapabilities": {}
            }),
        );
    }
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

pub(crate) fn legacy_request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

pub(crate) fn initialize_request(id: u64) -> Value {
    legacy_request(
        id,
        "initialize",
        json!({"protocolVersion": LEGACY_VERSION, "capabilities": {},
            "clientInfo": {"name": "Vega", "version": env!("CARGO_PKG_VERSION")}}),
    )
}

pub(crate) fn response_result(value: &Value, id: u64) -> Result<&Value, McpError> {
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || value.get("id") != Some(&Value::from(id))
    {
        return Err(McpError::InvalidMessage);
    }
    if let Some(error) = value.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_i64)
            .ok_or(McpError::InvalidMessage)?;
        return Err(McpError::Rpc(code));
    }
    value.get("result").ok_or(McpError::InvalidMessage)
}

pub(crate) fn is_modern_error(value: &Value) -> bool {
    value.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
        && matches!(
            value.pointer("/error/code").and_then(Value::as_i64),
            Some(-32022..=-32020)
        )
}

pub(crate) fn parse_discover(value: &Value) -> Result<(), McpError> {
    if value.get("resultType").and_then(Value::as_str) != Some("complete") {
        return Err(McpError::InvalidMessage);
    }
    if value.get("ttlMs").and_then(Value::as_u64).is_none()
        || !matches!(
            value.get("cacheScope").and_then(Value::as_str),
            Some("public" | "private")
        )
    {
        return Err(McpError::InvalidMessage);
    }
    let versions = value
        .get("supportedVersions")
        .and_then(Value::as_array)
        .ok_or(McpError::InvalidMessage)?;
    if !versions
        .iter()
        .any(|item| item.as_str() == Some(MODERN_VERSION))
    {
        return Err(McpError::IncompatibleVersion);
    }
    if !value
        .pointer("/capabilities/tools")
        .is_some_and(Value::is_object)
    {
        return Err(McpError::UnsupportedResult);
    }
    Ok(())
}

pub(crate) fn parse_initialize(value: &Value) -> Result<(), McpError> {
    if value.get("protocolVersion").and_then(Value::as_str) != Some(LEGACY_VERSION) {
        return Err(McpError::IncompatibleVersion);
    }
    if !value
        .pointer("/capabilities/tools")
        .is_some_and(Value::is_object)
    {
        return Err(McpError::UnsupportedResult);
    }
    Ok(())
}

pub(crate) fn checked_arguments(arguments: &Value) -> Result<(), McpError> {
    if !arguments.is_object() {
        return Err(McpError::InvalidMessage);
    }
    if serde_json::to_vec(arguments)
        .map_err(|_| McpError::InvalidMessage)?
        .len()
        > MAX_ARGUMENTS
    {
        return Err(McpError::LimitExceeded);
    }
    Ok(())
}

pub(crate) fn parse_tool_result(value: &Value, modern: bool) -> Result<ToolResult, McpError> {
    let bytes = serde_json::to_vec(value).map_err(|_| McpError::InvalidMessage)?;
    if bytes.len() > MAX_RESULT {
        return Err(McpError::LimitExceeded);
    }
    if modern && value.get("resultType").and_then(Value::as_str) != Some("complete") {
        return Err(McpError::UnsupportedResult);
    }
    let blocks = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or(McpError::InvalidMessage)?;
    let mut text = Vec::with_capacity(blocks.len());
    for block in blocks {
        if block.get("type").and_then(Value::as_str) != Some("text") {
            return Err(McpError::UnsupportedResult);
        }
        text.push(
            block
                .get("text")
                .and_then(Value::as_str)
                .ok_or(McpError::InvalidMessage)?
                .to_owned(),
        );
    }
    let is_error = value
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(ToolResult {
        text,
        structured_content: value.get("structuredContent").cloned(),
        is_error,
    })
}

pub(crate) struct CatalogBuilder {
    catalog: Catalog,
    seen_names: HashSet<String>,
    catalog_bytes: usize,
    schema_bytes: usize,
    tool_entries: usize,
    http: bool,
    modern: bool,
}

impl CatalogBuilder {
    pub(crate) fn new(http: bool, modern: bool) -> Self {
        Self {
            catalog: Catalog {
                tools: Vec::new(),
                rejected: Vec::new(),
            },
            seen_names: HashSet::new(),
            catalog_bytes: 0,
            schema_bytes: 0,
            tool_entries: 0,
            http,
            modern,
        }
    }

    pub(crate) fn add_page(&mut self, value: &Value) -> Result<Option<String>, McpError> {
        let page_bytes = serde_json::to_vec(value)
            .map_err(|_| McpError::InvalidMessage)?
            .len();
        self.catalog_bytes = self
            .catalog_bytes
            .checked_add(page_bytes)
            .ok_or(McpError::LimitExceeded)?;
        if self.catalog_bytes > MAX_CATALOG {
            return Err(McpError::LimitExceeded);
        }
        if self.modern && value.get("resultType").and_then(Value::as_str) != Some("complete") {
            return Err(McpError::UnsupportedResult);
        }
        if self.modern
            && (value.get("ttlMs").and_then(Value::as_u64).is_none()
                || !matches!(
                    value.get("cacheScope").and_then(Value::as_str),
                    Some("public" | "private")
                ))
        {
            return Err(McpError::InvalidMessage);
        }
        let tools = value
            .get("tools")
            .and_then(Value::as_array)
            .ok_or(McpError::InvalidMessage)?;
        for raw in tools {
            self.tool_entries = self
                .tool_entries
                .checked_add(1)
                .ok_or(McpError::LimitExceeded)?;
            if self.tool_entries > MAX_TOOLS {
                return Err(McpError::LimitExceeded);
            }
            match parse_tool(raw, self.http) {
                Ok(tool) => {
                    let schema_size = serde_json::to_vec(&tool.input_schema)
                        .map_err(|_| McpError::InvalidMessage)?
                        .len();
                    self.schema_bytes = self
                        .schema_bytes
                        .checked_add(schema_size)
                        .ok_or(McpError::LimitExceeded)?;
                    if self.schema_bytes > MAX_SCHEMAS {
                        return Err(McpError::LimitExceeded);
                    }
                    if self.seen_names.insert(tool.name.clone()) {
                        self.catalog.tools.push(tool);
                    } else {
                        self.catalog.rejected.push("duplicate tool name".into());
                    }
                }
                Err(()) => self.catalog.rejected.push("invalid tool definition".into()),
            }
        }
        match value.get("nextCursor") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(cursor)) if !cursor.is_empty() && cursor.len() <= 4096 => {
                Ok(Some(cursor.clone()))
            }
            _ => Err(McpError::InvalidMessage),
        }
    }

    pub(crate) fn finish(self) -> Catalog {
        self.catalog
    }
}

fn parse_tool(raw: &Value, http: bool) -> Result<Tool, ()> {
    let object = raw.as_object().ok_or(())?;
    let name = object.get("name").and_then(Value::as_str).ok_or(())?;
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(());
    }
    let schema = object.get("inputSchema").ok_or(())?;
    if !schema.is_object() || schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(());
    }
    let header_params = if http {
        collect_header_params(schema)?
    } else {
        Vec::new()
    };
    let description = match object.get("description") {
        None => None,
        Some(Value::String(text)) => Some(text.clone()),
        _ => return Err(()),
    };
    Ok(Tool {
        name: name.to_owned(),
        description,
        input_schema: schema.clone(),
        header_params,
    })
}

fn collect_header_params(schema: &Value) -> Result<Vec<HeaderParam>, ()> {
    let mut params = Vec::new();
    let mut seen = HashSet::new();
    inspect_schema(schema, true, &[], &mut params, &mut seen)?;
    Ok(params)
}

fn inspect_schema(
    node: &Value,
    reachable: bool,
    path: &[String],
    params: &mut Vec<HeaderParam>,
    seen: &mut HashSet<String>,
) -> Result<(), ()> {
    match node {
        Value::Object(object) => {
            if let Some(header) = object.get("x-mcp-header") {
                let name = header.as_str().ok_or(())?;
                if !reachable || path.is_empty() || !valid_header_token(name) {
                    return Err(());
                }
                let kind = match object.get("type").and_then(Value::as_str) {
                    Some("string") => HeaderKind::String,
                    Some("integer") => HeaderKind::Integer,
                    Some("boolean") => HeaderKind::Boolean,
                    _ => return Err(()),
                };
                if !seen.insert(name.to_ascii_lowercase()) {
                    return Err(());
                }
                params.push(HeaderParam {
                    name: name.to_owned(),
                    path: path.to_vec(),
                    kind,
                });
            }
            for (key, value) in object {
                if key == "x-mcp-header" {
                    continue;
                }
                if key == "properties" && reachable {
                    let properties = value.as_object().ok_or(())?;
                    for (property, child) in properties {
                        let mut next = path.to_vec();
                        next.push(property.clone());
                        inspect_schema(child, true, &next, params, seen)?;
                    }
                } else {
                    inspect_schema(value, false, path, params, seen)?;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                inspect_schema(item, false, path, params, seen)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn valid_header_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

pub(crate) fn header_value(value: &str) -> String {
    let plain = value
        .bytes()
        .all(|byte| matches!(byte, 0x20..=0x7e | b'\t'))
        && value.trim() == value
        && !(value.starts_with("=?base64?") && value.ends_with("?="));
    if plain {
        value.to_owned()
    } else {
        format!(
            "=?base64?{}?=",
            base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
        )
    }
}

impl Tool {
    pub(crate) fn mirrored_headers(
        &self,
        arguments: &Value,
    ) -> Result<Vec<(String, String)>, McpError> {
        let mut headers = Vec::with_capacity(self.header_params.len());
        for param in &self.header_params {
            let mut value = arguments;
            for segment in &param.path {
                value = match value.get(segment) {
                    Some(next) => next,
                    None => &Value::Null,
                };
            }
            if value.is_null() {
                continue;
            }
            let text = match param.kind {
                HeaderKind::String => value.as_str().ok_or(McpError::InvalidMessage)?.to_owned(),
                HeaderKind::Boolean => value.as_bool().ok_or(McpError::InvalidMessage)?.to_string(),
                HeaderKind::Integer => {
                    let integer = value.as_i64().ok_or(McpError::InvalidMessage)?;
                    if !(-(1_i64 << 53) + 1..=(1_i64 << 53) - 1).contains(&integer) {
                        return Err(McpError::InvalidMessage);
                    }
                    integer.to_string()
                }
            };
            headers.push((format!("Mcp-Param-{}", param.name), header_value(&text)));
        }
        Ok(headers)
    }
}

pub(crate) fn empty_params() -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CatalogBuilder, MAX_ARGUMENTS, checked_arguments, header_value, parse_tool_result,
    };
    use crate::McpError;

    #[test]
    fn unsafe_header_values_and_sentinel_literals_are_encoded() {
        assert_eq!(header_value("plain"), "plain");
        assert_eq!(
            header_value("=?base64?literal?="),
            "=?base64?PT9iYXNlNjQ/bGl0ZXJhbD89?="
        );
        assert_eq!(
            header_value("Hello, 世界"),
            "=?base64?SGVsbG8sIOS4lueVjA==?="
        );
    }

    #[test]
    fn unreachable_or_nonprimitive_header_annotations_reject_only_the_tool() {
        let page = json!({
            "resultType":"complete", "ttlMs":0, "cacheScope":"private",
            "tools":[
                {"name":"valid", "inputSchema":{"type":"object"}},
                {"name":"unreachable", "inputSchema":{"type":"object", "oneOf":[{"properties":{"x":{"type":"string", "x-mcp-header":"X"}}}]}},
                {"name":"number", "inputSchema":{"type":"object", "properties":{"n":{"type":"number", "x-mcp-header":"N"}}}}
            ]
        });
        let mut catalog = CatalogBuilder::new(true, true);
        assert!(catalog.add_page(&page).expect("catalog page").is_none());
        let catalog = catalog.finish();
        assert_eq!(catalog.tools.len(), 1);
        assert_eq!(catalog.rejected.len(), 2);
    }

    #[test]
    fn arguments_and_unsupported_content_fail_closed() {
        let too_large = json!({"input": "x".repeat(MAX_ARGUMENTS)});
        assert!(matches!(
            checked_arguments(&too_large),
            Err(McpError::LimitExceeded)
        ));
        let image = json!({"resultType":"complete", "content":[{"type":"image", "data":"a"}]});
        assert!(matches!(
            parse_tool_result(&image, true),
            Err(McpError::UnsupportedResult)
        ));
    }
}
