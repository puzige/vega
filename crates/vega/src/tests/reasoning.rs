use super::model_selection::model_selection_config;
use super::*;

use vega_store::config::AppConfig;
use vega_ui::settings::{ReasoningSettingsProjection, ReasoningTemplate};

/// Owned three-tier reasoning profile for the R57 P3 slider tests.
///
/// The tier list is deliberately three long: the reference implementation
/// renders seven because *its* model supports seven (spec §3.4 R7). A model
/// declaring three must render three, which is the property these tests pin.
fn three_tier_reasoning_config() -> vega_store::reasoning::ReasoningConfig {
    vega_store::reasoning::ReasoningConfig {
        version: vega_store::reasoning::REASONING_CONFIG_VERSION,
        profiles: vec![vega_store::reasoning::ReasoningProfile {
            provider: "owned".into(),
            model: "gpt-5.6-terra".into(),
            protocol: "openai_chat_completions".into(),
            support: "optional".into(),
            efforts: vec!["low".into(), "medium".into(), "high".into()],
            supports_disabled: false,
            disabled_wire: None,
            preserve_reasoning_content: false,
            preference: "medium".into(),
        }],
    }
}

/// R58: the same three-tier profile, but declaring the disabled operation.
///
/// `openai_chat_completions` + `reasoning_effort_none` is the one pairing the
/// store and runtime validators accept for `supports_disabled` on an OpenAI
/// protocol profile, so this is a real capability declaration rather than a
/// hand-written one.
fn three_tier_disabled_reasoning_config() -> vega_store::reasoning::ReasoningConfig {
    let mut config = three_tier_reasoning_config();
    if let Some(profile) = config.profiles.first_mut() {
        profile.supports_disabled = true;
        profile.disabled_wire = Some("reasoning_effort_none".into());
    }
    config
}

/// The R57 P3 fixture: a real `VegaWindow`, a real stream, the owned config and
/// reasoning files, and the same subscriptions production installs.
struct ThinkingSliderFixture {
    _config_root: TempDir,
    _data_root: TempDir,
    reasoning_path: std::path::PathBuf,
    database_path: std::path::PathBuf,
    thread: Thread,
    root: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    window: gpui_kit::WindowHandle<VegaWindow>,
    /// The concrete mock behind `agent_provider_override`, so a test can read
    /// the exact `ChatRequest` the run froze (R58 A3).
    provider: Arc<vega_runtime::MockProvider>,
}

impl ThinkingSliderFixture {
    /// Builds the fixture and waits until the catalog worker has published the
    /// owned reasoning profile onto the stream.
    fn open(cx: &mut gpui_kit::TestAppContext) -> Self {
        Self::open_with(cx, three_tier_reasoning_config())
    }

    /// R58: the same fixture over an arbitrary owned reasoning profile, so a
    /// test can declare the disabled capability without hand-writing a
    /// projection the store would reject.
    fn open_with(
        cx: &mut gpui_kit::TestAppContext,
        reasoning_config: vega_store::reasoning::ReasoningConfig,
    ) -> Self {
        let config_root = tempfile::tempdir().expect("slider config root");
        let config_path = config_root.path().join("config.toml");
        model_selection_config(&config_path);
        vega_store::keystore::set_key(
            config_root.path(),
            "owned",
            "fake-owned-provider-key-slider-73",
        )
        .expect("owned slider test credential");
        let reasoning_path = config_root.path().join("reasoning.toml");
        let bytes = vega_store::reasoning::encode(&reasoning_config)
            .expect("encode owned reasoning profile");
        fs::write(&reasoning_path, bytes).expect("owned reasoning file");

        let data_root = tempfile::tempdir().expect("slider data root");
        let database_path = data_root.path().join("vega.db");
        let store = Store::open(&database_path).expect("slider store");
        store.migrate().expect("slider migrations");
        let project = vega_store::projects::create(
            store.conn(),
            data_root.path().to_str().expect("UTF-8 slider path"),
            "slider-e2e",
            None,
        )
        .expect("slider project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "gpt-5.6-terra",
            PermissionMode::Confirm.as_str(),
        )
        .expect("slider thread");
        cx.update(|cx| {
            install_diff_window_globals(
                Store::open(&database_path).expect("slider global store"),
                thread.clone(),
                cx,
            )
        });

        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let provider = Arc::new(vega_runtime::MockProvider::new(Vec::new()));
        let root = cx.new(VegaWindow::new);
        root.update(cx, |root, cx| {
            root.model_selection_config_override = Some(config_path);
            root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
            root.stream_view = Some((thread.id.clone(), stream.clone()));
            // The production stream subscriptions (render.rs): only the tier
            // intent matters here, and it travels the real handler.
            cx.subscribe(
                &stream,
                |this,
                 stream,
                 request: &vega_ui::conversation_stream::ComposerDefaultsRequested,
                 cx| {
                    this.persist_composer_thinking(stream.clone(), request, cx);
                },
            )
            .detach();
            root.start_model_catalog_load(cx);
        });
        let window_root = root.clone();
        let window = cx.update(|cx| {
            cx.open_window(
                gpui_kit::WindowOptions {
                    window_bounds: Some(gpui_kit::WindowBounds::Windowed(
                        gpui_kit::Bounds::centered(
                            None,
                            gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(900.)),
                            cx,
                        ),
                    )),
                    ..Default::default()
                },
                move |_, _| window_root,
            )
            .expect("production slider window")
        });
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                root.configured_reasoning_authority.is_some()
                    && !root.model_catalog_loading
                    && root.reasoning_profile_for_model("gpt-5.6-terra").is_some()
                    && matches!(
                        root.pricing_controller.state,
                        PricingControllerState::Ready { .. }
                    )
            }) && stream.read_with(cx, |stream, _| stream.reasoning_profile().is_some())
        });
        // The model trigger only opens the popup once the priced catalog
        // projection reached the stream, exactly as production requires.
        pump_test_app(cx, |cx| {
            stream.read_with(cx, |stream, _| stream.model_options_len() > 0)
        });
        Self {
            _config_root: config_root,
            _data_root: data_root,
            reasoning_path,
            database_path,
            thread,
            root,
            stream,
            window,
            provider,
        }
    }

    /// Opens the model picker through the production trigger click, if it is
    /// not already open.
    ///
    /// R59 R1: the trigger opens **level one** — the tier slider — so this
    /// asserts the slider level specifically. The trigger toggles: while the
    /// slider level is mounted a second click closes it instead of re-opening,
    /// so the current state is read first.
    fn open_model_popup(&self, cx: &mut gpui_kit::TestAppContext) {
        if self
            .stream
            .read_with(cx, |stream, _| stream.model_picker_level())
            .is_open()
        {
            return;
        }
        let mut visual = gpui_kit::VisualTestContext::from_window(self.window.into(), cx);
        let trigger = visual
            .debug_bounds("composer-model")
            .expect("model trigger");
        visual.simulate_click(trigger.center(), gpui_kit::Modifiers::default());
        visual.run_until_parked();
        assert_eq!(
            self.stream
                .read_with(cx, |stream, _| stream.model_picker_level()),
            vega_ui::conversation_stream::ModelPickerLevel::Slider,
            "R59 R1: the model button must open the tier slider level"
        );
    }

    /// The durable preference read back from the owned reasoning file.
    fn durable_preference(&self) -> String {
        vega_store::reasoning::read_from(&self.reasoning_path)
            .expect("read owned reasoning file")
            .profiles
            .iter()
            .find(|profile| profile.model == "gpt-5.6-terra")
            .expect("owned profile")
            .preference
            .clone()
    }
}

/// R57 P3 / spec §3.4 R7: the slider renders the model's own tier count.
///
/// The model declares three efforts, so the mounted popup must show exactly
/// three dots. A fixed seven (the reference implementation's own model
/// happens to support seven) fails here.
#[gpui_kit::test]
async fn r57_thinking_slider_renders_the_models_declared_tier_count(
    cx: &mut gpui_kit::TestAppContext,
) {
    let fixture = ThinkingSliderFixture::open(cx);
    fixture.open_model_popup(cx);

    let mut visual = gpui_kit::VisualTestContext::from_window(fixture.window.into(), cx);
    assert!(
        visual.debug_bounds("composer-thinking-slider").is_some(),
        "the slider must be mounted inside the model popup"
    );
    assert!(
        visual.debug_bounds("thinking-slider-track").is_some(),
        "the tier track must render for a model that declares tiers"
    );
    // `debug_bounds` takes `&'static str`, so the selector ladder is spelled
    // out. Indices 0..3 exist for this model; 3..7 must not.
    const DOTS: [&str; 7] = [
        "thinking-slider-dot-0",
        "thinking-slider-dot-1",
        "thinking-slider-dot-2",
        "thinking-slider-dot-3",
        "thinking-slider-dot-4",
        "thinking-slider-dot-5",
        "thinking-slider-dot-6",
    ];
    for (index, selector) in DOTS.iter().enumerate() {
        if index < 3 {
            assert!(
                visual.debug_bounds(selector).is_some(),
                "dot {index} must render for a three-tier model"
            );
        } else {
            assert!(
                visual.debug_bounds(selector).is_none(),
                "dot {index} must not render: this model declares only three tiers"
            );
        }
    }

    // The composer's own projection carries the same three tiers.
    let efforts = fixture
        .stream
        .read_with(cx, |stream, _| {
            stream
                .reasoning_profile()
                .map(|profile| profile.efforts.clone())
        })
        .expect("owned profile");
    assert_eq!(efforts, vec!["low", "medium", "high"]);
}

/// R57 P3 / spec §6 A3 + A4: choosing a tier in the mounted slider persists it
/// through the existing `ComposerDefaultsRequested` path, and the composer
/// reflects the persisted tier after a reload.
#[gpui_kit::test]
async fn r57_slider_tier_selection_persists_and_survives_reload(cx: &mut gpui_kit::TestAppContext) {
    let fixture = ThinkingSliderFixture::open(cx);
    assert_eq!(
        fixture.durable_preference(),
        "medium",
        "the fixture starts at the configured default tier"
    );
    assert_eq!(
        fixture
            .stream
            .read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "medium"
    );

    fixture.open_model_popup(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(fixture.window.into(), cx);
    let track = visual
        .debug_bounds("thinking-slider-track")
        .expect("mounted tier track");
    // The left end of the track is the weakest declared tier (`low`). Using the
    // track's own bounds keeps this independent of the dot geometry constants.
    visual.simulate_click(
        gpui_kit::point(track.left() + gpui_kit::px(1.), track.center().y),
        gpui_kit::Modifiers::default(),
    );
    visual.run_until_parked();

    pump_test_app(cx, |cx| {
        fixture
            .root
            .read_with(cx, |root, _| root.reasoning_save_pending.is_none())
            && fixture.durable_preference() == "low"
    });
    assert_eq!(
        fixture.durable_preference(),
        "low",
        "the slider selection must reach the durable reasoning profile"
    );
    assert_eq!(
        fixture
            .stream
            .read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "low",
        "the composer must project the persisted tier"
    );

    // Reload: re-run the exact projection a reopened thread performs
    // (render.rs -> apply_reasoning_profile_to_stream) after dropping the
    // in-memory capability, and assert the tier comes back from disk.
    let model = fixture
        .stream
        .read_with(cx, |stream, _| stream.displayed_model().to_string());
    fixture
        .stream
        .update(cx, ConversationStream::clear_reasoning_profile);
    assert_eq!(
        fixture
            .stream
            .read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "provider_default",
        "a cleared projection carries no tier"
    );
    fixture.root.update(cx, |root, cx| {
        root.apply_reasoning_profile_to_stream(&fixture.stream, &model, cx)
    });
    assert_eq!(
        fixture
            .stream
            .read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "low",
        "reloading the profile must restore the persisted tier"
    );

    // The reopened popup shows the persisted tier at the reloaded position.
    fixture.open_model_popup(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(fixture.window.into(), cx);
    assert!(
        visual.debug_bounds("thinking-slider-knob").is_some(),
        "the reloaded slider still renders its knob"
    );
    let slider_tier = fixture
        .stream
        .read_with(cx, |stream, cx| {
            stream.thinking_slider().read(cx).tier().map(str::to_string)
        })
        .expect("slider tier");
    assert_eq!(slider_tier, "low", "the slider shows the reloaded tier");

    // R58 R7: the reset control is gone. Vega has exactly one persisted tier
    // field (`preference`) and a slider selection writes it, so "reset to the
    // configured default" was the identity operation (R57 §3.4 R11 is void).
    assert!(
        visual.debug_bounds("thinking-slider-reset").is_none(),
        "R58 R7 removes the reset control"
    );

    // A slider selection is a local preference change: it never starts a run.
    assert_eq!(
        fixture
            .root
            .read_with(cx, |root, _| root.agent_worker_start_probe.load()),
        0
    );
    // The durable thread is untouched: only the reasoning profile's preference
    // changed, never `threads.model`.
    let store = Store::open(&fixture.database_path).expect("reopened slider store");
    assert_eq!(
        vega_conversation::threads::open_thread(&store, &fixture.thread.id)
            .expect("durable thread")
            .model,
        "gpt-5.6-terra",
        "a tier change must not touch the durable thread model"
    );
}

/// R57 P3: a drag across the track emits one intent per tier while a save may
/// already be in flight. The newest position must win and a superseded tier
/// must never be replayed afterwards.
#[gpui_kit::test]
async fn r57_slider_drag_coalesces_to_the_newest_tier(cx: &mut gpui_kit::TestAppContext) {
    let fixture = ThinkingSliderFixture::open(cx);
    fixture.open_model_popup(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(fixture.window.into(), cx);
    assert!(visual.debug_bounds("thinking-slider-track").is_some());

    // Three presses in a row without waiting for the first save: `high` is the
    // newest intent, and `low`/`medium` are superseded. Each click targets a
    // dot's own center, which is always inside the track.
    for dot in [
        "thinking-slider-dot-0",
        "thinking-slider-dot-1",
        "thinking-slider-dot-2",
    ] {
        let bounds = visual.debug_bounds(dot).unwrap_or_else(|| panic!("{dot}"));
        visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
        visual.run_until_parked();
    }

    pump_test_app(cx, |cx| {
        fixture
            .root
            .read_with(cx, |root, _| root.reasoning_save_pending.is_none())
            && fixture.durable_preference() == "high"
    });
    assert_eq!(
        fixture.durable_preference(),
        "high",
        "the newest drag position must be the durable tier"
    );
    // No superseded intent may be replayed after the ack.
    pump_test_app(cx, |cx| {
        fixture
            .root
            .read_with(cx, |root, _| root.reasoning_save_pending.is_none())
    });
    assert_eq!(
        fixture.durable_preference(),
        "high",
        "a superseded tier must not be written after the newest one"
    );
    assert_eq!(
        fixture
            .stream
            .read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "high"
    );
}

/// R58 A1/A3: a model that declares `supports_disabled` + a `disabled_wire`
/// renders `efforts.len() + 1` dots with Off leftmost, and selecting Off
/// persists `"disabled"` — the `ReasoningChoice::Disabled` path — without ever
/// touching the declared `efforts`.
#[gpui_kit::test]
async fn r58_off_position_persists_disabled_without_touching_efforts(
    cx: &mut gpui_kit::TestAppContext,
) {
    let fixture = ThinkingSliderFixture::open_with(cx, three_tier_disabled_reasoning_config());
    assert_eq!(
        fixture.durable_preference(),
        "medium",
        "the fixture starts at the configured default tier"
    );
    fixture.open_model_popup(cx);

    // A1: four dots for three efforts plus Off, and Off is the leftmost.
    let mut visual = gpui_kit::VisualTestContext::from_window(fixture.window.into(), cx);
    assert!(visual.debug_bounds("composer-thinking-slider").is_some());
    for (index, expected) in [
        ("thinking-slider-dot-0", true),
        ("thinking-slider-dot-1", true),
        ("thinking-slider-dot-2", true),
        ("thinking-slider-dot-3", true),
        ("thinking-slider-dot-4", false),
    ] {
        assert_eq!(
            visual.debug_bounds(index).is_some(),
            expected,
            "dot {index} presence"
        );
    }
    let off_dot = visual
        .debug_bounds("thinking-slider-dot-0")
        .expect("the leftmost dot is the Off position");
    let track = visual
        .debug_bounds("thinking-slider-track")
        .expect("mounted tier track");
    assert!(
        f32::from(off_dot.center().x) < f32::from(track.center().x),
        "Off must sit at the far left of the track"
    );

    // A3: clicking the leftmost dot selects Off.
    visual.simulate_click(off_dot.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    pump_test_app(cx, |cx| {
        fixture
            .root
            .read_with(cx, |root, _| root.reasoning_save_pending.is_none())
            && fixture.durable_preference() == "disabled"
    });
    assert_eq!(
        fixture.durable_preference(),
        "disabled",
        "the Off position persists the store's own disabled name"
    );
    assert_eq!(
        fixture
            .stream
            .read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "disabled",
        "the composer projects the persisted disabled choice"
    );
    assert!(
        fixture
            .stream
            .read_with(cx, |stream, cx| stream.thinking_slider().read(cx).is_off()),
        "the slider shows Off as selected"
    );

    // The declared efforts are untouched: Off is a separate state, never a
    // member of the effort ladder.
    let profile = vega_store::reasoning::read_from(&fixture.reasoning_path)
        .expect("read owned reasoning file")
        .profiles
        .into_iter()
        .find(|profile| profile.model == "gpt-5.6-terra")
        .expect("owned profile");
    assert_eq!(
        profile.efforts,
        vec!["low".to_string(), "medium".to_string(), "high".to_string()],
        "selecting Off must not add an effort to the declared ladder"
    );
    assert!(
        profile
            .efforts
            .iter()
            .all(|effort| !matches!(effort.as_str(), "off" | "none" | "disabled")),
        "no off-like string may enter efforts"
    );
    assert!(profile.supports_disabled);
    assert_eq!(
        profile.disabled_wire.as_deref(),
        Some("reasoning_effort_none")
    );

    // The frozen request for the next run is `Disabled` with its wire — the
    // path `openai/mod.rs` encodes as `reasoning_effort: "none"`. It is never
    // `Effort("off")`.
    let frozen = fixture
        .stream
        .read_with(cx, |stream, _| stream.frozen_reasoning_for_submit())
        .expect("frozen profile")
        .expect("explicit owned profile");
    assert_eq!(
        frozen.choice,
        ReasoningChoice::Disabled,
        "Off must resolve to Disabled, never an Effort"
    );
    assert_eq!(
        frozen.disabled_wire,
        Some(vega_conversation::types::ReasoningDisabledWire::ReasoningEffortNone)
    );
    assert_eq!(frozen.declared_efforts, profile.efforts);

    // The choice reaches the provider boundary unchanged.
    let provider = fixture.provider.clone();
    fixture.root.update(cx, |root, cx| {
        root.start_agent_run_with_reasoning(
            fixture.stream.clone(),
            &fixture.thread.id,
            PendingAgentRun::UserMessage("off must reach the wire".into()),
            Some(frozen),
            cx,
        );
    });
    pump_test_app(cx, |cx| {
        fixture
            .root
            .read_with(cx, |root, _| root.agent_controller.active.is_none())
            && provider.requests().len() == 1
    });
    let request = provider
        .requests()
        .into_iter()
        .next()
        .expect("mock request");
    let reasoning = request.reasoning.expect("reasoning captured for the run");
    assert_eq!(
        reasoning.choice,
        ReasoningChoice::Disabled,
        "the provider must receive Disabled, not Effort(\"off\")"
    );
    assert_eq!(
        reasoning.disabled_wire,
        Some(vega_conversation::types::ReasoningDisabledWire::ReasoningEffortNone)
    );

    // A4 regression: selecting a tier still yields `Effort(efforts[i])`. With
    // Off at index 0, the first effort is index 1.
    fixture.open_model_popup(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(fixture.window.into(), cx);
    let first_tier = visual
        .debug_bounds("thinking-slider-dot-1")
        .expect("first effort dot");
    visual.simulate_click(first_tier.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    pump_test_app(cx, |cx| {
        fixture
            .root
            .read_with(cx, |root, _| root.reasoning_save_pending.is_none())
            && fixture.durable_preference() == "low"
    });
    assert_eq!(fixture.durable_preference(), "low");
    let frozen = fixture
        .stream
        .read_with(cx, |stream, _| stream.frozen_reasoning_for_submit())
        .expect("frozen profile")
        .expect("explicit owned profile");
    assert_eq!(
        frozen.choice,
        ReasoningChoice::Effort("low".to_string()),
        "a tier selection is unchanged by R58"
    );

    // R8: a tier selection must not close the popup. The control stays under
    // the user's pointer across the persistence round trip.
    assert_eq!(
        fixture
            .stream
            .read_with(cx, |stream, _| stream.model_picker_level()),
        vega_ui::conversation_stream::ModelPickerLevel::Slider,
        "R8: the picker stays on the slider level after a tier selection"
    );
    assert!(
        visual.debug_bounds("composer-thinking-slider").is_some(),
        "R8: the slider is still mounted after the round trip"
    );
}

/// R58 A2: a model without the disabled capability renders `efforts.len()`
/// dots and no Off position, so the leftmost dot is the first effort.
#[gpui_kit::test]
async fn r58_no_off_position_without_the_disabled_capability(cx: &mut gpui_kit::TestAppContext) {
    let fixture = ThinkingSliderFixture::open(cx);
    fixture.open_model_popup(cx);

    let mut visual = gpui_kit::VisualTestContext::from_window(fixture.window.into(), cx);
    for (selector, expected) in [
        ("thinking-slider-dot-0", true),
        ("thinking-slider-dot-1", true),
        ("thinking-slider-dot-2", true),
        ("thinking-slider-dot-3", false),
    ] {
        assert_eq!(
            visual.debug_bounds(selector).is_some(),
            expected,
            "{selector} presence"
        );
    }
    let leftmost = visual
        .debug_bounds("thinking-slider-dot-0")
        .expect("the first effort dot");
    visual.simulate_click(leftmost.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    pump_test_app(cx, |cx| {
        fixture
            .root
            .read_with(cx, |root, _| root.reasoning_save_pending.is_none())
            && fixture.durable_preference() == "low"
    });
    assert_eq!(
        fixture.durable_preference(),
        "low",
        "without Off the leftmost dot is the first effort"
    );
    assert!(
        !fixture
            .stream
            .read_with(cx, |stream, cx| stream.thinking_slider().read(cx).is_off()),
        "no Off position exists for this model"
    );

    // R8: the picker stays on the slider level after the tier selection.
    assert_eq!(
        fixture
            .stream
            .read_with(cx, |stream, _| stream.model_picker_level()),
        vega_ui::conversation_stream::ModelPickerLevel::Slider,
        "R8: the picker stays on the slider level after a tier selection"
    );
}

#[gpui_kit::test]
async fn reasoning_authority_reconcile_error_blocks_controller_before_provider(
    cx: &mut gpui_kit::TestAppContext,
) {
    let config_root = tempfile::tempdir().expect("reasoning controller config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);

    let data_root = tempfile::tempdir().expect("reasoning controller data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("reasoning controller store");
    store.migrate().expect("reasoning controller migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root
            .path()
            .to_str()
            .expect("UTF-8 reasoning controller path"),
        "reasoning-controller-e2e",
        None,
    )
    .expect("reasoning controller project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("reasoning controller thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("reasoning controller global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let provider = Arc::new(vega_runtime::MockProvider::new(Vec::new()));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path);
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.configured_models = Some(vec![thread.model.clone()]);
        root.configured_reasoning = Some(vec![ReasoningProfileProjection::unknown(
            "owned",
            thread.model.clone(),
        )]);
        // This is the post-rename-readback-failed state: no disk authority is
        // available, so the stale projection cannot be submitted as default.
        root.configured_reasoning_error = Some(vega_ui::settings::ReasoningSettingsErrorCode::Io);
        root.configured_reasoning_authority = None;
        root.apply_reasoning_profile_to_stream(&stream, &thread.model, cx);
    });

    assert!(stream.read_with(cx, |stream, _| stream.reasoning_unavailable()));
    root.update(cx, |root, cx| {
        root.start_agent_run(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("must remain local".into()),
            cx,
        );
    });
    assert!(provider.requests().is_empty());
    assert!(root.read_with(cx, |root, _| root.agent_controller.active.is_none()));

    // An authority may be readable while its typed projection is invalid
    // after a future grammar/conversion drift. The error must not turn the
    // unknown row into a legal provider-default submit.
    root.update(cx, |root, cx| {
        root.configured_reasoning_authority = Some((
            vega_store::reasoning::ReasoningConfig::default(),
            vega_store::reasoning::ReasoningFileSnapshot::absent(),
        ));
        root.configured_reasoning_error =
            Some(vega_ui::settings::ReasoningSettingsErrorCode::Invalid);
        root.apply_reasoning_profile_to_stream(&stream, &thread.model, cx);
    });
    assert!(stream.read_with(cx, |stream, _| stream.reasoning_unavailable()));
    root.update(cx, |root, cx| {
        root.start_agent_run(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("invalid profile must remain local".into()),
            cx,
        );
    });
    assert!(provider.requests().is_empty());
}

#[gpui_kit::test]
async fn settings_unknown_reasoning_template_saves_reads_back_and_reaches_mock_run(
    cx: &mut gpui_kit::TestAppContext,
) {
    // Start with a provider/model config but no reasoning file. The test uses
    // an owned Settings entity through the same public view constructor; the
    // rendered focus/keyboard path is covered by vega_ui tests, while this
    // app test drives the controller and provider boundary. No hand-written
    // capability profile is used.
    let config_root = tempfile::tempdir().expect("settings reasoning config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);
    vega_store::keystore::set_key(
        config_root.path(),
        "owned",
        "fake-owned-provider-key-reasoning-73",
    )
    .expect("owned reasoning test credential");
    let reasoning_path = config_root.path().join("reasoning.toml");
    assert!(!reasoning_path.exists());

    let data_root = tempfile::tempdir().expect("settings reasoning data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("settings reasoning store");
    store.migrate().expect("settings reasoning migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root
            .path()
            .to_str()
            .expect("UTF-8 settings reasoning path"),
        "settings-reasoning-e2e",
        None,
    )
    .expect("settings reasoning project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("settings reasoning thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("settings reasoning global store"),
            thread.clone(),
            cx,
        )
    });
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("settings path ok".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config_path.clone());
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
        root.stream_view = Some((thread.id.clone(), stream.clone()));
    });
    cx.update(|cx| {
        cx.set_global(SettingsOpen(true));
    });
    // Install the same typed Settings subscriptions that the production
    // VegaWindow renderer installs. Constructing from an owned config keeps
    // this controller acceptance from reading the host user's AppConfig;
    // separate UI tests exercise the rendered focus path.
    let settings =
        cx.new(|cx| SettingsView::from_config(vega_store::config::AppConfig::default(), None, cx));
    root.update(cx, |root, cx| {
        cx.subscribe(
            &settings,
            |root, view, request: &ReasoningProfileSaveRequested, cx| {
                root.request_reasoning_profile_save(view.clone(), request, cx);
            },
        )
        .detach();
        root.settings_view = Some(settings.clone());
        root.start_model_catalog_load(cx);
    });

    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.configured_models.is_some()
                && root.configured_reasoning_authority.is_some()
                && root.settings_view.is_some()
        })
    });
    let settings = root
        .read_with(cx, |root, _| root.settings_view.clone())
        .expect("owned Settings view");
    assert!(settings.read_with(cx, |settings, _| matches!(
        settings.reasoning_projection(),
        vega_ui::settings::ReasoningSettingsProjection::Ready {
            profiles,
            error: None,
            ..
        } if profiles.iter().any(|profile| profile.provider == "owned"
            && profile.model == "gpt-5.6-terra"
            && profile.protocol == vega_conversation::types::ReasoningProtocol::Unknown)
    )));

    // This is the Settings-page action behind the keyboard/mouse template
    // control. It emits the normal app-owned save request and waits for the
    // durable worker acknowledgement/readback.
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, vega_ui::settings::ReasoningTemplate::OpenAi, cx);
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root.configured_reasoning_authority.is_some()
                && root.configured_reasoning_error.is_none()
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                vega_ui::settings::ReasoningSettingsProjection::Ready {
                    profiles,
                    error: None,
                    ..
                } if profiles.iter().any(|profile| {
                    profile.provider == "owned"
                        && profile.model == "gpt-5.6-terra"
                        && profile.protocol
                            == vega_conversation::types::ReasoningProtocol::OpenAiChatCompletions
                })
            )
        })
    });
    let first_generation = root.read_with(cx, |root, _| root.model_catalog_generation);
    // A second edit must use the generation carried by the first acknowledged
    // projection. This catches publishing Ready with the pre-ack generation,
    // which would make an immediate follow-up save look stale.
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root.configured_reasoning_authority.is_some()
                && root.configured_reasoning_error.is_none()
                && root.model_catalog_generation > first_generation
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                vega_ui::settings::ReasoningSettingsProjection::Ready {
                    profiles,
                    error: None,
                    ..
                } if profiles.iter().any(|profile| {
                    profile.provider == "owned"
                        && profile.model == "gpt-5.6-terra"
                        && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                })
            )
        })
    });
    let saved = vega_store::reasoning::read_from(&reasoning_path)
        .expect("read settings-selected reasoning authority");
    assert_eq!(saved.profiles.len(), 1);
    assert_eq!(saved.profiles[0].provider, "owned");
    assert_eq!(saved.profiles[0].model, "gpt-5.6-terra");
    assert_eq!(saved.profiles[0].protocol, "openai_chat_completions");
    assert_eq!(saved.profiles[0].support, "optional");

    // Closing Settings lets the live window render the conversation stream;
    // the already-read-back projection is then frozen for the real app
    // submit. The provider boundary remains MockProvider, so no keychain or
    // network is involved.
    cx.update(|cx| {
        cx.set_global(SettingsOpen(false));
        cx.refresh_windows();
    });
    let frozen = stream
        .read_with(cx, |stream, _| stream.frozen_reasoning_for_submit())
        .expect("saved profile freezes for submit")
        .expect("explicit OpenAI profile");
    assert_eq!(frozen.model, "gpt-5.6-terra");
    root.update(cx, |root, cx| {
        root.start_agent_run_with_reasoning(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("use saved thinking settings".into()),
            Some(frozen),
            cx,
        );
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.agent_controller.active.is_none())
            && provider.requests().len() == 1
    });
    let request = provider
        .requests()
        .into_iter()
        .next()
        .expect("mock request");
    assert_eq!(request.model, "gpt-5.6-terra");
    assert_eq!(
        request.reasoning.map(|reasoning| reasoning.protocol),
        Some(vega_runtime::ReasoningProtocol::OpenAiChatCompletions)
    );
}

#[gpui_kit::test]
async fn frozen_reasoning_owner_change_same_model_fails_before_provider(
    cx: &mut gpui_kit::TestAppContext,
) {
    let config_root = tempfile::tempdir().expect("owner change config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);

    let data_root = tempfile::tempdir().expect("owner change data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("owner change store");
    store.migrate().expect("owner change migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root
            .path()
            .to_str()
            .expect("UTF-8 owner change project"),
        "owner-change-e2e",
        None,
    )
    .expect("owner change project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("owner change thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("owner change global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("must not run"),
    ]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.configured_models = Some(vec![thread.model.clone()]);
        root.configured_reasoning = Some(vec![ReasoningProfileProjection {
            provider: "owned".into(),
            model: thread.model.clone(),
            protocol: ReasoningProtocol::OpenAiChatCompletions,
            support: ReasoningSupport::Optional,
            efforts: vec!["low".into(), "high".into(), "max".into()],
            supports_disabled: false,
            disabled_wire: None,
            preserve_reasoning_content: false,
            preference: ReasoningChoice::Effort("high".into()),
        }]);
        root.configured_reasoning_authority = Some((
            vega_store::reasoning::ReasoningConfig::default(),
            vega_store::reasoning::ReasoningFileSnapshot::absent(),
        ));
        root.apply_reasoning_profile_to_stream(&stream, &thread.model, cx);
    });

    let frozen = stream
        .read_with(cx, |stream, _| stream.frozen_reasoning_for_submit())
        .expect("frozen owner profile")
        .expect("explicit owner profile");
    assert_eq!(frozen.provider, "owned");

    // Keep the model id stable while changing its configured provider. The
    // worker reads this owned path and must reject the stale A-owner snapshot
    // before invoking the test provider or entering the runtime.
    let mut changed = fs::read_to_string(&config_path).expect("read owner config");
    changed = changed.replace("name = \"owned\"", "name = \"other\"");
    changed = changed.replace("key_ref = \"owned\"", "key_ref = \"other\"");
    fs::write(&config_path, changed).expect("change configured owner");

    root.update(cx, |root, cx| {
        root.start_agent_run_with_reasoning(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("stale owner must remain local".into()),
            Some(frozen),
            cx,
        );
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.agent_controller.active.is_none())
    });
    assert!(
        provider.requests().is_empty(),
        "a changed provider owner must fail before any mock provider request"
    );
}

#[gpui_kit::test]
async fn provider_catalog_refresh_gates_settings_until_new_generation_is_ready(
    cx: &mut gpui_kit::TestAppContext,
) {
    let config_root = tempfile::tempdir().expect("catalog refresh config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);
    let data_root = tempfile::tempdir().expect("catalog refresh data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("catalog refresh store");
    store.migrate().expect("catalog refresh migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root
            .path()
            .to_str()
            .expect("UTF-8 catalog refresh project path"),
        "catalog-refresh-e2e",
        None,
    )
    .expect("catalog refresh project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("catalog refresh thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("catalog refresh global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let settings = cx.new(|cx| SettingsView::from_config(AppConfig::default(), None, cx));
    let root = cx.new(VegaWindow::new);
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.stream_view = Some((thread.id.clone(), stream));
        root.settings_view = Some(settings.clone());
        cx.subscribe(
            &settings,
            |root, view, request: &ReasoningProfileSaveRequested, cx| {
                root.request_reasoning_profile_save(view.clone(), request, cx);
            },
        )
        .detach();
        root.start_model_catalog_load(cx);
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.configured_models.is_some()
                && root.configured_reasoning_authority.is_some()
                && !root.model_catalog_loading
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready { error: None, .. }
            )
        })
    });
    let old_generation = root.read_with(cx, |root, _| root.model_catalog_generation);
    let catalog_gate = Arc::new(std::sync::Barrier::new(2));
    root.update(cx, |root, _| {
        root.model_catalog_worker_gate = Some(catalog_gate.clone());
    });

    // Provider SettingsSaved invalidates the catalog and starts a new worker.
    // The live Settings view must leave Ready(old_generation) before the gate
    // is released, so an attempted edit cannot become an unacknowledged stale
    // Saving request.
    root.update(cx, |root, cx| root.on_settings_saved(cx));
    assert!(root.read_with(cx, |root, _| {
        root.model_catalog_loading && root.model_catalog_generation > old_generation
    }));
    assert!(settings.read_with(cx, |settings, _| matches!(
        settings.reasoning_projection(),
        ReasoningSettingsProjection::Loading
    )));
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_none()));
    assert!(settings.read_with(cx, |settings, _| matches!(
        settings.reasoning_projection(),
        ReasoningSettingsProjection::Loading
    )));

    catalog_gate.wait();
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.configured_models.is_some()
                && root.configured_reasoning_authority.is_some()
                && !root.model_catalog_loading
                && root.model_catalog_generation > old_generation
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready { error: None, .. }
            )
        })
    });

    // After the new authority is Ready the same real Settings action emits a
    // request carrying the new generation and receives its durable ack.
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_some()));
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root.configured_reasoning_authority.is_some()
                && root.configured_reasoning_error.is_none()
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready {
                    profiles,
                    error: None,
                    ..
                } if profiles.iter().any(|profile| {
                    profile.provider == "owned"
                        && profile.model == "gpt-5.6-terra"
                        && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                })
            )
        })
    });
}

async fn exercise_catalog_reasoning_save_order(
    cx: &mut gpui_kit::TestAppContext,
    reasoning_first: bool,
) {
    let config_root = tempfile::tempdir().expect("catalog order config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);
    let data_root = tempfile::tempdir().expect("catalog order data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("catalog order store");
    store.migrate().expect("catalog order migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root.path().to_str().expect("UTF-8 catalog order path"),
        "catalog-order-e2e",
        None,
    )
    .expect("catalog order project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("catalog order thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("catalog order global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let settings = cx.new(|cx| SettingsView::from_config(AppConfig::default(), None, cx));
    let catalog_gate = Arc::new(std::sync::Barrier::new(2));
    let reasoning_gate = Arc::new(std::sync::Barrier::new(2));
    let root = cx.new(VegaWindow::new);
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.configured_reasoning = Some(vec![ReasoningProfileProjection::unknown(
            "owned",
            thread.model.clone(),
        )]);
        root.configured_reasoning_authority = Some((
            vega_store::reasoning::ReasoningConfig::default(),
            vega_store::reasoning::ReasoningFileSnapshot::absent(),
        ));
        root.model_catalog_worker_gate = Some(catalog_gate.clone());
        root.reasoning_save_worker_gate = Some(reasoning_gate.clone());
        root.start_model_catalog_load(cx);
        cx.subscribe(
            &settings,
            |root, view, request: &ReasoningProfileSaveRequested, cx| {
                root.request_reasoning_profile_save(view.clone(), request, cx);
            },
        )
        .detach();
        root.settings_view = Some(settings.clone());
        settings.update(cx, |settings, cx| {
            settings.apply_reasoning_projection(
                ReasoningSettingsProjection::Ready {
                    generation: root.model_catalog_generation,
                    profiles: root.configured_reasoning.clone().unwrap_or_default(),
                    error: None,
                },
                cx,
            )
        });
    });

    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_some()));

    // A Provider SettingsSaved signal arrives while reasoning owns the
    // authority. It records intent only; it cannot launch a worker that would
    // overwrite Saving or leave model_catalog_loading stuck.
    root.update(cx, |root, cx| root.on_settings_saved(cx));
    assert!(root.read_with(cx, |root, _| {
        root.model_catalog_refresh_pending && root.model_catalog_loading
    }));

    // Make the provider catalog change visible between the old worker and the
    // coalesced reload. Both completion orders must end with this new model.
    let mut changed = fs::read_to_string(&config_path).expect("read catalog order config");
    changed.push_str(
        "\n[[providers]]\nname = \"fresh\"\nbase_url = \"https://fresh.invalid/v1\"\nmodels = [\"fresh-model\"]\nkey_ref = \"fresh\"\n",
    );
    fs::write(&config_path, changed).expect("write catalog order provider");

    if reasoning_first {
        reasoning_gate.wait();
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                root.reasoning_save_pending.is_none()
                    && root.configured_models.is_some()
                    && root.configured_reasoning_authority.is_some()
                    && !root.model_catalog_loading
                    && !root.model_catalog_refresh_pending
                    && root.configured_reasoning.as_ref().is_some_and(|profiles| {
                        profiles.iter().any(|profile| {
                            profile.provider == "owned"
                                && profile.model == "gpt-5.6-terra"
                                && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                        })
                    })
            })
        });
        // The original catalog worker is now stale. Its late completion must
        // not regress the freshly reconciled provider/reasoning authority.
        catalog_gate.wait();
    } else {
        catalog_gate.wait();
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                root.reasoning_save_pending.is_some()
                    && !root.model_catalog_loading
                    && root.model_catalog_refresh_pending
            }) && settings.read_with(cx, |settings, _| {
                matches!(
                    settings.reasoning_projection(),
                    ReasoningSettingsProjection::Saving { .. }
                )
            })
        });
        reasoning_gate.wait();
    }

    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root
                    .configured_models
                    .as_ref()
                    .is_some_and(|models| models.iter().any(|model| model == "fresh-model"))
                && root.configured_reasoning_authority.is_some()
                && !root.model_catalog_loading
                && !root.model_catalog_refresh_pending
                && root.configured_reasoning.as_ref().is_some_and(|profiles| {
                    profiles.iter().any(|profile| {
                        profile.provider == "owned"
                            && profile.model == "gpt-5.6-terra"
                            && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                    })
                })
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready { profiles, error: None, .. }
                    if profiles.iter().any(|profile| {
                        profile.provider == "owned"
                            && profile.model == "gpt-5.6-terra"
                            && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                    })
            )
        })
    });
}

#[gpui_kit::test]
async fn reasoning_and_provider_catalog_completion_orders_are_coordinated(
    cx: &mut gpui_kit::TestAppContext,
) {
    exercise_catalog_reasoning_save_order(cx, true).await;
    exercise_catalog_reasoning_save_order(cx, false).await;
}

#[gpui_kit::test]
async fn failed_reasoning_save_keeps_error_and_draft_through_catalog_refresh(
    cx: &mut gpui_kit::TestAppContext,
) {
    let config_root = tempfile::tempdir().expect("failed reasoning config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);
    let reasoning_path = config_root.path().join("reasoning.toml");
    let data_root = tempfile::tempdir().expect("failed reasoning data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("failed reasoning store");
    store.migrate().expect("failed reasoning migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root
            .path()
            .to_str()
            .expect("UTF-8 failed reasoning project path"),
        "failed-reasoning-e2e",
        None,
    )
    .expect("failed reasoning project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("failed reasoning thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("failed reasoning global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let settings = cx.new(|cx| SettingsView::from_config(AppConfig::default(), None, cx));
    let catalog_gate = Arc::new(std::sync::Barrier::new(2));
    let fresh_catalog_gate = Arc::new(std::sync::Barrier::new(2));
    let reasoning_gate = Arc::new(std::sync::Barrier::new(2));
    let root = cx.new(VegaWindow::new);
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.stream_view = Some((thread.id.clone(), stream));
        root.configured_reasoning = Some(vec![ReasoningProfileProjection::unknown(
            "owned",
            thread.model.clone(),
        )]);
        root.configured_reasoning_authority = Some((
            vega_store::reasoning::ReasoningConfig::default(),
            vega_store::reasoning::ReasoningFileSnapshot::absent(),
        ));
        root.model_catalog_worker_gate = Some(catalog_gate.clone());
        root.reasoning_save_worker_gate = Some(reasoning_gate.clone());
        root.start_model_catalog_load(cx);
        cx.subscribe(
            &settings,
            |root, view, request: &ReasoningProfileSaveRequested, cx| {
                root.request_reasoning_profile_save(view.clone(), request, cx);
            },
        )
        .detach();
        root.settings_view = Some(settings.clone());
        settings.update(cx, |settings, cx| {
            settings.apply_reasoning_projection(
                ReasoningSettingsProjection::Ready {
                    generation: root.model_catalog_generation,
                    profiles: root.configured_reasoning.clone().unwrap_or_default(),
                    error: None,
                },
                cx,
            )
        });
    });

    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_some()));
    root.update(cx, |root, cx| root.on_settings_saved(cx));
    assert!(root.read_with(cx, |root, _| {
        root.model_catalog_refresh_pending && root.model_catalog_loading
    }));

    // Change the independent authority while the save worker is gated. The
    // compare-before-rename returns a typed conflict and readback supplies the
    // external authority; this is the post-rename/uncertain path's visible
    // counterpart without any provider, Keychain, or network request.
    let external = vega_store::reasoning::ReasoningConfig {
        version: vega_store::reasoning::REASONING_CONFIG_VERSION,
        profiles: vec![vega_store::reasoning::ReasoningProfile {
            provider: "owned".into(),
            model: "gpt-5.6-terra".into(),
            protocol: "openai_chat_completions".into(),
            support: "optional".into(),
            efforts: vec!["low".into(), "high".into(), "max".into()],
            supports_disabled: true,
            disabled_wire: Some("reasoning_effort_none".into()),
            preserve_reasoning_content: false,
            preference: "provider_default".into(),
        }],
    };
    fs::write(
        &reasoning_path,
        vega_store::reasoning::encode(&external).expect("encode external reasoning authority"),
    )
    .expect("write external reasoning authority");
    root.update(cx, |root, _| {
        // The first catalog worker remains gated as the stale result. The
        // acknowledgement starts a second, fresh worker under a separate gate
        // so the real Settings view can be exercised while that generation is
        // still loading.
        root.model_catalog_worker_gate = Some(fresh_catalog_gate.clone());
    });
    reasoning_gate.wait();

    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none() && root.model_catalog_loading
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Loading
            )
        })
    });

    // The actual Settings action is now attempted while the fresh catalog
    // worker is gated. Loading must reject the action without creating a stale
    // Saving projection, while the UI-level draft remains retained.
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_none()));
    assert!(settings.read_with(cx, |settings, _| matches!(
        settings.reasoning_projection(),
        ReasoningSettingsProjection::Loading
    )));

    // The save acknowledgement starts the deferred catalog read. Its fresh
    // result must retain the failure hold and the Settings draft instead of
    // replacing them with a clean Ready/Loading projection.
    catalog_gate.wait();
    fresh_catalog_gate.wait();
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root.configured_reasoning_authority.is_some()
                && root.configured_reasoning_error
                    == Some(vega_ui::settings::ReasoningSettingsErrorCode::Conflict)
                && !root.model_catalog_loading
                && !root.model_catalog_refresh_pending
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready {
                    profiles,
                    error: Some(vega_ui::settings::ReasoningSettingsErrorCode::Conflict),
                    ..
                } if profiles.iter().any(|profile| {
                    profile.provider == "owned"
                        && profile.model == "gpt-5.6-terra"
                        && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                })
            )
        })
    });

    // Once the exact external authority is reloaded, the retained owner is
    // retryable through the same Settings action and a successful ack clears
    // the held error.
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_some()));
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root.configured_reasoning_error.is_none()
                && root.configured_reasoning_authority.is_some()
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready { error: None, .. }
            )
        })
    });
}
