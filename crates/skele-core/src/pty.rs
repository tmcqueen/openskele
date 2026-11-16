//! Pseudo-terminals. The core owns them, so they outlive any one view of
//! them: a front end attaches, gets the recent output, then live output.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tokio::sync::broadcast;

use crate::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct TerminalOptions {
    /// Program to run; the user's default shell when `None` or empty.
    pub shell: Option<String>,
    pub cwd: Option<PathBuf>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum TerminalEvent {
    Data(String),
    Exit,
}

/// Recent output kept per terminal, in bytes of UTF-8.
pub const SCROLLBACK_BYTES: usize = 64 * 1024;

/// What a new viewer of a terminal receives.
pub struct Attachment {
    /// Output so far, up to [`SCROLLBACK_BYTES`].
    pub scrollback: String,
    /// The shell has already exited; no live events will follow.
    pub exited: bool,
    /// Output after `scrollback`, then `Exit`.
    pub events: broadcast::Receiver<TerminalEvent>,
}

/// Updated by the reader thread. Appending and broadcasting happen under one
/// lock, so an attach sees each chunk either in the scrollback or live, never both.
struct Shared {
    scrollback: String,
    exited: bool,
    tx: broadcast::Sender<TerminalEvent>,
}

struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    shared: Arc<Mutex<Shared>>,
}

#[derive(Default)]
pub struct TerminalManager {
    terminals: Mutex<HashMap<u32, Terminal>>,
    next_id: AtomicU32,
}

fn pty_err(e: impl std::fmt::Display) -> Error {
    Error::Other(format!("pty: {e}"))
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { cols: cols.max(1), rows: rows.max(1), pixel_width: 0, pixel_height: 0 }
}

impl TerminalManager {
    /// Starts a shell on a new PTY. A reader thread records its output until it exits.
    pub fn open(&self, options: TerminalOptions) -> Result<u32> {
        let pair = native_pty_system().openpty(size(options.cols, options.rows)).map_err(pty_err)?;
        let mut command = match options.shell.filter(|s| !s.is_empty()) {
            Some(shell) => CommandBuilder::new(shell),
            None => CommandBuilder::new_default_prog(),
        };
        command.env("TERM", "xterm-256color");
        if let Some(cwd) = options.cwd {
            command.cwd(cwd);
        }
        let child = pair.slave.spawn_command(command).map_err(pty_err)?;
        // Keeping the slave open would stop the reader from ever seeing EOF.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(pty_err)?;
        let writer = pair.master.take_writer().map_err(pty_err)?;
        let shared =
            Arc::new(Mutex::new(Shared { scrollback: String::new(), exited: false, tx: broadcast::channel(256).0 }));
        let recorder = shared.clone();
        std::thread::spawn(move || {
            let mut pending = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&buf[..n]);
                        let text = take_utf8(&mut pending);
                        if !text.is_empty() {
                            let mut shared = recorder.lock().unwrap();
                            push_bounded(&mut shared.scrollback, &text, SCROLLBACK_BYTES);
                            let _ = shared.tx.send(TerminalEvent::Data(text));
                        }
                    }
                }
            }
            let mut shared = recorder.lock().unwrap();
            shared.exited = true;
            let _ = shared.tx.send(TerminalEvent::Exit);
        });

        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        self.terminals.lock().unwrap().insert(id, Terminal { master: pair.master, writer, child, shared });
        Ok(id)
    }

    /// Recent output plus a subscription to what follows it.
    pub fn attach(&self, id: u32) -> Result<Attachment> {
        let shared = self.shared(id)?;
        let shared = shared.lock().unwrap();
        Ok(Attachment { scrollback: shared.scrollback.clone(), exited: shared.exited, events: shared.tx.subscribe() })
    }

    /// Recent output, up to [`SCROLLBACK_BYTES`].
    pub fn read(&self, id: u32) -> Result<String> {
        Ok(self.shared(id)?.lock().unwrap().scrollback.clone())
    }

    fn shared(&self, id: u32) -> Result<Arc<Mutex<Shared>>> {
        let terminals = self.terminals.lock().unwrap();
        Ok(terminals.get(&id).ok_or(Error::UnknownTerminal(id))?.shared.clone())
    }

    pub fn write(&self, id: u32, data: &str) -> Result<()> {
        let mut terminals = self.terminals.lock().unwrap();
        let terminal = terminals.get_mut(&id).ok_or(Error::UnknownTerminal(id))?;
        terminal.writer.write_all(data.as_bytes())?;
        terminal.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, id: u32, cols: u16, rows: u16) -> Result<()> {
        let terminals = self.terminals.lock().unwrap();
        let terminal = terminals.get(&id).ok_or(Error::UnknownTerminal(id))?;
        terminal.master.resize(size(cols, rows)).map_err(pty_err)
    }

    /// Kills the shell and releases the PTY. Unknown ids are ignored.
    pub fn close(&self, id: u32) {
        if let Some(mut terminal) = self.terminals.lock().unwrap().remove(&id) {
            let _ = terminal.child.kill();
            let _ = terminal.child.wait();
        }
    }

    pub fn close_all(&self) {
        let ids: Vec<u32> = self.terminals.lock().unwrap().keys().copied().collect();
        for id in ids {
            self.close(id);
        }
    }
}

/// Appends `text`, then drops the oldest output so at most `limit` bytes
/// remain, cutting on a character boundary.
fn push_bounded(buffer: &mut String, text: &str, limit: usize) {
    buffer.push_str(text);
    if buffer.len() > limit {
        let mut cut = buffer.len() - limit;
        while !buffer.is_char_boundary(cut) {
            cut += 1;
        }
        buffer.drain(..cut);
    }
}

/// Drains the longest valid UTF-8 prefix, leaving a split trailing sequence
/// for the next read. Bytes that can never be valid become U+FFFD.
fn take_utf8(pending: &mut Vec<u8>) -> String {
    let mut out = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(text) => {
                out.push_str(text);
                pending.clear();
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&pending[..valid]).unwrap());
                match e.error_len() {
                    Some(bad) => {
                        out.push('\u{FFFD}');
                        pending.drain(..valid + bad);
                    }
                    None => {
                        pending.drain(..valid);
                        return out;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_split_across_reads_is_reassembled() {
        let bytes = "aé€".as_bytes();
        let mut pending = bytes[..2].to_vec(); // 'a' + first byte of 'é'
        assert_eq!(take_utf8(&mut pending), "a");
        pending.extend_from_slice(&bytes[2..4]); // rest of 'é' + first byte of '€'
        assert_eq!(take_utf8(&mut pending), "é");
        pending.extend_from_slice(&bytes[4..]);
        assert_eq!(take_utf8(&mut pending), "€");
        assert!(pending.is_empty());

        let mut invalid = vec![b'x', 0xff, b'y'];
        assert_eq!(take_utf8(&mut invalid), "x\u{FFFD}y");
    }

    #[test]
    fn scrollback_is_bounded() {
        let mut scrollback = String::new();
        for _ in 0..(100 * 1024 / 2) {
            push_bounded(&mut scrollback, "é", SCROLLBACK_BYTES);
        }
        assert!(scrollback.len() <= SCROLLBACK_BYTES);
        assert!(scrollback.len() > SCROLLBACK_BYTES - 4);
        assert!(scrollback.chars().all(|c| c == 'é'));

        let mut small = String::from("abc");
        push_bounded(&mut small, "defg", 5);
        assert_eq!(small, "cdefg");
        push_bounded(&mut small, "0123456789", 5);
        assert_eq!(small, "56789");
    }

    #[test]
    fn unknown_terminal_errors() {
        let manager = TerminalManager::default();
        assert!(matches!(manager.attach(999), Err(Error::UnknownTerminal(999))));
        assert!(matches!(manager.read(999), Err(Error::UnknownTerminal(999))));
    }

    #[cfg(unix)]
    fn sh(manager: &TerminalManager) -> u32 {
        let options = TerminalOptions { shell: Some("/bin/sh".into()), cwd: None, cols: 80, rows: 24 };
        manager.open(options).unwrap()
    }

    #[cfg(unix)]
    fn wait_for(manager: &TerminalManager, id: u32, needle: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !manager.read(id).unwrap().contains(needle) {
            assert!(std::time::Instant::now() < deadline, "no {needle:?} in {:?}", manager.read(id));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn attach_sees_scrollback_then_live() {
        let manager = TerminalManager::default();
        let id = sh(&manager);
        // The quotes keep the echoed command line from matching.
        manager.write(id, "echo sk''ele-one\n").unwrap();
        wait_for(&manager, id, "skele-one");

        let mut attachment = manager.attach(id).unwrap();
        assert!(attachment.scrollback.contains("skele-one"));
        assert!(!attachment.exited);
        manager.write(id, "echo sk''ele-two; exit\n").unwrap();

        let mut live = String::new();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        runtime.block_on(async {
            loop {
                let next = tokio::time::timeout(std::time::Duration::from_secs(10), attachment.events.recv());
                match next.await.expect("terminal produced no exit event").expect("channel closed") {
                    TerminalEvent::Data(text) => live.push_str(&text),
                    TerminalEvent::Exit => break,
                }
            }
        });
        assert!(live.contains("skele-two"), "live output was: {live:?}");
        assert!(!live.contains("skele-one"), "scrollback was replayed live: {live:?}");
        manager.close(id);
    }

    #[cfg(unix)]
    #[test]
    fn attach_after_exit_reports_exited() {
        let manager = TerminalManager::default();
        let id = sh(&manager);
        manager.write(id, "echo sk''ele-bye; exit\n").unwrap();
        wait_for(&manager, id, "skele-bye");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !manager.attach(id).unwrap().exited {
            assert!(std::time::Instant::now() < deadline, "shell never exited");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(manager.attach(id).unwrap().scrollback.contains("skele-bye"));
        manager.close(id);
    }
}
