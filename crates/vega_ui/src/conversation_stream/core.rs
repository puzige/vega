use super::*;

/// The opened-thread content view: thread header (title + anchor status +
/// demo-inject button), the virtualized message stream, and the fixed-bottom
/// Composer. One entity per open thread; rebuilt by the window root when
/// another thread opens.
pub struct ConversationStream {
    pub(crate) thread: Thread,
    /// 消息块列表（T18）：user 回显与 assistant 流交替，顺序即会话顺序。
    pub(crate) entries: Vec<StreamEntry>,
    pub(crate) counters: Arc<StreamCounters>,
    /// Variable-height list state (S8-T44/C4): one item per semantic entry,
    /// natural heights. The same state owns the scroll position and the P4
    /// tail-follow semantics (`FollowMode::Tail`).
    pub(crate) list: gpui::ListState,
    /// Active demo injection (`None` = idle/finished).
    pub(crate) injecting: Option<InjectionState>,
    /// Composer 输入状态（独立 `TextInput` Entity，固定 3 行多行）。
    pub(crate) input: Entity<TextInput>,
    /// Synthetic block-id counter for user echo rows (diagnostics only).
    pub(crate) user_block_seq: u64,
    /// Opaque provider call ids are retained only as non-rendered map keys.
    pub(crate) tool_cards: HashMap<String, Entity<ToolCard>>,
    /// Exact call id to its sole inline artifact card.
    pub(crate) artifact_cards: HashMap<String, Entity<ArtifactCard>>,
    /// Route-owned safe branch selector; Git authority remains in the app controller.
    pub(crate) branch_selector: Entity<BranchSelector>,
    /// IO-free canonical commit panel; repository authority stays in app/headless.
    pub(crate) commit_panel: Entity<CommitPanel>,
    /// Concrete runtime permission hook shared by the owning conversation.
    pub(crate) permission_queue: PermissionQueue,
    /// The sole visible prompt; the opaque call id is only a map association.
    pub(crate) active_permission: Option<Entity<PermissionCard>>,
    /// Plan ids are opaque map keys; card content is a typed projection.
    pub(crate) plan_cards: HashMap<String, Entity<PlanCard>>,
    /// Sole applied per-task cost summary keyed by assistant message id
    /// (S7-T40); duplicate/later stale applications are ignored.
    pub(crate) summary_cards: HashMap<String, Entity<SummaryCard>>,
    /// Scroll-up hydration state (S8-T45/C7): keyset cursor of the oldest
    /// loaded page, in-flight flag, and the failure pause. The stream itself
    /// never queries SQLite — pages arrive as typed projections.
    pub(crate) hydration: HistoryHydration,
    /// Exact active durable assistant id and its stream-entry index.
    pub(crate) active_agent_message: Option<(String, usize)>,
    /// Most recently finished assistant entry, retained until the typed Plan
    /// projection can replace it in place.
    pub(crate) last_finished_agent_message: Option<(String, usize)>,
    /// Bounded token/cost meter projection (S7-T39/C3/C4). Pure shared
    /// `vega_conversation::types` state: no IO, no persistence; the Composer
    /// renders its snapshot and every update is checked arithmetic only.
    pub(crate) meter: ConversationMeter,
    /// Submitted drafts, scoped to this thread view.
    pub(crate) composer_history: Vec<String>,
    pub(crate) composer_submit_pending: bool,
    pub(crate) history_cursor: Option<usize>,
    pub(crate) history_draft: Option<String>,
    pub(crate) approved_not_started: bool,
    pub(crate) trusted_action_busy: bool,
    pub(crate) setting_focus: [FocusHandle; 7],
    pub(crate) controller_error: Option<String>,
    /// Bounded `@file` selector model (A2-12). Pure UI state over the typed
    /// [`FileIndexSnapshot`]; the app layer owns the filesystem walk.
    pub(crate) file_selector: FileSelectorModel,
    /// The latest bounded candidate projection for this thread's project.
    pub(crate) file_snapshot: FileIndexSnapshot,
    /// One bounded walk in flight / completed bookkeeping (one request per
    /// stream lifetime; later opens re-filter the projection locally).
    pub(crate) file_index_loading: bool,
    pub(crate) file_index_loaded: bool,
    /// Provider/model/thinking composer defaults (A2-14). Display state for
    /// the selector; authority is the app-level config seam.
    pub(crate) composer_defaults: ComposerDefaults,
    /// Priced model options for the selector (from the T36 pricing catalog
    /// via the app layer's typed projection; zero file IO here).
    pub(crate) model_options: Vec<String>,
    /// Model selector popover state (open/closed + highlight row).
    pub(crate) model_selector_open: bool,
    pub(crate) model_selector_highlight: usize,
    /// Keyboard focus stop for the model selector trigger (A2-14).
    pub(crate) model_focus: FocusHandle,
    /// Cancels the watch listener and drops its fail-closed guard with the view.
    pub(crate) _permission_listener_task: gpui::Task<()>,
}

impl EventEmitter<PlanReviewRequested> for ConversationStream {}
impl EventEmitter<ThreadSettingsRequested> for ConversationStream {}
impl EventEmitter<ComposerSubmitted> for ConversationStream {}
impl EventEmitter<OpenWorkspaceDiffRequested> for ConversationStream {}
impl EventEmitter<OpenCommitPanelRequested> for ConversationStream {}
impl EventEmitter<WorkspaceToolTerminal> for ConversationStream {}
impl EventEmitter<HistoryPageRequested> for ConversationStream {}
impl EventEmitter<FileIndexRequested> for ConversationStream {}
impl EventEmitter<ComposerDefaultsRequested> for ConversationStream {}

pub(crate) struct InjectionState {
    /// Which assistant entry the replayer feeds.
    pub(crate) entry_index: usize,
    /// The public mock replayer (vega_markdown::replay，T18 公共化).
    pub(crate) replay: MockReplay,
}

impl ConversationStream {
    /// Builds the view for `thread` with an empty in-memory stream (S3 无消息
    /// 持久化：会话内容由流式注入与 Composer 回显产生，不落库；重启后清空
    /// 是预期行为).
    pub fn new(thread: Thread, cx: &mut Context<Self>) -> Self {
        Self::new_with_permission_queue(thread, PermissionQueue::new(), cx)
    }

    /// Builds a stream with the exact permission queue passed to the runtime.
    pub fn new_with_permission_queue(
        thread: Thread,
        permission_queue: PermissionQueue,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextInput::new_multiline(
                cx,
                "输入消息…（Enter 换行 · Cmd+Enter 发送）",
                COMPOSER_ROWS,
            )
        });
        let branch_selector =
            cx.new(|cx| BranchSelector::new(thread.id.clone(), thread.project_id.clone(), cx));
        let commit_panel =
            cx.new(|cx| CommitPanel::new(thread.id.clone(), thread.project_id.clone(), cx));
        // 空输入禁用发送 + `@` 触发的文件选择跟随输入内容变化：输入内容
        // 变化即重渲染 Composer。
        cx.observe(&input, |this, input, cx| this.sync_at_query(&input, cx))
            .detach();
        let mut listener = permission_queue.subscribe();
        let permission_listener_task = cx.spawn(async move |this, cx| {
            while listener.changed().await {
                let alive = this
                    .update(cx, |this, cx| this.install_pending_permission(cx))
                    .is_ok();
                if !alive {
                    break;
                }
            }
        });
        cx.observe_global::<SettingsOpen>(|this, cx| {
            if cx
                .try_global::<SettingsOpen>()
                .is_some_and(|settings| settings.0)
            {
                this.timeout_permission(cx);
            }
        })
        .detach();
        // One item per semantic entry at natural height; Tail follow is the
        // P4 anchor (贴底跟随 / 上翻 detach / 回底 resume)，由列表原生承担
        // （任何上滚事件 detach，回到距底 1px 内恢复——容差与旧锚定状态机
        // 一致）。600px overdraw 保证滚动方向切换时前后各一屏已测量。
        let list = gpui::ListState::new(0, gpui::ListAlignment::Top, px(600.0));
        list.set_follow_mode(gpui::FollowMode::Tail);
        Self {
            thread,
            entries: Vec::new(),
            counters: Arc::new(StreamCounters::default()),
            list,
            injecting: None,
            input,
            user_block_seq: USER_BLOCK_BASE,
            tool_cards: HashMap::new(),
            artifact_cards: HashMap::new(),
            branch_selector,
            commit_panel,
            permission_queue,
            active_permission: None,
            plan_cards: HashMap::new(),
            summary_cards: HashMap::new(),
            hydration: HistoryHydration::default(),
            active_agent_message: None,
            last_finished_agent_message: None,
            meter: ConversationMeter::default(),
            composer_history: Vec::new(),
            composer_submit_pending: false,
            history_cursor: None,
            history_draft: None,
            approved_not_started: false,
            trusted_action_busy: false,
            setting_focus: [
                cx.focus_handle().tab_index(10).tab_stop(true),
                cx.focus_handle().tab_index(11).tab_stop(true),
                cx.focus_handle().tab_index(12).tab_stop(true),
                cx.focus_handle().tab_index(13).tab_stop(true),
                cx.focus_handle().tab_index(14).tab_stop(true),
                cx.focus_handle().tab_index(15).tab_stop(true),
                // Thinking-level chip (A2-14): its own keyboard stop.
                cx.focus_handle().tab_index(17).tab_stop(true),
            ],
            controller_error: None,
            file_selector: FileSelectorModel::default(),
            file_snapshot: FileIndexSnapshot::default(),
            file_index_loading: false,
            file_index_loaded: false,
            composer_defaults: ComposerDefaults::default(),
            model_options: Vec::new(),
            model_selector_open: false,
            model_selector_highlight: 0,
            model_focus: cx.focus_handle().tab_index(16).tab_stop(true),
            _permission_listener_task: permission_listener_task,
        }
    }

    // ── variable-height list maintenance (S8-T44/C4) ────────────────────────
    //
    // One list item per semantic entry. Structural entry mutations notify the
    // list through `splice`; content mutations through `remeasure_items`
    // (Absolute scroll anchor: the pixel offset into the scroll-top item is
    // preserved, keeping anchor drift <1px). Frozen items are never touched.

    /// Registers `entries.len() - previous_len` appended items with the list.
    pub(crate) fn list_append(&mut self, previous_len: usize) {
        let count = self.entries.len();
        debug_assert!(count >= previous_len);
        if count > previous_len {
            self.list
                .splice(previous_len..previous_len, count - previous_len);
        }
    }

    /// Registers one item inserted at `index` (e.g. an inline artifact after
    /// its exact tool).
    pub(crate) fn list_insert(&mut self, index: usize) {
        self.list.splice(index..index, 1);
    }

    /// Registers `count` items prepended above the loaded history
    /// (S8-T45/C7). `splice` shifts `logical_scroll_top` by the insert
    /// count while keeping the pixel offset into the scroll-top item, so
    /// the page-boundary anchor is exact (drift <1px by construction).
    pub(crate) fn list_prepend(&mut self, count: usize) {
        if count > 0 {
            self.list.splice(0..0, count);
        }
    }

    /// Registers the removal of the item at `index` (permission resolution).
    pub(crate) fn list_remove(&mut self, index: usize) {
        self.list.splice(index..index + 1, 0);
    }

    /// Marks the item at `index` for height re-measurement (mutable tail or
    /// explicitly invalidated item; the C4 rematerialize whitelist).
    pub(crate) fn invalidate_item(&mut self, index: Option<usize>) {
        if let Some(index) = index
            && index < self.list.item_count()
        {
            self.list.remeasure_items(index..index + 1);
        }
    }

    /// Entry index matching `predicate`, for in-place card invalidation.
    pub(crate) fn entry_index_where(
        &self,
        predicate: impl Fn(&StreamEntry) -> bool,
    ) -> Option<usize> {
        self.entries.iter().position(predicate)
    }

    /// Whether the list is currently following the tail (the P4 anchor
    /// state, read by the header indicator).
    pub(crate) fn following_tail(&self) -> bool {
        self.list.is_following_tail()
    }

    /// Applies the authoritative persisted thread settings after a request.
    pub fn apply_thread(&mut self, thread: Thread, cx: &mut Context<Self>) {
        if thread.id == self.thread.id && thread.project_id == self.thread.project_id {
            self.thread = thread;
            self.controller_error = None;
            cx.notify();
        }
    }

    /// Seeds thread-scoped Composer history from the typed conversation
    /// projection. This is called once when a stream entity is constructed.
    pub fn apply_composer_history(
        &mut self,
        thread_id: &str,
        history: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if thread_id != self.thread.id || self.composer_submit_pending {
            return;
        }
        self.composer_history = history;
        self.history_cursor = None;
        self.history_draft = None;
        cx.notify();
    }

    /// Displays a bounded controller failure without changing authoritative
    /// selected state.
    pub fn apply_controller_error(&mut self, cx: &mut Context<Self>) {
        self.controller_error = Some("操作未保存，请重试".into());
        cx.notify();
    }

    /// Applies the bounded `@file` candidate projection (A2-12) handed back
    /// by the app layer's worker. The open selector re-filters; a failed
    /// walk leaves the selector with whatever is already loaded (empty on
    /// first failure — the selector simply offers nothing, never guesses).
    pub fn apply_file_index(&mut self, snapshot: FileIndexSnapshot, cx: &mut Context<Self>) {
        self.file_index_loading = false;
        self.file_index_loaded = true;
        let query = self
            .input
            .read(cx)
            .trailing_at_query()
            .map(|(_, query)| query)
            .unwrap_or_default();
        if query.is_empty() && !self.file_selector.is_open() {
            self.file_snapshot = snapshot;
            cx.notify();
            return;
        }
        self.file_snapshot = snapshot;
        self.file_selector.open_for(&self.file_snapshot, &query);
        cx.notify();
    }

    /// Reflects the authoritative provider/model/thinking composer defaults
    /// (A2-14) after the app persisted a selection at the config seam.
    pub fn apply_composer_defaults(&mut self, defaults: ComposerDefaults, cx: &mut Context<Self>) {
        self.composer_defaults = defaults;
        self.model_selector_open = false;
        cx.notify();
    }

    /// Installs the priced model options for the composer selector (A2-14):
    /// the app layer projects the T36 pricing catalog's model list; the
    /// stream never reads pricing files.
    pub fn apply_model_options(&mut self, options: Vec<String>, cx: &mut Context<Self>) {
        self.model_options = options;
        self.model_selector_highlight = self
            .model_options
            .iter()
            .position(|model| *model == self.composer_defaults.model)
            .unwrap_or(0);
        cx.notify();
    }

    /// Follows the caret into/out of an `@token` (A2-12). The first `@` in a
    /// session triggers exactly one bounded index request (first-wins; later
    /// opens re-filter the cached projection). No token → selector closed.
    pub(crate) fn sync_at_query(&mut self, input: &Entity<TextInput>, cx: &mut Context<Self>) {
        let query = input.read(cx).trailing_at_query().map(|(_, query)| query);
        match query {
            None => {
                if self.file_selector.close() {
                    cx.notify();
                }
            }
            Some(query) => {
                if !self.file_index_loaded && !self.file_index_loading {
                    self.file_index_loading = true;
                    cx.emit(FileIndexRequested {
                        thread_id: self.thread.id.clone(),
                        project_id: self.thread.project_id.clone(),
                    });
                }
                self.file_selector.open_for(&self.file_snapshot, &query);
                cx.notify();
            }
        }
    }

    /// `up` in an open selector moves the highlight instead of history
    /// recall (ui-spec §6 键盘可达；选择器打开时按键先到选择器).
    pub(crate) fn on_selector_previous(
        &mut self,
        _: &PreviousFile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_selector.move_highlight(-1);
        cx.notify();
    }

    pub(crate) fn on_selector_next(
        &mut self,
        _: &NextFile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_selector.move_highlight(1);
        cx.notify();
    }

    pub(crate) fn on_selector_cancel(
        &mut self,
        _: &CancelFile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_selector.close();
        cx.notify();
    }

    /// Enter/Tab in an open selector accepts first-wins and completes the
    /// `@token` in the composer input; the send binding never fires through
    /// this path (the selector context shadows it while open).
    pub(crate) fn on_selector_accept(
        &mut self,
        _: &AcceptFile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.file_selector.accept() {
            self.input
                .update(cx, |input, cx| input.complete_at_query(&path, cx));
        }
        cx.notify();
    }

    /// Enter/Space on the focused model selector trigger (A2-14): closed →
    /// open; open → accept the highlighted model (first-wins default when
    /// nothing was highlighted).
    pub(crate) fn on_activate_model(
        &mut self,
        _: &ActivateModel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model_options.is_empty() {
            return;
        }
        if !self.model_selector_open {
            self.model_selector_open = true;
            self.model_selector_highlight = self
                .model_options
                .iter()
                .position(|model| *model == self.composer_defaults.model)
                .unwrap_or(0);
            cx.notify();
            return;
        }
        if let Some(model) = self
            .model_options
            .get(self.model_selector_highlight)
            .cloned()
        {
            self.composer_defaults.model = model;
            self.model_selector_open = false;
            cx.emit(ComposerDefaultsRequested {
                thread_id: self.thread.id.clone(),
                defaults: self.composer_defaults.clone(),
            });
        }
        cx.notify();
    }

    pub(crate) fn on_model_previous(
        &mut self,
        _: &PreviousModel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.model_selector_open || self.model_options.is_empty() {
            return;
        }
        self.model_selector_highlight = self.model_selector_highlight.saturating_sub(1);
        cx.notify();
    }

    pub(crate) fn on_model_next(&mut self, _: &NextModel, _: &mut Window, cx: &mut Context<Self>) {
        if !self.model_selector_open {
            return;
        }
        if self.model_selector_highlight + 1 < self.model_options.len() {
            self.model_selector_highlight += 1;
        }
        cx.notify();
    }

    pub(crate) fn on_model_close(
        &mut self,
        _: &CloseModel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model_selector_open = false;
        cx.notify();
    }

    pub(crate) fn select_model_option(&mut self, model: &str, cx: &mut Context<Self>) {
        self.composer_defaults.model = model.to_string();
        self.model_selector_open = false;
        cx.emit(ComposerDefaultsRequested {
            thread_id: self.thread.id.clone(),
            defaults: self.composer_defaults.clone(),
        });
        cx.notify();
    }

    pub(crate) fn cycle_thinking_clicked(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_thinking(cx);
    }

    pub(crate) fn on_cycle_thinking(
        &mut self,
        _: &CycleThinking,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_thinking(cx);
    }

    /// Rotates off → low → medium → high → off and persists through the
    /// app-level config seam (A2-14 thinking 档位).
    pub(crate) fn cycle_thinking(&mut self, cx: &mut Context<Self>) {
        let levels = ["off", "low", "medium", "high"];
        let next = levels
            .iter()
            .position(|level| *level == self.composer_defaults.thinking)
            .map_or(0, |index| (index + 1) % levels.len());
        self.composer_defaults.thinking = levels[next].to_string();
        cx.emit(ComposerDefaultsRequested {
            thread_id: self.thread.id.clone(),
            defaults: self.composer_defaults.clone(),
        });
        cx.notify();
    }

    /// Displays a bounded provider/runner failure after durable preparation.
    pub fn apply_agent_error(&mut self, cx: &mut Context<Self>) {
        self.controller_error = Some("执行未完成，可安全重试".into());
        // S7-T39: run-scoped estimate state never survives a failure path.
        self.meter.end_run();
        cx.notify();
    }

    /// Installs the frozen per-run provisional estimator (S7-T39/C3 run
    /// ownership). Called by the app exactly once per agent run, immediately
    /// after `agent_controller.begin`.
    pub fn install_meter_estimator(
        &mut self,
        estimator: Option<RunUsageEstimator>,
        cx: &mut Context<Self>,
    ) {
        self.meter.install_run_estimator(estimator);
        cx.notify();
    }

    /// Restores the calibrated counter baseline from the durable checked
    /// aggregate (S7-T39/C4 restart recovery). Called once when a stream
    /// entity is constructed for a thread with priced usage history.
    pub fn restore_meter(&mut self, usage: RestoredUsage, cx: &mut Context<Self>) {
        self.meter.restore(usage);
        cx.notify();
    }

    /// Current counter projection (C4): the Composer renders exactly this.
    pub fn meter_snapshot(&self) -> MeterSnapshot {
        self.meter.snapshot()
    }

    /// Feeds one conversation event through the meter projection, fenced by
    /// the same acceptance rules the stream applies (S7-T39 thread/run fence:
    /// late or stale-message events never calibrate or estimate).
    pub(crate) fn feed_meter(&mut self, event: &ConversationEvent, cx: &mut Context<Self>) {
        let accepted = match event {
            ConversationEvent::MessageStarted { .. } => self.active_agent_message.is_none(),
            ConversationEvent::TextDelta { message_id, .. }
            | ConversationEvent::UsageUpdated { message_id, .. } => self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active_id, _)| active_id == message_id),
            ConversationEvent::MessageFinished { message_id, .. }
            | ConversationEvent::Interrupted { message_id } => self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active_id, _)| active_id == message_id),
            ConversationEvent::Error { message_id, .. } => match message_id {
                Some(message_id) => self
                    .active_agent_message
                    .as_ref()
                    .is_some_and(|(active_id, _)| active_id == message_id),
                None => true,
            },
            // Tool proposals/results carry no assistant id; the meter gates
            // them on its own run state. Thinking is never visible output.
            ConversationEvent::ToolCallProposed { .. }
            | ConversationEvent::ToolCallApproved { .. }
            | ConversationEvent::ToolCallOutput { .. }
            | ConversationEvent::ToolCallFinished { .. } => true,
            ConversationEvent::ThinkingDelta { .. } => false,
        };
        if accepted && self.meter.apply(event) {
            cx.notify();
        }
    }
}
