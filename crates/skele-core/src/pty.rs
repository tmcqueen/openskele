//! Pseudo-terminals for the console panel.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

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

struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
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
    /// Starts a shell on a new PTY. `on_event` is called from a dedicated
    /// reader thread with decoded output, then once with `Exit`.
    pub fn open(
        &self,
        options: TerminalOptions,
        mut on_event: impl FnMut(TerminalEvent) + Send + 'static,
    ) -> Result<u32> {
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
                            on_event(TerminalEvent::Data(text));
                        }
                    }
                }
            }
            on_event(TerminalEvent::Exit);
        });

        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        self.terminals.lock().unwrap().insert(id, Terminal { master: pair.master, writer, child });
        Ok(id)
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

    #[cfg(unix)]
    #[test]
    fn shell_output_reaches_the_callback() {
        let manager = TerminalManager::default();
        let (tx, rx) = std::sync::mpsc::channel();
        let options = TerminalOptions { shell: Some("/bin/sh".into()), cwd: None, cols: 80, rows: 24 };
        let id = manager.open(options, move |event| drop(tx.send(event))).unwrap();
        manager.write(id, "echo sk''ele-ok; exit\n").unwrap();

        let mut output = String::new();
        let deadline = std::time::Duration::from_secs(10);
        while let TerminalEvent::Data(text) = rx.recv_timeout(deadline).expect("terminal produced no exit event") {
            output.push_str(&text);
        }
        // The quotes keep the echoed command line from matching.
        assert!(output.contains("skele-ok"), "output was: {output:?}");
        manager.close(id);
    }
}
