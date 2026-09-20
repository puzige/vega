use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gpui_kit::{
    Bounds, KeyBinding, Render, TestAppContext, VisualTestContext, WindowBounds, WindowHandle,
    WindowOptions, size,
};

use super::*;
use crate::settings::state::{
    PRICING_INPUT_BYTES_LIMIT, PROVIDER_MODELS_FRAME_INSET, PROVIDER_MODELS_MAX_ROWS,
    PROVIDER_MODELS_MIN_ROWS,
};

struct SettingsHarness {
    view: Entity<SettingsView>,
    closes: Arc<AtomicUsize>,
}

struct OwnedOAuthMetadataFixture {
    endpoint: String,
    issuer: String,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    registrations: Arc<AtomicUsize>,
    tokens: Arc<AtomicUsize>,
    accepted_paths: Arc<Mutex<Vec<String>>>,
}

impl OwnedOAuthMetadataFixture {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("owned OAuth fixture");
        listener
            .set_nonblocking(true)
            .expect("owned fixture nonblocking");
        let origin = format!("http://{}", listener.local_addr().expect("fixture address"));
        let stop = Arc::new(AtomicBool::new(false));
        let registrations = Arc::new(AtomicUsize::new(0));
        let tokens = Arc::new(AtomicUsize::new(0));
        let accepted_paths = Arc::new(Mutex::new(Vec::new()));
        let stop_worker = stop.clone();
        let registrations_worker = registrations.clone();
        let tokens_worker = tokens.clone();
        let accepted_paths_worker = accepted_paths.clone();
        let worker_origin = origin.clone();
        let thread = std::thread::spawn(move || {
            while !stop_worker.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        use std::io::{Read, Write};
                        // The listening socket is nonblocking so this
                        // worker can observe Stop. On macOS the accepted
                        // socket may inherit O_NONBLOCK; without this, a
                        // request that has connected but not sent its first
                        // byte yet gets a spurious 404 from our fixture.
                        stream
                            .set_nonblocking(false)
                            .expect("owned fixture accepted socket blocking");
                        stream
                            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                            .expect("fixture read timeout");
                        let mut request = Vec::new();
                        loop {
                            let mut chunk = [0u8; 4096];
                            let Ok(size) = stream.read(&mut chunk) else {
                                break;
                            };
                            if size == 0 || request.len() + size > 65536 {
                                break;
                            }
                            request.extend_from_slice(&chunk[..size]);
                            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                        // A peer that closes before sending complete HTTP
                        // headers has not made a request. A real HTTP server
                        // drops that connection; replying 404 to EOF can be
                        // misread as the response to a different pooled
                        // request and makes this fixture non-HTTP-compliant.
                        if !request.windows(4).any(|window| window == b"\r\n\r\n") {
                            accepted_paths_worker
                                .lock()
                                .expect("owned fixture path log")
                                .push(format!("incomplete | bytes={}", request.len()));
                            continue;
                        }
                        let path = String::from_utf8_lossy(&request)
                            .split_ascii_whitespace()
                            .nth(1)
                            .unwrap_or("")
                            .to_owned();
                        let (status, body, headers) = match path.as_str() {
                            "/mcp" => (
                                "401 Unauthorized",
                                String::new(),
                                format!(
                                    "WWW-Authenticate: Bearer resource_metadata=\"{worker_origin}/.well-known/oauth-protected-resource/mcp\", scope=\"tools:read\"\r\n"
                                ),
                            ),
                            "/.well-known/oauth-protected-resource/mcp" => (
                                "200 OK",
                                serde_json::json!({
                                    "resource":format!("{worker_origin}/mcp"),
                                    "authorization_servers":[format!("{worker_origin}/issuer")],
                                    "scopes_supported":["tools:read"]
                                })
                                .to_string(),
                                String::new(),
                            ),
                            "/.well-known/oauth-authorization-server/issuer" => (
                                "200 OK",
                                serde_json::json!({
                                    "issuer":format!("{worker_origin}/issuer"),
                                    "authorization_endpoint":format!("{worker_origin}/authorize"),
                                    "token_endpoint":format!("{worker_origin}/token"),
                                    "registration_endpoint":format!("{worker_origin}/register"),
                                    "code_challenge_methods_supported":["S256"],
                                    "authorization_response_iss_parameter_supported":true
                                })
                                .to_string(),
                                String::new(),
                            ),
                            "/register" => {
                                registrations_worker.fetch_add(1, Ordering::SeqCst);
                                ("500 Unexpected registration", String::new(), String::new())
                            }
                            "/token" => {
                                tokens_worker.fetch_add(1, Ordering::SeqCst);
                                (
                                    "200 OK",
                                    serde_json::json!({
                                        "access_token":"owned-access",
                                        "refresh_token":"owned-refresh",
                                        "token_type":"Bearer",
                                        "expires_in":3600,
                                        "scope":"tools:read"
                                    })
                                    .to_string(),
                                    String::new(),
                                )
                            }
                            _ => ("404 Not Found", String::new(), String::new()),
                        };
                        accepted_paths_worker
                            .lock()
                            .expect("owned fixture path log")
                            .push(format!(
                                "{path} | {status} | bytes={} | first-byte={:?}",
                                request.len(),
                                request.first()
                            ));
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            endpoint: format!("{origin}/mcp"),
            issuer: format!("{origin}/issuer"),
            stop,
            thread: Some(thread),
            registrations,
            tokens,
            accepted_paths,
        }
    }

    fn complete_callback(&self, redirect_uri: &str, authorization_url: &str) -> Vec<u8> {
        use std::io::{Read, Write};
        // This fixture URL is generated by Vega, not an untrusted browser
        // callback. Parse named query fields rather than matching a substring
        // that could belong to another parameter or an older flow.
        let query = authorization_url
            .split_once('?')
            .map(|(_, query)| query)
            .expect("consented browser URL query");
        let field = |name: &str| {
            query
                .split('&')
                .find_map(|pair| pair.split_once('=').filter(|(key, _)| *key == name))
                .map(|(_, value)| value)
        };
        let state = field("state").expect("consented browser state");
        assert!((32..=128).contains(&state.len()), "fresh OAuth state shape");
        let host_port = redirect_uri
            .strip_prefix("http://")
            .and_then(|url| url.split_once('/'))
            .map(|(host, _)| host)
            .expect("exact previewed loopback address");
        let expected_redirect =
            format!("http%3A%2F%2F{}%2Fcallback", host_port.replace(':', "%3A"));
        assert!(
            field("redirect_uri")
                .is_some_and(|actual| actual.eq_ignore_ascii_case(&expected_redirect)),
            "browser URL redirect must equal the currently previewed callback: preview={host_port}, browser_redirect={:?}",
            field("redirect_uri")
        );
        let mut stream = std::net::TcpStream::connect(host_port).expect("callback listener bound");
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("bounded callback wait");
        let request = format!(
            "GET /callback?code=owned-code&state={state}&iss={} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\n\r\n",
            self.issuer
        );
        stream
            .write_all(request.as_bytes())
            .expect("owned browser callback");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .expect("callback response");
        response
    }
}

impl Drop for OwnedOAuthMetadataFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}

impl Render for SettingsHarness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .on_action(cx.listener(|this, _: &CloseSettings, _, _| {
                this.closes.fetch_add(1, Ordering::SeqCst);
            }))
            .child(self.view.clone())
    }
}

fn provider(name: &str, models: &[&str]) -> ProviderConfig {
    ProviderConfig {
        enabled: true,
        name: name.to_string(),
        base_url: format!("https://{name}.example.com"),
        models: models.iter().map(|m| m.to_string()).collect(),
        key_ref: name.to_string(),
    }
}

fn assert_pixel_close(actual: gpui_kit::Pixels, expected: f32, label: &str) {
    let actual = f32::from(actual);
    assert!(
        (actual - expected).abs() <= 1.0,
        "{label}: expected {expected}±1px, got {actual}px"
    );
}

fn wait_for_mcp_idle(cx: &mut TestAppContext, view: &Entity<SettingsView>) {
    for _ in 0..150 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
        if view.read_with(cx, |view, _| !view.mcp.busy) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("MCP Settings operation did not finish within bounded fixture time");
}

/// A click can be dispatched after the current test-executor tick. Waiting
/// merely for `busy == false` can therefore observe the *previous* idle
/// state and race ahead before the requested operation even starts.
#[derive(Clone, Copy)]
enum McpExpectedOperation {
    Discovery,
    Preparation,
    RejectedStepUp,
}

fn mcp_test_state(view: &SettingsView) -> String {
    let confirmation = match view.mcp.confirmation.as_ref() {
        Some(super::mcp::McpConfirmation::DiscoverOAuth { .. }) => "discover_oauth",
        Some(super::mcp::McpConfirmation::Enable { .. }) => "enable",
        Some(super::mcp::McpConfirmation::Test { .. }) => "test",
        Some(super::mcp::McpConfirmation::Remove { .. }) => "remove",
        Some(super::mcp::McpConfirmation::DisconnectOAuth { .. }) => "disconnect_oauth",
        None => "none",
    };
    format!(
        "generation={}, busy={}, flow={}, confirmation={confirmation}, discovery={}, preparation={}, error={}, message={:?}",
        view.mcp.generation,
        view.mcp.busy,
        view.mcp.oauth_flow_id.is_some(),
        view.mcp.oauth_discovery.is_some(),
        view.mcp.oauth_preparation.is_some(),
        view.mcp.message_error,
        view.mcp.message,
    )
}

fn wait_for_mcp_operation(
    cx: &mut TestAppContext,
    view: &Entity<SettingsView>,
    previous_generation: u64,
    expected: McpExpectedOperation,
) {
    for _ in 0..150 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
        let (advanced, complete, snapshot) = view.read_with(cx, |view, _| {
            let expected_result = match expected {
                McpExpectedOperation::Discovery => {
                    view.mcp.oauth_discovery.is_some() && !view.mcp.message_error
                }
                McpExpectedOperation::Preparation => {
                    view.mcp.oauth_preparation.is_some() && !view.mcp.message_error
                }
                McpExpectedOperation::RejectedStepUp => {
                    view.mcp.oauth_preparation.is_none()
                        && view.mcp.step_up_offers.is_empty()
                        && view.mcp.message_error
                }
            };
            (
                view.mcp.generation > previous_generation && !view.mcp.busy,
                expected_result,
                mcp_test_state(view),
            )
        });
        if advanced {
            assert!(
                complete,
                "MCP UI action completed without expected projection: {snapshot}"
            );
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let snapshot = view.read_with(cx, |view, _| mcp_test_state(view));
    panic!(
        "MCP UI action did not start and finish after generation {previous_generation}: {snapshot}"
    );
}

#[gpui_kit::test]
async fn issue73_mcp_settings_create_stays_disabled_and_never_launches_on_save(
    cx: &mut TestAppContext,
) {
    let owned = tempfile::tempdir().expect("owned MCP Settings root");
    let service = vega_conversation::McpServerSettingsService::new(
        owned.path().join("vega.db"),
        owned.path().join("config"),
    );
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| {
        view.install_mcp_service(Some(service.clone()), cx)
    });
    wait_for_mcp_idle(cx, &view);
    cx.run_until_parked();
    let view_for_window = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(860.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: view_for_window,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("Settings window");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let nav = visual
        .debug_bounds("settings-nav-mcp")
        .expect("MCP navigation");
    visual.simulate_click(nav.center(), Default::default());
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-page-mcp")
            .is_some()
    );

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let add = visual
        .debug_bounds("mcp-add-local")
        .expect("add local button");
    visual.simulate_click(add.center(), Default::default());
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.mcp
            .name_input
            .update(cx, |input, cx| input.set_text("fixture", cx));
        view.mcp.executable_input.update(cx, |input, cx| {
            input.set_text("/definitely/not/a/real/program", cx)
        });
        view.mcp.args_input.update(cx, |input, cx| {
            input.set_text("--fixture\nvalue with spaces", cx)
        });
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let save = visual.debug_bounds("mcp-save").expect("save local server");
    visual.simulate_click(save.center(), Default::default());
    wait_for_mcp_idle(cx, &view);
    let rows = service.list().expect("saved servers");
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].enabled, "saving must never launch a local process");
    assert!(matches!(
        rows[0].health,
        vega_conversation::types::McpServerHealth::Disabled
    ));
    let vega_conversation::types::McpServerTransport::Local { args, .. } = &rows[0].form.transport
    else {
        panic!("expected local server");
    };
    assert_eq!(args, &["--fixture", "value with spaces"]);
}

#[gpui_kit::test]
async fn issue73_mcp_local_multiline_fields_reserve_rows_at_narrow_width(cx: &mut TestAppContext) {
    let owned = tempfile::tempdir().expect("owned MCP Settings root");
    let service = vega_conversation::McpServerSettingsService::new(
        owned.path().join("vega.db"),
        owned.path().join("config"),
    );
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| view.install_mcp_service(Some(service), cx));
    wait_for_mcp_idle(cx, &view);
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(960.), px(600.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("narrow MCP Settings window");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let nav = visual.debug_bounds("settings-nav-mcp").expect("MCP nav");
    visual.simulate_click(nav.center(), Default::default());
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let add = visual.debug_bounds("mcp-add-local").expect("add local");
    visual.simulate_click(add.center(), Default::default());
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let args_two = visual.debug_bounds("mcp-args-frame").expect("args field");
    let directory_two = visual
        .debug_bounds("mcp-working-directory-frame")
        .expect("directory field");
    let environment_two = visual
        .debug_bounds("mcp-environment-frame")
        .expect("environment field");
    assert!(directory_two.origin.y >= args_two.bottom());
    assert!(environment_two.origin.y >= directory_two.bottom());

    view.update(cx, |view, cx| {
        view.mcp.args_input.update(cx, |input, cx| {
            input.set_text("first\nsecond\nthird\nfourth", cx)
        });
        view.mcp.environment_input.update(cx, |input, cx| {
            input.set_text("MCP_FIRST\nMCP_SECOND\nMCP_THIRD\nMCP_FOURTH", cx)
        });
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let args_four = visual.debug_bounds("mcp-args-frame").expect("args field");
    let directory_four = visual
        .debug_bounds("mcp-working-directory-frame")
        .expect("directory field");
    let environment_four = visual
        .debug_bounds("mcp-environment-frame")
        .expect("environment field");
    let body_row = Typography::BODY * Typography::BODY_LINE_HEIGHT;
    assert!(
        args_four.size.height >= px(4.0 * body_row - 1.0),
        "args frame height {:?}, visible rows {}",
        args_four.size.height,
        view.read_with(cx, |view, cx| view.mcp.args_input.read(cx).visible_rows())
    );
    assert!(
        environment_four.size.height >= px(4.0 * body_row - 1.0),
        "environment frame height {:?}, visible rows {}",
        environment_four.size.height,
        view.read_with(cx, |view, cx| view
            .mcp
            .environment_input
            .read(cx)
            .visible_rows())
    );
    assert!(args_four.size.height > args_two.size.height);
    assert!(environment_four.size.height > environment_two.size.height);
    assert!(directory_four.origin.y >= args_four.bottom());
    assert!(environment_four.origin.y >= directory_four.bottom());

    view.update(cx, |view, cx| {
        view.mcp.args_input.update(cx, |input, cx| {
            input.clear(cx);
            input.set_text(&"narrow-path-segment/".repeat(80), cx);
        });
    });
    cx.run_until_parked();
    // The test window does not schedule the animation frame requested when
    // TextInput learns visual rows during paint; explicitly render that frame.
    window
        .update(cx, |_, window, _| window.refresh())
        .expect("soft-wrap frame refresh");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let wrapped_args = visual.debug_bounds("mcp-args-frame").expect("wrapped args");
    let wrapped_directory = visual
        .debug_bounds("mcp-working-directory-frame")
        .expect("directory after wrapped args");
    assert_eq!(
        view.read_with(cx, |view, cx| view.mcp.args_input.read(cx).visible_rows()),
        4
    );
    assert!(
        wrapped_args.size.height >= px(4.0 * body_row - 1.0),
        "wrapped args height {:?}, visible rows {}",
        wrapped_args.size.height,
        view.read_with(cx, |view, cx| view.mcp.args_input.read(cx).visible_rows())
    );
    assert!(wrapped_directory.origin.y >= wrapped_args.bottom());
}

#[gpui_kit::test]
async fn issue73_mcp_settings_requires_confirmation_before_activation_or_remove(
    cx: &mut TestAppContext,
) {
    let owned = tempfile::tempdir().expect("owned MCP Settings root");
    let service = vega_conversation::McpServerSettingsService::new(
        owned.path().join("vega.db"),
        owned.path().join("config"),
    );
    let created = service
        .create(vega_conversation::types::McpServerForm {
            display_name: "fixture".into(),
            transport: vega_conversation::types::McpServerTransport::Local {
                executable: "/definitely/not/a/real/program".into(),
                args: vec!["--fixture".into(), "two words".into()],
                working_directory: None,
                environment: vec![vega_conversation::types::McpEnvironmentVariable {
                    variable: "MCP_TOKEN".into(),
                }],
            },
        })
        .expect("saved disabled fixture");
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| {
        view.install_mcp_service(Some(service.clone()), cx)
    });
    wait_for_mcp_idle(cx, &view);
    let view_for_window = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(860.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: view_for_window,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("Settings window");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let nav = visual
        .debug_bounds("settings-nav-mcp")
        .expect("MCP navigation");
    visual.simulate_click(nav.center(), Default::default());
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let enable = visual.debug_bounds("mcp-enable").expect("enable action");
    visual.simulate_click(enable.center(), Default::default());
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |view, _| view.mcp.confirmation.is_some()),
        "enable click must enter explicit confirmation state"
    );
    let authority = super::mcp::mcp_confirmation_lines(&created).join("\n");
    assert!(authority.contains("[0] \"--fixture\""));
    assert!(authority.contains("[1] \"two words\""));
    assert!(authority.contains("MCP_TOKEN"));
    assert!(authority.contains("不受 Vega 内置 bash 沙箱约束"));
    assert!(authority.contains("不要把 token 写进命令参数"));
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("mcp-confirmation")
            .is_some()
    );
    assert!(!service.list().expect("after ask")[0].enabled);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let cancel = visual
        .debug_bounds("mcp-cancel-confirm")
        .expect("cancel confirmation");
    visual.simulate_click(cancel.center(), Default::default());
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("mcp-confirmation")
            .is_none()
    );
    assert!(!service.list().expect("after cancel")[0].enabled);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let remove = visual.debug_bounds("mcp-remove").expect("remove action");
    visual.simulate_click(remove.center(), Default::default());
    cx.run_until_parked();
    assert_eq!(service.list().expect("before remove confirmation").len(), 1);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let confirm = visual.debug_bounds("mcp-confirm").expect("confirm remove");
    visual.simulate_click(confirm.center(), Default::default());
    wait_for_mcp_idle(cx, &view);
    assert!(service.list().expect("after explicit remove").is_empty());
}

#[gpui_kit::test]
async fn issue73_mcp_enable_mounted_ui_describes_task_start_connection(cx: &mut TestAppContext) {
    let owned = tempfile::tempdir().expect("owned MCP Settings root");
    let script = owned.path().join("owned-server.sh");
    std::fs::write(
        &script,
        r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}}' ;;
    *tools/list*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}' ;;
  esac
done
"##,
    )
    .expect("owned local MCP server");
    let service = vega_conversation::McpServerSettingsService::new(
        owned.path().join("vega.db"),
        owned.path().join("config"),
    );
    let saved = service
        .create(vega_conversation::types::McpServerForm {
            display_name: "local lease fixture".into(),
            transport: vega_conversation::types::McpServerTransport::Local {
                executable: "/bin/sh".into(),
                args: vec![script.to_string_lossy().into_owned()],
                working_directory: Some(owned.path().into()),
                environment: Vec::new(),
            },
        })
        .expect("saved disabled MCP fixture");
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| {
        view.install_mcp_service(Some(service.clone()), cx)
    });
    wait_for_mcp_idle(cx, &view);
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(860.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("Settings window");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let nav = visual
        .debug_bounds("settings-nav-mcp")
        .expect("MCP navigation");
    visual.simulate_click(nav.center(), Default::default());
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let enable = visual.debug_bounds("mcp-enable").expect("enable action");
    visual.simulate_click(enable.center(), Default::default());
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let confirm = visual
        .debug_bounds("mcp-confirm")
        .expect("explicit enable confirmation");
    let generation = view.read_with(cx, |view, _| view.mcp.generation);
    visual.simulate_click(confirm.center(), Default::default());
    for _ in 0..150 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
        if view.read_with(cx, |view, _| {
            view.mcp.generation > generation && !view.mcp.busy
        }) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let row = service.list().expect("enabled server").remove(0);
    assert_eq!(row.id, saved.id);
    assert!(row.enabled);
    assert!(matches!(
        row.health,
        vega_conversation::types::McpServerHealth::Disconnected
    ));
    assert!(
        row.tool_names.is_empty(),
        "Settings must not retain a run lease"
    );
    let (projected, message, error) = view.read_with(cx, |view, _| {
        (
            view.mcp.servers.first().cloned(),
            view.mcp.message.clone(),
            view.mcp.message_error,
        )
    });
    assert!(matches!(
        projected.as_ref().map(|row| &row.health),
        Some(vega_conversation::types::McpServerHealth::Disconnected)
    ));
    assert_eq!(
        super::mcp::health_label(&row.health),
        "已启用 · 任务开始时连接"
    );
    assert_eq!(
        message.as_deref(),
        Some("已启用；将在任务开始时连接，连接失败时该任务不会广告工具")
    );
    assert!(!error);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual.debug_bounds("mcp-health").is_some(),
        "task-start health label must mount"
    );
    assert!(
        visual.debug_bounds("mcp-message").is_some(),
        "task-start enable message must mount"
    );
}

#[test]
fn issue73_mcp_enable_message_distinguishes_enabled_failure_states() {
    use vega_conversation::types::{
        McpServerForm, McpServerHealth, McpServerTransport, McpServerView,
    };

    let mut row = McpServerView {
        id: "owned-server".into(),
        config_revision: 1,
        enabled: true,
        deleting: false,
        form: McpServerForm {
            display_name: "owned fixture".into(),
            transport: McpServerTransport::Local {
                executable: "/bin/sh".into(),
                args: Vec::new(),
                working_directory: None,
                environment: Vec::new(),
            },
        },
        credential_configured: false,
        health: McpServerHealth::Error("owned_validation_failure".into()),
        tool_names: Vec::new(),
        rejected_tools: Vec::new(),
    };
    let (message, warning) = super::mcp::enable_result_message(Some(&row));
    assert!(warning);
    assert!(message.contains("连接验证失败"));
    assert!(message.contains("任务开始时可重试"));
    assert!(message.contains("不会向模型提供工具"));
    assert!(!message.contains("将在任务开始时连接"));

    row.health = McpServerHealth::NeedsCredential;
    let (message, warning) = super::mcp::enable_result_message(Some(&row));
    assert!(warning);
    assert!(message.contains("缺少凭据"));
    assert!(message.contains("配置后下次任务可重试连接"));

    row.health = McpServerHealth::NeedsAuthorization;
    let (message, warning) = super::mcp::enable_result_message(Some(&row));
    assert!(warning);
    assert!(message.contains("需要授权"));
    assert!(message.contains("授权后下次任务可重试连接"));
    assert!(message.contains("未授权时不会向模型提供工具"));

    row.health = McpServerHealth::Disconnected;
    assert_eq!(
        super::mcp::enable_result_message(Some(&row)),
        (
            "已启用；将在任务开始时连接，连接失败时该任务不会广告工具",
            false,
        )
    );
}

#[test]
fn issue73_mcp_remote_confirmation_discloses_auth_and_loopback_http() {
    let row = vega_conversation::types::McpServerView {
        id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        config_revision: 1,
        enabled: false,
        deleting: false,
        form: vega_conversation::types::McpServerForm {
            display_name: "local fixture".into(),
            transport: vega_conversation::types::McpServerTransport::Remote {
                endpoint: "http://127.0.0.1:4567/mcp".into(),
                allow_loopback_http: true,
                authorization: vega_conversation::types::McpRemoteAuthorization::Bearer,
            },
        },
        credential_configured: false,
        health: vega_conversation::types::McpServerHealth::Disabled,
        tool_names: Vec::new(),
        rejected_tools: Vec::new(),
    };
    let authority = super::mcp::mcp_confirmation_lines(&row).join("\n");
    assert!(authority.contains("http://127.0.0.1:4567/mcp"));
    assert!(authority.contains("独立 Bearer（非 OAuth）"));
    assert!(authority.contains("本机明文连接"));
}

#[test]
fn issue73_mcp_oauth_exact_preconsent_discloses_dcr_and_blocks_unavailable_identity() {
    use vega_conversation::types::{McpOAuthPreparation, McpOAuthRegistration};
    let preparation = McpOAuthPreparation {
        flow_id: "owned-flow".into(),
        resource: "https://resource.example/mcp".into(),
        issuer: "https://issuer.example".into(),
        redirect_uri: "http://127.0.0.1:54321/callback".into(),
        requested_scopes: vec!["tools:read".into(), "tools:write".into()],
        step_up_added_scopes: vec!["tools:write".into()],
        registration: McpOAuthRegistration::DynamicRegistration,
        registration_endpoint: Some("https://issuer.example/register".into()),
    };
    let lines = super::mcp::mcp_oauth_preparation_lines(&preparation).join("\n");
    assert!(lines.contains(&preparation.resource));
    assert!(lines.contains(&preparation.issuer));
    assert!(lines.contains(&preparation.redirect_uri));
    assert!(lines.contains("tools:read、tools:write"));
    assert!(lines.contains("本次授权总权限：tools:read、tools:write"));
    assert!(lines.contains("本次新增权限（来自经验证的 403 挑战）：tools:write"));
    assert!(lines.contains("失败，不会因授权升级而自动重放"));
    assert!(lines.contains("已弃用的动态客户端注册（DCR）"));
    assert!(lines.contains("https://issuer.example/register"));
    assert!(super::mcp::mcp_oauth_can_begin(preparation.registration));
    assert!(!super::mcp::mcp_oauth_can_begin(
        McpOAuthRegistration::CimdUnavailable
    ));
    assert!(!super::mcp::mcp_oauth_can_begin(
        McpOAuthRegistration::Unavailable
    ));
}

#[gpui_kit::test]
async fn issue73_mcp_oauth_ui_requires_discovery_preview_and_consent_and_cancels_on_close(
    cx: &mut TestAppContext,
) {
    use vega_conversation::types::{
        McpOAuthRegistration, McpRemoteAuthorization, McpServerForm, McpServerTransport,
    };
    let fixture = OwnedOAuthMetadataFixture::start();
    let owned = tempfile::tempdir().expect("owned MCP Settings root");
    let service = vega_conversation::McpServerSettingsService::new(
        owned.path().join("vega.db"),
        owned.path().join("config"),
    );
    let saved = service
        .create(McpServerForm {
            display_name: "owned OAuth metadata".into(),
            transport: McpServerTransport::Remote {
                endpoint: fixture.endpoint.clone(),
                allow_loopback_http: true,
                authorization: McpRemoteAuthorization::OAuth {
                    client_id: Some("owned-pre-registered-client".into()),
                },
            },
        })
        .expect("disabled OAuth server");
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| {
        view.install_mcp_service(Some(service.clone()), cx)
    });
    wait_for_mcp_idle(cx, &view);
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(860.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("Settings window");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let nav = visual
        .debug_bounds("settings-nav-mcp")
        .expect("MCP navigation");
    visual.simulate_click(nav.center(), Default::default());
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let discover = visual
        .debug_bounds("mcp-oauth-discover")
        .expect("discover OAuth");
    visual.simulate_click(discover.center(), Default::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.mcp.confirmation.is_some()));
    assert!(cx.opened_url().is_none());
    assert_eq!(fixture.registrations.load(Ordering::SeqCst), 0);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let confirm = visual
        .debug_bounds("mcp-confirm")
        .expect("confirm metadata discovery");
    let generation = view.read_with(cx, |view, _| view.mcp.generation);
    visual.simulate_click(confirm.center(), Default::default());
    wait_for_mcp_operation(cx, &view, generation, McpExpectedOperation::Discovery);
    let discovery = view.read_with(cx, |view, _| view.mcp.oauth_discovery.clone());
    let (_, _, discovery) = discovery.expect("validated OAuth metadata appears");
    assert_eq!(discovery.issuers, [fixture.issuer.as_str()]);
    assert_eq!(discovery.requested_scopes, ["tools:read"]);
    assert!(cx.opened_url().is_none());
    assert_eq!(fixture.registrations.load(Ordering::SeqCst), 0);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let issuer = visual
        .debug_bounds("mcp-oauth-issuer")
        .expect("select issuer");
    let generation = view.read_with(cx, |view, _| view.mcp.generation);
    visual.simulate_click(issuer.center(), Default::default());
    wait_for_mcp_operation(cx, &view, generation, McpExpectedOperation::Preparation);
    let prepared = view.read_with(cx, |view, _| view.mcp.oauth_preparation.clone());
    let prepared = prepared.expect("exact authorization preview");
    assert_eq!(prepared.issuer, fixture.issuer);
    assert_eq!(prepared.registration, McpOAuthRegistration::PreRegistered);
    assert!(prepared.redirect_uri.starts_with("http://127.0.0.1:"));
    assert_eq!(prepared.requested_scopes, ["tools:read"]);
    assert!(prepared.step_up_added_scopes.is_empty());
    assert!(
        !super::mcp::mcp_oauth_preparation_lines(&prepared)
            .join("\n")
            .contains("本次新增权限")
    );
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("mcp-oauth-preparation")
            .is_some()
    );
    assert!(cx.opened_url().is_none(), "prepare must not open browser");
    assert_eq!(fixture.registrations.load(Ordering::SeqCst), 0);
    assert!(!service.list().expect("not authorized yet")[0].credential_configured);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let authorize = visual
        .debug_bounds("mcp-oauth-authorize")
        .expect("explicit consent");
    visual.simulate_click(authorize.center(), Default::default());
    for _ in 0..100 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
        if cx.opened_url().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let url = cx.opened_url().expect("browser opens only after consent");
    assert!(url.starts_with(&format!(
        "{}/authorize?",
        fixture.endpoint.trim_end_matches("/mcp")
    )));
    assert!(view.read_with(cx, |view, _| view.mcp.oauth_waiting));
    assert!(!service.list().expect("still disabled")[0].enabled);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let cancel = visual
        .debug_bounds("mcp-oauth-cancel")
        .expect("cancel browser wait");
    visual.simulate_click(cancel.center(), Default::default());
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |view, _| view.mcp.oauth_flow_id.is_none()
            && !view.mcp.busy)
    );
    let row = service.list().expect("after cancel").remove(0);
    assert!(!row.enabled && !row.credential_configured);

    // A second prepared callback is cleaned up by closing Settings itself.
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let discover = visual
        .debug_bounds("mcp-oauth-discover")
        .expect("repeat discovery");
    visual.simulate_click(discover.center(), Default::default());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let confirm = visual
        .debug_bounds("mcp-confirm")
        .expect("confirm repeat discovery");
    let generation = view.read_with(cx, |view, _| view.mcp.generation);
    visual.simulate_click(confirm.center(), Default::default());
    wait_for_mcp_operation(cx, &view, generation, McpExpectedOperation::Discovery);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let issuer = visual
        .debug_bounds("mcp-oauth-issuer")
        .expect("select repeat issuer");
    let generation = view.read_with(cx, |view, _| view.mcp.generation);
    visual.simulate_click(issuer.center(), Default::default());
    wait_for_mcp_operation(cx, &view, generation, McpExpectedOperation::Preparation);
    assert!(view.read_with(cx, |view, _| view.mcp.oauth_flow_id.is_some()));
    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.mcp.oauth_flow_id.is_none()));
    let current = service.list().expect("current row").remove(0);
    let next = mcp_async_for_test(service.prepare_oauth(
        &saved.id,
        current.config_revision,
        &fixture.issuer,
    ))
    .expect("close releases previous callback capacity");
    service.cancel_oauth(&next.flow_id).expect("test cleanup");
    assert_eq!(fixture.registrations.load(Ordering::SeqCst), 0);

    // Finally complete the real callback through the same mounted UI. It
    // saves a credential but still cannot silently enable this MCP server.
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let discover = visual
        .debug_bounds("mcp-oauth-discover")
        .expect("discover after reopening Settings");
    visual.simulate_click(discover.center(), Default::default());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let confirm = visual
        .debug_bounds("mcp-confirm")
        .expect("confirm fresh metadata discovery");
    let generation = view.read_with(cx, |view, _| view.mcp.generation);
    visual.simulate_click(confirm.center(), Default::default());
    wait_for_mcp_operation(cx, &view, generation, McpExpectedOperation::Discovery);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let issuer = visual
        .debug_bounds("mcp-oauth-issuer")
        .expect("select fresh issuer");
    let generation = view.read_with(cx, |view, _| view.mcp.generation);
    visual.simulate_click(issuer.center(), Default::default());
    wait_for_mcp_operation(cx, &view, generation, McpExpectedOperation::Preparation);
    let preview = view.read_with(cx, |view, _| view.mcp.oauth_preparation.clone());
    let preview = preview.unwrap_or_else(|| {
        let (generation, busy, message, error, discovery) = view.read_with(cx, |view, _| {
            (
                view.mcp.generation,
                view.mcp.busy,
                view.mcp.message.clone(),
                view.mcp.message_error,
                view.mcp.oauth_discovery.is_some(),
            )
        });
        panic!(
            "fresh preview absent after issuer selection: generation={generation}, busy={busy}, message={message:?}, error={error}, discovery={discovery}"
        );
    });
    let (redirect, flow_id) = (preview.redirect_uri, preview.flow_id);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let authorize = visual
        .debug_bounds("mcp-oauth-authorize")
        .expect("consent for callback completion");
    visual.simulate_click(authorize.center(), Default::default());
    let mut later_browser_url = None;
    for _ in 0..100 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
        if let Some(later) = cx.opened_url()
            && later != url
        {
            later_browser_url = Some(later);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(view.read_with(cx, |view, _| {
        view.mcp.oauth_waiting
            && view.mcp.busy
            && view.mcp.oauth_flow_id.as_deref() == Some(flow_id.as_str())
    }));
    let callback_started = std::time::Instant::now();
    let response = fixture.complete_callback(
        &redirect,
        &later_browser_url.expect("only explicit consent opens fresh browser"),
    );
    wait_for_mcp_idle(cx, &view);
    let status = view.read_with(cx, |view, _| {
        (
            view.mcp.message.clone(),
            view.mcp.message_error,
            view.mcp.oauth_flow_id.clone(),
        )
    });
    assert!(
        response.starts_with(b"HTTP/1.1 200 OK"),
        "OAuth loopback callback failed: {:?}; elapsed: {:?}; UI status: {status:?}; token exchanges: {}; fixture paths: {:?}",
        String::from_utf8_lossy(&response),
        callback_started.elapsed(),
        fixture.tokens.load(Ordering::SeqCst),
        fixture.accepted_paths.lock().expect("owned fixture paths")
    );
    let row = service.list().expect("callback saved").remove(0);
    assert!(!row.enabled);
    assert!(row.credential_configured);
    assert_eq!(fixture.tokens.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.registrations.load(Ordering::SeqCst), 0);
    assert!(view.read_with(cx, |view, _| view.mcp.oauth_flow_id.is_none()));

    // A stale/forged projection is not authority: Settings presents a
    // distinct step-up path (never the normal OAuth discovery action), while
    // the service must reject a preview without the bound real 403 challenge.
    let browser_before_step_up = cx.opened_url();
    view.update(cx, |view, cx| {
        let row = view.mcp.servers.first_mut().expect("refreshed server row");
        row.enabled = true;
        view.mcp
            .step_up_offers
            .push(vega_conversation::types::McpOAuthStepUpOffer {
                server_id: row.id.clone(),
                config_revision: row.config_revision,
                added_scopes: vec!["tools:write".into()],
            });
        cx.notify();
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("mcp-oauth-step-up-notice").is_some());
    assert!(visual.debug_bounds("mcp-oauth-discover").is_none());
    let inspect = visual
        .debug_bounds("mcp-oauth-step-up")
        .expect("distinct verified scope upgrade action");
    let generation = view.read_with(cx, |view, _| view.mcp.generation);
    visual.simulate_click(inspect.center(), Default::default());
    wait_for_mcp_operation(cx, &view, generation, McpExpectedOperation::RejectedStepUp);
    assert!(service.step_up_offers().expect("real offers").is_empty());
    assert!(view.read_with(cx, |view, _| {
        view.mcp.oauth_preparation.is_none() && view.mcp.step_up_offers.is_empty()
    }));
    assert_eq!(cx.opened_url(), browser_before_step_up);
    assert_eq!(fixture.tokens.load(Ordering::SeqCst), 1);

    // Switching away from MCP settings must release a prepared listener too.
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let discover = visual
        .debug_bounds("mcp-oauth-discover")
        .expect("ordinary OAuth remains distinct after stale step-up");
    visual.simulate_click(discover.center(), Default::default());
    cx.run_until_parked();
    let row = service.list().expect("current OAuth server").remove(0);
    assert!(matches!(
        view.read_with(cx, |view, _| view.mcp.confirmation.clone()),
        Some(super::mcp::McpConfirmation::DiscoverOAuth { id, revision })
            if id == row.id && revision == row.config_revision
    ));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let confirm = visual
        .debug_bounds("mcp-confirm")
        .expect("confirm ordinary discovery");
    let generation = view.read_with(cx, |view, _| view.mcp.generation);
    visual.simulate_click(confirm.center(), Default::default());
    wait_for_mcp_operation(cx, &view, generation, McpExpectedOperation::Discovery);
    assert!(
        view.read_with(cx, |view, _| {
            view.mcp.oauth_discovery.is_some() && !view.mcp.message_error
        }),
        "ordinary OAuth metadata discovery failed after stale step-up: {:?}",
        view.read_with(cx, |view, _| view.mcp.message.clone())
    );
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let issuer = visual
        .debug_bounds("mcp-oauth-issuer")
        .expect("select ordinary issuer");
    let generation = view.read_with(cx, |view, _| view.mcp.generation);
    visual.simulate_click(issuer.center(), Default::default());
    wait_for_mcp_operation(cx, &view, generation, McpExpectedOperation::Preparation);
    assert!(view.read_with(cx, |view, _| view.mcp.oauth_flow_id.is_some()));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let general = visual
        .debug_bounds("settings-nav-general")
        .expect("leave MCP settings");
    visual.simulate_click(general.center(), Default::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.mcp.oauth_flow_id.is_none()));
    let current = service.list().expect("current row").remove(0);
    let next = mcp_async_for_test(service.prepare_oauth(
        &saved.id,
        current.config_revision,
        &fixture.issuer,
    ))
    .expect("leaving MCP tab releases listener capacity");
    service
        .cancel_oauth(&next.flow_id)
        .expect("fixture cleanup");
}

fn mcp_async_for_test<T>(
    operation: impl std::future::Future<Output = Result<T, vega_conversation::McpSettingsError>>,
) -> Result<T, vega_conversation::McpSettingsError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("owned test runtime")
        .block_on(operation)
}

#[gpui_kit::test]
async fn issue73_mcp_bearer_secret_is_masked_cleared_and_never_in_sqlite(cx: &mut TestAppContext) {
    let owned = tempfile::tempdir().expect("owned MCP Settings root");
    let database_path = owned.path().join("vega.db");
    let service = vega_conversation::McpServerSettingsService::new(
        database_path.clone(),
        owned.path().join("config"),
    );
    service
        .create(vega_conversation::types::McpServerForm {
            display_name: "remote bearer fixture".into(),
            transport: vega_conversation::types::McpServerTransport::Remote {
                endpoint: "https://example.com/mcp".into(),
                allow_loopback_http: false,
                authorization: vega_conversation::types::McpRemoteAuthorization::Bearer,
            },
        })
        .expect("saved disabled bearer fixture");
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| {
        view.install_mcp_service(Some(service.clone()), cx)
    });
    wait_for_mcp_idle(cx, &view);
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(860.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("Settings window");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let nav = visual
        .debug_bounds("settings-nav-mcp")
        .expect("MCP navigation");
    visual.simulate_click(nav.center(), Default::default());
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let edit = visual.debug_bounds("mcp-edit").expect("edit bearer server");
    visual.simulate_click(edit.center(), Default::default());
    cx.run_until_parked();

    let sentinel = "TEST_SECRET_DO_NOT_PERSIST_IN_DATABASE_73";
    view.update(cx, |view, cx| {
        view.mcp
            .bearer_input
            .update(cx, |input, cx| input.set_text(sentinel, cx));
    });
    assert!(view.read_with(cx, |view, cx| {
        let input = view.mcp.bearer_input.read(cx);
        input.display_text() != sentinel
            && input
                .display_text()
                .chars()
                .all(|character| character == '•')
    }));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let save = visual
        .debug_bounds("mcp-save-bearer")
        .expect("save masked bearer");
    visual.simulate_click(save.center(), Default::default());
    wait_for_mcp_idle(cx, &view);
    assert!(service.list().expect("after secret save")[0].credential_configured);
    assert!(view.read_with(cx, |view, cx| {
        view.mcp.bearer_input.read(cx).text().is_empty()
    }));
    let database = std::fs::read(database_path).expect("fixture database bytes");
    assert!(
        !database
            .windows(sentinel.len())
            .any(|bytes| bytes == sentinel.as_bytes())
    );
}

#[gpui_kit::test]
async fn issue73_mcp_stale_editor_refreshes_instead_of_overwriting_new_revision(
    cx: &mut TestAppContext,
) {
    let owned = tempfile::tempdir().expect("owned MCP Settings root");
    let service = vega_conversation::McpServerSettingsService::new(
        owned.path().join("vega.db"),
        owned.path().join("config"),
    );
    let created = service
        .create(vega_conversation::types::McpServerForm {
            display_name: "original".into(),
            transport: vega_conversation::types::McpServerTransport::Remote {
                endpoint: "https://example.com/mcp".into(),
                allow_loopback_http: false,
                authorization: vega_conversation::types::McpRemoteAuthorization::None,
            },
        })
        .expect("saved server");
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| {
        view.install_mcp_service(Some(service.clone()), cx)
    });
    wait_for_mcp_idle(cx, &view);
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(860.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("Settings window");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let nav = visual
        .debug_bounds("settings-nav-mcp")
        .expect("MCP navigation");
    visual.simulate_click(nav.center(), Default::default());
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let edit = visual.debug_bounds("mcp-edit").expect("edit server");
    visual.simulate_click(edit.center(), Default::default());
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.mcp
            .name_input
            .update(cx, |input, cx| input.set_text("stale UI draft", cx))
    });
    service
        .replace(
            &created.id,
            created.config_revision,
            vega_conversation::types::McpServerForm {
                display_name: "external winner".into(),
                transport: created.form.transport.clone(),
            },
        )
        .expect("external revision wins");

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let save = visual.debug_bounds("mcp-save").expect("save stale editor");
    visual.simulate_click(save.center(), Default::default());
    wait_for_mcp_idle(cx, &view);
    assert_eq!(
        service.list().expect("winner preserved")[0]
            .form
            .display_name,
        "external winner"
    );
    assert!(view.read_with(cx, |view, _| view.mcp.editor.is_none()));
    assert!(view.read_with(cx, |view, _| {
        view.mcp
            .message
            .as_deref()
            .is_some_and(|message| message.contains("已变化"))
    }));
}

#[gpui_kit::test]
async fn issue73_mcp_disable_remains_clickable_during_a_busy_connection(cx: &mut TestAppContext) {
    let owned = tempfile::tempdir().expect("owned MCP Settings root");
    let service = vega_conversation::McpServerSettingsService::new(
        owned.path().join("vega.db"),
        owned.path().join("config"),
    );
    let created = service
        .create(vega_conversation::types::McpServerForm {
            display_name: "revocation fixture".into(),
            transport: vega_conversation::types::McpServerTransport::Local {
                executable: "/definitely/not/a/real/program".into(),
                args: Vec::new(),
                working_directory: None,
                environment: Vec::new(),
            },
        })
        .expect("saved server");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("owned test runtime");
    runtime
        .block_on(service.set_enabled(&created.id, created.config_revision, true, true))
        .expect("persisted enabled configuration despite disconnected fixture");
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| {
        view.install_mcp_service(Some(service.clone()), cx)
    });
    wait_for_mcp_idle(cx, &view);
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(860.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("Settings window");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let nav = visual
        .debug_bounds("settings-nav-mcp")
        .expect("MCP navigation");
    visual.simulate_click(nav.center(), Default::default());
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.mcp.busy = true;
        cx.notify();
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let disable = visual
        .debug_bounds("mcp-disable")
        .expect("disable remains present");
    visual.simulate_click(disable.center(), Default::default());
    wait_for_mcp_idle(cx, &view);
    assert!(!service.list().expect("disabled immediately")[0].enabled);
}

#[gpui_kit::test]
async fn issue73_mcp_slow_test_then_remove_keeps_newer_settings_projection(
    cx: &mut TestAppContext,
) {
    assert_slow_test_then_revoke(cx, false);
}

#[gpui_kit::test]
async fn issue73_mcp_slow_test_then_disable_keeps_newer_settings_projection(
    cx: &mut TestAppContext,
) {
    assert_slow_test_then_revoke(cx, true);
}

fn assert_slow_test_then_revoke(cx: &mut TestAppContext, disable: bool) {
    let owned = tempfile::tempdir().expect("owned MCP Settings root");
    let marker = owned.path().join("test-started");
    let script = owned.path().join("slow-server.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 1\n").expect("owned initial fast server");
    let service = vega_conversation::McpServerSettingsService::new(
        owned.path().join("vega.db"),
        owned.path().join("config"),
    );
    let created = service
        .create(vega_conversation::types::McpServerForm {
            display_name: "slow revocation fixture".into(),
            transport: vega_conversation::types::McpServerTransport::Local {
                executable: "/bin/sh".into(),
                args: vec![
                    script.to_string_lossy().into_owned(),
                    marker.to_string_lossy().into_owned(),
                ],
                working_directory: Some(owned.path().into()),
                environment: Vec::new(),
            },
        })
        .expect("saved initially disabled fixture");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("owned test runtime");
    let enabled = runtime
        .block_on(service.set_enabled(&created.id, created.config_revision, true, true))
        .expect("persist enabled configuration despite failed fast child");
    assert!(enabled.enabled);
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf started > \"$1\"\nwhile IFS= read -r request; do :; done\n",
    )
    .expect("owned slow local server");
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| {
        view.install_mcp_service(Some(service.clone()), cx)
    });
    wait_for_mcp_idle(cx, &view);
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(860.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("Settings window");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let nav = visual
        .debug_bounds("settings-nav-mcp")
        .expect("MCP navigation");
    visual.simulate_click(nav.center(), Default::default());
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let test = visual.debug_bounds("mcp-test").expect("test connection");
    visual.simulate_click(test.center(), Default::default());
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let confirm = visual.debug_bounds("mcp-confirm").expect("confirm test");
    visual.simulate_click(confirm.center(), Default::default());
    for _ in 0..100 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
        if marker.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        marker.exists(),
        "local server must actually begin slow test"
    );
    assert!(view.read_with(cx, |view, _| view.mcp.busy));
    let first_generation = view.read_with(cx, |view, _| view.mcp.generation);

    // The second user intent supersedes a genuine in-flight discovery.
    if disable {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let disable = visual
            .debug_bounds("mcp-disable")
            .expect("disable while busy");
        visual.simulate_click(disable.center(), Default::default());
    } else {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let remove = visual
            .debug_bounds("mcp-remove")
            .expect("remove while busy");
        visual.simulate_click(remove.center(), Default::default());
        cx.run_until_parked();
        assert!(matches!(
            view.read_with(cx, |view, _| view.mcp.confirmation.clone()),
            Some(super::mcp::McpConfirmation::Remove { .. })
        ));
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let confirm = visual.debug_bounds("mcp-confirm").expect("confirm remove");
        visual.simulate_click(confirm.center(), Default::default());
    }
    assert!(view.read_with(cx, |view, _| view.mcp.generation > first_generation));
    for _ in 0..100 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
        if view.read_with(cx, |view, _| !view.mcp.busy) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let rows = service.list().expect("revoked row projection");
    if disable {
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].enabled);
    } else {
        assert!(rows.is_empty());
    }
    assert!(
        view.read_with(cx, |view, _| view.mcp.servers.len() == rows.len()
            && !view.mcp.busy)
    );
    assert!(view.read_with(cx, |view, _| !view.mcp.preview.contains_key(&created.id)));
    assert!(view.read_with(cx, |view, _| {
        !view.mcp.message.as_deref().is_some_and(|message| {
            message.contains("连接测试发现") || message.contains("配置已变化")
        })
    }));
}

#[gpui_kit::test]
async fn r21_settings_shell_opens_general_and_tracks_sidebar_width(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        cx.set_global(crate::sidebar::SidebarWidth(Layout::SIDEBAR_WIDTH));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    assert_eq!(view.read_with(cx, |view, _| view.section), 1);
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(860.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("R21 Settings window");
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let nav = visual
            .debug_bounds("settings-navigation")
            .expect("Settings navigation");
        let content = visual
            .debug_bounds("settings-content-column")
            .expect("Settings content column");
        assert_pixel_close(nav.size.width, Layout::SIDEBAR_WIDTH, "Settings rail width");
        assert_pixel_close(
            content.size.width,
            Layout::SETTINGS_CONTENT_MAX_WIDTH,
            "Settings content cap",
        );
        assert!(visual.debug_bounds("settings-page-general").is_some());
        let sidebar_switch = visual
            .debug_bounds("settings-sidebar-switch")
            .expect("Settings Sidebar switch");
        assert_pixel_close(
            sidebar_switch.size.width,
            Layout::SETTINGS_SWITCH_WIDTH,
            "Settings switch width",
        );
        assert_pixel_close(
            sidebar_switch.size.height,
            Layout::SETTINGS_SWITCH_HEIGHT,
            "Settings switch height",
        );
        let rows = [
            "settings-nav-general",
            "settings-nav-providers",
            "settings-nav-reasoning",
            "settings-nav-pricing",
            "settings-nav-usage",
            "settings-nav-skills",
        ]
        .map(|selector| {
            visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("missing {selector}"))
        });
        for row in rows {
            assert_pixel_close(
                row.size.height,
                Typography::SIDEBAR_LINE_HEIGHT,
                "Settings navigation row height",
            );
        }
        assert!(rows.windows(2).all(|pair| pair[0].top() <= pair[1].top()));

        let providers = visual
            .debug_bounds("settings-nav-providers")
            .expect("Providers navigation");
        visual.simulate_click(providers.center(), Default::default());
    }
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.section), 0);
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-page-providers")
            .is_some()
    );

    cx.update(|cx| crate::sidebar::set_width(Layout::SIDEBAR_MAX_WIDTH, cx));
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert_pixel_close(
        visual
            .debug_bounds("settings-navigation")
            .expect("resized Settings navigation")
            .size
            .width,
        Layout::SIDEBAR_MAX_WIDTH,
        "resized Settings rail width",
    );
    assert_pixel_close(
        visual
            .debug_bounds("settings-content-column")
            .expect("resized Settings content")
            .size
            .width,
        Layout::SETTINGS_CONTENT_MAX_WIDTH,
        "resized Settings content cap",
    );
}

#[test]
fn form_rejects_empty_fields() {
    assert!(!form_is_submittable("", "https://x", "k"));
    assert!(!form_is_submittable("n", "", "k"));
    assert!(!form_is_submittable("n", "https://x", ""));
    // 空白 name 视为空。
    assert!(!form_is_submittable("   ", "https://x", "k"));
    assert!(form_is_submittable("n", "https://x", "k"));
    assert!(provider_form_is_submittable("n", "https://x", "", true));
    assert!(!provider_form_is_submittable("n", "https://x", "", false));
}

#[test]
fn provider_models_normalize_and_preserve_exact_ids() {
    let parsed =
        parse_provider_models("  OpenAI/GPT-4.1-mini  \n\nclaude-3.5-sonnet\nvendor/model-v1.2\n")
            .expect("valid provider models");
    assert_eq!(
        parsed,
        vec![
            "OpenAI/GPT-4.1-mini",
            "claude-3.5-sonnet",
            "vendor/model-v1.2"
        ]
    );
    assert!(parse_provider_models("Foo\nfoo").is_ok());
}

#[test]
fn provider_models_reject_empty_invalid_duplicate_and_limits() {
    assert_eq!(
        parse_provider_models(" \n\t").unwrap_err(),
        ProviderModelsError::Empty
    );
    assert_eq!(
        parse_provider_models("valid/model\nvalid/model").unwrap_err(),
        ProviderModelsError::Duplicate { line: 2 }
    );
    assert_eq!(
        parse_provider_models("valid model").unwrap_err(),
        ProviderModelsError::Invalid { line: 1 }
    );
    assert_eq!(
        parse_provider_models(&"a".repeat(PROVIDER_MODEL_ID_MAX_BYTES + 1)).unwrap_err(),
        ProviderModelsError::TooLong { line: 1 }
    );
    let too_many = (0..=PROVIDER_MODEL_COUNT_MAX)
        .map(|index| format!("model-{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        parse_provider_models(&too_many).unwrap_err(),
        ProviderModelsError::TooMany {
            line: PROVIDER_MODEL_COUNT_MAX + 1
        }
    );
    assert_eq!(
        parse_provider_models(&"x".repeat(PROVIDER_MODELS_INPUT_BYTES_LIMIT + 1)).unwrap_err(),
        ProviderModelsError::InputTooLarge
    );
}

#[test]
fn provider_key_ref_keeps_existing_empty_key_semantics() {
    let mut existing = provider("legacy", &["model"]);
    existing.key_ref = "legacy-ref".to_string();
    assert_eq!(
        provider_key_ref(Some(&existing), "legacy", ""),
        "legacy-ref"
    );
    assert_eq!(
        provider_key_ref(Some(&existing), "legacy", "new-key"),
        "legacy"
    );
    assert_eq!(provider_key_ref(None, "new", "new-key"), "new");
    assert_eq!(provider_key_ref(None, "new", ""), "new");
}

#[test]
fn upsert_appends_new_and_updates_same_name_models() {
    let mut providers = vec![provider("deepseek", &["deepseek-chat"])];
    // 异名追加。
    assert!(!upsert_provider(&mut providers, provider("openai", &[])));
    assert_eq!(providers.len(), 2);
    assert_eq!(providers[1].name, "openai");
    // 同名更新：表单字段和 models 一起替换。
    let mut replacement = provider("deepseek", &["OpenAI/GPT-4.1-mini", "claude-3.5-sonnet"]);
    replacement.base_url = "https://api.deepseek.com/v1".to_string();
    assert!(upsert_provider(&mut providers, replacement));
    assert_eq!(providers.len(), 2);
    assert_eq!(providers[0].base_url, "https://api.deepseek.com/v1");
    assert_eq!(
        providers[0].models,
        vec!["OpenAI/GPT-4.1-mini", "claude-3.5-sonnet"]
    );
}

#[gpui_kit::test]
async fn provider_submit_uses_owned_backends_and_emits_only_after_save(cx: &mut TestAppContext) {
    let view = cx.new(SettingsView::new_for_test);
    let saved = Arc::new(Mutex::new(Vec::<AppConfig>::new()));
    let key_writes = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(AtomicUsize::new(0));
    let saved_copy = saved.clone();
    let key_writes_copy = key_writes.clone();
    let events_copy = events.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, _: &SettingsSaved, _| {
            events_copy.fetch_add(1, Ordering::SeqCst);
        })
        .detach();
    });
    view.update(cx, |view, cx| {
        view.config.providers = vec![provider("legacy", &["old/model-v1"])];
        view.key_writer = Some(Arc::new(move |_, _| {
            key_writes_copy.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        view.config_saver = Some(Arc::new(move |config| {
            saved_copy
                .lock()
                .expect("owned config capture")
                .push(config.clone());
            Ok(())
        }));
        view.name_input
            .update(cx, |input, cx| input.set_text("legacy", cx));
        view.base_url_input.update(cx, |input, cx| {
            input.set_text("https://legacy.invalid/v1", cx)
        });
        view.models_input.update(cx, |input, cx| {
            input.set_text("OpenAI/GPT-4.1-mini\nclaude-3.5-sonnet", cx)
        });
        view.key_input.update(cx, TextInput::clear);
        view.submit_provider(cx);
    });
    assert_eq!(key_writes.load(Ordering::SeqCst), 0);
    assert_eq!(events.load(Ordering::SeqCst), 1);
    let saved_config = saved.lock().expect("saved config capture");
    assert_eq!(saved_config[0].providers[0].key_ref, "legacy");
    assert_eq!(
        saved_config[0].providers[0].models,
        vec!["OpenAI/GPT-4.1-mini", "claude-3.5-sonnet"]
    );
    drop(saved_config);

    view.update(cx, |view, cx| {
        view.name_input
            .update(cx, |input, cx| input.set_text("new-provider", cx));
        view.base_url_input
            .update(cx, |input, cx| input.set_text("https://new.invalid/v1", cx));
        view.models_input
            .update(cx, |input, cx| input.set_text("vendor/model-v1.2", cx));
        view.key_input
            .update(cx, |input, cx| input.set_text("owned-test-key", cx));
        view.submit_provider(cx);
    });
    assert_eq!(key_writes.load(Ordering::SeqCst), 1);
    assert_eq!(events.load(Ordering::SeqCst), 2);
    let saved_config = saved.lock().expect("new saved config capture");
    assert_eq!(saved_config[1].providers[1].name, "new-provider");
    assert_eq!(
        saved_config[1].providers[1].models,
        vec!["vendor/model-v1.2"]
    );
    assert_eq!(saved_config[1].providers[1].key_ref, "new-provider");
    assert!(view.read_with(cx, |view, cx| {
        view.name_input.read(cx).text().is_empty()
            && view.models_input.read(cx).text().is_empty()
            && view.key_input.read(cx).text().is_empty()
    }));
}

#[gpui_kit::test]
async fn provider_submit_keeps_draft_and_authority_on_validation_or_save_failure(
    cx: &mut TestAppContext,
) {
    let view = cx.new(SettingsView::new_for_test);
    let saved = Arc::new(Mutex::new(Vec::<AppConfig>::new()));
    let key_writes = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(AtomicUsize::new(0));
    let saved_copy = saved.clone();
    let key_writes_copy = key_writes.clone();
    let events_copy = events.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, _: &SettingsSaved, _| {
            events_copy.fetch_add(1, Ordering::SeqCst);
        })
        .detach();
    });
    view.update(cx, |view, cx| {
        view.config.providers = vec![provider("legacy", &["old/model-v1"])];
        view.key_writer = Some(Arc::new(move |_, _| {
            key_writes_copy.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        view.config_saver = Some(Arc::new(move |config| {
            saved_copy
                .lock()
                .expect("failed config capture")
                .push(config.clone());
            Err("owned config rejected".to_string())
        }));
        view.name_input
            .update(cx, |input, cx| input.set_text("legacy", cx));
        view.base_url_input.update(cx, |input, cx| {
            input.set_text("https://legacy.invalid/v2", cx)
        });
        view.models_input.update(cx, |input, cx| {
            input.set_text("old/model-v1\nold/model-v1", cx)
        });
        view.key_input.update(cx, TextInput::clear);
        view.submit_provider(cx);
    });
    assert_eq!(key_writes.load(Ordering::SeqCst), 0);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert!(view.read_with(cx, |view, cx| {
        view.config.providers[0].models == vec!["old/model-v1"]
            && view.models_input.read(cx).text() == "old/model-v1\nold/model-v1"
            && view
                .error
                .as_deref()
                .is_some_and(|error| error.contains("重复"))
    }));

    view.update(cx, |view, cx| {
        view.models_input
            .update(cx, |input, cx| input.set_text("new/model-v2", cx));
        view.submit_provider(cx);
    });
    assert_eq!(key_writes.load(Ordering::SeqCst), 0);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert!(view.read_with(cx, |view, cx| {
        view.config.providers[0].models == vec!["old/model-v1"]
            && view.models_input.read(cx).text() == "new/model-v2"
            && view
                .error
                .as_deref()
                .is_some_and(|error| error.contains("当前输入仍未保存"))
    }));
}

#[gpui_kit::test]
async fn provider_form_focus_and_edit_action_follow_the_real_ui_path(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    let save_count = Arc::new(AtomicUsize::new(0));
    let save_count_copy = save_count.clone();
    view.update(cx, |view, _| {
        view.config.providers = vec![provider("legacy", &["old/model-v1"])];
        view.config_saver = Some(Arc::new(move |_| {
            save_count_copy.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
    });
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            let bounds = Bounds::centered(None, size(px(960.), px(600.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("provider settings window");
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let providers = visual
            .debug_bounds("settings-nav-providers")
            .expect("providers navigation");
        visual.simulate_click(providers.center(), Default::default());
    }
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let edit = visual.debug_bounds("provider-edit").expect("edit button");
        visual.simulate_click(edit.center(), Default::default());
    }
    cx.run_until_parked();
    window
        .update(cx, |_, window, cx| {
            let name = view.read(cx).name_input.read(cx).focus_handle(cx);
            name.focus(window, cx);
        })
        .expect("focus provider name");
    cx.simulate_keystrokes(window.into(), "tab tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                view.read(cx)
                    .models_input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            })
            .expect("models focus")
    );

    // The provider selector and edit form are separate surfaces in R14.
    // Reopen through the actual detail action, then verify loaded inputs.
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let cancel = visual
            .debug_bounds("provider-form-cancel")
            .expect("cancel form");
        visual.simulate_click(cancel.center(), Default::default());
    }
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let edit = visual.debug_bounds("provider-edit").expect("edit button");
        visual.simulate_click(edit.center(), Default::default());
    }
    window
        .update(cx, |_, window, cx| {
            view.read(cx)
                .name_input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx)
        })
        .expect("focus form");
    assert!(view.read_with(cx, |view, cx| {
        view.models_input.read(cx).text() == "old/model-v1"
    }));
    assert!(
        window
            .update(cx, |_, window, cx| {
                view.read(cx)
                    .name_input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            })
            .expect("edit focuses provider name")
    );

    view.update(cx, |view, cx| {
        view.models_input.update(cx, |input, cx| {
            input.set_text("old/model-v1\nold/model-v1", cx)
        });
    });
    window
        .update(cx, |_, window, cx| {
            view.read(cx)
                .models_input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        })
        .expect("focus invalid models");
    cx.simulate_keystrokes(window.into(), "cmd-enter");
    assert!(view.read_with(cx, |view, cx| {
        view.models_input.read(cx).text() == "old/model-v1\nold/model-v1"
            && view
                .error
                .as_deref()
                .is_some_and(|error| error.contains("重复"))
    }));

    view.update(cx, |view, cx| {
        view.base_url_input.update(cx, |input, cx| {
            input.set_text("https://legacy.invalid/v3", cx)
        });
        view.models_input
            .update(cx, |input, cx| input.set_text("new/model-v2", cx));
        view.key_input.update(cx, TextInput::clear);
    });
    cx.simulate_keystrokes(window.into(), "tab tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                view.read(cx).provider_save_focus.is_focused(window)
            })
            .expect("save focus")
    );
    cx.simulate_keystrokes(window.into(), "space");
    assert_eq!(save_count.load(Ordering::SeqCst), 1);
}

#[gpui_kit::test]
async fn provider_models_frame_reserves_rows_and_keeps_tail_visible(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            let bounds = Bounds::centered(None, size(px(960.), px(628.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("provider models layout window");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let providers = visual
        .debug_bounds("settings-nav-providers")
        .expect("providers navigation");
    visual.simulate_click(providers.center(), Default::default());
    cx.run_until_parked();
    let bounds = |visual: &mut VisualTestContext, selector: &'static str| {
        visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing debug bounds for {selector}"))
    };
    let frame = bounds(&mut visual, "provider-models-input-frame");
    let help = bounds(&mut visual, "provider-models-input-help");
    let body_row = Typography::BODY * Typography::BODY_LINE_HEIGHT;
    let minimum_frame =
        PROVIDER_MODELS_MIN_ROWS as f32 * body_row + PROVIDER_MODELS_FRAME_INSET - 1.0;
    assert!(frame.size.height >= px(minimum_frame));
    assert!(help.origin.y >= frame.bottom());

    view.update(cx, |view, cx| {
        view.models_input.update(cx, |input, cx| {
            input.set_text("vendor/model-1.0\nmodel-flash", cx)
        });
    });
    cx.run_until_parked();
    let two_line_frame = bounds(&mut visual, "provider-models-input-frame");
    let two_line_help = bounds(&mut visual, "provider-models-input-help");
    assert!(two_line_frame.size.height >= px(minimum_frame));
    assert!(two_line_help.origin.y >= two_line_frame.bottom());

    view.update(cx, |view, cx| {
        view.models_input
            .update(cx, |input, cx| input.set_text("one\ntwo\nthree\nfour", cx));
    });
    cx.run_until_parked();
    let four_line_frame = bounds(&mut visual, "provider-models-input-frame");
    let four_line_help = bounds(&mut visual, "provider-models-input-help");
    assert!(four_line_frame.size.height > two_line_frame.size.height);
    assert!(four_line_help.origin.y >= four_line_frame.bottom());

    let five_line_text = "one\ntwo\nthree\nfour\nfive";
    view.update(cx, |view, cx| {
        view.models_input
            .update(cx, |input, cx| input.set_text(five_line_text, cx));
    });
    cx.run_until_parked();
    let five_line_frame = bounds(&mut visual, "provider-models-input-frame");
    let five_line_help = bounds(&mut visual, "provider-models-input-help");
    assert_eq!(five_line_frame.size.height, four_line_frame.size.height);
    assert!(five_line_help.origin.y >= five_line_frame.bottom());
    assert!(view.read_with(cx, |view, cx| {
        let input = view.models_input.read(cx);
        input.visible_rows() == PROVIDER_MODELS_MAX_ROWS
            && input.first_visible_row() > 0
            && input.text() == five_line_text
    }));
}

#[test]
fn permission_mode_accepts_only_the_fixed_set() {
    let mut config = AppConfig::default();
    for mode in PERMISSION_MODES {
        select_permission_mode(&mut config, mode).unwrap();
        assert_eq!(config.defaults.permission_mode, mode);
    }
    assert!(select_permission_mode(&mut config, "yolo").is_err());
    assert_eq!(config.defaults.permission_mode, "full_access");
}

#[test]
fn default_model_changes_are_applied() {
    let mut config = AppConfig::default();
    assert!(config.defaults.model.is_empty());
    set_default_model(&mut config, "deepseek-chat");
    assert_eq!(config.defaults.model, "deepseek-chat");
}

#[test]
fn all_models_unions_and_dedups_providers() {
    let providers = vec![
        provider("deepseek", &["deepseek-chat", "deepseek-reasoner"]),
        provider("openai", &["gpt", "deepseek-chat"]),
    ];
    assert_eq!(
        all_models(&providers),
        vec!["deepseek-chat", "deepseek-reasoner", "gpt"]
    );
    assert!(all_models(&[]).is_empty());
}

#[test]
fn pricing_request_input_cap_is_checked_before_event_retention() {
    let rates = PricingRateInputs {
        input_usd_per_million: "0".repeat(PRICING_INPUT_BYTES_LIMIT - 4),
        output_usd_per_million: "0".to_string(),
        cache_read_usd_per_million: "0".to_string(),
        cache_write_usd_per_million: "0".to_string(),
    };
    let exact = PricingMutation::AddCustom {
        model: "m".to_string(),
        rates: rates.clone(),
    };
    assert_eq!(
        pricing_mutation_input_bytes(&exact),
        Some(PRICING_INPUT_BYTES_LIMIT)
    );
    let over = PricingMutation::AddCustom {
        model: "mm".to_string(),
        rates,
    };
    assert_eq!(
        pricing_mutation_input_bytes(&over),
        Some(PRICING_INPUT_BYTES_LIMIT + 1)
    );
}

fn explicit_openai_profile(provider: &str, model: &str) -> ReasoningProfileProjection {
    ReasoningProfileProjection {
        provider: provider.to_string(),
        model: model.to_string(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        support: ReasoningSupport::Optional,
        efforts: vec!["low".into(), "high".into()],
        supports_disabled: true,
        disabled_wire: Some(ReasoningDisabledWire::ReasoningEffortNone),
        preserve_reasoning_content: false,
        preference: ReasoningChoice::ProviderDefault,
    }
}

#[gpui_kit::test]
async fn reasoning_template_is_explicit_and_failed_draft_keeps_exact_owner(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    let events = Arc::new(Mutex::new(Vec::<ReasoningProfileSaveRequested>::new()));
    let captured = events.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &ReasoningProfileSaveRequested, _| {
            captured
                .lock()
                .expect("reasoning events")
                .push(event.clone());
        })
        .detach();
    });
    let unknown_a = ReasoningProfileProjection::unknown("provider-a", "model-a");
    let unknown_b = ReasoningProfileProjection::unknown("provider-b", "model-b");
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 1,
                profiles: vec![unknown_a.clone(), unknown_b.clone()],
                error: None,
            },
            cx,
        );
        // This is the same operation the unknown row's template button emits.
        view.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    let first = events
        .lock()
        .expect("first reasoning event")
        .first()
        .cloned()
        .expect("template save event");
    assert_eq!(first.profile.provider, "provider-a");
    assert_eq!(first.profile.model, "model-a");
    assert_eq!(
        first.profile.protocol,
        ReasoningProtocol::OpenAiChatCompletions
    );
    assert_eq!(first.profile.support, ReasoningSupport::Optional);
    assert!(first.profile.supports_disabled);
    assert_eq!(first.profile.preference, ReasoningChoice::ProviderDefault);

    // A provider catalog refresh may temporarily project Loading while this
    // failed operation is being reconciled. The exact draft must survive that
    // projection so the user can retry the same owner instead of re-entering
    // all capability fields.
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(ReasoningSettingsProjection::Loading, cx);
        assert_eq!(
            view.reasoning_draft
                .as_ref()
                .map(|draft| (&draft.provider, &draft.model)),
            Some((&first.profile.provider, &first.profile.model))
        );
    });

    // A failed save leaves draft A visible. A later click on row B must use B
    // authority, even though the stale draft is still present in the view.
    let profile_b = explicit_openai_profile("provider-b", "model-b");
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 2,
                profiles: vec![unknown_a, profile_b],
                error: Some(ReasoningSettingsErrorCode::Conflict),
            },
            cx,
        );
        view.cycle_reasoning_preference(1, cx);
    });
    let events = events.lock().expect("all reasoning events");
    let second = events.last().expect("row B event");
    assert_eq!(second.base.provider, "provider-b");
    assert_eq!(second.base.model, "model-b");
    assert_eq!(second.profile.provider, "provider-b");
    assert_eq!(second.profile.model, "model-b");
}

#[gpui_kit::test]
async fn reasoning_unknown_template_is_tab_reachable_and_enter_activates(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, _| view.section = 2);
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 1,
                profiles: vec![ReasoningProfileProjection::unknown("provider-a", "model-a")],
                error: None,
            },
            cx,
        );
    });
    let events = Arc::new(Mutex::new(Vec::<ReasoningProfileSaveRequested>::new()));
    let captured = events.clone();
    let root = view.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &ReasoningProfileSaveRequested, _| {
            captured
                .lock()
                .expect("reasoning events")
                .push(event.clone());
        })
        .detach();
    });
    let closes = Arc::new(AtomicUsize::new(0));
    let harness_closes = closes.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            let bounds = Bounds::centered(None, size(px(960.), px(600.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_cx| SettingsHarness {
                        view: root,
                        closes: harness_closes,
                    })
                },
            )
        })
        .expect("settings window");
    cx.run_until_parked();
    window
        .update(cx, |_, window, cx| {
            let reload = view
                .read(cx)
                .reasoning_focus(&ReasoningFocusTarget::Reload)
                .expect("reasoning reload focus");
            reload.focus(window, cx);
        })
        .expect("focus reasoning reload");
    cx.simulate_keystrokes(window.into(), "tab enter");
    let event = events
        .lock()
        .expect("template event")
        .first()
        .cloned()
        .expect("keyboard template save");
    assert_eq!(event.profile.provider, "provider-a");
    assert_eq!(event.profile.model, "model-a");
    assert_eq!(
        event.profile.protocol,
        ReasoningProtocol::OpenAiChatCompletions
    );
    assert_eq!(closes.load(Ordering::SeqCst), 0);

    // Rehydrate the default projection and exercise the same route with
    // Space. The production app uses this projection after a save ack.
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 2,
                profiles: vec![ReasoningProfileProjection::unknown("provider-a", "model-a")],
                error: None,
            },
            cx,
        );
    });
    window
        .update(cx, |_, window, cx| {
            let reload = view
                .read(cx)
                .reasoning_focus(&ReasoningFocusTarget::Reload)
                .expect("rehydrated reasoning reload focus");
            reload.focus(window, cx);
        })
        .expect("refocus reasoning reload");
    cx.simulate_keystrokes(window.into(), "tab space");
    assert_eq!(
        events.lock().expect("template events").len(),
        2,
        "Space activates the same explicit template action"
    );
}

#[gpui_kit::test]
async fn pricing_actions_are_tab_reachable_and_enter_space_activate_once(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
        cx.bind_keys([KeyBinding::new(
            "escape",
            CloseSettings,
            Some("PricingSettings"),
        )]);
    });
    let long_cjk_model = format!("custom/{}", "模型-".repeat(20));
    let view = cx.new(SettingsView::new_for_test);
    cx.update(|cx| cx.set_global(PricingSettingsRequested(true)));
    view.update(cx, |view, cx| {
        view.apply_pricing_projection(
            PricingSettingsProjection::Ready {
                generation: 7,
                entries: vec![PricingEntryProjection {
                    model: long_cjk_model.clone(),
                    kind: PricingEntryKind::CustomStatic,
                    base: PricingRateInputs {
                        input_usd_per_million: "1".into(),
                        output_usd_per_million: "2".into(),
                        cache_read_usd_per_million: "3".into(),
                        cache_write_usd_per_million: "4".into(),
                    },
                    peak: None,
                }],
                notice: None,
                draft_reason: None,
                error: None,
            },
            cx,
        );
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let closes = Arc::new(AtomicUsize::new(0));
    let captured = events.clone();
    let root = view.clone();
    let projection_root = view.clone();
    let harness_closes = closes.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            let bounds = Bounds::centered(None, size(px(960.), px(600.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|cx| {
                        cx.subscribe(&root, move |_, _, event: &PricingMutationRequested, cx| {
                            let mut events = captured.lock().expect("pricing event capture");
                            events.push((event.generation, event.mutation.is_ok()));
                            let first = events.len() == 1;
                            drop(events);
                            if first {
                                projection_root.update(cx, |view, cx| {
                                    view.apply_pricing_projection(
                                        PricingSettingsProjection::Saving {
                                            generation: event.generation,
                                            entries: Vec::new(),
                                        },
                                        cx,
                                    );
                                });
                            }
                        })
                        .detach();
                        SettingsHarness {
                            view: root,
                            closes: harness_closes,
                        }
                    })
                },
            )
        })
        .expect("settings window");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.section), 3);
    assert!(!cx.update(|cx| cx.global::<PricingSettingsRequested>().0));
    assert_eq!(
        window
            .update(cx, |_, window, _| window.viewport_size())
            .expect("settings viewport"),
        size(px(960.), px(600.))
    );
    assert!(view.read_with(cx, |view, _| matches!(
        &view.pricing,
        PricingSettingsProjection::Ready { entries, .. }
            if entries.first().is_some_and(|entry| entry.model == long_cjk_model)
    )));
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::dark());
        cx.refresh_windows();
    });
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| cx.global::<vega_theme::Theme>().appearance),
        vega_theme::Appearance::Dark
    );
    window
        .update(cx, |_, window, cx| {
            let reload = view
                .read(cx)
                .pricing_focus(&PricingFocusTarget::Reload)
                .expect("reload focus");
            reload.focus(window, cx);
        })
        .expect("focus reload");
    cx.simulate_keystrokes(window.into(), "tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                view.read(cx)
                    .pricing_focus(&PricingFocusTarget::Add)
                    .is_some_and(|focus| focus.is_focused(window))
            })
            .expect("add focus")
    );
    cx.simulate_keystrokes(window.into(), "shift-tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                view.read(cx)
                    .pricing_focus(&PricingFocusTarget::Reload)
                    .is_some_and(|focus| focus.is_focused(window))
            })
            .expect("reload focused after shift-tab")
    );
    cx.simulate_keystrokes(window.into(), "escape");
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    cx.simulate_keystrokes(window.into(), "tab");
    cx.simulate_keystrokes(window.into(), "enter");
    assert!(view.read_with(cx, |view, _| view.pricing_editor.is_some()));

    window
        .update(cx, |_, window, cx| {
            let save = view
                .read(cx)
                .pricing_focus(&PricingFocusTarget::Save)
                .expect("save focus");
            save.focus(window, cx);
        })
        .expect("focus save");
    cx.simulate_keystrokes(window.into(), "space space");
    assert_eq!(*events.lock().expect("pricing events"), vec![(7, true)]);
}

#[gpui_kit::test]
async fn local_credentials_settings_save_and_runtime_read_share_owned_root(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().expect("owned root");
    let config_path = root.path().join("config.toml");
    let view = cx.new(|cx| SettingsView::from_path(Some(config_path.clone()), cx));
    view.update(cx, |view, cx| {
        view.name_input
            .update(cx, |input, cx| input.set_text("owned-provider", cx));
        view.base_url_input.update(cx, |input, cx| {
            input.set_text("https://owned.invalid/v1", cx)
        });
        view.models_input
            .update(cx, |input, cx| input.set_text("owned-model", cx));
        view.key_input
            .update(cx, |input, cx| input.set_text("owned-test-secret", cx));
        view.submit_provider(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert!(view.error.is_none());
        assert_eq!(view.available_key_refs, vec!["owned-provider"]);
        assert!(view.key_input.read(cx).text().is_empty());
    });
    let loaded = config::read_from(&config_path).expect("saved config");
    assert_eq!(
        keystore::get_key(config_path.parent().unwrap(), &loaded.providers[0].key_ref).unwrap(),
        "owned-test-secret"
    );
    assert!(
        !std::fs::read_to_string(&config_path)
            .unwrap()
            .contains("owned-test-secret")
    );
    let reopened = cx.new(|cx| SettingsView::from_path(Some(config_path.clone()), cx));
    reopened.read_with(cx, |view, _| {
        assert_eq!(view.available_key_refs, vec!["owned-provider"])
    });
    keystore::delete_key(root.path(), "owned-provider").unwrap();
    let missing = cx.new(|cx| SettingsView::from_path(Some(config_path), cx));
    missing.read_with(cx, |view, _| {
        assert_eq!(view.config.providers[0].key_ref, "owned-provider");
        assert!(
            view.available_key_refs.is_empty(),
            "key_ref is not evidence of storage"
        );
    });
}

#[gpui_kit::test]
async fn preference_worker_preserves_new_provider_fields_and_detects_same_field_conflict(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    let config = AppConfig {
        providers: vec![provider("owned", &["model"])],
        ..Default::default()
    };
    config.save_to(&path).unwrap();
    let view = cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx));
    config::update_from(&path, |config| {
        config.providers[0].enabled = false;
        config.providers[0].models.push("new-model".into());
        config.ui.sidebar_collapsed = true;
    })
    .unwrap();
    view.update(cx, |view, cx| view.select_mode("auto", cx));
    cx.run_until_parked();
    let saved = config::read_from(&path).unwrap();
    assert!(!saved.providers[0].enabled);
    assert_eq!(saved.providers[0].models, ["model", "new-model"]);
    assert!(saved.ui.sidebar_collapsed);
    assert_eq!(saved.defaults.permission_mode, "auto");
    config::update_from(&path, |config| {
        config.defaults.permission_mode = "readonly".into()
    })
    .unwrap();
    view.update(cx, |view, cx| view.select_mode("confirm", cx));
    cx.run_until_parked();
    assert_eq!(
        config::read_from(&path).unwrap().defaults.permission_mode,
        "readonly"
    );
    assert!(view.read_with(cx, |view, _| {
        view.error.as_ref().is_some_and(|e| e.contains("已更改"))
    }));
}

#[gpui_kit::test]
async fn provider_rename_retains_exact_form_baseline_and_rejects_collision(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    let mut original = provider("first", &["model-a"]);
    original.enabled = false;
    let original_ref = original.key_ref.clone();
    AppConfig {
        providers: vec![original.clone(), provider("second", &["model-b"])],
        ..Default::default()
    }
    .save_to(&path)
    .unwrap();
    let view = cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx));
    view.update(cx, |view, cx| {
        view.begin_edit_provider("first", cx);
        view.name_input
            .update(cx, |input, cx| input.set_text("second", cx));
        view.submit_provider(cx);
    });
    cx.run_until_parked();
    assert_eq!(config::read_from(&path).unwrap().providers[0], original);
    assert_eq!(
        config::read_from(&path).unwrap().providers[1].models,
        ["model-b"]
    );
    assert!(view.read_with(cx, |view, cx| view.error.is_some()
        && view.name_input.read(cx).text() == "second"));
    view.update(cx, |view, cx| {
        view.name_input
            .update(cx, |input, cx| input.set_text("renamed", cx));
        view.submit_provider(cx);
    });
    cx.run_until_parked();
    let saved = config::read_from(&path).unwrap();
    assert_eq!(saved.providers[0].name, "renamed");
    assert!(!saved.providers[0].enabled);
    assert_eq!(saved.providers[0].key_ref, original_ref);
    assert_eq!(saved.providers[1].models, ["model-b"]);
}
