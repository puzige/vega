use nsh_core::prelude::*;
use vega_theme::{Layout, ThemeColors, Typography};

pub(super) fn field(
    colors: impl Fn() -> ThemeColors + Copy + 'static,
    label: impl IntoElement,
    description: Option<AnyElement>,
    control: impl IntoElement,
) -> AnyElement {
    let mut labels = view()
        .w(px(Layout::SETTINGS_LABEL_WIDTH))
        .max_w(relative(1.))
        .flex_shrink_0()
        .py_1()
        .gap_1()
        .child(label);
    if let Some(description) = description {
        labels = labels.child(
            view()
                .text_size(px(Typography::METADATA))
                .line_height(px(Typography::METADATA * Typography::BODY_LINE_HEIGHT))
                .text_color(move || crate::nsh::color(colors().text_secondary))
                .child(description),
        );
    }
    view()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex_row()
        .flex_wrap()
        .items_start()
        .gap_x(px(Layout::SETTINGS_FIELD_GAP))
        .gap_y_2()
        .py(px(Layout::SETTINGS_ROW_PADDING_Y))
        .border_b_1()
        .border_color(move || crate::nsh::color(colors().border_subtle))
        .child(labels)
        .child(
            view()
                .flex_1()
                .basis(px(220.))
                .min_w_0()
                .max_w(relative(1.))
                .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
                .justify_center()
                .child(control),
        )
        .into_element()
}

pub(super) fn status(
    colors: impl Fn() -> ThemeColors + Copy + 'static,
    foreground: impl Fn() -> gpui_kit::Rgba + Copy + 'static,
    content: impl IntoElement,
) -> AnyElement {
    view()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex_row()
        .items_start()
        .gap_2()
        .p_2()
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border(px(1.))
        .border_color(move || crate::nsh::color(colors().border_subtle))
        .text_color(move || crate::nsh::color(foreground()))
        .text_size(px(Typography::BODY))
        .line_height(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
        .child(
            view()
                .w(px(Layout::ICON_SIZE))
                .h(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .child(dynamic(move || {
                    let palette = colors();
                    let color = foreground();
                    let icon = if color == palette.danger || color == palette.warning {
                        crate::icons::Icon::Warning
                    } else if color == palette.success {
                        crate::icons::Icon::Check
                    } else {
                        crate::icons::Icon::Summary
                    };
                    svg(nsh_core::element::ImageSrc::svg_markup(
                        &crate::icons::nsh_source(icon).unwrap_or_default(),
                    ))
                    .size(px(Layout::ICON_SIZE))
                    .text_color(crate::nsh::color(color))
                })),
        )
        .child(view().min_w_0().flex_1().child(content))
        .into_element()
}
