use super::*;

fn observations() -> Vec<ScriptStep> {
    batch(&[
        (
            "a",
            "ComputerObserve",
            json!({"target_id":"fixture-2","cursor":null}),
        ),
        (
            "b",
            "ComputerObserve",
            json!({"target_id":"fixture-3","cursor":null}),
        ),
    ])
}

fn screenshots() -> Vec<ScriptStep> {
    batch(&[
        (
            "shot-a",
            "ComputerScreenshot",
            json!({"target_id":"fixture-2"}),
        ),
        (
            "shot-b",
            "ComputerScreenshot",
            json!({"target_id":"fixture-3"}),
        ),
    ])
}

fn screenshot_target(message: &ChatMessage) -> Value {
    serde_json::from_str(
        message
            .content
            .lines()
            .find_map(|line| line.strip_prefix("Screenshot target: "))
            .unwrap(),
    )
    .unwrap()
}
