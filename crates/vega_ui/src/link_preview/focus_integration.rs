use super::*;
use gpui_kit::TestAppContext;
use std::cell::Cell;

fn link() -> LinkRange {
    LinkRange {
        range: 0..12,
        target: "https://example.com/path".into(),
        occurrence: 0,
        label: "wrapped link".into(),
        owner: "mounted-reading".into(),
        starts_here: true,
        text_identity: 7,
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

struct FocusFixture {
    links: Vec<(LinkRange, Vec<Bounds<Pixels>>)>,
    validity: Rc<Cell<bool>>,
    painted: Rc<std::cell::RefCell<Vec<Bounds<Pixels>>>>,
}

impl Render for FocusFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let links = self.links.clone();
        let owners = links
            .iter()
            .map(|(link, _)| link.clone())
            .collect::<Vec<_>>();
        let valid = self.validity.clone();
        let painted = self.painted.clone();
        div()
            .size_full()
            .child(
                canvas(
                    move |_, window, cx| {
                        let mut targets = Vec::new();
                        for (link, bounds) in links {
                            let valid = valid.clone();
                            targets.extend(prepaint_measured_link_target(
                                &link,
                                bounds,
                                Vec::new(),
                                Some(Rc::new(move |_, _| valid.get())),
                                window,
                                cx,
                            ));
                        }
                        targets
                    },
                    move |_, mut targets, window, cx| {
                        for target in &mut targets {
                            target.paint(window, cx);
                        }
                        let current = coordinator(window, cx).unwrap();
                        *painted.borrow_mut() =
                            measured_focus_rects(&current.read(cx).visible, &owners, window, cx);
                    },
                )
                .size_full(),
            )
            .child(MarkdownLinkPreviewLayer)
    }
}

pub fn measured_focus_paints_wrapped_fragments_and_tracks_tab_reverse_and_escape(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
    let first = link();
    let mut second = first.clone();
    second.owner = "second-occurrence".into();
    second.occurrence = 1;
    let first_bounds = vec![rect(20., 20., 80., 15.), rect(20., 40., 35., 15.)];
    let second_bounds = vec![rect(20., 80., 65., 15.)];
    let painted = Rc::new(std::cell::RefCell::new(Vec::new()));
    let output = painted.clone();
    let valid = Rc::new(Cell::new(true));
    let validity = valid.clone();
    let (_, visual) = cx.add_window_view(move |_, _| FocusFixture {
        links: vec![(first, first_bounds), (second, second_bounds)],
        validity,
        painted: output,
    });
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let entity = visual.update(acquire);
    visual.update(|window, cx| {
        assert_eq!(entity.read(cx).visible.len(), 2);
        let focus = entity.read(cx).visible[0].focus.clone();
        window.focus(&focus, cx);
        window.refresh();
        window.draw(cx).clear(cx);
        assert_eq!(
            *painted.borrow(),
            vec![rect(20., 20., 80., 15.), rect(20., 40., 35., 15.)]
        );
        entity.update(cx, |state, cx| {
            state.dismiss(false, window, cx);
        });
        keyboard(&entity, "tab", false, window, cx);
        assert!(entity.read(cx).visible[1].focus.is_focused(window));
        window.refresh();
        window.draw(cx).clear(cx);
        assert_eq!(*painted.borrow(), vec![rect(20., 80., 65., 15.)]);
        keyboard(&entity, "tab", true, window, cx);
        assert!(entity.read(cx).visible[0].focus.is_focused(window));
        window.refresh();
        window.draw(cx).clear(cx);
        keyboard(&entity, "escape", false, window, cx);
        assert!(entity.read(cx).active.is_none());
        assert!(entity.read(cx).armed.is_none());
        window.refresh();
        window.draw(cx).clear(cx);
        assert_eq!(
            *painted.borrow(),
            vec![rect(20., 20., 80., 15.), rect(20., 40., 35., 15.)]
        );
        assert!(
            !painted
                .borrow()
                .iter()
                .any(|bounds| bounds.contains(&point(px(25.), px(37.))))
        );
        valid.set(false);
        window.refresh();
        window.draw(cx).clear(cx);
        assert!(painted.borrow().is_empty());
    });
}
