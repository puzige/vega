use super::*;
use gpui_kit as gpui;
use gpui_kit::{ParentElement, Styled, TestAppContext, div};
use vega_theme::Typography;

struct BubbleProbe {
    body: AnyElement,
    bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
}

impl IntoElement for BubbleProbe {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for BubbleProbe {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.body.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        *self.bounds.borrow_mut() = Some(bounds);
        self.body.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.body.paint(window, cx);
    }
}

#[gpui::test]
fn long_links_stay_inside_bubbles_and_keep_per_line_targets(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    let cx = cx.add_empty_window();
    let target = "https://github.com/puzige/vega/issues/632";
    let content = format!("为什么这个卡最后被拖回来了{target}");
    let mut heights = Vec::new();
    for width in [1200., 560., 240., 560., 1200., 240.25, 560.75, 1200.25] {
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let captured = recorded.clone();
        let bubble_bounds = Rc::new(RefCell::new(None));
        let bubble_slot = bubble_bounds.clone();
        let source = content.clone();
        cx.draw(
            point(px(0.), px(0.)),
            size(
                AvailableSpace::Definite(px(width)),
                AvailableSpace::MaxContent,
            ),
            move |window, _| {
                assert_eq!(window.scale_factor(), 2.);
                let spans = super::super::autolinks::text_spans(&source);
                let links = super::super::links::link_ranges(&spans);
                let style = window.text_style();
                let runs = vec![style.to_run(source.len())];
                let flow = flow_with_links(
                    source.into(),
                    runs,
                    links.clone(),
                    move |range, content, runs| {
                        let fragment_links = fragment_link_ranges(&links, &range);
                        let text = StyledText::new(content.clone()).with_runs(runs.clone());
                        captured.borrow_mut().push((
                            range,
                            text.layout().clone(),
                            fragment_links.clone(),
                        ));
                        InlineCodeText {
                            id: None,
                            text,
                            content,
                            runs,
                            links: fragment_links,
                        }
                        .into_any_element()
                    },
                );
                div()
                    .w_full()
                    .flex_shrink_0()
                    .pt(px(Layout::USER_MESSAGE_PADDING_TOP))
                    .pb(px(Layout::CONVERSATION_USER_BOTTOM))
                    .flex()
                    .flex_col()
                    .items_end()
                    .child(BubbleProbe {
                        body: div()
                            .max_w(gpui_kit::relative(Layout::CONVERSATION_USER_RATIO))
                            .px(px(Layout::CONVERSATION_USER_PADDING))
                            .py_2()
                            .text_size(px(Typography::CONVERSATION))
                            .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
                            .block()
                            .child(flow)
                            .into_any_element(),
                        bounds: bubble_slot,
                    })
            },
        );
        let bounds = bubble_bounds
            .borrow()
            .expect("bubble probe never reached prepaint");
        let recorded = recorded.borrow();
        assert!(!recorded.is_empty());
        let mut reconstructed = String::new();
        let mut icons = 0;
        let mut linked_rows = Vec::new();
        for (range, layout, links) in recorded.iter() {
            reconstructed.push_str(&content[range.clone()]);
            let text_bounds = layout.bounds();
            assert!(
                text_bounds.bottom() <= bounds.bottom(),
                "{width}: {text_bounds:?} outside {bounds:?}"
            );
            assert!(text_bounds.right() <= bounds.right());
            assert_eq!(
                layout
                    .line_layouts()
                    .iter()
                    .map(|line| line.wrap_boundaries.len())
                    .sum::<usize>(),
                0
            );
            for region in super::super::links::link_regions(layout, links, TextAlign::Left) {
                assert_eq!(links[region.index].target, target);
                assert!(region.bounds.bottom() <= bounds.bottom());
                assert!(region.bounds.right() <= bounds.right());
                linked_rows.push(region.bounds.top());
            }
            icons += links.iter().filter(|link| link.starts_here).count();
        }
        assert_eq!(reconstructed, content);
        assert_eq!(icons, 1);
        linked_rows.dedup();
        if width == 240. {
            assert!(linked_rows.len() >= 2);
        }
        heights.push(bounds.size.height);
    }
    assert!(heights[2] > heights[1]);
    assert!(heights[1] > heights[0]);
    assert_eq!(heights[1], heights[3]);
    assert_eq!(heights[0], heights[4]);
    assert!(heights[5] > heights[6]);
    assert!(heights[6] > heights[7]);
}

#[gpui::test]
fn earlier_measurements_survive_later_width_probes(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    cx.draw(
        point(px(0.), px(0.)),
        size(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        |window, _| {
            let source: SharedString = "https://github.com/puzige/vega/issues/632".into();
            let style = window.text_style();
            let runs = RunIndex::new(vec![style.to_run(source.len())]);
            let mut measurements = FlowMeasurements::default();
            let wide = layout_flow(&source, &runs, &style, Some(px(1000.)), window);
            let wide_size = measurements.insert(Some(px(1000.)), wide);
            let narrow = layout_flow(&source, &runs, &style, Some(px(70.)), window);
            let narrow_size = measurements.insert(Some(px(70.)), narrow);
            assert!(narrow_size.height > wide_size.height);
            let selected = measurements
                .for_bounds(wide_size, window.scale_factor())
                .unwrap();
            assert_eq!(selected.size.height, wide_size.height);
            assert!(
                selected
                    .fragments
                    .iter()
                    .all(|fragment| fragment.origin.y == px(0.))
            );
            let selected = measurements
                .for_bounds(narrow_size, window.scale_factor())
                .unwrap();
            assert_eq!(selected.size.height, narrow_size.height);
            assert!(
                selected
                    .fragments
                    .iter()
                    .any(|fragment| fragment.origin.y > px(0.))
            );
            div()
        },
    );
}
