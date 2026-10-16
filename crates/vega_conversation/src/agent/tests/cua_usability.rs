use super::*;
use vega_runtime::computer_use::{ComputerUseControl, test_support};

fn picture() -> crate::types::ImageAttachment {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(4, 3)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    crate::types::ImageAttachment::from_bytes(bytes.into_inner()).unwrap()
}

fn prepare(store: &Store) -> (ComputerUseControl, u64) {
    store.conn().execute_batch("INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at) VALUES ('capture-owner','thread-1',1,'assistant','text','Screenshot attached.','streaming',1);").unwrap();
    tool_calls::insert(store.conn(), tool_calls::NewToolCall { id:"capture", thread_id:"thread-1", message_id:"capture-owner", seq:1, tool:"ComputerScreenshot", input_json:&serde_json::json!({"kind":"computer_use","version":1,"action":"screenshot","argument_bytes":2,"argument_digest":"a".repeat(64)}).to_string(), status:"running", created_at:1 }).unwrap();
    let control = test_support::connected_desktop().unwrap();
    let epoch = control.status().epoch;
    (control, epoch)
}

fn persist(store: &Store, project: &str, event: &RuntimeEvent) -> Result<(), VegaError> {
    persist_runtime_event(
        store,
        project,
        "thread-1",
        "capture-owner",
        "mock-model",
        false,
        "Screenshot attached.",
        &mut 2,
        event,
        None,
    )
}
