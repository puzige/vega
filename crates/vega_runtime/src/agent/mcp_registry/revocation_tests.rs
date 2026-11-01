use super::*;

fn ready_server() -> McpReadyServer {
    McpReadyServer {
        server_id: "synthetic-server".into(),
        config_revision: 1,
        catalog: Catalog {
            tools: Vec::new(),
            rejected: Vec::new(),
        },
        dispatcher: Arc::new(LiveMcpDispatcher {
            connection: Arc::new(Mutex::new(None)),
            tools_by_name: HashMap::new(),
            scope_challenge_sink: None,
            admission_check: OnceLock::new(),
        }),
        revoked: CancellationToken::new(),
        known_credentials: Arc::new(KnownCredentials::default()),
        transport: McpTransportKind::StreamableHttp,
        server_display_name: None,
    }
}

#[test]
fn revocation_remains_effective_after_a_new_connection_is_created() {
    let old = ready_server();
    let old_clone = old.clone();
    let lease = old.revocation_lease();
    assert!(lease.is_live());
    lease.revoke();
    assert!(old.is_revoked());
    assert!(old_clone.is_revoked());
    assert!(!lease.is_live());

    let fresh = ready_server();
    let fresh_lease = fresh.revocation_lease();
    assert!(!fresh.is_revoked());
    assert!(fresh_lease.is_live());
    assert!(!fresh_lease.same_connection(&lease));
    assert!(old.is_revoked());
    lease.revoke();
    assert!(fresh_lease.is_live());
}

#[test]
fn dropping_a_transport_never_leaves_a_live_revocation_lease() {
    let ready = ready_server();
    let lease = ready.revocation_lease();
    assert!(lease.is_live());
    drop(ready);
    assert!(!lease.is_live());
    let fresh = ready_server();
    assert!(fresh.revocation_lease().is_live());
    assert!(!lease.is_live());
}
