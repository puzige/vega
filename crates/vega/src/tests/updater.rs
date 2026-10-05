use super::*;

const INSTALL_BUSY_MESSAGE: &str = "请先结束所有会话中的运行任务并关闭终端，再重启安装";

struct InstallingReset;

impl Drop for InstallingReset {
    fn drop(&mut self) {
        crate::updater::set_installing(false);
    }
}

fn assert_install_rejected(
    root: &Entity<VegaWindow>,
    cx: &mut gpui_kit::TestAppContext,
    receiver: &mpsc::Receiver<UpdateRequest>,
    owner: &str,
) {
    root.update(cx, |root, cx| {
        root.request_update(UpdateRequest::Install, cx)
    });
    let (phase, message) = root.read_with(cx, |root, _| {
        (
            root.updater.state.phase.clone(),
            root.updater.state.message.clone(),
        )
    });
    assert_eq!(phase, UpdatePhase::Ready, "{owner}");
    assert_eq!(message, INSTALL_BUSY_MESSAGE, "{owner}");
    assert!(!crate::updater::installing(), "{owner}");
    assert!(
        matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "{owner} dispatched an install request"
    );
}

#[gpui_kit::test]
async fn issue181_install_refuses_each_active_owner_without_dispatching(
    cx: &mut gpui_kit::TestAppContext,
) {
    crate::updater::set_installing(false);
    let _installing_reset = InstallingReset;
    let repo = tempfile::tempdir().expect("updater workspace root");
    let store = Store::open(":memory:").expect("updater test store");
    store.migrate().expect("updater test migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 updater fixture root"),
        "updater-guard",
        None,
    )
    .expect("updater test project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("updater test thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    let (sender, receiver) = mpsc::sync_channel(1);
    root.update(cx, |root, _| {
        root.updater.state.phase = UpdatePhase::Ready;
        root.updater.state.message = "更新已准备好".into();
        root.updater.set_test_request_sender(sender);
    });

    root.update(cx, |root, _| {
        root.agent_controller
            .begin(thread.id.clone(), stream.clone(), None, None)
            .expect("active agent fixture");
    });
    assert_install_rejected(&root, cx, &receiver, "active agent run");
    root.update(cx, |root, _| {
        root.agent_controller.active.clear();
    });

    root.update(cx, |root, _| {
        root.agent_controller.preparation_stream = Some(stream.clone());
    });
    assert_install_rejected(&root, cx, &receiver, "agent preparation");
    root.update(cx, |root, _| {
        root.agent_controller.preparation_stream = None;
    });

    root.update(cx, |root, _| {
        root.trusted_actions
            .acquire(TrustedActionKind::ContextSettings, 1, 1)
            .expect("trusted action fixture");
    });
    assert_install_rejected(&root, cx, &receiver, "trusted action");
    root.update(cx, |root, _| {
        root.trusted_actions = TrustedActionCoordinator::default();
    });

    root.update(cx, |root, _| {
        root.reasoning_save_pending = Some((1, 1));
    });
    assert_install_rejected(&root, cx, &receiver, "reasoning save");
    root.update(cx, |root, _| {
        root.reasoning_save_pending = None;
    });

    let terminal = cx.new(|cx| {
        vega_ui::terminal::TerminalView::for_test_target(
            vega_conversation::types::TerminalTarget::Directory(repo.path().to_path_buf()),
            cx,
        )
    });
    root.update(cx, |root, _| {
        root.add_test_terminal(1, thread.project_id.clone(), terminal);
    });
    assert_install_rejected(&root, cx, &receiver, "workspace terminal");
    root.update(cx, |root, _| {
        root.clear_test_terminals();
    });

    root.update(cx, |root, _| {
        root.artifact_controller
            .begin(&thread, stream.clone(), repo.path().to_path_buf())
            .expect("artifact route fixture");
        root.artifact_controller
            .active
            .as_mut()
            .expect("active artifact route")
            .terminal_in_flight = Some(1);
    });
    assert_install_rejected(&root, cx, &receiver, "active artifact terminal");
    root.update(cx, |root, _| {
        let mut route = root
            .artifact_controller
            .active
            .take()
            .expect("active artifact route");
        route.terminal_in_flight = None;
        route.terminal_queue.push_back(ArtifactTerminalJob {
            sequence: 2,
            work: ArtifactTerminalWork::Refresh,
        });
        let epoch = route.identity.epoch;
        root.artifact_controller.retained.insert(epoch, route);
    });
    assert_install_rejected(&root, cx, &receiver, "retained artifact terminal");
}
