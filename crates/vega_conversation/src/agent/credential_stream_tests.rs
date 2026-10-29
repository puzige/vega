use super::protected_stream;
use futures::StreamExt;
use vega_runtime::{EventStream, ProviderEvent};

fn delta(channel: usize, text: &str) -> ProviderEvent {
    match channel {
        0 => ProviderEvent::TextDelta(text.into()),
        1 => ProviderEvent::ThinkingDelta(text.into()),
        _ => ProviderEvent::SummaryDelta(text.into()),
    }
}

async fn project(parts: Vec<ProviderEvent>, known: Vec<String>) -> [String; 3] {
    let source: EventStream = Box::pin(futures::stream::iter(parts.into_iter().map(Ok)));
    let mut stream = protected_stream(source, known, None);
    let mut output = [String::new(), String::new(), String::new()];
    while let Some(event) = stream.next().await {
        match event.expect("synthetic stream succeeds") {
            ProviderEvent::TextDelta(text) => output[0].push_str(&text),
            ProviderEvent::ThinkingDelta(text) => output[1].push_str(&text),
            ProviderEvent::SummaryDelta(text) => output[2].push_str(&text),
            _ => {}
        }
    }
    output
}

#[tokio::test]
async fn arbitrary_splits_preserve_paths_and_redact_complete_credentials() {
    let marker = vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER;
    let route = "GET /workspaces/:workspaceId/threads/:threadId/task-plan/snapshot\n";
    let secret = concat!("sk-", "Z8mQ2vR7pL4n", "B9xT6cH3");
    let long_secret = format!("sk-{}", "Z8mQ2vR7pL4nB9xT6cH3".repeat(16));
    let jwt = concat!(
        "eyJhbGci",
        "OiJIUzI1NiJ9.",
        "eyJzdWIiOiJzeW50aGV0aWMifQ.",
        "U3ludGhldGljU2lnbmF0dXJl"
    );
    let known = "owner value;with spaces Q7mZ9pL2";
    let cases = vec![
        (route.to_string(), route.to_string(), vec![]),
        (
            "任务 task-Z8mQ2vR7pL4nB9xT6cH3 done\n".into(),
            "任务 task-Z8mQ2vR7pL4nB9xT6cH3 done\n".into(),
            vec![],
        ),
        (
            format!("value {secret} done\n"),
            format!("value {marker} done\n"),
            vec![],
        ),
        (
            format!("value {long_secret}\n"),
            format!("value {marker}\n"),
            vec![],
        ),
        (
            concat!("Authorization: Bearer ", "Z8mQ2vR7pL4n", "B9xT6cH3\n").into(),
            format!("Authorization: Bearer {marker}\n"),
            vec![],
        ),
        (
            "api_key=Z8mQ2vR7pL4nB9xT6cH3\n".into(),
            format!("api_key={marker}\n"),
            vec![],
        ),
        (format!("jwt {jwt}\n"), format!("jwt {marker}\n"), vec![]),
        (
            format!("before {known} after\n"),
            format!("before {marker} after\n"),
            vec![known.into()],
        ),
        (
            format!("{secret}。中文\n"),
            format!("{marker}。中文\n"),
            vec![],
        ),
    ];
    for (input, expected, known) in cases {
        for channel in 0..3 {
            for split in input
                .char_indices()
                .map(|(index, _)| index)
                .chain([input.len()])
            {
                let output = project(
                    vec![
                        delta(channel, &input[..split]),
                        delta(channel, &input[split..]),
                    ],
                    known.clone(),
                )
                .await;
                assert_eq!(
                    output[channel], expected,
                    "channel {channel}, split {split}"
                );
                for (other, text) in output.iter().enumerate() {
                    if other != channel {
                        assert!(text.is_empty());
                    }
                }
            }
            let output = project(
                input
                    .chars()
                    .map(|ch| delta(channel, &ch.to_string()))
                    .collect(),
                known.clone(),
            )
            .await;
            assert_eq!(output[channel], expected);
        }
    }
}

#[tokio::test]
async fn interleaved_channels_keep_independent_boundaries_and_flush_at_eof() {
    let output = project(
        vec![
            delta(0, "GET /ta"),
            delta(1, concat!("sk-", "Z8mQ2vR7pL4nB9")),
            delta(2, "safe ta"),
            delta(0, concat!("sk-", "plan/snapshot")),
            delta(2, concat!("sk-", "Z8mQ2vR7pL4nB9xT6cH3")),
            delta(1, "xT6cH3"),
        ],
        vec![],
    )
    .await;
    assert_eq!(output[0], "GET /task-plan/snapshot");
    assert_eq!(
        output[1],
        vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER
    );
    assert_eq!(output[2], "safe task-Z8mQ2vR7pL4nB9xT6cH3");
}
