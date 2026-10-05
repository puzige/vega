use super::*;

#[test]
fn issue87_s14_pipeline_and_projection_share_strict_asset_decoder() {
    let path = "assets/pixel.png";
    let input = serde_json::json!({"name":"reviewer","path_bytes":path.len(),"path_sha256":format!("{:x}", Sha256::digest(path.as_bytes()))}).to_string();
    let metadata = serde_json::json!({"name":"reviewer","path":path,"type":"unknown","size_bytes":42,"lower_trust":true});
    let approval = ApprovalAudit {
        decision: Approval::Once,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
        note: None,
    };
    let output = format!("[Lower-trust Skill asset metadata]\n{metadata}");
    assert!(valid_skill_result(
        "read_skill_resource",
        &input,
        &output,
        RuntimeToolStatus::Success,
        &approval
    ));
    for field in ["text", "content_sha256", "base64", "image", "handle"] {
        let mut forged = metadata.clone();
        forged[field] = serde_json::json!("PRIVATE BODY");
        assert!(!valid_skill_result(
            "read_skill_resource",
            &input,
            &format!("[Lower-trust Skill asset metadata]\n{forged}"),
            RuntimeToolStatus::Success,
            &approval
        ));
    }
    for (field, value) in [
        ("name", "\"reviewer\""),
        ("type", "\"unknown\""),
        ("size_bytes", "42"),
    ] {
        let member = format!("\"{field}\":{value}");
        let duplicated = output.replace(&member, &format!("{member},{member}"));
        assert!(!valid_skill_result(
            "read_skill_resource",
            &input,
            &duplicated,
            RuntimeToolStatus::Success,
            &approval
        ));
    }
    assert!(!valid_skill_result(
        "load_skill",
        &input,
        &output,
        RuntimeToolStatus::Success,
        &approval
    ));
    assert!(!valid_skill_result(
        "read_skill_resource",
        &input,
        &output,
        RuntimeToolStatus::Failed,
        &approval
    ));
    let wrong = ApprovalAudit {
        source: ApprovalSource::FullAccess,
        ..approval
    };
    assert!(!valid_skill_result(
        "read_skill_resource",
        &input,
        &output,
        RuntimeToolStatus::Success,
        &wrong
    ));
}
