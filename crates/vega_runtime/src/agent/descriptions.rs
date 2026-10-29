use super::*;

pub fn is_native_tool(tool: &str) -> bool {
    matches!(
        tool,
        "Read"
            | "read"
            | "glob"
            | "grep"
            | "Write"
            | "write"
            | "Edit"
            | "edit"
            | "bash"
            | USER_QUESTION_TOOL
            | "load_skill"
            | "read_skill_resource"
    ) || is_subagent_tool(tool)
        || automation::is_tool(tool)
        || crate::computer_use::ComputerUseRun::tool_capability(tool).is_some()
}

pub(crate) fn describe_definition(mut definition: ToolDefinition) -> ToolDefinition {
    if !is_native_tool(&definition.name) {
        return definition;
    }
    if let Some(properties) = definition.input_schema["properties"].as_object_mut() {
        properties.insert("description".into(), vega_tools::description_field_schema());
    }
    if definition.strict
        && let Some(schema) = definition.input_schema.as_object_mut()
        && let Some(required) = schema
            .entry("required")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
        && !required.iter().any(|field| field == "description")
    {
        required.push(Value::String("description".into()));
    }
    definition
}

pub(crate) fn extract_call_description(
    call: &mut RuntimeToolCall,
    config: &RuntimeToolConfig,
) -> Result<(), VegaError> {
    if !is_native_tool(&call.name) {
        return Ok(());
    }
    if let Ok((input_json, description)) = vega_tools::split_tool_description(&call.input_json) {
        call.input_json = input_json;
        call.description = description
            .map(|text| config.redact_tool_output(&text))
            .transpose()?
            .and_then(|text| vega_tools::tool_description_title(&text));
    }
    Ok(())
}
