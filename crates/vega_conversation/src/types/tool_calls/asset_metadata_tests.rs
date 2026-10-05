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

#[test]
fn issue87_s14_asset_projection_strictly_accepts_only_validated_metadata() {
    let (input, value, output) = fixture();
    assert!(matches!(
        projection(&input, &output),
        ToolCardResultProjection::Skill {
            outcome: SkillCardOutcome::AssetMetadata { size_bytes: 42 },
            ..
        }
    ));
    for scenario in 0..15 {
        let mut changed = value.clone();
        match scenario {
            0 => changed["text"] = json!("PRIVATE BODY"),
            1 => changed["content_sha256"] = json!("0".repeat(64)),
            2 => changed["base64"] = json!("PRIVATE BODY"),
            3 => changed["name"] = json!("other"),
            4 => changed["path"] = json!("assets/../outside"),
            5 => changed["path"] = json!("/assets/pixel.png"),
            6 => changed["path"] = json!("assets/./pixel.png"),
            7 => changed["path"] = json!("assets//pixel.png"),
            8 => changed["path"] = json!("references/pixel.png"),
            9 => changed["type"] = json!("image/png"),
            10 => changed["lower_trust"] = json!(false),
            11 => changed["size_bytes"] = json!(-1),
            12 => changed["size_bytes"] = json!(1.5),
            13 => {
                changed.as_object_mut().unwrap().remove("size_bytes");
            }
            _ => changed["private"] = json!("PRIVATE BODY"),
        }
        let forged = format!("[Lower-trust Skill asset metadata]\n{changed}");
        assert!(
            matches!(
                projection(&input, &forged),
                ToolCardResultProjection::Corrupt
            ),
            "{scenario}"
        );
    }
    for forged in [
        output.replace("42", "18446744073709551616"),
        output.replace(
            "\"type\":\"unknown\"",
            "\"type\":\"unknown\",\"type\":\"unknown\"",
        ),
        output.replace(
            "\"name\":\"reviewer\"",
            "\"name\":\"reviewer\",\"name\":\"reviewer\"",
        ),
        output.replace("\"size_bytes\":42", "\"size_bytes\":42,\"size_bytes\":42"),
        format!("{output}{}", " ".repeat(8192)),
    ] {
        assert!(matches!(
            projection(&input, &forged),
            ToolCardResultProjection::Corrupt
        ));
    }
    for path in [
        "assets/pixel\0.png".to_string(),
        format!("assets/{}", "a".repeat(1018)),
    ] {
        let matching = json!({"name":"reviewer","path_bytes":path.len(),"path_sha256":format!("{:x}", Sha256::digest(path.as_bytes()))}).to_string();
        let mut forged = value.clone();
        forged["path"] = json!(path);
        assert!(matches!(
            projection(
                &matching,
                &format!("[Lower-trust Skill asset metadata]\n{forged}")
            ),
            ToolCardResultProjection::Corrupt
        ));
    }
    let mut wrong_input: Value = serde_json::from_str(&input).unwrap();
    wrong_input["path_bytes"] = json!(1);
    assert!(matches!(
        projection(&wrong_input.to_string(), &output),
        ToolCardResultProjection::Corrupt
    ));
    wrong_input["path_bytes"] = json!("assets/pixel.png".len());
    wrong_input["path_sha256"] = json!("0".repeat(64));
    assert!(matches!(
        projection(&wrong_input.to_string(), &output),
        ToolCardResultProjection::Corrupt
    ));
}
