//! Real interactive PTY sessions, independent from agent tools and GPUI.
use crate::types::{TerminalCell, TerminalColor, TerminalSnapshot, TerminalStatus, TerminalTarget};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

const MAX_INPUT: usize = 64 * 1024;
const SCROLLBACK: usize = 2_000;

/// Content-free terminal failures; shell output is never put into diagnostics.
#[derive(Debug, thiserror::Error)]
#[error("terminal operation unavailable")]
pub struct TerminalError;

enum Command {
    Input(Vec<u8>),
    Resize(u16, u16),
    Scroll(i32),
}
struct State {
    parser: vt100::Parser,
    status: TerminalStatus,
    generation: u64,
}

/// Owns a persistent shell. Dropping it closes and reaps the shell.
/// All PTY read/write/spawn work is performed by a dedicated worker.
pub struct TerminalSession {
    commands: mpsc::SyncSender<Command>,
    state: Arc<Mutex<State>>,
    control: Arc<Control>,
}

impl TerminalSession {
    /// Start a login shell in an explicitly supplied trusted project directory.
    pub fn start(root: &Path) -> Result<Self, TerminalError> {
        Self::start_target(TerminalTarget::Directory(root.to_path_buf()))
    }

    /// Resolve registered project authority on the worker before launching a shell.
    pub fn start_target(target: TerminalTarget) -> Result<Self, TerminalError> {
        let (commands, receiver) = mpsc::sync_channel(32);
        let state = Arc::new(Mutex::new(State {
            parser: vt100::Parser::new(24, 80, SCROLLBACK),
            status: TerminalStatus::Starting,
            generation: 0,
        }));
        let control = Arc::new(Control {
            stop: AtomicBool::new(false),
            done: AtomicBool::new(false),
        });
        {
            let mut registry = REGISTRY.lock().map_err(|_| TerminalError)?;
            registry.retain(|entry| entry.strong_count() > 0);
            registry.push(Arc::downgrade(&control));
        }
        let worker_state = state.clone();
        let worker_control = control.clone();
        std::thread::Builder::new()
            .name("vega-terminal".into())
            .spawn(move || {
                let _done = Done(worker_control.clone());
                let result = (|| {
                    if worker_control.stop.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                    let root = match target {
                        TerminalTarget::Directory(root) => root,
                        TerminalTarget::Project {
                            database_path,
                            project_id,
                        } => {
                            let store = vega_store::Store::open(database_path)
                                .map_err(|_| TerminalError)?;
                            let project = vega_store::projects::find(store.conn(), &project_id)
                                .map_err(|_| TerminalError)?
                                .ok_or(TerminalError)?;
                            PathBuf::from(project.path)
                        }
                    };
                    if worker_control.stop.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                    run(root, receiver, &worker_state, &worker_control.stop)
                })();
                if result.is_err() {
                    update(&worker_state, |state| state.status = TerminalStatus::Failed);
                }
            })
            .map_err(|_| TerminalError)?;
        Ok(Self {
            commands,
            state,
            control,
        })
    }

    /// Queue explicit human input. Bounded backpressure is reported, never dropped silently.
    pub fn input(&self, bytes: &[u8]) -> Result<(), TerminalError> {
        if bytes.len() > MAX_INPUT {
            return Err(TerminalError);
        }
        self.commands
            .try_send(Command::Input(bytes.into()))
            .map_err(|_| TerminalError)
    }

    /// Resize the kernel PTY and parser together using viewport cell dimensions.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), TerminalError> {
        self.commands
            .try_send(Command::Resize(rows.clamp(2, 240), cols.clamp(2, 400)))
            .map_err(|_| TerminalError)
    }

    /// Move within bounded parsed scrollback; positive values move into history.
    pub fn scroll(&self, rows: i32) -> Result<(), TerminalError> {
        self.commands
            .try_send(Command::Scroll(rows))
            .map_err(|_| TerminalError)
    }

    /// Read a changed snapshot without blocking the render/UI thread.
    pub fn snapshot(&self, since: Option<u64>) -> Option<TerminalSnapshot> {
        let state = self.state.try_lock().ok()?;
        if since == Some(state.generation) {
            return None;
        }
        let screen = state.parser.screen();
        let (rows, cols) = screen.size();
        let cells = (0..rows)
            .map(|row| {
                (0..cols)
                    .filter_map(|col| screen.cell(row, col))
                    .map(|cell| TerminalCell {
                        text: cell.contents().into(),
                        foreground: color(cell.fgcolor()),
                        background: color(cell.bgcolor()),
                        bold: cell.bold(),
                        inverse: cell.inverse(),
                        continuation: cell.is_wide_continuation(),
                    })
                    .collect()
            })
            .collect();
        Some(TerminalSnapshot {
            generation: state.generation,
            status: state.status,
            cells,
            cursor: (!screen.hide_cursor() && screen.scrollback() == 0)
                .then(|| screen.cursor_position()),
            application_cursor: screen.application_cursor(),
            bracketed_paste: screen.bracketed_paste(),
            scrollback: screen.scrollback(),
        })
    }

    /// Request asynchronous close. The worker kills and reaps; UI never joins a thread.
    pub fn close(&mut self) {
        self.control.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.close();
    }
}
struct Control {
    stop: AtomicBool,
    done: AtomicBool,
}
struct Done(Arc<Control>);
impl Drop for Done {
    fn drop(&mut self) {
        self.0.done.store(true, Ordering::SeqCst);
    }
}
static REGISTRY: Mutex<Vec<std::sync::Weak<Control>>> = Mutex::new(Vec::new());

/// Await worker cleanup during application quit, without blocking the UI thread.
pub async fn shutdown_all() {
    let controls = REGISTRY
        .lock()
        .map(|registry| {
            registry
                .iter()
                .filter_map(std::sync::Weak::upgrade)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for control in &controls {
        control.stop.store(true, Ordering::SeqCst);
    }
    let (sender, receiver) = futures::channel::oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("vega-terminal-reaper".into())
        .spawn(move || {
            while controls
                .iter()
                .any(|control| !control.done.load(Ordering::SeqCst))
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = sender.send(());
        });
    if spawned.is_ok() {
        let _ = receiver.await;
    }
}

fn color(color: vt100::Color) -> TerminalColor {
    match color {
        vt100::Color::Default => TerminalColor::Default,
        vt100::Color::Idx(i) => TerminalColor::Indexed(i),
        vt100::Color::Rgb(r, g, b) => TerminalColor::Rgb(r, g, b),
    }
}
fn update(state: &Mutex<State>, f: impl FnOnce(&mut State)) {
    if let Ok(mut state) = state.lock() {
        f(&mut state);
        state.generation = state.generation.wrapping_add(1);
    }
}

#[cfg(unix)]
fn run(
    root: PathBuf,
    receiver: mpsc::Receiver<Command>,
    state: &Mutex<State>,
    stop: &AtomicBool,
) -> Result<(), TerminalError> {
    use portable_pty::{CommandBuilder, PtySize};
    use std::io::{Read, Write};
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let root = root.canonicalize().map_err(|_| TerminalError)?;
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    if !root.is_dir() {
        return Err(TerminalError);
    }
    let pair = portable_pty::native_pty_system()
        .openpty(PtySize::default())
        .map_err(|_| TerminalError)?;
    let mut command = CommandBuilder::new(if cfg!(target_os = "macos") {
        "/bin/zsh"
    } else {
        "/bin/sh"
    });
    command.arg("-l");
    command.cwd(root);
    command.env("TERM", "xterm-256color");
    let mut reader = pair.master.try_clone_reader().map_err(|_| TerminalError)?;
    let mut writer = pair.master.take_writer().map_err(|_| TerminalError)?;
    let fd = pair.master.as_raw_fd().ok_or(TerminalError)?;
    // SAFETY: the PTY master owns fd for this entire worker; nonblocking IO bounds shutdown.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(TerminalError);
    }
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|_| TerminalError)?;
    drop(pair.slave);
    update(state, |s| s.status = TerminalStatus::Running);
    let mut pending = VecDeque::new();
    let mut buffer = [0u8; 8192];
    let mut exited = false;
    let result = (|| {
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            for _ in 0..32 {
                match receiver.try_recv() {
                    Ok(Command::Input(bytes)) => {
                        if pending.len() + bytes.len() > MAX_INPUT * 32 {
                            return Err(TerminalError);
                        }
                        pending.extend(bytes);
                        update(state, |s| s.parser.screen_mut().set_scrollback(0));
                    }
                    Ok(Command::Resize(rows, cols)) => {
                        pair.master
                            .resize(PtySize {
                                rows,
                                cols,
                                pixel_width: 0,
                                pixel_height: 0,
                            })
                            .map_err(|_| TerminalError)?;
                        update(state, |s| s.parser.screen_mut().set_size(rows, cols));
                    }
                    Ok(Command::Scroll(rows)) => update(state, |s| {
                        let screen = s.parser.screen_mut();
                        screen.set_scrollback(
                            screen.scrollback().saturating_add_signed(rows as isize),
                        );
                    }),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                }
            }
            if !pending.is_empty() {
                let bytes = pending.make_contiguous();
                match writer.write(bytes) {
                    Ok(written) => {
                        pending.drain(..written);
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => return Err(TerminalError),
                }
            }
            for _ in 0..32 {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => update(state, |s| s.parser.process(&buffer[..count])),
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) || e.raw_os_error() == Some(libc::EIO) =>
                    {
                        break;
                    }
                    Err(_) => return Err(TerminalError),
                }
            }
            if let Some(exit) = child.try_wait().map_err(|_| TerminalError)? {
                exited = true;
                update(state, |s| {
                    s.status = TerminalStatus::Exited(exit.exit_code())
                });
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    // Close both the foreground job and shell process groups. Ctrl+C remains normal PTY input.
    if !exited
        && let Some(group) = pair
            .master
            .process_group_leader()
            .filter(|group| *group > 1)
    {
        // SAFETY: group comes from this owned PTY's controlling terminal.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
    }
    if !exited && let Some(pid) = child.process_id().filter(|pid| *pid > 1) {
        // SAFETY: this is the owned child/session process group, never a caller-supplied pid.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    if !exited {
        let _ = child.kill();
    }
    let exit = child.wait().map_err(|_| TerminalError)?;
    update(state, |s| {
        s.status = TerminalStatus::Exited(exit.exit_code())
    });
    result
}

#[cfg(not(unix))]
fn run(
    _: PathBuf,
    _: mpsc::Receiver<Command>,
    _: &Mutex<State>,
    _: &AtomicBool,
) -> Result<(), TerminalError> {
    Err(TerminalError)
}

#[cfg(all(test, unix))]
mod tests {

    #[test]
    fn parser_bounds_and_control_sequences_are_not_plain_text() {
        let mut parser = vt100::Parser::new(4, 12, 5);
        for _ in 0..100 {
            parser.process(b"row\r\n");
        }
        parser.process(b"\x1b[2J\x1b[H\x1b[32mgreen\x1b[0m\r\nwide:\xe4\xb8\xad");
        assert_eq!(
            parser.screen().cell(0, 0).unwrap().fgcolor(),
            vt100::Color::Idx(2)
        );
        assert!(parser.screen().contents().contains("wide:中"));
        parser.screen_mut().set_scrollback(usize::MAX);
        assert!(parser.screen().scrollback() <= 5);
    }
}
