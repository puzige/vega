use super::*;
use gpui_kit::{Context, Render, TestAppContext, div, prelude::*};

struct LayoutHost;

impl Render for LayoutHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn assistant_style() -> TextStyle {
    TextStyle {
        font_size: px(16.).into(),
        line_height: gpui_kit::relative(1.625),
        ..TextStyle::default()
    }
}

fn code_style(style: &TextStyle) -> TextStyle {
    TextStyle {
        font_family: "Menlo".into(),
        background_color: Some(vega_theme::LIGHT.conversation_code.into()),
        ..style.clone()
    }
}

fn text_layout(
    text: SharedString,
    runs: Vec<TextRun>,
    fragment: &Fragment,
    window: &mut Window,
    cx: &mut App,
) -> TextLayout {
    let styled = StyledText::new(text).with_runs(runs);
    let layout = styled.layout().clone();
    let mut element = styled.into_any_element();
    window.with_text_style(
        Some(TextStyleRefinement {
            font_size: Some(fragment.font_size.into()),
            line_height: Some(gpui_kit::relative(
                f32::from(fragment.line_height) / f32::from(fragment.font_size),
            )),
            white_space: Some(WhiteSpace::Nowrap),
            text_align: Some(TextAlign::Left),
            ..Default::default()
        }),
        |window| {
            element.prepaint_as_root(
                fragment.origin,
                size(
                    AvailableSpace::Definite(fragment.width),
                    AvailableSpace::Definite(fragment.line_height),
                ),
                window,
                cx,
            );
        },
    );
    layout
}

#[gpui_kit::test]
fn punctuation_groups_survive_zero_fit_and_multibyte_wrapping(cx: &mut TestAppContext) {
    let handle = cx.add_window(|_, _| LayoutHost);
    cx.update_window(handle.into(), |_, window, _| {
        let style = assistant_style();
        for (source, first) in [("“中文", "“中"), ("中，文", "中，")] {
            let content: SharedString = source.into();
            let runs =
                RunIndex::new(vec![style.to_run(content.len())]).with_punctuation_rules(true);
            let flow = layout_flow(&content, &runs, &style, Some(px(0.01)), window);
            assert_eq!(&content[flow.fragments[0].range.clone()], first);
            assert_eq!(
                flow.fragments
                    .iter()
                    .map(|part| &content[part.range.clone()])
                    .collect::<String>(),
                source
            );
        }
        let source = "中文，English e\u{301} 👩‍💻（混合）文字！";
        let content: SharedString = source.into();
        let runs = RunIndex::new(vec![style.to_run(content.len())]).with_punctuation_rules(true);
        for width in [1., 35., 83., 240.] {
            let flow = layout_flow(&content, &runs, &style, Some(px(width)), window);
            let mut end = 0;
            for fragment in &flow.fragments {
                assert_eq!(fragment.range.start, end);
                assert!(fragment.range.end > end);
                assert!(source.is_char_boundary(fragment.range.start));
                assert!(source.is_char_boundary(fragment.range.end));
                let part = &source[fragment.range.clone()];
                assert!(!part.starts_with('\u{301}'));
                assert!(!part.starts_with('\u{200d}'));
                assert!(!part.ends_with('\u{200d}'));
                if fragment.range.start > 0 {
                    assert!(!"，。！）”".contains(part.chars().next().unwrap()));
                }
                if fragment.range.end < source.len() {
                    assert!(!"（“".contains(part.chars().next_back().unwrap()));
                }
                end = fragment.range.end;
            }
            assert_eq!(end, source.len());
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn mixed_code_fragments_match_text_layout_and_share_a_baseline(cx: &mut TestAppContext) {
    let handle = cx.add_window(|_, _| LayoutHost);
    cx.update_window(handle.into(), |_, window, cx| {
        let style = assistant_style();
        let content: SharedString = "正文 inline_code tail".into();
        let prefix = "正文 ".len();
        let code_end = prefix + "inline_code".len();
        let runs = RunIndex::new(vec![
            style.to_run(prefix),
            code_style(&style).to_run(code_end - prefix),
            style.to_run(content.len() - code_end),
        ])
        .with_punctuation_rules(true);
        let flow = layout_flow(&content, &runs, &style, None, window);
        assert_eq!(flow.fragments.len(), 3);
        let baseline = flow.fragments[0].origin.y + flow.fragments[0].baseline;
        for fragment in &flow.fragments {
            let layout = text_layout(
                content[fragment.range.clone()].to_owned().into(),
                runs.slice(fragment.range.clone()),
                fragment,
                window,
                cx,
            );
            assert!((layout.bounds().size.width - fragment.width).abs() <= px(0.01));
            assert_eq!(layout.line_height(), fragment.line_height);
            let line = layout.line_layouts()[0].clone();
            let actual = layout.bounds().top()
                + (layout.line_height() - line.ascent() - line.descent()) / 2.
                + line.ascent();
            assert!((actual - baseline).abs() <= px(0.01));
            for glyph in line.runs().iter().flat_map(|run| &run.glyphs) {
                let point = layout.position_for_index(glyph.index).unwrap();
                assert_eq!(layout.index_for_position(point).unwrap(), glyph.index);
            }
        }
        let code = &flow.fragments[1];
        assert_eq!(code.font_size, px(16. * 0.92));
        assert_eq!(code.origin.x - flow.fragments[0].width, px(6.));
        assert_eq!(
            flow.fragments[2].origin.x - code.origin.x - code.width,
            px(6.)
        );
        assert!(flow.size.height >= px(26.));
        assert!(code.origin.y - px(1.) >= px(0.));
        assert!(code.origin.y + code.line_height + px(1.) <= flow.size.height);
        let legacy = RunIndex::new(runs.slice(0..content.len()));
        let old = layout_flow(&content, &legacy, &style, None, window);
        assert!(old.fragments.iter().all(|part| part.font_size == px(16.)));
        assert_eq!(old.fragments[1].origin.x - old.fragments[0].width, px(4.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn code_link_icons_share_hit_geometry_without_changing_instance_identity(cx: &mut TestAppContext) {
    let handle = cx.add_window(|_, _| LayoutHost);
    cx.update_window(handle.into(), |_, window, cx| {
        let style = assistant_style();
        let content: SharedString = "code_label continuation".into();
        let spans = vec![super::super::model::StreamSpan {
            text: content.to_string(),
            style: super::super::model::SpanStyle::Code,
            link: Some("https://example.com/path?q=1#fragment".into()),
            link_occurrence: Some(4),
        }];
        let mut links = super::super::links::link_ranges(&spans);
        super::super::links::own_links(&mut links, "assistant-owner");
        let runs = RunIndex::new(vec![code_style(&style).to_run(content.len())])
            .with_links(links.clone())
            .with_punctuation_rules(true);
        let flow = layout_flow(&content, &runs, &style, Some(px(90.)), window);
        assert!(flow.fragments.len() > 1);
        let mut icons = 0;
        for fragment in &flow.fragments {
            let layout = text_layout(
                content[fragment.range.clone()].to_owned().into(),
                runs.slice(fragment.range.clone()),
                fragment,
                window,
                cx,
            );
            let mut local = fragment_link_ranges(&links, &fragment.range);
            for link in &mut local {
                link.assistant_code_padding = fragment.code && link.starts_here;
            }
            let hit = crate::link_preview::interactive_bounds(layout.bounds(), &local);
            for link in &local {
                assert_eq!(link.owner, "assistant-owner");
                assert_eq!(link.occurrence, 4);
                assert_eq!(link.text_identity, links[0].text_identity);
                if let Some(icon) = crate::link_preview::icon_bounds(&layout, link) {
                    icons += 1;
                    assert_eq!(layout.bounds().left() - px(6.) - icon.right(), px(4.));
                    assert!(hit.contains(&icon.center()));
                }
            }
        }
        assert_eq!(icons, 1);
        let mut repeated = spans;
        repeated.push(super::super::model::StreamSpan {
            text: "again".into(),
            style: super::super::model::SpanStyle::Link,
            link: Some(links[0].target.clone()),
            link_occurrence: Some(99),
        });
        let repeated = super::super::links::link_ranges(&repeated);
        assert_eq!(repeated.len(), 2);
        assert_ne!(repeated[0].occurrence, repeated[1].occurrence);
    })
    .unwrap();
}
