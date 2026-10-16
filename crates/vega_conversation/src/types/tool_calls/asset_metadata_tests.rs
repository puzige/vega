use super::*;
use serde_json::{Value, json};

fn fixture() -> (String, Value, String) {
    let path = "assets/pixel.png";
    let input = json!({"name":"reviewer","path_bytes":path.len(),"path_sha256":format!("{:x}", Sha256::digest(path.as_bytes()))}).to_string();
    let value =
        json!({"name":"reviewer","path":path,"type":"unknown","size_bytes":42,"lower_trust":true});
    (
        input,
        value.clone(),
        format!("[Lower-trust Skill asset metadata]\n{value}"),
    )
}

fn projection(input: &str, output: &str) -> ToolCardResultProjection {
    let call = ToolCall {
        id: "asset".into(),
        tool: "read_skill_resource".into(),
        input_json: input.into(),
    };
    let input = tool_card_input_projection(&call);
    tool_card_result_projection(
        Some(&input),
        &ToolResult {
            status: ToolCallStatus::Success,
            output: output.into(),
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: Some(false),
            invalid: None,
        },
    )
}
