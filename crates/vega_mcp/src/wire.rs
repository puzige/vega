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

#[derive(Clone)]
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

    pub(crate) fn add_page(
        &mut self,
        value: &Value,
        page_wire_bytes: usize,
    ) -> Result<Option<String>, McpError> {
        self.catalog_bytes = self
            .catalog_bytes
            .checked_add(page_wire_bytes)
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
                Err(reason) => self.catalog.rejected.push(format!(
                    "{}: {reason}",
                    diagnostic_label(
                        raw.get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("<unnamed>")
                    )
                )),
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

fn diagnostic_label(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(160)
        .collect()
}

fn parse_tool(raw: &Value, http: bool) -> Result<Tool, String> {
    let object = raw.as_object().ok_or("/: expected tool object")?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or("/name: expected string")?;
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err("/name: invalid tool name".into());
    }
    let schema = object
        .get("inputSchema")
        .ok_or("/inputSchema: missing schema")?;
    validate_schema(schema)?;
    let header_params = if http {
        collect_header_params(schema)
            .map_err(|()| "/inputSchema: invalid or unreachable x-mcp-header")?
    } else {
        Vec::new()
    };
    let description = match object.get("description") {
        None => None,
        Some(Value::String(text)) => Some(text.clone()),
        _ => return Err("/description: expected string".into()),
    };
    Ok(Tool {
        name: name.to_owned(),
        description,
        input_schema: schema.clone(),
        header_params,
    })
}

fn validate_schema(schema: &Value) -> Result<jsonschema::Validator, String> {
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err("/inputSchema/type: expected object root".into());
    }
    if serde_json::to_vec(schema)
        .map_err(|_| "/inputSchema: invalid JSON")?
        .len()
        > MAX_SCHEMAS
    {
        return Err("/inputSchema: schema byte limit exceeded".into());
    }
    schema_budget(schema, 0, &mut 0, &mut 0, "")?;
    let draft = schema_draft(schema, jsonschema::Draft::Draft202012, "")?;
    let (has_references, _) = reference_header_inventory(schema);
    validate_dialects(schema, draft, "", has_references)?;
    jsonschema::options()
        .with_draft(draft)
        .offline()
        .with_pattern_options(
            jsonschema::PatternOptions::fancy_regex()
                .backtrack_limit(10_000)
                .size_limit(1 << 20)
                .dfa_size_limit(1 << 20),
        )
        .build(schema)
        .map_err(|error| {
            format!(
                "/inputSchema{}: {}",
                diagnostic_label(&error.instance_path().to_string()),
                if matches!(
                    error.kind(),
                    jsonschema::error::ValidationErrorKind::Referencing(_)
                ) {
                    "unresolved schema reference (external retrieval disabled)"
                } else {
                    "invalid JSON Schema"
                }
            )
        })
}

fn schema_budget(
    node: &Value,
    depth: usize,
    nodes: &mut usize,
    objects: &mut usize,
    path: &str,
) -> Result<(), String> {
    *nodes += 1;
    if depth > 32 || *nodes > 8192 {
        return Err(format!(
            "/inputSchema{}: schema resource limit exceeded",
            diagnostic_label(path)
        ));
    }
    match node {
        Value::Object(object) => {
            *objects += 1;
            if *objects > 512 {
                return Err("/inputSchema: schema object limit exceeded".into());
            }
            for (key, child) in object {
                let next = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
                schema_budget(child, depth + 1, nodes, objects, &next)?;
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                schema_budget(child, depth + 1, nodes, objects, &format!("{path}/{index}"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn schema_draft(
    schema: &Value,
    inherited: jsonschema::Draft,
    path: &str,
) -> Result<jsonschema::Draft, String> {
    let Some(declaration) = schema.get("$schema") else {
        return Ok(inherited);
    };
    let draft = declaration.as_str().and_then(|uri| {
        let uri = uri.strip_suffix('#').unwrap_or(uri);
        let path = uri
            .strip_prefix("https://json-schema.org/")
            .or_else(|| uri.strip_prefix("http://json-schema.org/"))?;
        match path {
            "draft-04/schema" => Some(jsonschema::Draft::Draft4),
            "draft-06/schema" => Some(jsonschema::Draft::Draft6),
            "draft-07/schema" => Some(jsonschema::Draft::Draft7),
            "draft/2019-09/schema" => Some(jsonschema::Draft::Draft201909),
            "draft/2020-12/schema" => Some(jsonschema::Draft::Draft202012),
            _ => None,
        }
    });
    draft.ok_or_else(|| {
        format!(
            "/inputSchema{}/$schema: unsupported schema dialect",
            diagnostic_label(path)
        )
    })
}

struct SchemaChild<'a> {
    node: &'a Value,
    path: String,
    property: Option<&'a str>,
}

fn schema_children(schema: &Value, draft: jsonschema::Draft) -> Vec<SchemaChild<'_>> {
    use jsonschema::Draft;
    let Some(object) = schema.as_object() else {
        return Vec::new();
    };
    let modern = matches!(draft, Draft::Draft201909 | Draft::Draft202012);
    let tuple = draft != Draft::Draft202012;
    let mut children = Vec::new();
    for (key, value) in object {
        let map = matches!(
            key.as_str(),
            "properties" | "patternProperties" | "definitions"
        ) || (modern && matches!(key.as_str(), "$defs" | "dependentSchemas"));
        let dependencies = key == "dependencies";
        if map || dependencies {
            if let Some(entries) = value.as_object() {
                for (name, node) in entries {
                    if dependencies && !(node.is_object() || node.is_boolean()) {
                        continue;
                    }
                    children.push(SchemaChild {
                        node,
                        path: format!("/{key}/{}", name.replace('~', "~0").replace('/', "~1")),
                        property: (key == "properties").then_some(name.as_str()),
                    });
                }
            }
            continue;
        }
        let array = matches!(key.as_str(), "allOf" | "anyOf" | "oneOf")
            || (draft == Draft::Draft202012 && key == "prefixItems")
            || (tuple && key == "items" && value.is_array());
        if array {
            if let Some(entries) = value.as_array() {
                for (index, node) in entries.iter().enumerate() {
                    children.push(SchemaChild {
                        node,
                        path: format!("/{key}/{index}"),
                        property: None,
                    });
                }
            }
            continue;
        }
        let single = matches!(key.as_str(), "items" | "additionalProperties" | "not")
            || (tuple && key == "additionalItems")
            || (draft != Draft::Draft4 && matches!(key.as_str(), "contains" | "propertyNames"))
            || (matches!(
                draft,
                Draft::Draft7 | Draft::Draft201909 | Draft::Draft202012
            ) && matches!(key.as_str(), "if" | "then" | "else"))
            || (modern
                && matches!(
                    key.as_str(),
                    "unevaluatedItems" | "unevaluatedProperties" | "contentSchema"
                ));
        if single {
            children.push(SchemaChild {
                node: value,
                path: format!("/{key}"),
                property: None,
            });
        }
    }
    children
}

fn validate_dialects(
    schema: &Value,
    draft: jsonschema::Draft,
    path: &str,
    referenced: bool,
) -> Result<(), String> {
    let declared = schema_draft(schema, draft, path)?;
    if declared != draft {
        return Err(format!(
            "/inputSchema{}/$schema: mixed schema dialects unsupported",
            diagnostic_label(path)
        ));
    }
    if schema
        .as_object()
        .is_some_and(|object| object.contains_key("$vocabulary"))
    {
        return Err(format!(
            "/inputSchema{}/$vocabulary: custom schema vocabulary unsupported",
            diagnostic_label(path)
        ));
    }
    if referenced {
        match schema {
            Value::Object(object) => {
                for (key, node) in object {
                    let next = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
                    validate_dialects(node, draft, &next, true)?;
                }
            }
            Value::Array(entries) => {
                for (index, node) in entries.iter().enumerate() {
                    validate_dialects(node, draft, &format!("{path}/{index}"), true)?;
                }
            }
            _ => {}
        }
    } else {
        for child in schema_children(schema, draft) {
            validate_dialects(child.node, draft, &format!("{path}{}", child.path), false)?;
        }
    }
    Ok(())
}

fn collect_header_params(schema: &Value) -> Result<Vec<HeaderParam>, ()> {
    let mut params = Vec::new();
    let mut seen = HashSet::new();
    let draft = schema_draft(schema, jsonschema::Draft::Draft202012, "").map_err(|_| ())?;
    inspect_schema(schema, draft, true, &[], &mut params, &mut seen)?;
    let (has_references, header_declarations) = reference_header_inventory(schema);
    if has_references && header_declarations != params.len() {
        return Err(());
    }
    Ok(params)
}

fn reference_header_inventory(node: &Value) -> (bool, usize) {
    match node {
        Value::Object(object) => {
            let mut references = object.contains_key("$ref")
                || object.contains_key("$dynamicRef")
                || object.contains_key("$recursiveRef");
            let mut headers = usize::from(object.contains_key("x-mcp-header"));
            for child in object.values() {
                let (child_references, child_headers) = reference_header_inventory(child);
                references |= child_references;
                headers += child_headers;
            }
            (references, headers)
        }
        Value::Array(items) => items
            .iter()
            .fold((false, 0), |(references, headers), child| {
                let (child_references, child_headers) = reference_header_inventory(child);
                (references || child_references, headers + child_headers)
            }),
        _ => (false, 0),
    }
}

fn inspect_schema(
    node: &Value,
    draft: jsonschema::Draft,
    reachable: bool,
    path: &[String],
    params: &mut Vec<HeaderParam>,
    seen: &mut HashSet<String>,
) -> Result<(), ()> {
    let Some(object) = node.as_object() else {
        return Ok(());
    };
    let ignored_siblings = matches!(
        draft,
        jsonschema::Draft::Draft4 | jsonschema::Draft::Draft6 | jsonschema::Draft::Draft7
    ) && object.contains_key("$ref");
    let reachable = reachable && !ignored_siblings;
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
    for child in schema_children(node, draft) {
        let mut next = path.to_vec();
        let child_reachable = reachable && child.property.is_some();
        if let Some(property) = child.property {
            next.push(property.to_owned());
        }
        inspect_schema(child.node, draft, child_reachable, &next, params, seen)?;
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
mod dialect_tests {
    use super::*;

    const DIALECTS: [&str; 5] = [
        "http://json-schema.org/draft-04/schema#",
        "http://json-schema.org/draft-06/schema#",
        "http://json-schema.org/draft-07/schema#",
        "https://json-schema.org/draft/2019-09/schema",
        "https://json-schema.org/draft/2020-12/schema",
    ];

    fn definition(schema: Value) -> Value {
        json!({"name":"example", "inputSchema":schema})
    }

    fn accepts(schema: Value, valid: Value, invalid: Value) {
        let validator = validate_schema(&schema).expect("valid schema definition");
        assert!(validator.is_valid(&valid), "valid instance: {schema}");
        assert!(!validator.is_valid(&invalid), "invalid instance: {schema}");
    }

    #[test]
    fn admits_five_dialects_and_preserves_original_definitions() {
        for dialect in DIALECTS {
            for scheme in ["http:", "https:"] {
                for fragment in ["", "#"] {
                    let uri = format!(
                        "{scheme}{}{fragment}",
                        dialect.split_once(':').unwrap().1.trim_end_matches('#')
                    );
                    let schema = json!({"$schema":uri,"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false});
                    let tool = parse_tool(&definition(schema.clone()), true).unwrap();
                    assert_eq!(tool.input_schema, schema);
                    accepts(schema, json!({"query":"MCP"}), json!({"query":1}));
                }
            }
        }
        let schema = json!({"type":"object","properties":{"tuple":{"type":"array","prefixItems":[{"type":"string"}],"items":false}}});
        accepts(schema, json!({"tuple":["a"]}), json!({"tuple":[1]}));
        assert!(
            validate_schema(&json!({"type":"object","properties":{"x":{"exclusiveMinimum":true}}}))
                .is_err()
        );
    }

    #[test]
    fn uses_draft_specific_boundaries_ids_and_reference_siblings() {
        for (index, dialect) in DIALECTS.iter().enumerate() {
            let exclusive = if index == 0 {
                json!({"minimum":2,"exclusiveMinimum":true})
            } else {
                json!({"exclusiveMinimum":2})
            };
            accepts(
                json!({"$schema":dialect,"type":"object","properties":{"n":exclusive}}),
                json!({"n":3}),
                json!({"n":2}),
            );
            let wrong = if index == 0 { json!(2) } else { json!(true) };
            assert!(validate_schema(&json!({"$schema":dialect,"type":"object","properties":{"n":{"exclusiveMinimum":wrong}}})).is_err());
            let id = if index == 0 { "id" } else { "$id" };
            let mut schema = json!({"$schema":dialect,"type":"object","definitions":{"number":{"type":"integer"}},"properties":{"n":{"$ref":"#/definitions/number","minimum":5}}});
            schema[id] = json!("https://example.invalid/local");
            let validator = validate_schema(&schema).unwrap();
            assert!(validator.is_valid(&json!({"n":6})));
            assert!(!validator.is_valid(&json!({"n":"no"})));
            assert_eq!(validator.is_valid(&json!({"n":2})), index < 3);
            let mut embedded = json!({"type":"integer"});
            embedded[id] = json!("https://example.invalid/number");
            schema["definitions"]["number"] = embedded;
            schema["properties"]["n"] = json!({"$ref":"https://example.invalid/number"});
            accepts(schema, json!({"n":1}), json!({"n":"bad"}));
        }
    }

    #[test]
    fn validates_legacy_tuples_dependencies_and_modern_recursive_schemas() {
        for dialect in &DIALECTS[..4] {
            accepts(
                json!({"$schema":dialect,"type":"object","properties":{"tuple":{"type":"array","items":[{"type":"string"}],"additionalItems":false}},"dependencies":{"trigger":["needed"],"other":{"required":["extra"]}}}),
                json!({"tuple":["x"],"trigger":true,"needed":1,"other":true,"extra":1}),
                json!({"tuple":["x",2]}),
            );
            accepts(
                json!({"$schema":dialect,"type":"object","dependencies":{"trigger":["needed"],"other":{"required":["extra"]}}}),
                json!({"trigger":1,"needed":1,"other":1,"extra":1}),
                json!({"trigger":1}),
            );
            accepts(
                json!({"$schema":dialect,"type":"object","dependencies":{"other":{"required":["extra"]}}}),
                json!({"other":1,"extra":1}),
                json!({"other":1}),
            );
        }
        accepts(
            json!({"$schema":DIALECTS[3],"$recursiveAnchor":true,"type":"object","properties":{"value":{"type":"integer"},"next":{"$recursiveRef":"#"}},"required":["value"]}),
            json!({"value":1,"next":{"value":2}}),
            json!({"value":1,"next":{"value":"bad"}}),
        );
        accepts(
            json!({"$schema":DIALECTS[4],"type":"object","properties":{"tuple":{"type":"array","prefixItems":[{"type":"string"}],"items":false}},"dependentRequired":{"trigger":["needed"]}}),
            json!({"tuple":["x"]}),
            json!({"trigger":1}),
        );
    }

    #[test]
    fn rejects_unknown_mixed_and_custom_dialects_without_echoing_values() {
        for declaration in [
            json!(null),
            json!(1),
            json!("https://secret.invalid/custom"),
            json!("https://json-schema.org/schema"),
            json!("https://json-schema.org/draft-07/schema##"),
        ] {
            let error =
                validate_schema(&json!({"$schema":declaration,"type":"object"})).unwrap_err();
            assert!(error.contains("unsupported schema dialect"));
            assert!(!error.contains("secret.invalid"));
        }
        for dialect in DIALECTS {
            let schema = json!({"$schema":dialect,"type":"object","properties":{"child":{"$schema":dialect,"type":"string"}}});
            assert!(validate_schema(&schema).is_ok());
            let mut mixed = schema;
            mixed["properties"]["child"]["$schema"] = json!(if dialect == DIALECTS[4] {
                DIALECTS[2]
            } else {
                DIALECTS[4]
            });
            mixed["properties"]["child"]["$id"] = json!("https://example.invalid/child");
            assert!(
                validate_schema(&mixed)
                    .unwrap_err()
                    .contains("mixed schema dialects")
            );
        }
        let annotation = json!({"type":"object","default":{"$schema":"custom","x-mcp-header":17}});
        assert!(parse_tool(&definition(annotation.clone()), true).is_ok());
        let mut referenced = annotation;
        referenced["properties"] = json!({"x":{"$ref":"#/default"}});
        assert!(
            validate_schema(&referenced)
                .unwrap_err()
                .contains("unsupported schema dialect")
        );
        assert!(validate_schema(&json!({"type":"object","$vocabulary":{}})).is_err());
        for keyword in ["items", "additionalItems", "dependencies"] {
            let mut schema = json!({"$schema":DIALECTS[2],"type":"object"});
            let child = json!({"$schema":"custom"});
            schema[keyword] = match keyword {
                "items" => json!([child]),
                "dependencies" => json!({"x":child}),
                _ => child,
            };
            assert!(
                validate_schema(&schema)
                    .unwrap_err()
                    .contains("unsupported schema dialect")
            );
        }
    }

    #[test]
    fn checks_legacy_dependencies_under_default_and_explicit_modern_roots() {
        for declaration in [None, Some(DIALECTS[4])] {
            let mut schema = json!({"type":"object"});
            if let Some(dialect) = declaration {
                schema["$schema"] = json!(dialect);
            }
            schema["dependencies"] = json!({"trigger":["needed"]});
            assert!(parse_tool(&definition(schema.clone()), true).is_ok());
            schema["dependencies"] = json!({"trigger":{"$schema":DIALECTS[2],"type":"object"}});
            assert!(
                validate_schema(&schema)
                    .unwrap_err()
                    .contains("mixed schema dialects")
            );
            schema["dependencies"] = json!({"trigger":{"$vocabulary":{}}});
            assert!(
                validate_schema(&schema)
                    .unwrap_err()
                    .contains("custom schema vocabulary")
            );
            schema["dependencies"] = json!({"trigger":{"$schema":DIALECTS[4],"type":"object","properties":{"token":{"type":"string","x-mcp-header":"Token"}}}});
            assert!(parse_tool(&definition(schema.clone()), true).is_err());
            assert!(parse_tool(&definition(schema), false).is_ok());
        }
    }

    #[test]
    fn protects_http_header_extraction_in_all_dialects() {
        for dialect in DIALECTS {
            let schema = json!({"$schema":dialect,"type":"object","properties":{"nested":{"type":"object","properties":{"token":{"type":"string","x-mcp-header":"Token"}}}}});
            let tool = parse_tool(&definition(schema.clone()), true).unwrap();
            assert_eq!(
                tool.mirrored_headers(&json!({"nested":{"token":"value"}}))
                    .unwrap(),
                vec![("Mcp-Param-Token".into(), "value".into())]
            );
            assert!(
                tool.mirrored_headers(&json!({"nested":{"token":3}}))
                    .is_err()
            );
            for child in [
                json!({"type":"number","x-mcp-header":"X"}),
                json!({"type":"string","x-mcp-header":"bad name"}),
            ] {
                let mut invalid = schema.clone();
                invalid["properties"]["extra"] = child;
                assert!(parse_tool(&definition(invalid), true).is_err());
            }
            let mut duplicate = schema;
            duplicate["properties"]["extra"] = json!({"type":"string","x-mcp-header":"token"});
            assert!(parse_tool(&definition(duplicate), true).is_err());
        }
        for dialect in &DIALECTS[..4] {
            for keyword in ["items", "additionalItems", "dependencies"] {
                let child = json!({"type":"string","x-mcp-header":"Hidden"});
                let mut schema = json!({"$schema":dialect,"type":"object"});
                schema[keyword] = match keyword {
                    "items" => json!([child]),
                    "dependencies" => json!({"x":child}),
                    _ => child,
                };
                assert!(parse_tool(&definition(schema.clone()), true).is_err());
                assert!(parse_tool(&definition(schema), false).is_ok());
            }
        }
        for (index, dialect) in DIALECTS.iter().enumerate() {
            let schema = json!({"$schema":dialect,"type":"object","definitions":{"text":{"type":"string"}},"properties":{"x":{"$ref":"#/definitions/text","type":"string","x-mcp-header":"X"}}});
            assert_eq!(parse_tool(&definition(schema), true).is_ok(), index >= 3);
        }
        let hidden = json!({"$schema":DIALECTS[3],"$recursiveAnchor":true,"type":"object","default":{"x-mcp-header":"Hidden"},"properties":{"next":{"$recursiveRef":"#"}}});
        assert!(parse_tool(&definition(hidden.clone()), true).is_err());
        assert!(parse_tool(&definition(hidden), false).is_ok());
    }

    #[test]
    fn keeps_offline_reference_and_resource_limits_and_catalog_diagnostics() {
        for dialect in DIALECTS {
            for uri in [
                "https://example.invalid/not-registered",
                "file:///not-registered",
            ] {
                let schema =
                    json!({"$schema":dialect,"type":"object","properties":{"x":{"$ref":uri}}});
                assert!(
                    validate_schema(&schema)
                        .unwrap_err()
                        .contains("unresolved schema reference")
                );
            }
        }
        let oversized = json!({"type":"object","description":"x".repeat(MAX_SCHEMAS)});
        assert!(
            validate_schema(&oversized)
                .unwrap_err()
                .contains("byte limit")
        );
        let mut deep = json!({"type":"string"});
        for _ in 0..34 {
            deep = json!({"type":"object","properties":{"x":deep}});
        }
        assert!(
            validate_schema(&deep)
                .unwrap_err()
                .contains("resource limit")
        );
        let many = json!({"type":"object","default":vec![json!({});513]});
        assert!(validate_schema(&many).unwrap_err().contains("object limit"));
        let nodes = json!({"type":"object","default":vec![json!(0);8192]});
        assert!(
            validate_schema(&nodes)
                .unwrap_err()
                .contains("resource limit")
        );
        assert!(
            validate_schema(&json!({"type":"object","properties":{"x":{"pattern":"["}}})).is_err()
        );
        assert!(
            validate_schema(
                &json!({"type":"object","properties":{"x":{"pattern":"a{1000000000}"}}})
            )
            .is_err()
        );
        let mut builder = CatalogBuilder::new(true, false);
        let valid = definition(json!({"$schema":DIALECTS[2],"type":"object"}));
        let mut invalid = definition(json!({"$schema":"custom","type":"object"}));
        invalid["name"] = json!("rejected");
        builder
            .add_page(&json!({"tools":[valid, invalid.clone()]}), 100)
            .unwrap();
        let catalog = builder.finish();
        assert_eq!(catalog.tools.len(), 1);
        assert_eq!(catalog.rejected.len(), 1);
        assert!(
            catalog.rejected[0]
                .contains("rejected: /inputSchema/$schema: unsupported schema dialect")
        );
        let mut builder = CatalogBuilder::new(true, false);
        builder.add_page(&json!({"tools":[invalid]}), 100).unwrap();
        let catalog = builder.finish();
        assert!(catalog.tools.is_empty());
        assert_eq!(catalog.rejected.len(), 1);
    }
}
