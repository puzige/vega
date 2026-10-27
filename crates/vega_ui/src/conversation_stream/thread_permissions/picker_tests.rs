use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{AppContext, TestAppContext};
use vega_conversation::types::{
    DefaultPermissionSnapshot, FullAccessGrant, FullAccessScope, PermissionMode, Thread,
};

use crate::conversation_stream::{ConversationStream, ThreadSettingsRequested};
use crate::sidebar::NewTaskDefaults;

const EPOCH: &str = "01M5STGCTHCT3S8Q1BPZA2HCYG";

#[gpui_kit::test]
fn selecting_current_full_access_only_requests_missing_authorization(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
    });
    let mut authorized = vega_conversation::threads::draft_thread(None, "fixture", "full_access");
    authorized.full_access_grant = FullAccessGrant::confirmed(
        EPOCH,
        FullAccessScope::Thread {
            thread_id: authorized.id.clone(),
        },
        1,
    );
    authorized.permission_revision = 2;
    let configuration_id = "/isolated/config.toml".to_owned();
    let default_grant = FullAccessGrant::confirmed(
        EPOCH,
        FullAccessScope::FutureThreads {
            configuration_id: configuration_id.clone(),
        },
        1,
    )
    .expect("future authorization");
    let snapshot = DefaultPermissionSnapshot {
        config_path: Some(configuration_id.clone().into()),
        configuration_id: Some(configuration_id),
        permission_mode: "full_access".to_owned(),
        permission_revision: 1,
        full_access_grant: Some(serde_json::to_string(&default_grant).expect("encoded grant")),
    };
    let inherited =
        vega_conversation::threads::draft_thread_from_defaults(None, "fixture", snapshot);
    let mut missing = authorized.clone();
    missing.full_access_grant = None;
    let mut wrong_thread = authorized.clone();
    wrong_thread.id = vega_conversation::threads::new_thread_id();
    let mut wrong_scope = authorized.clone();
    wrong_scope.full_access_grant = Some(default_grant);
    let mut mismatched_defaults = inherited.clone();
    mismatched_defaults
        .default_permission
        .as_mut()
        .expect("defaults")
        .configuration_id = Some("/other/config.toml".to_owned());
    let mut missing_source = inherited.clone();
    missing_source
        .default_permission
        .as_mut()
        .expect("defaults")
        .config_path = None;
    let mut corrupt_defaults = inherited.clone();
    corrupt_defaults
        .default_permission
        .as_mut()
        .expect("defaults")
        .full_access_grant = Some("{}".to_owned());
    let mut fork = inherited.clone();
    fork.forked_from_thread_id = Some(authorized.id.clone());
    let mut changed_revision = inherited.clone();
    changed_revision.permission_revision = 1;
    for (name, thread, needs_authorization) in [
        ("authorized conversation", authorized, false),
        ("inherited future authorization", inherited, false),
        ("legacy without authorization", missing, true),
        ("another conversation's grant", wrong_thread, true),
        ("future grant on existing conversation", wrong_scope, true),
        ("another configuration's default", mismatched_defaults, true),
        ("missing configuration source", missing_source, true),
        ("corrupt default grant", corrupt_defaults, true),
        ("fork cannot inherit defaults", fork, true),
        ("changed draft revision", changed_revision, true),
    ] {
        cx.update(|cx| {
            if let Some(permission) = thread.default_permission.clone() {
                cx.set_global(NewTaskDefaults {
                    model: "fixture".to_owned(),
                    permission,
                });
            }
        });
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let received = requests.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&stream, move |_, request: &ThreadSettingsRequested, _| {
                received.borrow_mut().push(request.permission_mode);
            })
        });
        stream.update(cx, |stream, cx| {
            assert_eq!(
                stream.full_access_needs_confirmation(cx),
                needs_authorization,
                "{name}"
            );
            stream.request_permission_mode(PermissionMode::FullAccess, cx);
        });
        cx.run_until_parked();
        assert_eq!(
            requests.borrow().as_slice(),
            if needs_authorization {
                &[Some(PermissionMode::FullAccess)][..]
            } else {
                &[][..]
            },
            "{name}"
        );
        stream.read_with(cx, |stream, _| assert_eq!(stream.thread, thread, "{name}"));
        requests.borrow_mut().clear();
        stream.update(cx, |stream, cx| {
            stream.request_permission_mode(PermissionMode::Confirm, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            requests.borrow().as_slice(),
            &[Some(PermissionMode::Confirm)],
            "{name}"
        );
        if !needs_authorization && let Some(snapshot) = &thread.default_permission {
            for change_mode in [true, false] {
                let mut permission = snapshot.clone();
                if change_mode {
                    permission.permission_mode = "confirm".to_owned();
                    permission.full_access_grant = None;
                } else {
                    permission.permission_revision += 1;
                }
                cx.update(|cx| {
                    cx.set_global(NewTaskDefaults {
                        model: "fixture".to_owned(),
                        permission,
                    })
                });
                requests.borrow_mut().clear();
                stream.update(cx, |stream, cx| {
                    assert!(stream.full_access_needs_confirmation(cx));
                    stream.request_permission_mode(PermissionMode::FullAccess, cx);
                });
                cx.run_until_parked();
                assert_eq!(
                    requests.borrow().as_slice(),
                    &[Some(PermissionMode::FullAccess)]
                );
                stream.read_with(cx, |stream, _| assert_eq!(stream.thread, thread));
            }
        }
    }
}

#[gpui_kit::test]
fn entering_full_access_from_a_restricted_mode_still_requests_confirmation(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
    });
    for mode in [
        PermissionMode::ReadOnly,
        PermissionMode::Confirm,
        PermissionMode::Auto,
    ] {
        let thread: Thread =
            vega_conversation::threads::draft_thread(None, "fixture", mode.as_str());
        let stream = cx.new(|cx| ConversationStream::new(thread, cx));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let received = requests.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&stream, move |_, request: &ThreadSettingsRequested, _| {
                received.borrow_mut().push(request.permission_mode);
            })
        });
        stream.update(cx, |stream, cx| stream.request_permission_mode(mode, cx));
        cx.run_until_parked();
        assert!(requests.borrow().is_empty());
        stream.update(cx, |stream, cx| {
            stream.request_permission_mode(PermissionMode::FullAccess, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            requests.borrow().as_slice(),
            &[Some(PermissionMode::FullAccess)]
        );
    }
}
