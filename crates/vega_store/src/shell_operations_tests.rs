use super::*;

fn fixture() -> crate::Store {
    let store = crate::Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    store.conn().execute_batch("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('p','/workspace','project',0,0); INSERT INTO threads(id,project_id,model,created_at,updated_at) VALUES('t','p','model',0,0),('other','p','model',0,0); INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('r','t',1,'assistant','','streaming',0); INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,status,created_at) VALUES('c','t','r',1,'bash','{}','running',0);").unwrap();
    store
}

fn record_for(state: &str) -> ShellOperationRecord<'_> {
    ShellOperationRecord {operation_id:"op",thread_id:"t",run_id:"r",call_id:"c",generation:"generation",workspace:"/workspace",state,exit_code:None,error:None,duration_ms:None}
}

#[test]
fn records_require_starting_authority_and_keep_terminal_identity() {
    let store = fixture();
    assert!(record(store.conn(), &record_for("running"), 1).is_err());
    let mut invalid = record_for("prepared");
    invalid.thread_id = "other";
    assert!(record(store.conn(), &invalid, 1).is_err());
    record(store.conn(), &record_for("prepared"), 1).unwrap();
    record(store.conn(), &record_for("running"), 2).unwrap();
    let mut done = record_for("succeeded");
    done.exit_code = Some(0);
    done.duration_ms = Some(15);
    record(store.conn(), &done, 3).unwrap();
    record(store.conn(), &done, 4).unwrap();
    assert!(record(store.conn(), &record_for("cancelled"), 5).is_err());
    let actual:(String,Option<i32>,Option<i64>) = store.conn().query_row("SELECT state,exit_code,duration_ms FROM shell_operations WHERE operation_id='op'",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!(actual,("succeeded".into(),Some(0),Some(15)));
}

#[test]
fn recovery_preserves_unknown_outcome_without_replaying_or_rebinding() {
    let store = fixture();
    record(store.conn(), &record_for("prepared"), 1).unwrap();
    record(store.conn(), &record_for("running"), 2).unwrap();
    assert!(!has_unknown_workspace(store.conn(),"/workspace").unwrap());
    assert_eq!(recover_thread(store.conn(),"other",3).unwrap(),0);
    assert_eq!(recover_thread(store.conn(),"t",3).unwrap(),1);
    assert_eq!(recover_thread(store.conn(),"t",4).unwrap(),0);
    assert!(has_unknown_workspace(store.conn(),"/workspace").unwrap());
    assert!(!has_unknown_workspace(store.conn(),"/unrelated").unwrap());
    assert!(record(store.conn(), &record_for("running"),5).is_err());
    let actual:(String,String,Option<i32>) = store.conn().query_row("SELECT state,error,exit_code FROM shell_operations WHERE operation_id='op'",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!(actual,("outcome_unknown".into(),"session_expired".into(),None));
}
