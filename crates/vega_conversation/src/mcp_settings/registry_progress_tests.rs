use super::*;
use std::sync::mpsc;
use std::thread;

struct Fixture {
    root: PathBuf,
    service: McpServerSettingsService,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-mcp-progress-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let service = McpServerSettingsService::new(root.join("test.sqlite"), root.clone());
        service.reconcile_declarations().unwrap();
        Self { root, service }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn bounded(operation: impl FnOnce() + Send + 'static) {
    let (sender, receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        operation();
        sender.send(()).unwrap();
    });
    receiver
        .recv_timeout(Duration::from_secs(15))
        .expect("MCP service operation did not finish");
    worker.join().unwrap();
}

#[test]
fn empty_settings_load_and_refresh_finish() {
    bounded(|| {
        let fixture = Fixture::new();
        for _ in 0..3 {
            assert!(fixture.service.list().unwrap().is_empty());
            assert!(fixture.service.step_up_offers().unwrap().is_empty());
        }
    });
}

#[test]
fn empty_offers_still_check_owner_credential_authority() {
    bounded(|| {
        let fixture = Fixture::new();
        let secret = "synthetic-owner-secret";
        vega_store::keystore::set_key(&fixture.root, "test-provider", secret).unwrap();
        assert!(fixture.service.step_up_offers().unwrap().is_empty());
        assert_eq!(
            fixture.service.reject_owner_secret_in_ui([secret]),
            Err(McpSettingsError::AuthorizationFailed)
        );
        assert_eq!(
            fixture.service.reject_owner_secret_in_ui(["safe-scope"]),
            Ok(())
        );
        let reference = vega_store::keystore::available_refs(&fixture.root).unwrap();
        assert_eq!(reference, vec!["test-provider".to_owned()]);
        vega_store::keystore::set_key(&fixture.root, "test-provider", "rotated-test-secret")
            .unwrap();
        assert_eq!(
            fixture
                .service
                .reject_owner_secret_in_ui(["rotated-test-secret"]),
            Err(McpSettingsError::AuthorizationFailed)
        );
        assert_eq!(fixture.service.reject_owner_secret_in_ui([secret]), Ok(()));
    });
}

#[test]
fn settings_and_parallel_run_authority_readers_leave_cancellation_accessible() {
    bounded(|| {
        let fixture = Arc::new(Fixture::new());
        vega_store::keystore::set_key(&fixture.root, "test-provider", "parallel-test-secret")
            .unwrap();
        assert!(fixture.service.step_up_offers().unwrap().is_empty());
        let barrier = Arc::new(std::sync::Barrier::new(6));
        let workers = (0..6)
            .map(|index| {
                let fixture = fixture.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    barrier.wait();
                    for _ in 0..2 {
                        if index == 0 {
                            assert!(fixture.service.step_up_offers().unwrap().is_empty());
                        } else {
                            let ready = runtime.block_on(fixture.service.ready_for_run()).unwrap();
                            assert!(ready.ready_servers.is_empty());
                            assert!(ready.unavailable.is_empty());
                            assert_eq!(
                                fixture.service.current_owner_credential_values().unwrap(),
                                vec!["parallel-test-secret".to_owned()]
                            );
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        let cancel = CancellationToken::new();
        fixture.service.lock().unwrap().active_oauth.insert(
            "test-flow".to_owned(),
            ActiveOAuth {
                server_id: "test-server".to_owned(),
                created: Instant::now(),
                cancel: cancel.clone(),
            },
        );
        fixture.service.cancel_oauth("test-flow").unwrap();
        assert!(cancel.is_cancelled());
    });
}

#[test]
fn declaration_reconciliation_still_revokes_inflight_authority() {
    bounded(|| {
        let fixture = Fixture::new();
        let initial = "schema_version: 1\nmcp_servers:\n  - id: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n    display_name: Test\n    enabled: false\n    transport:\n      type: remote\n      endpoint: https://example.invalid/mcp\n      authorization:\n        type: none\n";
        std::fs::write(fixture.root.join("config.yaml"), initial).unwrap();
        fixture.service.reconcile_declarations().unwrap();
        let before = fixture.service.list().unwrap().remove(0);
        let connecting = fixture.service.lock().unwrap().connection_token(&before.id);
        std::fs::write(
            fixture.root.join("config.yaml"),
            initial.replace("display_name: Test", "display_name: Changed"),
        )
        .unwrap();
        assert!(fixture.service.step_up_offers().unwrap().is_empty());
        assert!(connecting.is_cancelled());
        let after = fixture.service.list().unwrap().remove(0);
        assert!(after.config_revision > before.config_revision);
        assert_eq!(after.form.display_name, "Changed");
        let connecting = fixture.service.lock().unwrap().connection_token(&before.id);
        std::fs::write(
            fixture.root.join("config.yaml"),
            "schema_version: [invalid]",
        )
        .unwrap();
        assert!(fixture.service.step_up_offers().is_err());
        assert!(connecting.is_cancelled());
    });
}
