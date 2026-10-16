use super::*;

struct ResumeProvider {
    inner: MockProvider,
    run: ComputerUseRun,
    pause_round: usize,
    paused: Option<Arc<tokio::sync::Notify>>,
    release: Option<Arc<tokio::sync::Notify>>,
}

fn fresh_windows() -> Vec<DesktopTarget> {
    [2, 3]
        .into_iter()
        .map(|id| {
            let mut target = window(id);
            target.selection_id = format!("fresh-{id}");
            target
        })
        .collect()
}

fn refresh() -> Vec<ScriptStep> {
    batch(&[(
        "refresh",
        "ComputerObserve",
        json!({"target_id":null,"cursor":null}),
    )])
}

impl Provider for ResumeProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        let pause = self.inner.requests().len() == self.pause_round;
        let future = self.inner.chat_stream(request, cancel);
        let run = self.run.clone();
        let paused = self.paused.clone();
        let release = self.release.clone();
        Box::pin(async move {
            if pause {
                run.control.pause();
                run.control.finish_cleanup();
                run.set_desktop_targets_fixture(fresh_windows()).await;
                if let (Some(paused), Some(release)) = (paused, release) {
                    paused.notify_one();
                    release.notified().await;
                } else {
                    run.control.resume(run.control.status().epoch).unwrap();
                }
            }
            future.await
        })
    }
}

struct ActResumeProvider {
    run: ComputerUseRun,
    mode: &'static str,
    requests: Mutex<Vec<ChatRequest>>,
}

impl Provider for ActResumeProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len();
        requests.push(request.clone());
        drop(requests);
        let script = match index {
            0 => directory(),
            1 => batch(&[(
                "observe",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            )]),
            2 => {
                let observation = result(&request, "observe")
                    .content
                    .lines()
                    .find_map(|line| line.strip_prefix("observation_id: "))
                    .unwrap();
                let mut calls = vec![(
                    "action",
                    "ComputerAct",
                    json!({"observation_id":observation,"action":"type_text","target":"edit-2","text":"ONCE"}),
                )];
                if self.mode != "unknown" {
                    calls.push((
                        "old-shot",
                        "ComputerScreenshot",
                        json!({"target_id":"fixture-2"}),
                    ));
                }
                if self.mode == "prepared" {
                    self.run.control.pause();
                    self.run.control.finish_cleanup();
                    self.run
                        .control
                        .resume(self.run.control.status().epoch)
                        .unwrap();
                }
                batch(&calls)
            }
            3 => refresh(),
            4 => batch(&[(
                "fresh",
                "ComputerScreenshot",
                json!({"target_id":"fixture-2"}),
            )]),
            _ => finish(),
        };
        MockProvider::new(script).chat_stream(request, cancel)
    }
}
