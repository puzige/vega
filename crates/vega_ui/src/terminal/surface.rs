use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::Arc};

use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{TerminalColor, TerminalSnapshot, TerminalStatus};
use vega_theme::{ThemeColors, Typography};

use crate::nsh::OrderedCommands;

pub(super) const MAX_PASTE: usize = 64 * 1024 - 12;
const FONT: &str = "Menlo, Consolas, monospace";
const PAD: f32 = 8.;

#[derive(Clone)]
pub(super) struct Projection {
    pub snapshot: Option<Arc<TerminalSnapshot>>,
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

fn draw(projection: &Projection, metrics: Metrics, selection: Option<Selection>) -> Painter {
    let mut painter = Painter::new();
    let Some(snapshot) = &projection.snapshot else {
        return painter;
    };
    let cols = snapshot.cells.first().map_or(1, |row| row.len().max(1));
    for (index, cell) in snapshot.cells.iter().flatten().enumerate() {
        let x = PAD + (index % cols) as f32 * metrics.cell;
        let y = PAD + (index / cols) as f32 * metrics.line;
        let (foreground, background) = if cell.inverse {
            (cell.background, cell.foreground)
        } else {
            (cell.foreground, cell.background)
        };
        let selected = selection.is_some_and(|selection| {
            selection.generation == snapshot.generation
                && (selection.start.min(selection.end)..=selection.start.max(selection.end))
                    .contains(&index)
        });
        let background = if selected {
            rgb(projection.colors.bg_active)
        } else {
            ansi(background, projection.colors.bg_base)
        };
        painter = painter
            .fill_style(background)
            .fill_rect(x, y, metrics.cell, metrics.line);
        if !cell.continuation && !cell.text.is_empty() {
            let font = format!(
                "{}{}px {FONT}",
                if cell.bold { "bold " } else { "" },
                Typography::CODE
            );
            painter = painter
                .font(&font)
                .text_baseline(nsh_core::canvas::CanvasTextBaseline::Middle)
                .fill_style(ansi(foreground, projection.colors.text_primary))
                .fill_text(&cell.text, x, y + metrics.line * 0.5);
        }
    }
    if snapshot.scrollback == 0
        && let Some((row, col)) = snapshot.cursor
    {
        painter = painter.fill_style(rgb(projection.colors.accent)).fill_rect(
            PAD + f32::from(col) * metrics.cell,
            PAD + f32::from(row) * metrics.line,
            2.,
            metrics.line,
        );
    }
    painter
}

fn cell_at(point: Point<Points>, snapshot: &TerminalSnapshot, metrics: Metrics) -> usize {
    let col = ((point.x.0 - PAD).max(0.) / metrics.cell) as usize;
    let row = ((point.y.0 - PAD).max(0.) / metrics.line) as usize;
    let cols = snapshot.cells.first().map_or(1, |row| row.len().max(1));
    row.min(snapshot.cells.len().saturating_sub(1)) * cols + col.min(cols - 1)
}

fn copy_text(snapshot: &TerminalSnapshot, selection: Option<Selection>) -> String {
    let selection = selection.filter(|selection| selection.generation == snapshot.generation);
    if let Some(selection) = selection {
        let start = selection.start.min(selection.end);
        let end = selection.start.max(selection.end);
        let mut text = String::new();
        let cols = snapshot.cells.first().map_or(1, |row| row.len().max(1));
        for index in start..=end {
            if index > start && index % cols == 0 {
                text.push('\n');
            }
            if let Some(cell) = snapshot
                .cells
                .get(index / cols)
                .and_then(|row| row.get(index % cols))
                .filter(|cell| !cell.continuation)
            {
                text.push_str(&cell.text);
            }
        }
        text
    } else {
        snapshot
            .cells
            .iter()
            .map(|row| {
                row.iter()
                    .filter(|cell| !cell.continuation)
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn bindings(
    queue: InputQueue,
    projection: Signal<Projection>,
    selection: Signal<Option<Selection>>,
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
                queue.push(Command::Input(bytes.clone()));
            },
        );
    };
    for (key, value) in [
        ("enter", "\r"),
        ("tab", "\t"),
        ("shift-tab", "\x1b[Z"),
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
    bind("ctrl+^", vec![30]);
    bind("ctrl+_", vec![31]);
    on_key_with(
        "mod+c",
        KeyOptions {
            when: nsh_core::keymap::When::Always,
            ..Default::default()
        },
        move |_| {
            if let Some(snapshot) = projection.get().snapshot {
                set_clipboard(copy_text(&snapshot, selection.get()));
            }
        },
    );
    view()
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
                let metrics = signal(Metrics {
                    cell: 8.,
                    line: 18.,
                });
                let bounds = signal(Size {
                    width: px(0.),
                    height: px(0.),
                });
                let scroll_ref = element_ref();
                let native_input = element_ref();
                let text_value = signal(String::new());
                let commit = queue.clone();
                let paste = queue.clone();
                let submit = queue.clone();
                let keys = queue.clone();
                let retry = queue.clone();
                let resize = commands.clone();
                let restart = commands.clone();
                let scrolling = commands.clone();
                let font = format!("{}px {FONT}", Typography::CODE);
                let measured = text_metrics(&font, "M");
                metrics.set(Metrics {
                    cell: measured.width.0.max(1.),
                    line: measured.height.0.max(Typography::CODE * 1.25),
                });
                effect(move || {
                    let size = bounds.get();
                    let metrics = metrics.get();
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
                                        * metrics.get().line,
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
                                && !composing.get()
                                && !queue_failed.get()
                                && text_value.get().is_empty()
                        },
                        move || bindings(keys.clone(), projection, selection),
                    ))
                    .child(
                        view()
                            .flex_row()
                            .gap_2()
                            .px_2()
                            .py_1()
                            .child(text(move || projection.get().status_label))
                            .child(
                                nsh_base::button()
                                    .on_press(move || {
                                        if let Some(snapshot) = projection.get().snapshot {
                                            set_clipboard(copy_text(&snapshot, selection.get()));
                                        }
                                    })
                                    .child(text(move || projection.get().copy_label)),
                            )
                            .child(
                                nsh_base::button()
                                    .on_press(move || {
                                        restart.send(Request {
                                            epoch: epoch.get(),
                                            command: Command::Restart,
                                            sequence: None,
                                            reply: None,
                                        })
                                    })
                                    .child(text(move || projection.get().restart_label)),
                            ),
                    )
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
                        scroll()
                            .flex_1()
                            .min_h(px(0.))
                            .element_ref(&scroll_ref)
                            .on_layout(move |rect: &Bounds<Points>| bounds.set(rect.size))
                            .content_offset(point(
                                px(0.),
                                px(projection.get_untracked().snapshot.map_or(0, |snapshot| {
                                    snapshot.max_scrollback.saturating_sub(snapshot.scrollback)
                                }) as f32
                                    * metrics.get_untracked().line),
                            ))
                            .on_scroll(move |event: &ScrollEvent| {
                                let maximum = projection
                                    .get()
                                    .snapshot
                                    .map_or(0, |snapshot| snapshot.max_scrollback);
                                let offset = maximum.saturating_sub(
                                    (event.offset.y.0 / metrics.get().line).round().max(0.)
                                        as usize,
                                );
                                if projection
                                    .get()
                                    .snapshot
                                    .is_some_and(|snapshot| snapshot.scrollback != offset)
                                {
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
                                                .map_or(0, |snapshot| snapshot.max_scrollback)
                                                as f32
                                                * metrics.get().line)
                                    })
                                    .child(
                                        canvas(move || {
                                            draw(&projection.get(), metrics.get(), selection.get())
                                        })
                                        .sticky()
                                        .top(px(0.))
                                        .w_full()
                                        .h(move || bounds.get().height)
                                        .claim_touches(TouchClaim::on_start())
                                        .on_responder_grant(move |event: &ResponderEvent| {
                                            native_input.focus();
                                            if let Some(snapshot) = projection.get().snapshot {
                                                let index = cell_at(
                                                    event.location,
                                                    &snapshot,
                                                    metrics.get(),
                                                );
                                                selection.set(Some(Selection {
                                                    generation: snapshot.generation,
                                                    start: index,
                                                    end: index,
                                                }));
                                            }
                                        })
                                        .on_responder_move(move |event: &ResponderEvent| {
                                            if let Some(snapshot) = projection.get().snapshot {
                                                let index = cell_at(
                                                    event.location,
                                                    &snapshot,
                                                    metrics.get(),
                                                );
                                                selection.update(|selection| {
                                                    if let Some(selection) =
                                                        selection.as_mut().filter(|selection| {
                                                            selection.generation
                                                                == snapshot.generation
                                                        })
                                                    {
                                                        selection.end = index;
                                                    }
                                                });
                                            }
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        nsh_core::element::input()
                            .element_ref(&native_input)
                            .value(text_value)
                            .label(vega_i18n::t("ui.terminal"))
                            .font_family(FONT.to_owned())
                            .text_size(px(Typography::CODE))
                            .auto_correct(false)
                            .caret(true)
                            .readonly(move || {
                                queue_failed.get()
                                    || projection.get().snapshot.is_none_or(|snapshot| {
                                        snapshot.status != TerminalStatus::Running
                                    })
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
                                text_value.set(text.to_owned());
                                if !text.is_empty()
                                    && commit.push(Command::Input(text.as_bytes().to_vec()))
                                {
                                    text_value.set(String::new());
                                }
                            })
                            .on_paste(move |event: &PasteEvent| {
                                if !event.text.is_empty()
                                    && !paste.push(Command::Paste(event.text.clone()))
                                {
                                    text_value.update(|text| text.push_str(&event.text));
                                }
                            })
                            .on_submit(move |text: &str| {
                                if !composing.get()
                                    && !queue_failed.get()
                                    && !text.is_empty()
                                    && submit.push(Command::Paste(text.to_owned()))
                                {
                                    text_value.set(String::new());
                                }
                            }),
                    )
            },
        ))
        .child(when(
            move || failed.get(),
            move || text(move || projection.get().error_label),
        ))
        .into_element()
}
