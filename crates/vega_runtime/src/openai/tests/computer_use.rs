use super::*;

fn observation_image() -> crate::ImageAttachment {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(4, 3)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    crate::ImageAttachment::from_bytes(bytes.into_inner()).unwrap()
}

fn observed_request() -> ChatRequest {
    let calls = ["a", "b"].map(|id| ChatToolCall {
        id: id.into(),
        name: "ComputerObserve".into(),
        input_json: "{}".into(),
    });
    let mut first = ChatMessage::tool_result("a", "window observation");
    first.images.push(observation_image());
    ChatRequest {
        model: MODEL.into(),
        messages: vec![
            ChatMessage::assistant_with_tools("", calls.into()),
            first,
            ChatMessage::tool_result("b", "second result"),
        ],
        ..Default::default()
    }
}

#[test]
fn issue75_provider_chat_observation_follows_complete_tool_batch() {
    let request = observed_request();
    let wire = build_request_body(&request);
    let messages = wire["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[1]["role"], "tool");
    assert_eq!(messages[2]["role"], "tool");
    assert_eq!(messages[3]["role"], "user");
    let content = messages[3]["content"].as_array().unwrap();
    assert!(
        content[0]["text"]
            .as_str()
            .unwrap()
            .contains("external tool observation")
    );
    assert!(content[0]["text"].as_str().unwrap().contains("a"));
    assert_eq!(content[1]["type"], "image_url");
    assert_eq!(request.messages.len(), 3);
    assert_eq!(request.messages[1].role, ChatRole::Tool);
}

#[test]
fn issue75_provider_responses_observation_retains_tool_identity() {
    let wire = responses::build_body(&observed_request()).unwrap();
    let item = &wire["input"][2];
    assert_eq!(item["type"], "function_call_output");
    assert_eq!(item["call_id"], "a");
    assert_eq!(item["output"][0]["type"], "input_text");
    assert_eq!(item["output"][1]["type"], "input_image");
    assert_eq!(item["output"][1]["detail"], "auto");
}
