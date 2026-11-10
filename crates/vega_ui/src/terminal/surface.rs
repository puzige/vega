use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::Arc};

use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{TerminalCell, TerminalColor, TerminalSnapshot, TerminalStatus};
use vega_theme::{Layout, ThemeColors, Typography};

use crate::nsh::OrderedCommands;

pub(super) const MAX_PASTE: usize = 64 * 1024 - 12;
const FONT: &str = "Menlo, Consolas, monospace";
const PAD: f32 = 8.;

#[derive(Clone)]
pub(super) struct Projection {
    pub snapshot: Option<Arc<TerminalSnapshot>>,
    pub cell_width: Option<f32>,
    pub menu_dismissal: u64,
    pub epoch: u64,
    pub colors: ThemeColors,
    pub status_label: String,
    pub copy_label: String,
    pub restart_label: String,
    pub error_label: String,
    pub input_error: bool,
}

#[derive(Clone)]
pub(super) enum Command {
    Input(Vec<u8>),
    Paste(String),
    Resize(u16, u16),
    ScrollTo(usize),
    Restart,
}

pub(super) struct Request {
    pub epoch: u64,
    pub command: Command,
    pub sequence: Option<u64>,
    pub reply: Option<oneshot::Sender<bool>>,
}

#[derive(Clone, Copy, PartialEq)]
struct Metrics {
    cell: f32,
    line: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Selection {
    generation: u64,
    start: usize,
    end: usize,
}

impl Selection {
    fn range(self, generation: u64) -> Option<std::ops::Range<usize>> {
        (self.generation == generation && self.start != self.end)
            .then_some(self.start.min(self.end)..self.start.max(self.end))
    }
}

struct PendingInput {
    sequence: u64,
    command: Command,
    bytes: usize,
}

#[derive(Clone)]
struct InputQueue {
    sender: mpsc::Sender<Request>,
    items: Rc<RefCell<VecDeque<PendingInput>>>,
    next_sequence: Signal<u64>,
    count: Signal<usize>,
    bytes: Signal<usize>,
    running: Signal<bool>,
    failed: Signal<bool>,
    epoch: u64,
}

impl InputQueue {
    fn new(sender: mpsc::Sender<Request>, epoch: u64) -> Self {
        Self {
            sender,
            items: Rc::new(RefCell::new(VecDeque::new())),
            next_sequence: signal(1),
            count: signal(0),
            bytes: signal(0),
            running: signal(false),
            failed: signal(false),
            epoch,
        }
    }

    fn push(&self, command: Command) -> bool {
        let bytes = match &command {
            Command::Input(bytes) => bytes.len(),
            Command::Paste(text) => text.len(),
            _ => return false,
        };
        if bytes > MAX_PASTE
            || self.count.get_untracked() >= crate::nsh::COMMAND_CAPACITY
            || self.bytes.get_untracked().saturating_add(bytes) > 2 * 1024 * 1024
        {
            self.failed.set(true);
            return false;
        }
        let sequence = self.next_sequence.get_untracked();
        let Some(next) = sequence.checked_add(1) else {
            self.failed.set(true);
            return false;
        };
        self.next_sequence.set(next);
        self.count.update(|count| *count += 1);
        self.bytes.update(|size| *size += bytes);
        self.items.borrow_mut().push_back(PendingInput {
            sequence,
            command,
            bytes,
        });
        if !self.failed.get_untracked() {
            self.drain();
        }
        true
    }

    fn drain(&self) {
        if self.running.get_untracked() {
            return;
        }
        self.running.set(true);
        self.failed.set(false);
        let queue = self.clone();
        spawn(async move {
            loop {
                let Some(pending) = queue.items.borrow_mut().pop_front() else {
                    break;
                };
                let (reply, accepted) = oneshot::channel();
                let request = Request {
                    epoch: queue.epoch,
                    command: pending.command.clone(),
                    sequence: Some(pending.sequence),
                    reply: Some(reply),
                };
                if queue.sender.send(request).await.is_err() {
                    queue.items.borrow_mut().push_front(pending);
                    queue.failed.set(true);
                    break;
                }
                match accepted.await {
                    Ok(true) => {
                        queue.count.update(|count| *count -= 1);
                        queue.bytes.update(|bytes| *bytes -= pending.bytes);
                    }
                    Ok(false) | Err(_) => {
                        queue.items.borrow_mut().push_front(pending);
                        queue.failed.set(true);
                        break;
                    }
                }
            }
            queue.running.set(false);
        });
    }
}

fn rgb(color: gpui_kit::Rgba) -> Rgba {
    crate::nsh::color(color)
}

fn ansi(color: TerminalColor, fallback: gpui_kit::Rgba) -> Rgba {
    match color {
        TerminalColor::Default => rgb(fallback),
        TerminalColor::Rgb(r, g, b) => Rgba {
            r: r as f32 / 255.,
            g: g as f32 / 255.,
            b: b as f32 / 255.,
            a: 1.,
        },
        TerminalColor::Indexed(index) => rgb(vega_theme::terminal_indexed_color(index)),
    }
}

fn cell_colors(cell: &TerminalCell, colors: &ThemeColors, selected: bool) -> (Rgba, Rgba) {
    if selected {
        return (rgb(colors.brand_on_accent), rgb(colors.brand_primary));
    }
    let foreground = ansi(cell.foreground, colors.text_primary);
    let background = ansi(cell.background, colors.bg_base);
    if cell.inverse {
        (background, foreground)
    } else {
        (foreground, background)
    }
}

fn draw(
    projection: &Projection,
    metrics: Metrics,
    selection: Option<Selection>,
    show_cursor: bool,
) -> Painter {
    let mut painter = Painter::new();
    let Some(snapshot) = &projection.snapshot else {
        return painter;
    };
    let cols = snapshot.cells.first().map_or(1, |row| row.len().max(1));
    let range = selection.and_then(|selection| selection.range(snapshot.generation));
    let selected = |index| range.as_ref().is_some_and(|range| range.contains(&index));
    for (row, cells) in snapshot.cells.iter().enumerate() {
        let y = PAD + row as f32 * metrics.line;
        for (col, cell) in cells.iter().enumerate() {
            let lead = if cell.continuation {
                col.saturating_sub(1)
            } else {
                col
            };
            let (_, background) = cell_colors(
                &cells[lead],
                &projection.colors,
                selected(row * cols + lead),
            );
            painter = painter.fill_style(background).fill_rect(
                PAD + col as f32 * metrics.cell,
                y,
                metrics.cell,
                metrics.line,
            );
        }
        for (col, cell) in cells.iter().enumerate() {
            if !cell.continuation && !cell.text.is_empty() {
                let (foreground, _) =
                    cell_colors(cell, &projection.colors, selected(row * cols + col));
                let font = format!(
                    "{}{}px {FONT}",
                    if cell.bold { "bold " } else { "" },
                    Typography::CODE
                );
                painter = painter
                    .font(&font)
                    .text_baseline(nsh_core::canvas::CanvasTextBaseline::Middle)
                    .fill_style(foreground)
                    .fill_text(
                        &cell.text,
                        PAD + col as f32 * metrics.cell,
                        y + metrics.line * 0.5,
                    );
            }
        }
    }
    if show_cursor
        && snapshot.scrollback == 0
        && let Some((row, col)) = snapshot.cursor
    {
        painter = painter.fill_style(rgb(projection.colors.accent)).fill_rect(
            PAD + f32::from(col) * metrics.cell,
            PAD + (f32::from(row) + 1.) * metrics.line - 2.,
            metrics.cell,
            2.,
        );
    }
    painter
}

fn cell_boundary(point: Point<Points>, snapshot: &TerminalSnapshot, metrics: Metrics) -> usize {
    let cols = snapshot.cells.first().map_or(0, Vec::len);
    let row = (((point.y.0 - PAD) / metrics.line).floor() as usize)
        .min(snapshot.cells.len().saturating_sub(1));
    let mut col = (((point.x.0 - PAD) / metrics.cell).round() as usize).min(cols);
    if col > 0
        && snapshot
            .cells
            .get(row)
            .and_then(|cells| cells.get(col))
            .is_some_and(|cell| cell.continuation)
    {
        col -= 1;
    }
    row * cols + col
}

fn copy_text(snapshot: &TerminalSnapshot, selection: Option<Selection>) -> String {
    let selection = selection.and_then(|selection| selection.range(snapshot.generation));
    let cols = snapshot.cells.first().map_or(0, Vec::len);
    snapshot
        .cells
        .iter()
        .enumerate()
        .filter_map(|(row, cells)| {
            let start = row * cols;
            let end = start + cols;
            let range = selection.as_ref().map_or(start..end, |range| {
                range.start.max(start)..range.end.min(end)
            });
            if range.is_empty() {
                return None;
            }
            let mut text = cells
                .iter()
                .enumerate()
                .filter(|(col, cell)| range.contains(&(start + col)) && !cell.continuation)
                .map(|(_, cell)| {
                    if cell.text.is_empty() {
                        " "
                    } else {
                        cell.text.as_str()
                    }
                })
                .collect::<String>();
            if range.end == end {
                text.truncate(text.trim_end().len());
            }
            Some(text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn bindings(
    queue: InputQueue,
    projection: Signal<Projection>,
    selection: Signal<Option<Selection>>,
    menu_open: Signal<bool>,
) -> impl IntoElement {
    let bind = |key: &str, bytes: Vec<u8>| {
        let queue = queue.clone();
        on_key_with(
            key,
            KeyOptions {
                when: nsh_core::keymap::When::Always,
                repeat: true,
            },
            move |_| {
                if !menu_open.get_untracked() {
                    queue.push(Command::Input(bytes.clone()));
                }
            },
        );
    };
    for (key, value) in [
        ("enter", "\r"),
        ("tab", "\t"),
        ("shift+tab", "\x1b[Z"),
        ("backspace", "\x7f"),
        ("escape", "\x1b"),
        ("home", "\x1b[H"),
        ("end", "\x1b[F"),
        ("delete", "\x1b[3~"),
        ("pageup", "\x1b[5~"),
        ("pagedown", "\x1b[6~"),
    ] {
        bind(key, value.as_bytes().to_vec());
    }
    for (key, normal, application) in [
        ("up", "\x1b[A", "\x1bOA"),
        ("down", "\x1b[B", "\x1bOB"),
        ("right", "\x1b[C", "\x1bOC"),
        ("left", "\x1b[D", "\x1bOD"),
    ] {
        let queue = queue.clone();
        on_key_with(
            key,
            KeyOptions {
                when: nsh_core::keymap::When::Always,
                repeat: true,
            },
            move |_| {
                if menu_open.get_untracked() {
                    return;
                }
                let application_cursor = projection
                    .get()
                    .snapshot
                    .is_some_and(|snapshot| snapshot.application_cursor);
                queue.push(Command::Input(
                    if application_cursor {
                        application
                    } else {
                        normal
                    }
                    .as_bytes()
                    .to_vec(),
                ));
            },
        );
    }
    for byte in b'a'..=b'z' {
        let key = format!("ctrl+{}", char::from(byte));
        bind(&key, vec![byte - b'a' + 1]);
    }
    bind("ctrl+space", vec![0]);
    bind("ctrl+[", vec![27]);
    bind("ctrl+\\", vec![28]);
    bind("ctrl+]", vec![29]);
    bind("ctrl+shift+6", vec![30]);
    bind("ctrl+shift+-", vec![31]);
    on_key_with(
        "mod+c",
        KeyOptions {
            when: nsh_core::keymap::When::Always,
            ..Default::default()
        },
        move |_| {
            if !menu_open.get_untracked()
                && let Some(snapshot) = projection.get().snapshot
            {
                set_clipboard(copy_text(&snapshot, selection.get()));
            }
        },
    );
    on_key_with(
        "shift+f10",
        KeyOptions {
            when: nsh_core::keymap::When::Always,
            ..Default::default()
        },
        move |_| menu_open.set(true),
    );
    view()
}

fn terminal_action(
    projection: Signal<Projection>,
    kind: crate::icons::Icon,
    label: fn(&Projection) -> String,
    press: impl Fn() + 'static,
) -> AnyElement {
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .label(move || label(&projection.get()))
        .tooltip(text(move || label(&projection.get())))
        .size(px(Layout::TITLEBAR_CONTROL_SIZE))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border(px(1.))
        .border_color(move || {
            let colors = projection.get().colors;
            rgb(if focused.get() {
                colors.accent
            } else {
                colors.bg_base.opacity(0.)
            })
        })
        .hover(move |style| style.bg(rgb(projection.get().colors.bg_hover)))
        .items_center()
        .justify_center()
        .on_press(move |_: &nsh_core::PressEvent| press())
        .child(
            svg(nsh_core::element::ImageSrc::svg_markup(
                &crate::icons::nsh_source(kind).unwrap_or_default(),
            ))
            .size(px(Layout::ICON_SIZE))
            .text_color(move || rgb(projection.get().colors.text_secondary))
            .tint_color(move || rgb(projection.get().colors.text_secondary).to_hsla()),
        )
        .into_element()
}

pub(super) fn render(projection: Signal<Projection>, sender: mpsc::Sender<Request>) -> AnyElement {
    let commands = OrderedCommands::new(sender.clone());
    let failed = commands.failed;
    view()
        .size_full()
        .bg(move || rgb(projection.get().colors.bg_base).to_hsla())
        .text_color(move || rgb(projection.get().colors.text_primary).to_hsla())
        .child(each(
            move || vec![projection.get().epoch],
            |epoch| *epoch,
            move |epoch, _| {
                let queue = InputQueue::new(sender.clone(), epoch.get());
                let queue_failed = queue.failed;
                let focused = signal(false);
                let composing = signal(false);
                let selection = signal(None::<Selection>);
                let metrics = Metrics {
                    cell: projection.get_untracked().cell_width.unwrap_or_else(|| {
                        text_metrics(&format!("{}px {FONT}", Typography::CODE), "M")
                            .width
                            .0
                            .max(1.)
                    }),
                    line: Typography::CODE * 1.5,
                };
                let bounds = signal(Size {
                    width: px(0.),
                    height: px(0.),
                });
                let scroll_ref = element_ref();
                let native_input = element_ref();
                let update_selection = move |event: &ResponderEvent| {
                    if let Some(snapshot) = projection.get_untracked().snapshot {
                        selection.update(|selection| {
                            if let Some(selection) = selection
                                .as_mut()
                                .filter(|selection| selection.generation == snapshot.generation)
                            {
                                selection.end = cell_boundary(event.location, &snapshot, metrics);
                            }
                        });
                    }
                };
                let menu_open = signal(false);
                let menu_items = [element_ref(), element_ref()];
                let menu_selection = signal(0usize);
                let menu_focused = signal(false);
                for (index, item) in menu_items.into_iter().enumerate() {
                    item.on_focus_in(move || menu_selection.set(index));
                }
                let leave_menu = move |restore_focus: bool| {
                    menu_open.set(false);
                    menu_focused.set(false);
                    if restore_focus {
                        native_input.focus();
                    }
                };
                let last_dismissal =
                    std::cell::Cell::new(projection.get_untracked().menu_dismissal);
                effect(move || {
                    let requested = projection.with(|value| value.menu_dismissal);
                    if last_dismissal.replace(requested) != requested && menu_open.get_untracked() {
                        leave_menu(false);
                    }
                });
                let text_value = signal(String::new());
                let commit = queue.clone();
                let paste = queue.clone();
                let submit = queue.clone();
                let keys = queue.clone();
                let retry = queue.clone();
                let resize = commands.clone();
                let restart = commands.clone();
                let scrolling = commands.clone();
                let menu_restart = commands.clone();
                let choose_menu = move |index: usize| {
                    if !menu_open.get_untracked() {
                        return;
                    }
                    leave_menu(true);
                    if index == 0 {
                        if let Some(snapshot) = projection.get_untracked().snapshot {
                            set_clipboard(copy_text(&snapshot, selection.get_untracked()));
                        }
                    } else {
                        menu_restart.send(Request {
                            epoch: epoch.get(),
                            command: Command::Restart,
                            sequence: None,
                            reply: None,
                        });
                    }
                };
                effect(move || {
                    let size = bounds.get();
                    if size.width.0 > PAD * 2. && size.height.0 > PAD * 2. {
                        resize.send(Request {
                            epoch: epoch.get(),
                            command: Command::Resize(
                                ((size.height.0 - PAD * 2.) / metrics.line)
                                    .floor()
                                    .clamp(2., 240.) as u16,
                                ((size.width.0 - PAD * 2.) / metrics.cell)
                                    .floor()
                                    .clamp(2., 400.) as u16,
                            ),
                            sequence: None,
                            reply: None,
                        });
                    }
                });
                effect(move || {
                    if let Some(snapshot) = projection.get().snapshot {
                        selection.update(|selection| {
                            if selection.is_some_and(|selection| {
                                selection.generation != snapshot.generation
                            }) {
                                *selection = None;
                            }
                        });
                        scroll_ref.scroll_to(
                            point(
                                px(0.),
                                px(
                                    (snapshot.max_scrollback.saturating_sub(snapshot.scrollback))
                                        as f32
                                        * metrics.line,
                                ),
                            ),
                            false,
                        );
                    }
                });
                view()
                    .size_full()
                    .child(when(
                        move || {
                            focused.get()
                                && !menu_open.get()
                                && !composing.get()
                                && !queue_failed.get()
                                && text_value.get().is_empty()
                        },
                        move || bindings(keys.clone(), projection, selection, menu_open),
                    ))
                    .child(when(move || menu_open.get(), {
                        let choose = choose_menu.clone();
                        move || {
                            let options = || KeyOptions {
                                when: nsh_core::keymap::When::Always,
                                ..Default::default()
                            };
                            for key in ["up", "down", "tab", "shift+tab"] {
                                on_key_with(key, options(), move |_| {
                                    let next = 1 - menu_selection.get_untracked();
                                    menu_selection.set(next);
                                    menu_items[next].focus();
                                });
                            }
                            on_key_with("escape", options(), move |_| leave_menu(true));
                            for key in ["enter", "space"] {
                                let choose = choose.clone();
                                on_key_with(key, options(), move |_| {
                                    choose(menu_selection.get_untracked())
                                });
                            }
                            view()
                        }
                    }))
                    .child(when(
                        move || !projection.get().status_label.is_empty(),
                        move || {
                            let restart = restart.clone();
                            view()
                                .h(px(Layout::TERMINAL_TOOLBAR_HEIGHT))
                                .flex_shrink_0()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .px_3()
                                .border_b_1()
                                .border_color(move || rgb(projection.get().colors.border_subtle))
                                .text_size(px(Typography::METADATA))
                                .text_color(move || rgb(projection.get().colors.text_secondary))
                                .child(
                                    text(move || projection.get().status_label)
                                        .min_w_0()
                                        .flex_1()
                                        .truncate(),
                                )
                                .child(terminal_action(
                                    projection,
                                    crate::icons::Icon::Document,
                                    |p| p.copy_label.clone(),
                                    move || {
                                        if let Some(snapshot) = projection.get().snapshot {
                                            set_clipboard(copy_text(&snapshot, None));
                                        }
                                    },
                                ))
                                .child(terminal_action(
                                    projection,
                                    crate::icons::Icon::Refresh,
                                    |p| p.restart_label.clone(),
                                    move || {
                                        restart.send(Request {
                                            epoch: epoch.get(),
                                            command: Command::Restart,
                                            sequence: None,
                                            reply: None,
                                        })
                                    },
                                ))
                        },
                    ))
                    .child(when(
                        move || projection.get().input_error || queue_failed.get(),
                        move || {
                            let retry = retry.clone();
                            view()
                                .flex_row()
                                .gap_2()
                                .child(text(move || projection.get().error_label).text_color(
                                    move || rgb(projection.get().colors.danger).to_hsla(),
                                ))
                                .child(
                                    nsh_base::button()
                                        .disabled(move || !queue_failed.get())
                                        .on_press(move || retry.drain())
                                        .child(text(vega_i18n::t("ui.retry"))),
                                )
                        },
                    ))
                    .child(
                        nsh_base::context_menu()
                            .open(move || Some(menu_open.get()))
                            .disabled(move || composing.get())
                            .on_open_change(move |event: &nsh_base::MenuEvent| {
                                if let nsh_base::MenuEvent::OpenChange(open, reason) = event {
                                    if *open {
                                        menu_selection.set(0);
                                        menu_focused.set(false);
                                        menu_open.set(true);
                                    } else if menu_open.get_untracked() {
                                        leave_menu(matches!(
                                            reason,
                                            nsh_base::OpenChangeReason::Cancel
                                                | nsh_base::OpenChangeReason::Select
                                        ));
                                    }
                                }
                            })
                            .flex_1()
                            .w_full()
                            .min_h_0()
                            .trigger(
                                view()
                                    .relative()
                                    .size_full()
                                    .overflow_hidden()
                                    .child(
                                        scroll()
                                            .size_full()
                                            .min_h(px(0.))
                                            .element_ref(&scroll_ref)
                                            .on_layout(move |rect: &Bounds<Points>| {
                                                bounds.set(rect.size)
                                            })
                                            .content_offset(point(
                                                px(0.),
                                                px(projection.get_untracked().snapshot.map_or(
                                                    0,
                                                    |snapshot| {
                                                        snapshot
                                                            .max_scrollback
                                                            .saturating_sub(snapshot.scrollback)
                                                    },
                                                )
                                                    as f32
                                                    * metrics.line),
                                            ))
                                            .on_scroll(move |event: &ScrollEvent| {
                                                let maximum = projection
                                                    .get()
                                                    .snapshot
                                                    .map_or(0, |snapshot| snapshot.max_scrollback);
                                                let offset = maximum.saturating_sub(
                                                    (event.offset.y.0 / metrics.line)
                                                        .round()
                                                        .max(0.)
                                                        as usize,
                                                );
                                                if projection.get().snapshot.is_some_and(
                                                    |snapshot| snapshot.scrollback != offset,
                                                ) {
                                                    scrolling.send(Request {
                                                        epoch: epoch.get(),
                                                        command: Command::ScrollTo(offset),
                                                        sequence: None,
                                                        reply: None,
                                                    });
                                                }
                                            })
                                            .child(
                                                view()
                                                    .w_full()
                                                    .h(move || {
                                                        px(bounds.get().height.0
                                                            + projection
                                                                .get()
                                                                .snapshot
                                                                .map_or(0, |snapshot| {
                                                                    snapshot.max_scrollback
                                                                })
                                                                as f32
                                                                * metrics.line)
                                                    })
                                                    .child(
                                                        canvas(move || {
                                                            draw(
                                                                &projection.get(),
                                                                metrics,
                                                                selection.get(),
                                                                focused.get() && !composing.get(),
                                                            )
                                                        })
                                                        .sticky()
                                                        .top(px(0.))
                                                        .w_full()
                                                        .h(move || bounds.get().height)
                                                        .claim_touches(TouchClaim::on_start())
                                                        .on_responder_grant(
                                                            move |event: &ResponderEvent| {
                                                                native_input.focus();
                                                                if let Some(snapshot) =
                                                                    projection.get().snapshot
                                                                {
                                                                    let index = cell_boundary(
                                                                        event.location,
                                                                        &snapshot,
                                                                        metrics,
                                                                    );
                                                                    selection.set(Some(
                                                                        Selection {
                                                                            generation: snapshot
                                                                                .generation,
                                                                            start: index,
                                                                            end: index,
                                                                        },
                                                                    ));
                                                                }
                                                            },
                                                        )
                                                        .on_responder_move(update_selection)
                                                        .on_responder_release(update_selection),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        nsh_core::element::input()
                                            .absolute()
                                            .left(move || {
                                                px(PAD
                                                    + projection
                                                        .get()
                                                        .snapshot
                                                        .and_then(|s| s.cursor)
                                                        .map_or(0., |(_, col)| {
                                                            f32::from(col) * metrics.cell
                                                        }))
                                            })
                                            .top(move || {
                                                px(PAD
                                                    + projection
                                                        .get()
                                                        .snapshot
                                                        .and_then(|s| s.cursor)
                                                        .map_or(0., |(row, _)| {
                                                            f32::from(row) * metrics.line
                                                        }))
                                            })
                                            .w(move || {
                                                let col = projection
                                                    .get()
                                                    .snapshot
                                                    .and_then(|s| s.cursor)
                                                    .map_or(0., |(_, col)| f32::from(col));
                                                px(
                                                    if composing.get()
                                                        || !text_value.get().is_empty()
                                                    {
                                                        (bounds.get().width.0
                                                            - PAD * 2.
                                                            - col * metrics.cell)
                                                            .max(metrics.cell)
                                                    } else {
                                                        metrics.cell
                                                    },
                                                )
                                            })
                                            .h(px(metrics.line))
                                            .line_height(px(metrics.line))
                                            .p_0()
                                            .bg(move || {
                                                rgb(projection.get().colors.bg_base.opacity(0.))
                                            })
                                            .element_ref(&native_input)
                                            .value(text_value)
                                            .label(vega_i18n::t("ui.terminal"))
                                            .accessibility_actions(vec![
                                                nsh_core::AccessibilityActionDef {
                                                    name: "show-menu".into(),
                                                    label: Some(vega_i18n::t("ui.actions").into()),
                                                },
                                            ])
                                            .on_accessibility_action(move |name: &str| {
                                                if name == "show-menu" && !composing.get_untracked()
                                                {
                                                    menu_open.set(true);
                                                }
                                            })
                                            .font_family(FONT.to_owned())
                                            .text_size(px(Typography::CODE))
                                            .auto_correct(false)
                                            .caret(move || composing.get())
                                            .readonly(move || {
                                                menu_open.get()
                                                    || queue_failed.get()
                                                    || projection.get().snapshot.is_none_or(
                                                        |snapshot| {
                                                            snapshot.status
                                                                != TerminalStatus::Running
                                                        },
                                                    )
                                            })
                                            .max_length(MAX_PASTE as u32)
                                            .submit_behavior(SubmitBehavior::Submit)
                                            .paste_behavior(PasteBehavior::Report)
                                            .on_focus(move |_: &()| focused.set(true))
                                            .on_blur(move |_: &()| focused.set(false))
                                            .on_composition_change(move |event: &Composition| {
                                                composing.set(event.composing)
                                            })
                                            .on_change(move |text: &str| {
                                                if menu_open.get_untracked() {
                                                    return;
                                                }
                                                text_value.set(text.to_owned());
                                                if !text.is_empty()
                                                    && commit.push(Command::Input(
                                                        text.as_bytes().to_vec(),
                                                    ))
                                                {
                                                    text_value.set(String::new());
                                                }
                                            })
                                            .on_paste(move |event: &PasteEvent| {
                                                if !menu_open.get_untracked()
                                                    && !event.text.is_empty()
                                                    && !paste
                                                        .push(Command::Paste(event.text.clone()))
                                                {
                                                    text_value
                                                        .update(|text| text.push_str(&event.text));
                                                }
                                            })
                                            .on_submit(move |text: &str| {
                                                if !menu_open.get_untracked()
                                                    && !composing.get()
                                                    && !queue_failed.get()
                                                    && !text.is_empty()
                                                    && submit.push(Command::Paste(text.to_owned()))
                                                {
                                                    text_value.set(String::new());
                                                }
                                            }),
                                    ),
                            )
                            .children(menu_items.into_iter().enumerate().map(|(index, item)| {
                                let choose = choose_menu.clone();
                                view()
                                    .role(AccessibilityRole::Menuitem)
                                    .element_ref(&item)
                                    .focusable(true)
                                    .on_layout(move |_: &Bounds<Points>| {
                                        if index == 0
                                            && menu_open.get_untracked()
                                            && !menu_focused.get_untracked()
                                        {
                                            menu_focused.set(true);
                                            item.focus();
                                        }
                                    })
                                    .px_3()
                                    .py_1()
                                    .bg(move || {
                                        let colors = projection.get().colors;
                                        rgb(if menu_selection.get() == index {
                                            colors.bg_active
                                        } else {
                                            colors.bg_elevated
                                        })
                                    })
                                    .on_press(move |_: &nsh_core::PressEvent| choose(index))
                                    .child(text(move || {
                                        if index == 0 {
                                            projection.get().copy_label
                                        } else {
                                            projection.get().restart_label
                                        }
                                    }))
                            })),
                    )
            },
        ))
        .child(when(
            move || failed.get(),
            move || text(move || projection.get().error_label),
        ))
        .into_element()
}
