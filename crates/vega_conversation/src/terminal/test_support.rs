use super::*;

pub struct SessionDriver {
    receiver: mpsc::Receiver<Command>,
    state: Arc<Mutex<State>>,
    control: Arc<Control>,
    inputs: Vec<Vec<u8>>,
    sizes: Vec<(u16, u16)>,
    scrolls: Vec<i32>,
}

pub fn workspace_file_id(slot: u32) -> crate::types::WorkspaceFileId {
    crate::types::WorkspaceFileId {
        generation: 1,
        slot,
        seal: 1,
    }
}

pub fn diff_projection(file_id: crate::types::WorkspaceFileId) -> crate::types::DiffTextProjection {
    use crate::types::{
        DiffHunk, DiffLanguage, DiffLayer, DiffRow, DiffRowKind, DiffSection, DiffTextProjection,
    };
    DiffTextProjection {
        file_id,
        language: DiffLanguage::Plain,
        sections: vec![DiffSection {
            layer: DiffLayer::Unstaged,
            hunks: (0..8)
                .map(|hunk| DiffHunk {
                    old_start: hunk * 30 + 1,
                    old_count: 20,
                    new_start: hunk * 30 + 1,
                    new_count: 20,
                    heading_suffix: None,
                    missing_trailing_newline: false,
                    rows: (0..20)
                        .map(|row| DiffRow {
                            kind: DiffRowKind::Context,
                            old_line: Some(hunk * 30 + row + 1),
                            new_line: Some(hunk * 30 + row + 1),
                            text: format!("diff hunk {hunk}, row {row}"),
                        })
                        .collect(),
                })
                .collect(),
        }],
    }
}

pub fn session() -> (TerminalSession, SessionDriver) {
    let (commands, receiver) = mpsc::sync_channel(32);
    let state = Arc::new(Mutex::new(State {
        parser: vt100::Parser::new(24, 80, SCROLLBACK),
        status: TerminalStatus::Starting,
        generation: 0,
        max_scrollback: 0,
    }));
    let control = Arc::new(Control {
        stop: AtomicBool::new(false),
        done: AtomicBool::new(true),
    });
    (
        TerminalSession {
            commands,
            state: state.clone(),
            control: control.clone(),
        },
        SessionDriver {
            receiver,
            state,
            control,
            inputs: Vec::new(),
            sizes: Vec::new(),
            scrolls: Vec::new(),
        },
    )
}

impl SessionDriver {
    pub fn drain(&mut self) {
        while let Ok(command) = self.receiver.try_recv() {
            match command {
                Command::Input(bytes) => {
                    self.inputs.push(bytes);
                    update(&self.state, |state| {
                        state.parser.screen_mut().set_scrollback(0)
                    });
                }
                Command::Resize(rows, cols) => {
                    self.sizes.push((rows, cols));
                    update(&self.state, |state| {
                        state.parser.screen_mut().set_size(rows, cols)
                    });
                }
                Command::Scroll(rows) => {
                    self.scrolls.push(rows);
                    update(&self.state, |state| {
                        let screen = state.parser.screen_mut();
                        screen.set_scrollback(
                            screen.scrollback().saturating_add_signed(rows as isize),
                        );
                    });
                }
            }
        }
    }

    pub fn output(&mut self, bytes: &[u8]) {
        update(&self.state, |state| state.parser.process(bytes));
    }

    pub fn status(&mut self, status: TerminalStatus) {
        update(&self.state, |state| state.status = status);
    }

    pub fn inputs(&self) -> &[Vec<u8>] {
        &self.inputs
    }

    pub fn sizes(&self) -> &[(u16, u16)] {
        &self.sizes
    }

    pub fn scrolls(&self) -> &[i32] {
        &self.scrolls
    }

    pub fn closed(&self) -> bool {
        self.control.stop.load(Ordering::SeqCst)
    }
}
