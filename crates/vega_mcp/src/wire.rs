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
    let is_error = match value.get("isError") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(McpError::InvalidMessage),
    };
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
    let mut nodes = 0;
    if validate_schema(schema, 0, &mut nodes).is_err()
        || schema.get("type").and_then(Value::as_str) != Some("object")
    {
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

fn validate_schema(node: &Value, depth: usize, nodes: &mut usize) -> Result<(), ()> {
    *nodes = nodes.checked_add(1).ok_or(())?;
    if depth > 32 || *nodes > 512 {
        return Err(());
    }
    let object = node.as_object().ok_or(())?;
    let kind = object.get("type").and_then(Value::as_str).ok_or(())?;
    if !matches!(
        kind,
        "object" | "array" | "string" | "integer" | "number" | "boolean" | "null"
    ) {
        return Err(());
    }
    for (key, value) in object {
        match key.as_str() {
            "type" => {}
            "title" | "description" | "format" => {
                if !value.is_string() {
                    return Err(());
                }
            }
            "default" | "const" => {}
            "enum" => {
                let items = value.as_array().ok_or(())?;
                if items.is_empty() || items.len() > 128 {
                    return Err(());
                }
                for (index, item) in items.iter().enumerate() {
                    if !item.is_string() {
                        return Err(());
                    }
                    if items[..index].contains(item) {
                        return Err(());
                    }
                }
            }
            "properties" if kind == "object" => {
                let properties = value.as_object().ok_or(())?;
                if properties.len() > 128 {
                    return Err(());
                }
                for child in properties.values() {
                    validate_schema(child, depth + 1, nodes)?;
                }
            }
            "required" if kind == "object" => {
                let required = value.as_array().ok_or(())?;
                let mut seen = HashSet::new();
                for item in required {
                    if !seen.insert(item.as_str().ok_or(())?) {
                        return Err(());
                    }
                }
            }
            "additionalProperties" if kind == "object" => {
                if !value.is_boolean() {
                    validate_schema(value, depth + 1, nodes)?;
                }
            }
            "items" if kind == "array" => validate_schema(value, depth + 1, nodes)?,
            "minLength" | "maxLength" if kind == "string" => {
                if value.as_u64().is_none() {
                    return Err(());
                }
            }
            "minItems" | "maxItems" if kind == "array" => {
                if value.as_u64().is_none() {
                    return Err(());
                }
            }
            "minProperties" | "maxProperties" if kind == "object" => {
                if value.as_u64().is_none() {
                    return Err(());
                }
            }
            "minimum" | "maximum" if matches!(kind, "integer" | "number") => {
                if !value.is_number() {
                    return Err(());
                }
            }
            "x-mcp-header" if matches!(kind, "string" | "integer" | "boolean") => {
                if !value.as_str().is_some_and(valid_header_token) {
                    return Err(());
                }
            }
            _ => return Err(()),
        }
    }
    for (min, max) in [
        ("minLength", "maxLength"),
        ("minItems", "maxItems"),
        ("minProperties", "maxProperties"),
    ] {
        if let (Some(min), Some(max)) = (
            object.get(min).and_then(Value::as_u64),
            object.get(max).and_then(Value::as_u64),
        ) && min > max
        {
            return Err(());
        }
    }
    if let (Some(min), Some(max)) = (
        object.get("minimum").and_then(Value::as_f64),
        object.get("maximum").and_then(Value::as_f64),
    ) && min > max
    {
        return Err(());
    }
    Ok(())
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
                if key == "properties" && reachable {
                    let properties = value.as_object().ok_or(())?;
                    for (property, child) in properties {
                        let mut next = path.to_vec();
                        next.push(property.clone());
                        inspect_schema(child, true, &next, params, seen)?;
                    }
                } else if matches!(key.as_str(), "items" | "additionalProperties") {
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
        && value.len() <= 128
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

    #[test]
    fn malformed_or_unsupported_schema_is_rejected_before_catalog_advertisement() {
        let page = json!({"resultType":"complete", "ttlMs":0, "cacheScope":"private", "tools":[
            {"name":"valid", "inputSchema":{"type":"object","properties":{"term":{"type":"string","minLength":1}},"required":["term"]}},
            {"name":"required_number", "inputSchema":{"type":"object","required":7}},
            {"name":"duplicate_required", "inputSchema":{"type":"object","required":["x","x"]}},
            {"name":"items_number", "inputSchema":{"type":"object","properties":{"x":{"type":"array","items":7}}}},
            {"name":"invalid_type", "inputSchema":{"type":"object","properties":{"x":{"type":"unknown"}}}},
            {"name":"unsupported_ref", "inputSchema":{"type":"object","properties":{"x":{"$ref":"#/$defs/X"}},"$defs":{"X":{"type":"string"}}}}
        ]});
        let mut catalog = CatalogBuilder::new(false, true);
        assert!(catalog.add_page(&page).expect("catalog page").is_none());
        let catalog = catalog.finish();
        assert_eq!(catalog.tools.len(), 1);
        assert_eq!(catalog.tools[0].name, "valid");
        assert_eq!(catalog.rejected.len(), 5);
    }

    #[test]
    fn non_boolean_is_error_never_becomes_success() {
        let result = json!({"resultType":"complete", "content":[], "isError":"true"});
        assert!(matches!(
            parse_tool_result(&result, true),
            Err(McpError::InvalidMessage)
        ));
    }
}
