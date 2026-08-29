//! Settings view (A1-10 UI skeleton): provider list, add-provider form, and
//! default model / permission mode pickers.
//!
//! The view is opened with Cmd+, ([`OpenSettings`]) and closed with Esc or
//! the back button ([`CloseSettings`]); whether it replaces the session
//! placeholder is tracked by the [`SettingsOpen`] global, following the
//! global pattern proven in T07. It loads the config from the config root
//! (`vega_store::paths`, tech-spec §6) when constructed and saves it back on
//! every mutation, so configuration survives a restart.
//!
//! Credentials never appear in the UI: the key form field is masked while
//! typing and every stored provider shows the constant "•••••••已存储"
//! placeholder; the key value itself only ever goes to the Keychain.

use gpui::prelude::*;
use gpui::{
    AnyElement, Div, Entity, Global, MouseButton, MouseUpEvent, Window, actions, div, px, relative,
};
use vega_store::config::{self, AppConfig, ProviderConfig};
use vega_store::keystore;
use vega_theme::{Typography, theme};

use crate::text_input::TextInput;

actions!(vega_settings, [OpenSettings, CloseSettings]);

/// Whether the settings view currently replaces the session placeholder.
///
/// Toggled by the app-level [`OpenSettings`]/[`CloseSettings`] handlers.
pub struct SettingsOpen(pub bool);

impl Global for SettingsOpen {}

/// Fixed permission-mode vocabulary (matches `vega_store::config::Defaults`).
const PERMISSION_MODES: [&str; 3] = ["readonly", "confirm", "auto"];

/// Status placeholder shown for every provider with a non-empty `key_ref`;
/// the key value itself is never rendered (safety red line).
const KEY_STORED_PLACEHOLDER: &str = "•••••••已存储";

/// The settings view: a plain page with the provider list, the add-provider
/// form, and the default pickers. Holds its own form input buffers, so it
/// must be cached by the parent across re-renders (it is rebuilt — reloading
/// the config — each time settings is opened).
pub struct SettingsView {
    config: AppConfig,
    name_input: Entity<TextInput>,
    base_url_input: Entity<TextInput>,
    key_input: Entity<TextInput>,
    mode_open: bool,
    model_open: bool,
    /// Inline error message (ui-spec §4.6: no modals); empty until an IO or
    /// Keychain failure occurs.
    error: Option<String>,
}

impl SettingsView {
    /// Loads the current config and creates empty form inputs.
    pub fn new(cx: &mut Context<Self>) -> Self {
        // 构造时载入现有配置：这是"重启后配置恢复"的落点。失败时以内联
        // 错误提示，并以默认配置继续渲染（ui-spec §4.6：不弹模态）。
        let (config, error) = match config::load() {
            Ok(config) => (config, None),
            Err(err) => (AppConfig::default(), Some(format!("配置加载失败：{err}"))),
        };
        let name_input = cx.new(|cx| TextInput::new(cx, "名称", false));
        let base_url_input = cx.new(|cx| TextInput::new(cx, "Base URL", false));
        let key_input = cx.new(|cx| TextInput::new(cx, "API Key", true));
        Self {
            config,
            name_input,
            base_url_input,
            key_input,
            mode_open: false,
            model_open: false,
            error,
        }
    }

    fn on_back(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        // 与 Esc 同效：派发同一动作，由 app 级处理器统一收口。
        window.dispatch_action(Box::new(CloseSettings), cx);
    }

    fn on_submit(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).text().trim().to_string();
        let base_url = self.base_url_input.read(cx).text().trim().to_string();
        let key = self.key_input.read(cx).text().to_string();
        if !form_is_submittable(&name, &base_url, &key) {
            // 空字段时按钮本应无效；这里的守卫保证即便触发也无副作用。
            return;
        }

        // 凭据只进 Keychain（安全红线）；key_ref 约定为 provider 名称。
        if let Err(error) = keystore::set_key(&name, &key) {
            self.error = Some(format!("Keychain 写入失败：{error}"));
            cx.notify();
            return;
        }

        upsert_provider(
            &mut self.config.providers,
            ProviderConfig {
                name: name.clone(),
                base_url,
                // 表单不编辑 models：新增时为空，同名更新时保留原值。
                models: Vec::new(),
                key_ref: name,
            },
        );

        // 立即落盘，满足"保存 → config.toml 更新；重启后配置恢复"。
        if let Err(error) = self.config.save() {
            self.error = Some(format!("配置保存失败：{error}"));
            cx.notify();
            return;
        }

        self.name_input.update(cx, TextInput::clear);
        self.base_url_input.update(cx, TextInput::clear);
        self.key_input.update(cx, TextInput::clear);
        self.error = None;
        cx.notify();
    }

    fn select_mode(&mut self, mode: &'static str, cx: &mut Context<Self>) {
        self.mode_open = false;
        if select_permission_mode(&mut self.config, mode).is_ok() {
            // 改动即保存。
            if let Err(error) = self.config.save() {
                self.error = Some(format!("配置保存失败：{error}"));
            }
        }
        cx.notify();
    }

    fn select_model(&mut self, model: &str, cx: &mut Context<Self>) {
        self.model_open = false;
        set_default_model(&mut self.config, model);
        // 改动即保存。
        if let Err(error) = self.config.save() {
            self.error = Some(format!("配置保存失败：{error}"));
        }
        cx.notify();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::SIDEBAR))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_back))
                    .child("返回"),
            )
            .child(
                div()
                    .text_size(px(Typography::HEADING_PAGE))
                    .font_weight(Typography::HEADING_PAGE_WEIGHT)
                    .child("设置"),
            )
            .into_any_element()
    }

    fn render_provider_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("Provider", colors.text_primary))
            .children(self.config.providers.is_empty().then(|| {
                div()
                    .text_color(colors.text_tertiary)
                    .text_size(px(Typography::BODY))
                    .child("暂无 Provider，使用下方表单添加")
                    .into_any_element()
            }))
            .children(self.config.providers.iter().map(|provider| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(Typography::HEADING_CARD))
                                    .font_weight(Typography::HEADING_CARD_WEIGHT)
                                    .child(provider.name.clone()),
                            )
                            // 凡 key_ref 非空即显示已存储占位，永不回显 key 值。
                            .children((!provider.key_ref.is_empty()).then(|| {
                                div()
                                    .text_color(colors.text_secondary)
                                    .text_size(px(Typography::BODY))
                                    .child(KEY_STORED_PLACEHOLDER)
                            })),
                    )
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .text_size(px(Typography::BODY))
                            .child(provider.base_url.clone()),
                    )
                    .children((!provider.models.is_empty()).then(|| {
                        div()
                            .text_color(colors.text_tertiary)
                            .text_size(px(Typography::BODY))
                            .child(provider.models.join(", "))
                    }))
            }))
            .into_any_element()
    }

    fn render_add_form(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        // 渲染期读取输入内容：既驱动提交按钮的有效态，也让每次键入
        // 重新渲染本视图（GPUI 的渲染期读取即依赖注册）。
        let name = self.name_input.read(cx).text().to_string();
        let base_url = self.base_url_input.read(cx).text().to_string();
        let key = self.key_input.read(cx).text().to_string();
        let submittable = form_is_submittable(&name, &base_url, &key);

        let (button_bg, button_text) = if submittable {
            (colors.accent, colors.bg_base)
        } else {
            (colors.bg_hover, colors.text_tertiary)
        };

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("添加 Provider", colors.text_primary))
            .child(self.name_input.clone())
            .child(self.base_url_input.clone())
            .child(self.key_input.clone())
            .child(
                div()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .self_start()
                    .bg(button_bg)
                    .text_color(button_text)
                    .text_size(px(Typography::SIDEBAR))
                    .when(submittable, |button| {
                        button
                            .cursor_pointer()
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_submit))
                    }),
            )
            .into_any_element()
    }

    fn render_defaults(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("默认项", colors.text_primary))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(field_label("权限模式", colors.text_secondary))
                    .child(self.render_mode_selector(cx)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(field_label("模型", colors.text_secondary))
                    .child(self.render_model_selector(cx)),
            )
            .into_any_element()
    }

    /// In-place expandable picker for the permission mode (minimal equivalent
    /// of a dropdown: click to expand, click an option to choose and collapse).
    fn render_mode_selector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let current = self.config.defaults.permission_mode.clone();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::BODY))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                            this.mode_open = !this.mode_open;
                            cx.notify();
                        }),
                    )
                    .child(current.clone())
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .child(if self.mode_open { "▾" } else { "▸" }),
                    ),
            )
            .when(self.mode_open, |column| {
                column.children(PERMISSION_MODES.iter().map(|mode| {
                    let selected = *mode == current;
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_size(px(Typography::BODY))
                        .cursor_pointer()
                        .when(selected, move |row| row.bg(colors.bg_active))
                        .when(!selected, move |row| {
                            row.hover(move |s| s.bg(colors.bg_hover))
                        })
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                this.select_mode(mode, cx);
                            }),
                        )
                        .child(*mode)
                }))
            })
            .into_any_element()
    }

    /// In-place expandable picker for the default model; options are the
    /// union of all providers' models, with an empty-state hint when none.
    fn render_model_selector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let models = all_models(&self.config.providers);
        let current = self.config.defaults.model.clone();
        let trigger_label = if current.is_empty() {
            "未选择".to_string()
        } else {
            current.clone()
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::BODY))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                            this.model_open = !this.model_open;
                            cx.notify();
                        }),
                    )
                    .child(trigger_label)
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .child(if self.model_open { "▾" } else { "▸" }),
                    ),
            )
            .when(self.model_open, |column| {
                column
                    .children(models.is_empty().then(|| {
                        div()
                            .text_color(colors.text_tertiary)
                            .text_size(px(Typography::BODY))
                            .child("暂无可选模型，先在下方添加 Provider")
                    }))
                    .children(models.iter().map(|model| {
                        let model = model.clone();
                        let selected = model == current;
                        let row_model = model.clone();
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_size(px(Typography::BODY))
                            .cursor_pointer()
                            .when(selected, move |row| row.bg(colors.bg_active))
                            .when(!selected, move |row| {
                                row.hover(move |s| s.bg(colors.bg_hover))
                            })
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                    this.select_model(&row_model, cx);
                                }),
                            )
                            .child(model)
                    }))
            })
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .id("settings-page")
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .text_size(px(Typography::BODY))
            .line_height(relative(Typography::BODY_LINE_HEIGHT))
            .overflow_y_scroll()
            .child(
                // Content column per UI spec §1: max 820px, centered, ≥24px
                // side padding.
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w_full()
                    .max_w(px(820.))
                    .mx_auto()
                    .px(px(24.))
                    .py(px(24.))
                    .child(self.render_header(cx))
                    .children(self.error.clone().map(|message| {
                        div()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(colors.bg_elevated)
                            .border_1()
                            .border_color(colors.danger)
                            .text_color(colors.danger)
                            .text_size(px(Typography::BODY))
                            .child(message)
                    }))
                    .child(self.render_provider_list(cx))
                    .child(self.render_add_form(cx))
                    .child(self.render_defaults(cx)),
            )
    }
}

fn section_title(label: &'static str, color: gpui::Rgba) -> Div {
    div()
        .text_size(px(Typography::HEADING_BLOCK))
        .font_weight(Typography::HEADING_BLOCK_WEIGHT)
        .text_color(color)
        .child(label)
}

fn field_label(label: &'static str, color: gpui::Rgba) -> Div {
    div()
        .w(px(72.))
        .text_color(color)
        .text_size(px(Typography::BODY))
        .child(label)
}

/// Whether the add-provider form may be submitted: name, base_url, and key
/// must all be non-empty (name/base_url trimmed). Empty fields keep the
/// submit button inert (ui-spec §4.6: no error modal).
fn form_is_submittable(name: &str, base_url: &str, key: &str) -> bool {
    !name.trim().is_empty() && !base_url.trim().is_empty() && !key.is_empty()
}

/// Inserts `entry` into `providers`, appending when no provider with the same
/// name exists and replacing the existing one otherwise (the form does not
/// edit models, so a replacement keeps the stored models). Returns whether an
/// existing entry was replaced.
fn upsert_provider(providers: &mut Vec<ProviderConfig>, entry: ProviderConfig) -> bool {
    if let Some(existing) = providers
        .iter_mut()
        .find(|provider| provider.name == entry.name)
    {
        let models = std::mem::take(&mut existing.models);
        *existing = entry;
        existing.models = models;
        true
    } else {
        providers.push(entry);
        false
    }
}

/// Applies a permission-mode choice, rejecting values outside the fixed set.
fn select_permission_mode(config: &mut AppConfig, mode: &str) -> Result<(), &'static str> {
    if !PERMISSION_MODES.contains(&mode) {
        return Err("unknown permission mode");
    }
    config.defaults.permission_mode = mode.to_string();
    Ok(())
}

/// Sets the default model for new conversations.
fn set_default_model(config: &mut AppConfig, model: &str) {
    config.defaults.model = model.to_string();
}

/// Union of every provider's models in first-seen order, deduplicated.
fn all_models(providers: &[ProviderConfig]) -> Vec<String> {
    let mut models = Vec::new();
    for provider in providers {
        for model in &provider.models {
            if !models.contains(model) {
                models.push(model.clone());
            }
        }
    }
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(name: &str, models: &[&str]) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            base_url: format!("https://{name}.example.com"),
            models: models.iter().map(|m| m.to_string()).collect(),
            key_ref: name.to_string(),
        }
    }

    #[test]
    fn form_rejects_empty_fields() {
        assert!(!form_is_submittable("", "https://x", "k"));
        assert!(!form_is_submittable("n", "", "k"));
        assert!(!form_is_submittable("n", "https://x", ""));
        // 空白 name 视为空。
        assert!(!form_is_submittable("   ", "https://x", "k"));
        assert!(form_is_submittable("n", "https://x", "k"));
    }

    #[test]
    fn upsert_appends_new_and_updates_same_name() {
        let mut providers = vec![provider("deepseek", &["deepseek-chat"])];
        // 异名追加。
        assert!(!upsert_provider(&mut providers, provider("openai", &[])));
        assert_eq!(providers.len(), 2);
        assert_eq!(providers[1].name, "openai");
        // 同名更新：表单字段刷新，models 保留。
        let mut replacement = provider("deepseek", &[]);
        replacement.base_url = "https://api.deepseek.com/v1".to_string();
        assert!(upsert_provider(&mut providers, replacement));
        assert_eq!(providers.len(), 2);
        assert_eq!(providers[0].base_url, "https://api.deepseek.com/v1");
        assert_eq!(providers[0].models, vec!["deepseek-chat"]);
    }

    #[test]
    fn permission_mode_accepts_only_the_fixed_set() {
        let mut config = AppConfig::default();
        for mode in PERMISSION_MODES {
            select_permission_mode(&mut config, mode).unwrap();
            assert_eq!(config.defaults.permission_mode, mode);
        }
        assert!(select_permission_mode(&mut config, "yolo").is_err());
        assert_eq!(config.defaults.permission_mode, "auto");
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
}
