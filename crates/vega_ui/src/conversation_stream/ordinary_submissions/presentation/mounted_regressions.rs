use super::*;
use nsh_platform_gpui::IntegrationTextObservation;
use std::cell::RefCell;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use vega_conversation::types::RawSubmission;

pub use crate::conversation_stream::timestamps::mounted_regressions::timestamp_native_narrow_height_and_passive_input;

#[derive(Clone)]
struct NativeLayout {
    id: ProjectionId,
    action: Option<Action>,
    bounds: [f32; 4],
}

static LAYOUTS: Mutex<Vec<NativeLayout>> = Mutex::new(Vec::new());
static TEXTS: Mutex<Vec<(ProjectionId, String)>> = Mutex::new(Vec::new());

pub(super) fn record_text(id: &ProjectionId, text: &str, lines: &[nsh_core::TextLine]) {
    if lines.is_empty() {
        return;
    }
    let mut texts = TEXTS.lock().expect("native controls text");
    if !texts
        .iter()
        .any(|(old, value)| old.matches(id) && value == text)
    {
        texts.push((id.clone(), text.into()));
    }
}

pub(super) fn record_layout(
    id: &ProjectionId,
    action: Option<Action>,
    bounds: &nsh_core::geometry::Bounds<nsh_core::geometry::Points>,
) {
    let mut layouts = LAYOUTS.lock().expect("controls layout observation");
    layouts.retain(|layout| !layout.id.matches(id) || layout.action != action);
    layouts.push(NativeLayout {
        id: id.clone(),
        action,
        bounds: [
            bounds.origin.x.0,
            bounds.origin.y.0,
            bounds.size.width.0,
            bounds.size.height.0,
        ],
    });
}

fn current_texts(host: &Host, cx: &App) -> Option<Vec<IntegrationTextObservation>> {
    host.surface
        .as_ref()?
        .entity()
        .read(cx)
        .integration_text_observation()
}

fn expected_texts(snapshot: &Snapshot) -> Vec<&str> {
    snapshot
        .message
        .iter()
        .chain(snapshot.detail.iter())
        .chain(snapshot.buttons.iter().map(|(_, label)| label))
        .map(String::as_str)
        .collect()
}

fn text_ready(text: &IntegrationTextObservation) -> bool {
    [text.frame, text.content].iter().all(|bounds| {
        [
            bounds.origin.x.0,
            bounds.origin.y.0,
            bounds.size.width.0,
            bounds.size.height.0,
        ]
        .iter()
        .all(|value| value.is_finite())
            && bounds.size.width.0 > 0.0
            && bounds.size.height.0 > 0.0
    }) && !text.lines.is_empty()
        && text.lines.iter().all(|line| {
            line.width.0.is_finite()
                && line.width.0 >= 0.0
                && line.height.0.is_finite()
                && line.height.0 > 0.0
        })
        && text.lines.iter().any(|line| line.width.0 > 0.0)
}

fn texts_match(expected: &[&str], observed: Option<&[IntegrationTextObservation]>) -> bool {
    observed.is_some_and(|texts| {
        texts.len() == expected.len()
            && texts.iter().all(text_ready)
            && expected.iter().all(|label| {
                texts.iter().filter(|text| text.text == *label).count()
                    == expected.iter().filter(|value| *value == label).count()
            })
    })
}

struct Root {
    stream: Entity<ConversationStream>,
    visible: bool,
    width: f32,
    requests: Vec<OrdinaryActionRequested>,
    _events: Subscription,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(
            div()
                .w(px(self.width))
                .h_full()
                .flex()
                .when(self.visible, |root| root.child(self.stream.clone())),
        )
    }
}

struct Fixture {
    visual: VisualTestContext,
    root: Entity<Root>,
    stream: Entity<ConversationStream>,
    submission: OrdinarySubmission,
    hosts: Rc<RefCell<Vec<WeakEntity<Host>>>>,
    _observer: Subscription,
    _store: vega_store::Store,
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        LAYOUTS.lock().expect("clear layouts").clear();
        TEXTS.lock().expect("clear native text").clear();
        let store = vega_store::Store::open(":memory:").expect("in-memory SQLite");
        store.migrate().expect("SQLite migrations");
        let thread = vega_conversation::threads::draft_thread(None, "fixture", "confirm");
        let database = store.database_identity();
        let admission = vega_store::thread_admission::try_acquire(database, &thread.id)
            .expect("admission result")
            .expect("exclusive admission");
        let (submission, worker) = OrdinarySubmission::begin(
            database,
            thread.id.clone(),
            RawSubmission::new(
                "Mounted original ordinary message".into(),
                Vec::new(),
                Vec::new(),
                1,
                "fixture".into(),
                None,
                None,
            ),
            admission,
        )
        .expect("real ordinary submission");
        let attempt = worker.attempt_id();
        worker
            .complete(Err(SubmissionFailure::SpawnFailed))
            .expect("worker failure");
        submission.seal(attempt).expect("sealed first attempt");
        cx.update(|cx| {
            assert!(!cx.text_system().all_font_names().is_empty());
            cx.set_global(vega_theme::Theme::dark());
            cx.set_global(crate::settings::SettingsOpen(false));
            cx.set_global(crate::sidebar::OpenedThread(Some(thread.clone())));
        });
        let hosts = Rc::new(RefCell::new(Vec::new()));
        let observed = hosts.clone();
        let observer = cx.update(|cx| {
            cx.observe_new::<Host>(move |_, _, cx| observed.borrow_mut().push(cx.weak_entity()))
        });
        let projected = submission.clone();
        let window = cx.open_window(size(px(820.), px(780.)), move |_, cx| {
            let stream = cx.new(|cx| ConversationStream::new(thread, cx));
            stream.update(cx, |stream, cx| {
                stream.project_ordinary_submission(projected, cx)
            });
            let events = cx.subscribe(
                &stream,
                |root: &mut Root, _, event: &OrdinaryActionRequested, _| {
                    root.requests.push(OrdinaryActionRequested {
                        thread_id: event.thread_id.clone(),
                        submission: event.submission,
                        attempt: event.attempt,
                        action: event.action,
                    });
                },
            );
            Root {
                stream,
                visible: true,
                width: 820.,
                requests: Vec::new(),
                _events: events,
            }
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let (root, stream) = visual.update(|window, cx| {
            window.activate_window();
            let root = window
                .root::<Root>()
                .flatten()
                .expect("mounted controls root");
            let stream = root.read(cx).stream.clone();
            (root, stream)
        });
        let mut fixture = Self {
            visual,
            root,
            stream,
            submission,
            hosts,
            _observer: observer,
            _store: store,
        };
        fixture.ready();
        fixture
    }

    fn drain(&mut self) {
        let start = Instant::now();
        for tick in 0..512 {
            let worked = self.visual.cx.background_executor.tick();
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "controls drain deadline tick={tick}"
            );
            if !worked {
                return;
            }
        }
        panic!("controls scheduler did not quiesce in 512 ticks");
    }

    fn frame(&mut self) {
        self.drain();
        self.visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.drain();
    }

    fn wait(&mut self, reason: &str, mut ready: impl FnMut(&mut Self) -> bool) {
        let start = Instant::now();
        loop {
            self.frame();
            if ready(self) {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "controls timeout: {reason}\n{}",
                self.readiness_summary()
            );
            std::thread::sleep(Duration::from_millis(10));
            self.visual
                .cx
                .background_executor
                .advance_clock(Duration::from_millis(10));
        }
    }

    fn current(&mut self) -> Option<Entity<Host>> {
        let hosts = self.hosts.borrow().clone();
        let stream = self.stream.clone();
        self.visual.update(|_, cx| {
            hosts
                .into_iter()
                .rev()
                .filter_map(|host| host.upgrade())
                .find(|host| {
                    let host = host.read(cx);
                    host.owner == stream.downgrade() && host.lease.upgrade().is_some()
                })
        })
    }

    fn readiness_summary(&mut self) -> String {
        let bounds = self.visual.debug_bounds("nsh-ordinary-controls");
        let hosts = self.hosts.borrow().clone();
        let stream = self.stream.clone();
        self.visual.update(|window, cx| {
            let candidates: Vec<_> = hosts.into_iter().filter_map(|host| host.upgrade()).collect();
            let owned: Vec<_> = candidates
                .iter()
                .filter(|host| host.read(cx).owner == stream.downgrade())
                .collect();
            let current = owned
                .iter()
                .rev()
                .find(|host| host.read(cx).lease.upgrade().is_some())
                .or_else(|| owned.last());
            let mut summary = vec![format!(
                "readiness: hosts={} owned={} host_current={} bounds={bounds:?}",
                candidates.len(),
                owned.len(),
                current.is_some_and(|host| host.read(cx).lease.upgrade().is_some()),
            )];
            let Some(host) = current else {
                summary.push(
                    "geometry/displayed/valid/source/live_layout/native_height/text/buttons/surface_errors=unavailable(no owned host)"
                        .into(),
                );
                return summary.join("\n");
            };
            let host = host.read(cx);
            let stream = stream.read(cx);
            let live = host.lease.upgrade().and_then(|mount| mount.geometry.get());
            let geometry = |geometry: Geometry| {
                (
                    geometry.bounds,
                    geometry.visible,
                    geometry.viewport,
                    geometry.row,
                    geometry.foreground,
                )
            };
            let viewport = stream.list.viewport_bounds();
            let row = stream.list.bounds_for_item(host.snapshot.source.index);
            summary.push(format!(
                "displayed={} valid={} enabled={} source_current={} source_capture_matches={} submission_matches={} active={} width={} height={:?}",
                host.displayed,
                host.valid(&host.snapshot, window, cx),
                host.snapshot.enabled,
                host.snapshot.source.current(stream, cx),
                Source::capture(stream, host.snapshot.source.index, cx)
                    .is_some_and(|source| source.matches(&host.snapshot.source)),
                host.submission.id() == host.snapshot.source.submission,
                window.is_window_active(),
                host.snapshot.width,
                host.height,
            ));
            summary.push(format!(
                "geometry: snapshot={:?} live={:?} matches={} bounds_match={} viewport={viewport:?} row={row:?} foreground={:?}",
                host.snapshot.geometry.map(geometry),
                live.map(geometry),
                live.is_some() && host.snapshot.geometry == live,
                host.snapshot.geometry.is_some_and(|geometry| Some(geometry.bounds) == bounds),
                stream.foreground_height,
            ));
            summary.push(format!(
                "live_layout: visible={} viewport_current={} row_current={} foreground_current={} visible_in_viewport={} host_height_matches={}",
                live.is_some_and(|geometry| !geometry.visible.is_empty()),
                live.is_some_and(|geometry| geometry.viewport == viewport),
                live.is_some_and(|geometry| Some(geometry.row) == row),
                live.is_some_and(|geometry| geometry.foreground == stream.foreground_height),
                live.is_some_and(|geometry| {
                    geometry.visible.intersect(&geometry.viewport) == geometry.visible
                }),
                bounds.is_some_and(|bounds| host.height.is_some_and(|height| {
                    (height - f32::from(bounds.size.height)).abs() <= 0.5
                })),
            ));
            let layouts = LAYOUTS.lock().expect("diagnostic native layouts");
            let native = layouts.iter().find(|layout| {
                layout.id.matches(&host.snapshot.id) && layout.action.is_none()
            });
            let texts = current_texts(host, cx);
            let expected = expected_texts(&host.snapshot);
            let text_matches: Vec<_> = expected
                .iter()
                .map(|label| {
                    texts.as_ref().is_some_and(|texts| {
                        texts
                            .iter()
                            .filter(|text| text.text == *label && text_ready(text))
                            .count()
                            == expected.iter().filter(|value| *value == label).count()
                    })
                })
                .collect();
            let observations = texts.as_ref().map(|texts| {
                texts
                    .iter()
                    .take(16)
                    .map(|text| {
                        (
                            text.node,
                            text.text.chars().take(256).collect::<String>(),
                            text.lines.len(),
                            text.lines
                                .iter()
                                .take(4)
                                .map(|line| (line.width.0, line.height.0))
                                .collect::<Vec<_>>(),
                            text.frame,
                            text.content,
                            text_ready(text),
                        )
                    })
                    .collect::<Vec<_>>()
            });
            summary.push(format!(
                "current_native_text: available={} exact_multiplicity={} expected={expected:?} observations_first16_node_text256_linecount_metrics4_frame_content_ready={observations:?}",
                texts.is_some(),
                texts_match(&expected, texts.as_deref()),
            ));
            let historical_texts = TEXTS.lock().expect("historical text events");
            let button_matches: Vec<_> = host
                .snapshot
                .buttons
                .iter()
                .map(|(action, _)| {
                    (
                        *action,
                        layouts.iter().any(|layout| {
                            layout.id.matches(&host.snapshot.id) && layout.action == Some(*action)
                        }),
                    )
                })
                .collect();
            summary.push(format!(
                "native: root={:?} width_matches={} height_matches={} text_matches={text_matches:?} button_matches={button_matches:?} current_layouts={} current_texts={} total_layouts={} historical_text_events={}",
                native.map(|native| native.bounds),
                bounds.is_some_and(|bounds| native.is_some_and(|native| {
                    (native.bounds[2] - f32::from(bounds.size.width)).abs() <= 0.5
                })),
                bounds.is_some_and(|bounds| native.is_some_and(|native| {
                    (native.bounds[3] - f32::from(bounds.size.height)).abs() <= 0.5
                })),
                layouts
                    .iter()
                    .filter(|layout| layout.id.matches(&host.snapshot.id))
                    .count(),
                texts.as_ref().map_or(0, Vec::len),
                layouts.len(),
                historical_texts.len(),
            ));
            let error = host.surface.as_ref().map(|surface| {
                let errors = surface.errors();
                let error = errors.borrow();
                error
                    .as_ref()
                    .map(|error| error.chars().take(256).collect::<String>())
            });
            summary.push(format!(
                "surface_present={} surface_healthy={} surface_errors={error:?}",
                host.surface.is_some(),
                error.as_ref().is_some_and(|error| error.is_none()),
            ));
            summary.join("\n")
        })
    }

    fn ready(&mut self) {
        self.wait(
            "production NSH displayed/layout/host convergence",
            |fixture| {
                let Some(host) = fixture.current() else {
                    return false;
                };
                let Some(bounds) = fixture.visual.debug_bounds("nsh-ordinary-controls") else {
                    return false;
                };
                fixture.visual.update(|window, cx| {
                    let host = host.read(cx);
                    let Some(geometry) = host.snapshot.geometry else {
                        return false;
                    };
                    let layouts = LAYOUTS.lock().expect("native controls layouts");
                    let native = layouts.iter().find(|layout| {
                        layout.id.matches(&host.snapshot.id) && layout.action.is_none()
                    });
                    let texts = current_texts(host, cx);
                    let expected = expected_texts(&host.snapshot);
                    host.displayed
                        && texts_match(&expected, texts.as_deref())
                        && host.valid(&host.snapshot, window, cx)
                        && host.height.is_some_and(|height| {
                            (height - f32::from(bounds.size.height)).abs() <= 0.5
                        })
                        && native.is_some_and(|native| {
                            (native.bounds[2] - f32::from(bounds.size.width)).abs() <= 0.5
                                && (native.bounds[3] - f32::from(bounds.size.height)).abs() <= 0.5
                        })
                        && geometry.bounds == bounds
                        && host.snapshot.buttons.iter().all(|(action, _)| {
                            layouts.iter().any(|layout| {
                                layout.id.matches(&host.snapshot.id)
                                    && layout.action == Some(*action)
                            })
                        })
                })
            },
        );
        let host = self.current().expect("live production host");
        let stream = self.stream.clone();
        self.visual.update(|_, cx| {
            let host = host.read(cx);
            let source = Source::capture(stream.read(cx), host.snapshot.source.index, cx)
                .expect("original row source");
            assert!(source.matches(&host.snapshot.source));
            assert_eq!(source.submission, self.submission.id());
            assert_eq!(
                source.identity.message_id.as_deref(),
                Some(self.submission.user_message_id())
            );
            assert_eq!(
                host.snapshot.message.as_deref(),
                Some(vega_i18n::t("ui.ordinary_send.spawn_failed"))
            );
            assert_eq!(host.snapshot.buttons.len(), 2);
            assert_eq!(
                host.snapshot.buttons[0].0,
                Action::Submission(OrdinaryAction::Retry)
            );
            assert_eq!(host.snapshot.buttons[1].0, Action::Diagnostics);
        });
    }

    fn asserts_current_text_observation(&mut self) {
        let host = self.current().expect("current text host");
        self.visual.update(|_, cx| {
            let host = host.read(cx);
            let texts = current_texts(host, cx).expect("current attached native text");
            let expected = expected_texts(&host.snapshot);
            assert!(texts_match(&expected, Some(&texts)));
            assert!(!texts_match(&expected, None));
            assert!(!texts_match(&expected, Some(&[])));
            assert!(!texts_match(&expected, Some(&texts[1..])));
            let mut repeated = texts.clone();
            let different = texts
                .iter()
                .position(|text| text.text != repeated[0].text)
                .expect("distinct actual message and button text");
            repeated[different].text = repeated[0].text.clone();
            assert!(!texts_match(&expected, Some(&repeated)));
            let mut wrong = texts.clone();
            wrong[0].text.push_str(" incorrect current text");
            assert!(!texts_match(&expected, Some(&wrong)));
            let mut unlaid = texts.clone();
            unlaid[0].lines.clear();
            assert!(!texts_match(&expected, Some(&unlaid)));
            let mut invalid = texts.clone();
            invalid[0].frame.size.width.0 = f32::NAN;
            assert!(!texts_match(&expected, Some(&invalid)));
            invalid[0].frame.size.width.0 = 0.0;
            assert!(!texts_match(&expected, Some(&invalid)));
        });
    }

    fn input(&mut self, event: impl InputEvent) {
        self.visual.update(|window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
        self.drain();
    }

    fn click(&mut self, action: Action) {
        self.ready();
        let host = self.current().expect("click host");
        let point = self.visual.update(|_, cx| {
            let host = host.read(cx);
            let geometry = host.snapshot.geometry.expect("mounted geometry");
            let layouts = LAYOUTS.lock().expect("native button layouts");
            let root = layouts
                .iter()
                .find(|layout| layout.id.matches(&host.snapshot.id) && layout.action.is_none())
                .expect("native root");
            let button = layouts
                .iter()
                .find(|layout| {
                    layout.id.matches(&host.snapshot.id) && layout.action == Some(action)
                })
                .expect("native button");
            let point = geometry.bounds.origin
                + point(
                    px(button.bounds[0] - root.bounds[0] + button.bounds[2] / 2.),
                    px(button.bounds[1] - root.bounds[1] + button.bounds[3] / 2.),
                );
            assert!(
                geometry.visible.contains(&point),
                "real NSH button must be visible"
            );
            point
        });
        self.input(MouseMoveEvent {
            position: point,
            ..Default::default()
        });
        self.input(MouseDownEvent {
            position: point,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        });
        self.input(MouseUpEvent {
            position: point,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        });
    }

    fn event(&mut self, attempt: AttemptId) {
        self.wait("one original dispatch", |fixture| {
            let root = fixture.root.clone();
            fixture
                .visual
                .update(|_, cx| !root.read(cx).requests.is_empty())
        });
        let root = self.root.clone();
        self.visual.update(|_, cx| {
            root.update(cx, |root, cx| {
                assert_eq!(
                    root.requests.len(),
                    1,
                    "one activation must dispatch exactly once"
                );
                let request = root.requests.remove(0);
                assert_eq!(request.thread_id, root.stream.read(cx).thread.id);
                assert_eq!(request.submission, self.submission.id());
                assert_eq!(request.attempt, attempt);
                assert_eq!(request.action, OrdinaryAction::Retry);
            });
        });
    }

    fn capture(&mut self) -> (Entity<Host>, Snapshot) {
        self.ready();
        let host = self.current().expect("displayed host");
        let snapshot = self.visual.update(|_, cx| host.read(cx).snapshot.clone());
        (host, snapshot)
    }

    fn rejects(&mut self, host: &Entity<Host>, observed: &Snapshot) {
        self.visual.update(|window, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(
                "unchanged stale clipboard".into(),
            ));
            host.update(cx, |host, cx| {
                host.activate(
                    observed,
                    Action::Submission(OrdinaryAction::Retry),
                    false,
                    window,
                    cx,
                );
                host.activate(observed, Action::Diagnostics, false, window, cx);
            });
        });
        self.drain();
        let root = self.root.clone();
        self.visual.update(|_, cx| {
            assert!(
                root.read(cx).requests.is_empty(),
                "stale presentation dispatched"
            );
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some("unchanged stale clipboard".into())
            );
        });
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let hosts = self.hosts.borrow().clone();
        let window = self.visual.window_handle();
        let cleanup = self.visual.cx.update_window(window, |_, window, cx| {
            for host in hosts.into_iter().filter_map(|host| host.upgrade()) {
                host.update(cx, |host, cx| host.stop(cx));
            }
            window.remove_window();
        });
        if let Err(error) = cleanup {
            if std::thread::panicking() {
                eprintln!("controls fixture cleanup during unwind: {error:#}");
            } else {
                panic!("controls fixture cleanup: {error:#}");
            }
        } else if !std::thread::panicking() {
            self.drain();
        }
    }
}

pub fn production_buttons_pass_pointer_through_full_inset_wrappers_and_keep_focus_dispatch(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx);
    fixture.asserts_current_text_observation();
    let attempt = fixture
        .submission
        .snapshot()
        .attempts
        .last()
        .expect("attempt")
        .id;
    fixture.click(Action::Submission(OrdinaryAction::Retry));
    fixture.event(attempt);
    fixture.ready();
    let host = fixture.current().expect("focused host");
    fixture.visual.update(|window, cx| {
        assert!(host.read(cx).focus.is_focused(window));
        window.focus_next(cx);
        assert!(
            host.read(cx).diagnostic_focus.is_focused(window),
            "original action-before-diagnostics focus order"
        );
        window.focus_prev(cx);
        assert!(host.read(cx).focus.is_focused(window));
    });
    for key in ["enter", "space"] {
        fixture.ready();
        fixture.visual.simulate_keystrokes(key);
        fixture.event(attempt);
    }
    fixture.click(Action::Diagnostics);
    let expected = fixture
        .submission
        .preparation_diagnostics(attempt)
        .expect("displayed diagnostics");
    fixture.wait("matching displayed-attempt clipboard", |fixture| {
        fixture.visual.update(|_, cx| {
            cx.read_from_clipboard().and_then(|item| item.text()) == Some(expected.clone())
        })
    });
    let root = fixture.root.clone();
    fixture.visual.update(|_, cx| {
        assert!(
            root.read(cx).requests.is_empty(),
            "diagnostics emitted a submission action"
        )
    });
    fixture.root.update(&mut fixture.visual.cx, |root, cx| {
        root.width = 360.;
        cx.notify();
    });
    fixture.ready();
}

pub fn retry_and_same_attempt_fact_changes_reject_old_displayed_controls(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let (host, displayed) = fixture.capture();
    let worker = fixture
        .submission
        .try_retry(displayed.source.attempt)
        .expect("actual retry");
    let next = worker.attempt_id();
    assert_ne!(next, displayed.source.attempt);
    fixture.rejects(&host, &displayed);
    let old_diagnostics = fixture
        .submission
        .preparation_diagnostics(displayed.source.attempt)
        .expect("old diagnostics");
    let next_diagnostics = fixture
        .submission
        .preparation_diagnostics(next)
        .expect("new diagnostics");
    assert_ne!(old_diagnostics, next_diagnostics);
    worker
        .complete(Err(SubmissionFailure::SpawnFailed))
        .expect("second worker failure");
    fixture.submission.seal(next).expect("second seal");
    let stream = fixture.stream.clone();
    fixture
        .visual
        .update(|_, cx| stream.update(cx, |stream, cx| stream.refresh_ordinary_submissions(cx)));
    fixture.ready();
    fixture.click(Action::Diagnostics);
    let expected = fixture
        .submission
        .preparation_diagnostics(next)
        .expect("next attempt diagnostics");
    fixture.wait("new diagnostics only after real reprojection", |fixture| {
        fixture.visual.update(|_, cx| {
            cx.read_from_clipboard().and_then(|item| item.text()) == Some(expected.clone())
        })
    });
    let (host, displayed) = fixture.capture();
    fixture
        .submission
        .invalidate_recovery(next)
        .expect("same-attempt fact mutation");
    assert_eq!(
        fixture
            .submission
            .snapshot()
            .attempts
            .last()
            .expect("latest attempt")
            .id,
        next
    );
    fixture.rejects(&host, &displayed);
}

pub fn route_settings_geometry_and_unmount_reject_captured_controls(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let (host, displayed) = fixture.capture();
    fixture
        .visual
        .update(|_, cx| cx.set_global(crate::settings::SettingsOpen(true)));
    fixture.rejects(&host, &displayed);
    fixture
        .visual
        .update(|_, cx| cx.set_global(crate::settings::SettingsOpen(false)));
    let original = fixture
        .visual
        .update(|_, cx| cx.global::<crate::sidebar::OpenedThread>().0.clone());
    fixture
        .visual
        .update(|_, cx| cx.set_global(crate::sidebar::OpenedThread(None)));
    fixture.rejects(&host, &displayed);
    fixture
        .visual
        .update(|_, cx| cx.set_global(crate::sidebar::OpenedThread(original)));
    fixture.ready();
    let (host, displayed) = fixture.capture();
    let stream = fixture.stream.clone();
    fixture
        .visual
        .update(|_, cx| stream.update(cx, |stream, _| stream.foreground_height += px(1.)));
    fixture.rejects(&host, &displayed);
    fixture
        .visual
        .update(|_, cx| stream.update(cx, |stream, _| stream.foreground_height -= px(1.)));
    fixture.ready();
    let (host, displayed) = fixture.capture();
    fixture.root.update(&mut fixture.visual.cx, |root, cx| {
        root.width = 360.;
        cx.notify();
    });
    fixture.frame();
    fixture.rejects(&host, &displayed);
    fixture.ready();
    let (host, displayed) = fixture.capture();
    let retired_surface = fixture.visual.update(|_, cx| {
        host.read(cx)
            .surface
            .as_ref()
            .expect("mounted text surface")
            .entity()
    });
    fixture.root.update(&mut fixture.visual.cx, |root, cx| {
        root.visible = false;
        cx.notify();
    });
    fixture.frame();
    fixture.visual.update(|_, cx| {
        assert!(
            host.read(cx).lease.upgrade().is_none(),
            "actual unmount must release lease"
        )
    });
    fixture.rejects(&host, &displayed);
    fixture.visual.update(|_, cx| {
        host.update(cx, |host, cx| host.stop(cx));
        assert!(current_texts(host.read(cx), cx).is_none());
        assert!(
            retired_surface
                .read(cx)
                .integration_text_observation()
                .is_none()
        );
    });
    fixture.root.update(&mut fixture.visual.cx, |root, cx| {
        root.visible = true;
        cx.notify();
    });
    fixture.ready();
    let replacement = fixture.current().expect("remounted current host");
    fixture.visual.update(|_, cx| {
        assert_ne!(
            replacement
                .read(cx)
                .surface
                .as_ref()
                .expect("replacement text surface")
                .entity(),
            retired_surface
        );
        assert!(
            retired_surface
                .read(cx)
                .integration_text_observation()
                .is_none()
        );
    });
    fixture.asserts_current_text_observation();
}
