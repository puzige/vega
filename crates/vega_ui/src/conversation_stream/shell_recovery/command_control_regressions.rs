use super::*;
use crate::tool_card::{ToolCard, shell_control_mounted_regressions as controls};
use gpui_kit::*;
use std::{collections::VecDeque, time::Instant};
use vega_conversation::types::{
    ShellCommandProjection, ToolCallStatus, ToolCardInputProjection, ToolCardResultProjection,
};

struct Root {
    stream: Entity<ConversationStream>,
    card: Entity<ToolCard>,
    mounted: bool,
    requests: VecDeque<ShellRecoveryRequested>,
    _events: Subscription,
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(
            self.mounted
                .then(|| ToolCard::render(self.card.clone(), false, window, cx)),
        )
    }
}

struct Fixture {
    visual: VisualTestContext,
    root: Entity<Root>,
    stream: Entity<ConversationStream>,
    card: Entity<ToolCard>,
    command: ShellCommandProjection,
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
        let thread =
            vega_conversation::threads::draft_thread(Some("control-project"), "fixture", "confirm");
        let command = ShellCommandProjection {
            operation_id: "original-operation".into(),
            call_id: "original-call".into(),
            thread_id: thread.id.clone(),
            command: "printf control".into(),
            state: "running".into(),
            exit_code: None,
            duration_ms: None,
            output_incomplete: false,
            concurrent: true,
            can_cancel: true,
            can_retry: false,
            cleanup_unresolved: false,
        };
        let initial = command.clone();
        let window = cx.open_window(size(px(360.), px(240.)), move |_, cx| {
            let stream = cx.new(|cx| ConversationStream::new(thread, cx));
            let events = cx.subscribe(
                &stream,
                |root: &mut Root, _, request: &ShellRecoveryRequested, _| {
                    root.requests.push_back(request.clone());
                },
            );
            let card = stream.update(cx, |stream, cx| {
                let mut card = ToolCard::hydrated(
                    Some(ToolCardInputProjection::Bash {
                        command: "printf control".into(),
                    }),
                    ToolCallStatus::Success,
                    None,
                    Some(ToolCardResultProjection::Bash {
                        status: ToolCallStatus::Success,
                        output: String::new(),
                        exit_code: None,
                        duration_ms: None,
                        truncated: None,
                        reused: false,
                        session_state: Some("running".into()),
                        operation_id: Some("original-operation".into()),
                        output_incomplete: false,
                    }),
                    None,
                );
                assert!(card.apply_shell_command(&initial));
                stream.push_tool_card(initial.call_id.clone(), card, cx);
                stream.tool_cards[&initial.call_id].clone()
            });
            cx.observe(&card, |_, _, cx| cx.notify()).detach();
            Root {
                stream,
                card,
                mounted: true,
                requests: VecDeque::new(),
                _events: events,
            }
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let (root, stream, card) = visual.update(|window, cx| {
            window.activate_window();
            let root = window.root::<Root>().flatten().unwrap();
            let state = root.read(cx);
            (root.clone(), state.stream.clone(), state.card.clone())
        });
        Self {
            visual,
            root,
            stream,
            card,
            command,
        }
    }

    fn drain(&mut self) {
        let start = Instant::now();
        for _ in 0..512 {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "command control drain deadline"
            );
            if !self.visual.cx.background_executor.tick() {
                return;
            }
        }
        panic!("command control scheduler did not quiesce");
    }

    fn frame(&mut self) {
        self.drain();
        self.visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.drain();
    }

    fn diagnostic(&mut self) -> String {
        let host = self.visual.debug_bounds("shell-command-native-host");
        let card = self.card.clone();
        let stream = self.stream.clone();
        let root = self.root.clone();
        let state = self.visual.update(|_, cx| {
            let stream = stream.read(cx);
            let recovery = &stream.shell_recovery;
            format!(
                "root_mounted={} requests={} source_thread={} original_call_owned={} recovery_sequence={} recovery_pending={:?} recovery_pending_read={} {}",
                root.read(cx).mounted,
                root.read(cx).requests.len(),
                stream.thread.id,
                stream.tool_cards.get("original-call") == Some(&card),
                recovery.sequence,
                recovery.pending,
                recovery.pending_read,
                controls::diagnostic(card.read(cx), cx),
            )
        });
        format!("host={host:?} {state}")
    }

    fn ready(&mut self, phase: &str) -> controls::Observed {
        let start = Instant::now();
        loop {
            self.frame();
            let card = self.card.clone();
            if let Some(observed) = self
                .visual
                .update(|_, cx| controls::observe(card.read(cx), cx))
            {
                return observed;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "mounted command control deadline phase={phase}: {}",
                self.diagnostic(),
            );
            std::thread::sleep(Duration::from_millis(10));
            self.visual
                .cx
                .background_executor
                .advance_clock(Duration::from_millis(10));
        }
    }

    fn click(&mut self, phase: &str, observed: &controls::Observed) {
        let host = self
            .visual
            .debug_bounds("shell-command-native-host")
            .unwrap();
        let position = observed.position(host);
        self.visual.update(|window, cx| {
            window.dispatch_event(
                MouseMoveEvent {
                    position,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                MouseDownEvent {
                    position,
                    button: MouseButton::Left,
                    click_count: 1,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                MouseUpEvent {
                    position,
                    button: MouseButton::Left,
                    click_count: 1,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
        });
        let start = Instant::now();
        loop {
            self.frame();
            let card = self.card.clone();
            if self
                .visual
                .update(|_, cx| controls::changed(card.read(cx), observed, cx))
            {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "native command press acknowledgement deadline phase={phase}: {}",
                self.diagnostic(),
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn request(&mut self, action: ShellRecoveryAction) -> ShellRecoveryRequested {
        let stream = self.stream.clone();
        self.visual.update(|_, cx| {
            stream.update(cx, |stream, cx| stream.request_shell_recovery(action, cx))
        });
        self.take()
    }

    fn take(&mut self) -> ShellRecoveryRequested {
        self.drain();
        let root = self.root.clone();
        self.visual.update(|_, cx| {
            root.update(cx, |root, _| {
                root.requests
                    .pop_front()
                    .expect("production recovery dispatch")
            })
        })
    }

    fn no_requests(&mut self) {
        self.drain();
        let root = self.root.clone();
        self.visual
            .update(|_, cx| assert!(root.read(cx).requests.is_empty()));
    }

    fn projection(&self) -> ShellRecoveryProjection {
        ShellRecoveryProjection {
            workspace: "/workspace".into(),
            epoch: vega_store::shell_operations::ShellRecoveryVersion {
                epoch: 1,
                scopes: Vec::new(),
            },
            rows: Vec::new(),
            active: Vec::new(),
            commands: vec![self.command.clone()],
        }
    }

    fn finish(
        &mut self,
        request: &ShellRecoveryRequested,
        result: Result<ShellRecoveryOutcome, ShellRecoveryError>,
    ) {
        let stream = self.stream.clone();
        self.visual.update(|_, cx| {
            stream.update(cx, |stream, cx| {
                stream.finish_shell_recovery(request, result, cx)
            })
        });
    }

    fn pending(&mut self, pending: bool, failed: bool) {
        let card = self.card.clone();
        self.visual
            .update(|_, cx| controls::assert_pending(card.read(cx), pending, failed));
    }

    fn reject(&mut self, observed: &controls::Observed) {
        let card = self.card.clone();
        self.visual
            .update(|_, cx| card.update(cx, |card, cx| controls::reject(card, observed, cx)));
        self.no_requests();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let card = self.card.clone();
        self.visual.update(|window, cx| {
            card.update(cx, controls::stop);
            window.remove_window();
        });
    }
}

pub fn mounted_cancel_supersedes_read_and_retry_keeps_operation_authority(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let initial = fixture.ready("initial Cancel");
    initial.assert_control("ui.shell_session.cancel", true);
    let card = fixture.card.clone();
    fixture.visual.update(|_, cx| {
        card.update(cx, |card, cx| {
            controls::reject_wrong_operation(card, &initial, cx)
        })
    });
    fixture.no_requests();
    let read = fixture.request(ShellRecoveryAction::Read);
    fixture.click("Cancel supersedes Read", &initial);
    let cancel = fixture.take();
    assert!(cancel.sequence > read.sequence);
    assert!(
        matches!(&cancel.action, ShellRecoveryAction::Cancel(operation) if operation == "original-operation")
    );
    fixture.pending(true, false);
    fixture
        .ready("pending Stopping")
        .assert_control("ui.shell_session.stopping", false);
    fixture.reject(&initial);
    fixture.finish(&read, Err(ShellRecoveryError::Unavailable));
    fixture.pending(true, false);
    fixture.finish(&cancel, Err(ShellRecoveryError::Unavailable));
    fixture.pending(false, true);
    fixture.ready("Cancel failure visible").assert_failure(true);

    let read = fixture.request(ShellRecoveryAction::Read);
    fixture.command.cleanup_unresolved = true;
    fixture.command.can_cancel = false;
    fixture.command.can_retry = true;
    fixture.finish(
        &read,
        Ok(ShellRecoveryOutcome::Projection(fixture.projection())),
    );
    fixture.pending(false, true);
    let retry = fixture.ready("cleanup-unresolved Retry with failure");
    retry.assert_control("ui.shell_recovery.retry", true);
    retry.assert_failure(true);
    fixture.click("Retry dispatches original operation", &retry);
    let cleanup = fixture.take();
    assert!(
        matches!(&cleanup.action, ShellRecoveryAction::Cancel(operation) if operation == "original-operation")
    );
    fixture.finish(
        &cleanup,
        Ok(ShellRecoveryOutcome::Projection(fixture.projection())),
    );
    fixture.pending(false, false);

    let retry = fixture.ready("Retry after completion");
    let mutation = fixture.request(ShellRecoveryAction::Check(fixture.projection()));
    fixture.click("pending mutation rejects Retry", &retry);
    fixture.no_requests();
    fixture.pending(false, true);
    fixture.finish(&mutation, Err(ShellRecoveryError::Unavailable));
    fixture.reject(&initial);

    let read = fixture.request(ShellRecoveryAction::Read);
    fixture.command.state = "succeeded".into();
    fixture.command.cleanup_unresolved = false;
    fixture.command.can_retry = false;
    fixture.finish(
        &read,
        Ok(ShellRecoveryOutcome::Projection(fixture.projection())),
    );
    fixture.pending(false, false);
    fixture.reject(&retry);

    for invalid in ["thread", "call"] {
        let read = fixture.request(ShellRecoveryAction::Read);
        fixture.command.state = "running".into();
        fixture.command.can_cancel = true;
        fixture.finish(
            &read,
            Ok(ShellRecoveryOutcome::Projection(fixture.projection())),
        );
        let displayed = fixture.ready(&format!("source rejection preparation {invalid}"));
        let stream = fixture.stream.clone();
        let original_thread = fixture.command.thread_id.clone();
        fixture.visual.update(|_, cx| {
            stream.update(cx, |stream, _| {
                if invalid == "thread" {
                    stream.thread.id = "different-thread".into();
                } else {
                    let card = stream.tool_cards.remove("original-call").unwrap();
                    stream.tool_cards.insert("different-call".into(), card);
                }
            })
        });
        fixture.click(&format!("source rejection {invalid}"), &displayed);
        fixture.no_requests();
        fixture.pending(false, true);
        fixture.visual.update(|_, cx| {
            stream.update(cx, |stream, _| {
                stream.thread.id = original_thread;
                if invalid == "call" {
                    let card = stream.tool_cards.remove("different-call").unwrap();
                    stream.tool_cards.insert("original-call".into(), card);
                }
            })
        });
    }

    let displayed = fixture.ready("capability loss preparation");
    let read = fixture.request(ShellRecoveryAction::Read);
    fixture.command.concurrent = false;
    fixture.finish(
        &read,
        Ok(ShellRecoveryOutcome::Projection(fixture.projection())),
    );
    fixture.reject(&displayed);
    fixture.command.concurrent = true;
    let read = fixture.request(ShellRecoveryAction::Read);
    fixture.finish(
        &read,
        Ok(ShellRecoveryOutcome::Projection(fixture.projection())),
    );
    let mounted = fixture.ready("unmount preparation");
    let root = fixture.root.clone();
    fixture.visual.update(|_, cx| {
        root.update(cx, |root, cx| {
            root.mounted = false;
            cx.notify();
        })
    });
    fixture.frame();
    fixture.reject(&mounted);
}
