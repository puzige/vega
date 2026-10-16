use super::*;

struct SteeringFixture(Mutex<Option<RuntimeSteeringInput>>);

impl RuntimeSteeringSource for SteeringFixture {
    fn take(&self) -> BoxFuture<'_, Result<Option<RuntimeSteeringInput>, VegaError>> {
        async move { Ok(self.0.lock().unwrap().take()) }.boxed()
    }
}

fn steering() -> Arc<SteeringFixture> {
    Arc::new(SteeringFixture(Mutex::new(Some(RuntimeSteeringInput {
        original_content: "follow-up".into(),
        user_message_seq: 2,
        id: "pending-b".into(),
        assistant_message_id: "assistant-b".into(),
        message: ChatMessage::new(ChatRole::User, "follow-up"),
    }))))
}
