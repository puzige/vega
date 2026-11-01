use super::*;

fn receipt(state: &str) -> Value {
    serde_json::json!({"handle":"01ARZ3NDEKTSV4RRFFQ69G5FAV","operation_id":"01ARZ3NDEKTSV4RRFFQ69G5FAW","state":state,"exit_code":null,"output":"safe output","next_cursor":11,"retained_from":0,"output_gap":false,"truncated":false,"duration_ms":15,"error":null,"cleanup_safe":false})
}

fn approval(source: ApprovalSource) -> ApprovalAudit {
    ApprovalAudit { decision:Approval::Once,note:None,source,danger:None }
}

#[test]
fn running_receipt_is_valid_without_turning_legacy_success_into_running() {
    let output=receipt("running").to_string();
    let audit=approval(ApprovalSource::Auto);
    assert_eq!(validate_projection("bash",r#"{"command":"check","timeout":120,"wait_ms":0}"#,&output,RuntimeToolStatus::Success,&audit,None,Some(15)),Some(true));
    assert_eq!(validate_projection("bash",r#"{"command":"check","timeout":120}"#,&output,RuntimeToolStatus::Success,&audit,None,Some(15)),None);
    assert_eq!(validate_projection("bash",r#"{"command":"check","timeout":120,"wait_ms":null}"#,&output,RuntimeToolStatus::Success,&audit,None,Some(15)),None);
    let mut invalid=receipt("running");invalid["exit_code"]=serde_json::json!(0);
    assert_eq!(validate_projection("bash_wait",r#"{"handle":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}"#,&invalid.to_string(),RuntimeToolStatus::Success,&approval(ApprovalSource::ReadonlyTool),Some(0),Some(15)),Some(false));
}

#[test]
fn terminal_receipt_requires_matching_cleanup_identity_and_status() {
    let input=r#"{"handle":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}"#;
    let audit=approval(ApprovalSource::ReadonlyTool);
    let mut done=receipt("succeeded");done["exit_code"]=serde_json::json!(0);done["cleanup_safe"]=serde_json::json!(true);
    assert_eq!(validate_projection("bash_wait",input,&done.to_string(),RuntimeToolStatus::Success,&audit,Some(0),Some(15)),Some(true));
    done["cleanup_safe"]=serde_json::json!(false);
    assert_eq!(validate_projection("bash_wait",input,&done.to_string(),RuntimeToolStatus::Success,&audit,Some(0),Some(15)),Some(false));
    let mut unknown=receipt("outcome_unknown");unknown["error"]=serde_json::json!("termination_failed");
    assert_eq!(validate_projection("bash_cancel",input,&unknown.to_string(),RuntimeToolStatus::Failed,&audit,None,Some(15)),Some(true));
    assert_eq!(validate_projection("bash_cancel",input,&unknown.to_string(),RuntimeToolStatus::Success,&audit,None,Some(15)),Some(false));
    unknown["unexpected_authority"]=serde_json::json!(true);
    assert_eq!(validate_projection("bash_cancel",input,&unknown.to_string(),RuntimeToolStatus::Failed,&audit,None,Some(15)),Some(false));
}
