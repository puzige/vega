use super::*;
use vega_conversation::types::{
    CatalogAction, CatalogIdentity, CatalogModel, CatalogReasoningOption, ModelCatalogProjection,
    ModelCatalogRequested,
};

pub(crate) struct CatalogUi {
    owner: Option<CatalogIdentity>,
    generation: u64,
    projection: ModelCatalogProjection,
    open: bool,
    mapping: bool,
    selected_provider: Option<String>,
    candidate: Option<CatalogIdentity>,
    search: Entity<TextInput>,
    message: Option<String>,
}
impl CatalogUi {
    pub(crate) fn new(cx: &mut Context<SettingsView>) -> Self {
        let search =
            cx.new(|cx| TextInput::new(cx, "搜索提供方或完整模型 ID", false).with_tab_stop(true));
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        Self {
            owner: None,
            generation: 0,
            projection: Default::default(),
            open: false,
            mapping: false,
            selected_provider: None,
            candidate: None,
            search,
            message: None,
        }
    }
}

fn compatible_efforts(model: &CatalogModel) -> Option<Vec<String>> {
    let [CatalogReasoningOption::Effort(values)] = model.reasoning_options.as_slice() else {
        return None;
    };
    if model.reasoning != Some(true) || values.is_empty() {
        return None;
    }
    let mut efforts = Vec::new();
    for value in values {
        let value = value.as_deref()?;
        if !matches!(
            value,
            "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
        ) || efforts.iter().any(|v| v == value)
        {
            return None;
        }
        efforts.push(value.to_string());
    }
    Some(efforts)
}

fn simple_prices(model: &CatalogModel) -> Option<[String; 4]> {
    if !model.text_pricing {
        return None;
    }
    let cost = model.cost.as_ref().filter(|cost| !cost.complex)?;
    let rates = [
        cost.input.clone()?,
        cost.output.clone()?,
        cost.cache_read.clone()?,
        cost.cache_write.clone()?,
    ];
    let representable = rates.iter().all(|rate| {
        let (whole, fraction) = rate.split_once('.').unwrap_or((rate, ""));
        if whole.is_empty()
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || fraction.len() > 6
            || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return false;
        }
        whole
            .parse::<u64>()
            .ok()
            .and_then(|n| n.checked_mul(1_000_000))
            .and_then(|n| {
                n.checked_add(
                    fraction.parse::<u64>().unwrap_or(0) * 10_u64.pow(6 - fraction.len() as u32),
                )
            })
            .is_some()
    });
    representable.then_some(rates)
}

impl SettingsView {
    pub(crate) fn ensure_model_catalog(&mut self, cx: &mut Context<Self>) {
        let owner = if matches!(self.section, 0 | 2 | 3) {
            self.model_settings
                .as_ref()
                .map(|(provider, model)| CatalogIdentity {
                    provider: provider.clone(),
                    model: model.clone(),
                })
        } else {
            None
        };
        if self.catalog.owner == owner {
            return;
        }
        self.catalog.owner = owner;
        self.catalog.projection = Default::default();
        self.catalog.open = false;
        self.catalog.mapping = false;
        self.catalog.message = None;
        self.catalog.selected_provider = None;
        self.catalog.candidate = None;
        self.request_model_catalog(CatalogAction::Load, cx);
    }

    fn request_model_catalog(&mut self, action: CatalogAction, cx: &mut Context<Self>) {
        self.catalog.generation = self.catalog.generation.saturating_add(1);
        let Some(owner) = self.catalog.owner.clone() else {
            return;
        };
        self.catalog.projection.refreshing = true;
        self.catalog.message = None;
        cx.emit(ModelCatalogRequested {
            generation: self.catalog.generation,
            owner,
            action,
        });
        cx.notify();
    }

    pub fn apply_model_catalog(
        &mut self,
        request: &ModelCatalogRequested,
        projection: ModelCatalogProjection,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.catalog.owner.as_ref() != Some(&request.owner)
            || self.catalog.generation != request.generation
            || !matches!(self.section, 0 | 2 | 3)
            || self.model_settings.as_ref()
                != Some(&(request.owner.provider.clone(), request.owner.model.clone()))
        {
            return false;
        }
        if matches!(request.action, CatalogAction::Map(_)) && projection.error.is_none() {
            self.catalog.mapping = false;
            self.catalog.candidate = None;
        }
        self.catalog.projection = projection;
        cx.notify();
        true
    }

    fn catalog_apply_capacity(&mut self, input: bool, cx: &mut Context<Self>) {
        let Some(model) = self.catalog.projection.matched_model().cloned() else {
            return;
        };
        let Some(value) = (if input {
            model.input
        } else {
            Some(model.output)
        })
        .filter(|value| *value > 0) else {
            return;
        };
        self.apply_catalog_capacity_candidate(input, value, cx);
        self.catalog.message = Some("已填入编辑框；保存模型设置后生效".into());
        cx.notify();
    }

    fn catalog_apply_prices(&mut self, cx: &mut Context<Self>) {
        let Some((_, model)) = self.model_settings.clone() else {
            return;
        };
        let Some(rates) = self
            .catalog
            .projection
            .matched_model()
            .and_then(simple_prices)
        else {
            return;
        };
        if self.settings_have_draft(cx) || !self.pricing_allows_editing() {
            return;
        }
        let entry = match &self.pricing {
            PricingSettingsProjection::Ready { entries, .. } => {
                entries.iter().find(|entry| entry.model == model).cloned()
            }
            _ => None,
        };
        if let Some(entry) = entry {
            if entry.kind != PricingEntryKind::CustomStatic {
                return;
            }
            self.begin_edit_pricing(entry, cx);
        } else {
            self.begin_add_custom(cx);
        }
        for (input, value) in self.pricing_rate_inputs[..4].iter().zip(rates) {
            input.update(cx, |input, cx| input.set_text(&value, cx));
        }
        self.section = 3;
        cx.notify();
    }

    fn catalog_apply_efforts(&mut self, cx: &mut Context<Self>) {
        if self.settings_have_draft(cx) {
            return;
        }
        let Some((provider, model)) = &self.model_settings else {
            return;
        };
        let Some(efforts) = self
            .catalog
            .projection
            .matched_model()
            .and_then(compatible_efforts)
        else {
            return;
        };
        let ReasoningSettingsProjection::Ready { profiles, .. } = &self.reasoning else {
            return;
        };
        let Some((index, mut profile)) = profiles
            .iter()
            .enumerate()
            .find(|(_, p)| p.provider == *provider && p.model == *model)
            .map(|(i, p)| (i, p.clone()))
        else {
            return;
        };
        if profile.protocol != ReasoningProtocol::OpenAiChatCompletions
            || matches!(
                profile.support,
                ReasoningSupport::Unknown | ReasoningSupport::Unsupported
            )
            || matches!(&profile.preference, ReasoningChoice::Effort(value) if !efforts.contains(value))
        {
            return;
        }
        if profile.efforts == efforts {
            return;
        }
        profile.efforts = efforts;
        self.section = 2;
        self.settings_help_open = None;
        self.rebuild_reasoning_focuses(cx);
        self.emit_reasoning_save(index, profile, cx);
    }

    pub(crate) fn render_model_catalog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        if self.model_settings.is_none() {
            return div().into_any_element();
        }
        let open = self.catalog.open;
        let busy = self.catalog.projection.refreshing;
        let mut section = settings_section().child(
            settings_actions()
                .child(self.settings_route_button(
                    "model-catalog-toggle",
                    if open {
                        "收起模型资料"
                    } else {
                        "模型资料 · models.dev"
                    },
                    true,
                    cx.listener(|this, _, _, cx| {
                        this.catalog.open = !this.catalog.open;
                        cx.notify();
                    }),
                    cx,
                ))
                .child(self.settings_route_button(
                    "model-catalog-refresh",
                    if busy { "正在读取" } else { "刷新资料" },
                    !busy,
                    cx.listener(|this, _, _, cx| {
                        this.request_model_catalog(CatalogAction::Refresh, cx)
                    }),
                    cx,
                ))
                .child(self.settings_help(
                    "model-catalog-help",
                    "模型资料帮助",
                    format!(
                        "目录资料仅作为候选值；刷新不会改动手动配置。应用输入、输出或价格后仍需保存，应用思考档位会立即保存。缓存格式 {}；内容摘要 {}。",
                        self.catalog.projection.schema_version,
                        self.catalog.projection.digest.as_deref().map(|value| value.chars().take(12).collect::<String>()).unwrap_or_else(|| "未获取".into())
                    ),
                    cx,
                )),
        );
        if let Some(error) = self.catalog.projection.error.clone() {
            section = section.child(settings_status(error, colors.warning, colors));
        }
        if !open {
            return section.into_any_element();
        }
        let status = if busy {
            Some("正在读取公开目录")
        } else if self.catalog.projection.stale {
            Some("缓存已过期；仍可查看或手动配置")
        } else if self.catalog.projection.fetched_at_ms.is_none() {
            Some("尚无目录资料；可继续手动配置")
        } else {
            None
        };
        section = section
            .children(status.map(|status| div().text_color(colors.text_secondary).child(status)));
        if let Some(timestamp) = self.catalog.projection.fetched_at_ms {
            section = section.child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .debug_selector(|| "model-catalog-source".into())
                    .child(format!(
                        "models.dev · {} 分钟前获取",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| (d.as_millis() as u64).saturating_sub(timestamp) / 60000)
                            .unwrap_or_default()
                    )),
            );
        }
        section = section.child(
            settings_actions()
                .child(self.settings_route_button(
                    "model-catalog-map",
                    "关联目录模型",
                    !busy,
                    cx.listener(|this, _, _, cx| {
                        this.catalog.mapping = !this.catalog.mapping;
                        this.catalog.selected_provider = None;
                        this.catalog.candidate = None;
                        this.catalog.search.update(cx, TextInput::clear);
                        cx.notify();
                    }),
                    cx,
                ))
                .when(self.catalog.projection.mapped.is_some(), |actions| {
                    actions.child(self.settings_route_button(
                        "model-catalog-unmap",
                        "取消关联",
                        !busy,
                        cx.listener(|this, _, _, cx| {
                            this.request_model_catalog(CatalogAction::Unmap, cx)
                        }),
                        cx,
                    ))
                }),
        );
        if self.catalog.mapping {
            section = section.child(self.render_catalog_mapping(cx));
        }
        if let Some(target) = &self.catalog.projection.mapped {
            section = section.child(
                div()
                    .min_w_0()
                    .whitespace_normal()
                    .child(format!("目录身份：{} / {}", target.provider, target.model)),
            );
        }
        if let Some(model) = self.catalog.projection.matched_model().cloned() {
            section = section.child(div().text_color(colors.text_secondary).child(format!(
                        "总上下文 {} · 输入 {} · 输出 {} token",
                        model.context,
                        model
                            .input
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "未提供".into()),
                        model.output
                    )));
            let support = |value: Option<bool>| match value {
                Some(true) => "支持",
                Some(false) => "不支持",
                None => "未提供",
            };
            section = section.child(div().text_color(colors.text_secondary).child(format!(
                "工具调用：{} · 思考：{}",
                support(model.tool_call),
                support(model.reasoning)
            )));
            if let Some(canonical) = &model.canonical_id {
                section = section.child(
                    div()
                        .text_color(colors.text_secondary)
                        .child(format!("标准模型身份：{canonical}")),
                );
            }
            if let Some(status) = &model.status {
                section = section.child(
                    div()
                        .text_color(colors.warning)
                        .child(format!("目录状态：{status}")),
                );
            }
            let capacity_ready = self.catalog_capacity_editable();
            section = section.child(
                settings_actions()
                    .child(self.settings_route_button(
                        "model-catalog-input",
                        "使用目录输入上限",
                        capacity_ready && model.input.is_some_and(|v| v > 0),
                        cx.listener(|this, _, _, cx| this.catalog_apply_capacity(true, cx)),
                        cx,
                    ))
                    .child(self.settings_route_button(
                        "model-catalog-output",
                        "使用目录输出上限",
                        capacity_ready && model.output > 0,
                        cx.listener(|this, _, _, cx| this.catalog_apply_capacity(false, cx)),
                        cx,
                    )),
            );
            if !model.reasoning_options.is_empty() {
                let text = model
                    .reasoning_options
                    .iter()
                    .map(|option| match option {
                        CatalogReasoningOption::Toggle => "开关".into(),
                        CatalogReasoningOption::Effort(values) => values
                            .iter()
                            .map(|v| v.as_deref().unwrap_or("null"))
                            .collect::<Vec<_>>()
                            .join(" / "),
                        CatalogReasoningOption::Budget { min, max } => {
                            format!(
                                "预算 {}–{}",
                                min.map(|v| v.to_string())
                                    .unwrap_or_else(|| "未提供".into()),
                                max.map(|v| v.to_string())
                                    .unwrap_or_else(|| "未提供".into())
                            )
                        }
                        CatalogReasoningOption::Unknown(value) => value.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join("；");
                section = section.child(
                    div()
                        .text_color(colors.text_secondary)
                        .child(format!("思考资料：{text}")),
                );
            }
            let can_efforts = compatible_efforts(&model).is_some_and(|efforts| match &self.reasoning {
                ReasoningSettingsProjection::Ready { profiles, .. } => profiles.iter().any(|profile| self.model_settings.as_ref() == Some(&(profile.provider.clone(), profile.model.clone()))
                    && profile.protocol == ReasoningProtocol::OpenAiChatCompletions && !matches!(profile.support, ReasoningSupport::Unknown | ReasoningSupport::Unsupported)
                    && !matches!(&profile.preference, ReasoningChoice::Effort(v) if !efforts.contains(v))), _ => false });
            section = section.child(self.settings_route_button(
                "model-catalog-efforts",
                "应用目录思考档位",
                can_efforts && !self.settings_have_draft(cx),
                cx.listener(|this, _, _, cx| this.catalog_apply_efforts(cx)),
                cx,
            ));
            if let Some(cost) = &model.cost {
                section = section
                    .child(div().text_color(colors.text_secondary).child(format!(
                        "USD / 1M token · 输入 {} · 输出 {} · 缓存读取 {} · 缓存写入 {}",
                        cost.input.as_deref().unwrap_or("未提供"),
                        cost.output.as_deref().unwrap_or("未提供"),
                        cost.cache_read.as_deref().unwrap_or("未提供"),
                        cost.cache_write.as_deref().unwrap_or("未提供")
                    )))
                    .when(cost.complex, |s| {
                        s.child(
                            div()
                                .text_color(colors.text_secondary)
                                .child("目录含阶梯或额外计费维度；请手动配置价格"),
                        )
                    });
            }
            let price_target = match (&self.pricing, &self.model_settings) {
                (PricingSettingsProjection::Ready { entries, .. }, Some((_, model))) => entries
                    .iter()
                    .find(|e| e.model == *model)
                    .is_none_or(|e| e.kind == PricingEntryKind::CustomStatic),
                _ => false,
            };
            section = section
                .child(self.settings_route_button(
                    "model-catalog-prices",
                    "填入共享模型价格…",
                    simple_prices(&model).is_some()
                        && price_target
                        && self.pricing_allows_editing()
                        && !self.settings_have_draft(cx),
                    cx.listener(|this, _, _, cx| this.catalog_apply_prices(cx)),
                    cx,
                ))
                .child(
                    div()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child("价格以模型 ID 共享给所有提供方；填入后仍需保存确认"),
                );
        } else if self.catalog.projection.mapped.is_some() {
            section = section.child(settings_status(
                "关联模型不在当前目录中；保留手动配置",
                colors.warning,
                colors,
            ));
        }
        if let Some(message) = &self.catalog.message {
            section = section.child(
                div()
                    .text_color(colors.text_secondary)
                    .child(message.clone()),
            );
        }
        section.into_any_element()
    }

    fn render_catalog_mapping(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let query = self.catalog.search.read(cx).text().to_lowercase();
        let mut body = settings_section().child(self.catalog.search.clone());
        if let Some(provider) = self.catalog.selected_provider.clone() {
            body = body.child(self.settings_route_button(
                "catalog-provider-back",
                "重新选择目录提供方",
                true,
                cx.listener(|this, _, _, cx| {
                    this.catalog.selected_provider = None;
                    this.catalog.candidate = None;
                    this.catalog.search.update(cx, TextInput::clear);
                    cx.notify();
                }),
                cx,
            ));
            if let Some(provider) = self
                .catalog
                .projection
                .providers
                .iter()
                .find(|p| p.id == provider)
                .cloned()
            {
                let models = provider
                    .models
                    .iter()
                    .filter(|m| {
                        m.id.to_lowercase().contains(&query)
                            || m.name.to_lowercase().contains(&query)
                    })
                    .take(20);
                for model in models {
                    let target = CatalogIdentity {
                        provider: provider.id.clone(),
                        model: model.id.clone(),
                    };
                    let selected = self.catalog.candidate.as_ref() == Some(&target);
                    let keyboard_target = target.clone();
                    body = body.child(
                        settings_action_chrome(colors)
                            .id(format!("catalog-model-{}", model.id))
                            .debug_selector({
                                let id = format!("catalog-model-{}", model.id);
                                move || id.clone()
                            })
                            .focus_visible(move |style| style.border_color(colors.accent))
                            .on_key_down(cx.listener(
                                move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.catalog.candidate = Some(keyboard_target.clone());
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                },
                            ))
                            .focusable()
                            .tab_stop(true)
                            .cursor_pointer()
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.catalog.candidate = Some(target.clone());
                                    cx.notify();
                                }),
                            )
                            .child(format!("{}{}", if selected { "✓ " } else { "" }, model.id)),
                    );
                }
            }
            body = body.child(self.settings_route_button(
                "catalog-map-confirm",
                "关联所选模型资料",
                self.catalog.candidate.is_some() && !self.catalog.projection.refreshing,
                cx.listener(|this, _, _, cx| {
                    if let Some(target) = this.catalog.candidate.clone() {
                        this.request_model_catalog(CatalogAction::Map(target), cx);
                    }
                }),
                cx,
            ));
        } else {
            let providers = self
                .catalog
                .projection
                .providers
                .iter()
                .filter(|p| {
                    p.id.to_lowercase().contains(&query) || p.name.to_lowercase().contains(&query)
                })
                .take(20)
                .cloned()
                .collect::<Vec<_>>();
            for provider in providers {
                let keyboard_provider = provider.id.clone();
                let pointer_provider = provider.id.clone();
                body = body.child(
                    settings_action_chrome(colors)
                        .id(format!("catalog-provider-{}", provider.id))
                        .debug_selector({
                            let id = format!("catalog-provider-{}", provider.id);
                            move || id.clone()
                        })
                        .focus_visible(move |style| style.border_color(colors.accent))
                        .on_key_down(cx.listener(
                            move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.catalog.selected_provider =
                                        Some(keyboard_provider.clone());
                                    this.catalog.search.update(cx, TextInput::clear);
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                            },
                        ))
                        .focusable()
                        .tab_stop(true)
                        .cursor_pointer()
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.catalog.selected_provider = Some(pointer_provider.clone());
                                this.catalog.search.update(cx, TextInput::clear);
                                cx.notify();
                            }),
                        )
                        .child(format!("{} ({})", provider.name, provider.id)),
                );
            }
        }
        body.child(
            div()
                .text_size(px(Typography::METADATA))
                .text_color(colors.text_secondary)
                .child("最多显示 20 项；可用完整 ID 缩小结果"),
        )
        .into_any_element()
    }
}
