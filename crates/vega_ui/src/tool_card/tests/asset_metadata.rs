use super::*;
use gpui_kit::{
    Bounds, Context, Render, TestAppContext, VisualTestContext, Window, WindowBounds, WindowHandle,
    WindowOptions, size,
};

fn asset_card() -> ToolCard {
    let call = ToolCall { id:"asset".into(), tool:"read_skill_resource".into(), input_json:r#"{"name":"reviewer","path_bytes":16,"path_sha256":"408fcc923f23ca7719f09e0e225a1ef0e666627a768d46731fc2ab9062ba72e7"}"#.into() };
    let mut card = ToolCard::proposed(&call);
    assert!(card.apply_approved(Approval::Once));
    let mut returned = result(
        ToolCallStatus::Success,
        r#"[Lower-trust Skill asset metadata]
{"name":"reviewer","path":"assets/pixel.png","type":"unknown","size_bytes":42,"lower_trust":true}"#,
    );
    returned.truncated = Some(false);
    assert!(card.apply_finished(&returned));
    card
}

#[test]
fn issue87_s14_metadata_card_is_truthful_content_free_and_reusable() {
    let card = asset_card();
    let text = card.visible_text();
    assert!(text.contains("已查看 Skill reviewer 资源元数据 · 42 bytes · 类型未知 · 未读取内容"));
    for absent in [
        CORRUPT_LABEL,
        "assets/pixel.png",
        "SHA-256",
        "PRIVATE BODY",
        "上传",
        "已读取 Skill reviewer 引用",
    ] {
        assert!(!text.contains(absent));
    }
    let hydrated = ToolCard::hydrated(
        card.input.clone(),
        card.status,
        card.approval,
        card.result.clone(),
    );
    assert_eq!(hydrated.visible_text(), text);
}

struct AssetHarness(Entity<ToolCard>);

impl Render for AssetHarness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(ToolCard::render(
            self.0.clone(),
            "s14-asset".into(),
            false,
            cx,
        ))
    }
}

#[gpui_kit::test]
async fn issue87_s14_metadata_card_mounts_in_light_dark_narrow_and_wide(cx: &mut TestAppContext) {
    for dark in [false, true] {
        for width in [960., 1200., 1229., 1230., 1403.] {
            cx.update(|cx| {
                cx.set_global(if dark {
                    vega_theme::Theme::dark()
                } else {
                    vega_theme::Theme::light()
                });
                crate::init(cx);
            });
            let card = cx.new(|_| asset_card());
            let mounted = card.clone();
            let window: WindowHandle<AssetHarness> = cx
                .update(|cx| {
                    cx.open_window(
                        WindowOptions {
                            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                                None,
                                size(px(width), px(600.)),
                                cx,
                            ))),
                            ..Default::default()
                        },
                        move |_, cx| cx.new(|_| AssetHarness(mounted)),
                    )
                })
                .unwrap();
            cx.run_until_parked();
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            let row = visual.debug_bounds("s14-asset").unwrap();
            let title = visual.debug_bounds("s14-asset-title").unwrap();
            assert!(row.size.width > px(0.) && row.size.width <= px(width));
            assert!(title.size.width > px(0.) && title.size.height > px(0.));
            assert!(title.left() >= row.left() && title.right() <= row.right());
            assert!(
                !card
                    .read_with(cx, |card, _| card.visible_text())
                    .contains("assets/pixel.png")
            );
        }
    }
}
