//! Output channels: what plugins, agents and the host write for people to read
//! while debugging. Each channel keeps its most recent lines in memory.

mod log;

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use skele_protocol::methods::MessageLevel;

use crate::events::HostEvent;
use crate::{Error, Result, Workbench};

pub use log::{log_file_name, LogSink};

/// Settings that turn on log files; both are needed.
pub const LOG_TO_FILE: &str = "output.logToFile";
pub const LOG_DIRECTORY: &str = "output.logDirectory";

/// Lines kept per channel; older ones are dropped.
pub const OUTPUT_LINES: usize = 10_000;

/// Bytes kept per channel, so long or never-ending lines (a progress bar
/// redrawn with `\r`) cannot grow without bound either.
pub const OUTPUT_BYTES: usize = 4 * 1024 * 1024;

/// The host's own channel.
pub const WORKBENCH_CHANNEL: &str = "Workbench";

/// Lines of one channel, each with its line ending. The last may be partial.
#[derive(Default)]
struct Buffer {
    lines: VecDeque<String>,
    bytes: usize,
}

impl Buffer {
    fn push(&mut self, text: &str) {
        for piece in text.split_inclusive('\n') {
            match self.lines.back_mut() {
                Some(last) if !last.ends_with('\n') => last.push_str(piece),
                _ => self.lines.push_back(piece.to_owned()),
            }
        }
        self.bytes += text.len();
        while self.lines.len() > OUTPUT_LINES || (self.bytes > OUTPUT_BYTES && self.lines.len() > 1) {
            self.bytes -= self.lines.pop_front().map_or(0, |line| line.len());
        }
        // One line alone over the limit keeps its end.
        if self.bytes > OUTPUT_BYTES {
            let line = self.lines.front_mut().expect("bytes are counted from lines");
            let mut cut = line.len() - OUTPUT_BYTES;
            while !line.is_char_boundary(cut) {
                cut += 1;
            }
            line.drain(..cut);
            self.bytes = line.len();
        }
    }

    fn clear(&mut self) {
        self.lines.clear();
        self.bytes = 0;
    }

    fn text(&self) -> String {
        self.lines.iter().map(String::as_str).collect()
    }
}

#[derive(Default)]
pub struct OutputStore {
    channels: Mutex<BTreeMap<String, Buffer>>,
}

impl OutputStore {
    /// Appends text to a channel, creating it if needed. Returns whether it was created.
    pub fn append(&self, channel: &str, text: &str) -> bool {
        let mut channels = self.channels.lock().unwrap();
        let created = !channels.contains_key(channel);
        channels.entry(channel.to_owned()).or_default().push(text);
        created
    }

    /// Channel names: the Workbench channel first, then the rest by name.
    pub fn channels(&self) -> Vec<String> {
        let channels = self.channels.lock().unwrap();
        let mut names: Vec<String> = channels.keys().cloned().collect();
        names.sort_by_key(|name| (name != WORKBENCH_CHANNEL, name.clone()));
        names
    }

    pub fn read(&self, channel: &str) -> Option<String> {
        self.channels.lock().unwrap().get(channel).map(Buffer::text)
    }

    /// Empties a channel but keeps it listed. False if there is no such channel.
    pub fn clear(&self, channel: &str) -> bool {
        match self.channels.lock().unwrap().get_mut(channel) {
            Some(buffer) => {
                buffer.clear();
                true
            }
            None => false,
        }
    }
}

fn level_name(level: MessageLevel) -> &'static str {
    match level {
        MessageLevel::Info => "info",
        MessageLevel::Warning => "warning",
        MessageLevel::Error => "error",
    }
}

/// Where output is mirrored to files, if anywhere.
#[derive(Default)]
pub(crate) struct LogState {
    /// The configured base directory the sink was opened for.
    base: Option<PathBuf>,
    sink: Option<LogSink>,
}

impl Workbench {
    /// Writes to an output channel, creating it on first use, and to its log
    /// file when logging is on.
    pub fn output(&self, channel: &str, text: &str) {
        // The log lock is held across the append so a sink being opened sees
        // each write exactly once: in the buffer it copies, or here.
        let failed = {
            let mut log = self.inner.log.lock().unwrap();
            let created = self.inner.output.append(channel, text);
            let failed = match log.sink.as_mut().map(|sink| sink.write(channel, text)) {
                Some(Err(e)) => {
                    log.sink = None;
                    Some(e)
                }
                _ => None,
            };
            if created {
                self.inner.bus.emit(HostEvent::OutputChannelsChanged);
            }
            self.inner.bus.emit(HostEvent::OutputAppended { channel: channel.to_owned(), text: text.to_owned() });
            failed
        };
        if let Some(e) = failed {
            self.notify(MessageLevel::Warning, &format!("stopped writing log files: {e}"), None);
        }
    }

    /// The folder this run's log files are written to, if logging is on.
    pub fn log_dir(&self) -> Option<PathBuf> {
        self.inner.log.lock().unwrap().sink.as_ref().map(|sink| sink.run_dir().to_owned())
    }

    /// Starts, stops or moves log files to match the startup flag or, without
    /// it, the two settings.
    pub(crate) fn apply_log_settings(&self) {
        let (base, warning) = self.log_base();
        // Warnings are written after the lock is released: writing output takes it too.
        let error = {
            let mut log = self.inner.log.lock().unwrap();
            if log.base == base {
                None
            } else {
                log.sink = None;
                log.base = base.clone();
                match base.as_deref().map(|dir| self.open_sink(dir)) {
                    Some(Ok(sink)) => {
                        log.sink = Some(sink);
                        None
                    }
                    Some(Err(e)) => Some(format!("cannot write log files to {}: {e}", base.unwrap().display())),
                    None => None,
                }
            }
        };
        self.warn(warning.or(error));
    }

    fn warn(&self, warning: Option<String>) {
        if let Some(warning) = warning {
            self.notify(MessageLevel::Warning, &warning, None);
        }
    }

    /// The directory log files go under, and a warning if the settings ask for
    /// logging but cannot be used.
    fn log_base(&self) -> (Option<PathBuf>, Option<String>) {
        if let Some(flag) = &self.inner.log_flag {
            return (Some(flag.clone()), None);
        }
        let config = &self.inner.config;
        let dir = config.get(LOG_DIRECTORY);
        let dir = dir.as_str().unwrap_or_default();
        if config.get(LOG_TO_FILE) != serde_json::Value::Bool(true) || dir.is_empty() {
            return (None, None);
        }
        let dir = Path::new(dir);
        if dir.is_absolute() {
            return (Some(dir.to_owned()), None);
        }
        match &self.inner.workspace {
            Some(workspace) => (Some(workspace.join(dir)), None),
            None => {
                (None, Some(format!("{LOG_DIRECTORY} is relative and no workspace is open; not writing log files")))
            }
        }
    }

    /// Opens a sink and copies every channel's buffer into it. Called with the
    /// log lock held, so no write can slip between the copy and the switch.
    fn open_sink(&self, base: &Path) -> std::io::Result<LogSink> {
        let mut sink = LogSink::open(base)?;
        for channel in self.inner.output.channels() {
            let text = self.inner.output.read(&channel).unwrap_or_default();
            sink.write(&channel, &text)?;
        }
        Ok(sink)
    }

    pub fn output_channels(&self) -> Vec<String> {
        self.inner.output.channels()
    }

    pub fn output_read(&self, channel: &str) -> Result<String> {
        self.inner.output.read(channel).ok_or_else(|| Error::UnknownChannel(channel.to_owned()))
    }

    pub fn output_clear(&self, channel: &str) -> Result<()> {
        if !self.inner.output.clear(channel) {
            return Err(Error::UnknownChannel(channel.to_owned()));
        }
        self.inner.bus.emit(HostEvent::OutputCleared { channel: channel.to_owned() });
        Ok(())
    }

    /// Tells the user something: a transient message, also kept in the Workbench channel.
    pub fn notify(&self, level: MessageLevel, message: &str, source: Option<&str>) {
        self.inner.bus.message(level, message, source);
        let from = source.map(|s| format!(" {s}:")).unwrap_or_default();
        self.output(WORKBENCH_CHANNEL, &format!("[{}]{from} {message}\r\n", level_name(level)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_lines_join() {
        let store = OutputStore::default();
        store.append("a", "ab");
        store.append("a", "c\r\n");
        store.append("a", "d");
        assert_eq!(store.read("a").as_deref(), Some("abc\r\nd"));
    }

    #[test]
    fn buffer_keeps_last_lines() {
        let store = OutputStore::default();
        for i in 0..OUTPUT_LINES + 5 {
            store.append("a", &format!("{i}\n"));
        }
        let text = store.read("a").unwrap();
        assert!(text.starts_with("5\n"), "starts with {:?}", &text[..10]);
        assert_eq!(text.lines().count(), OUTPUT_LINES);
    }

    #[test]
    fn many_lines_in_one_chunk_are_bounded() {
        let store = OutputStore::default();
        let chunk: String = (0..OUTPUT_LINES + 3).map(|i| format!("{i}\n")).collect();
        store.append("a", &chunk);
        assert!(store.read("a").unwrap().starts_with("3\n"));
    }

    #[test]
    fn a_line_that_never_ends_is_bounded() {
        let store = OutputStore::default();
        let chunk = "x".repeat(100 * 1024);
        for _ in 0..100 {
            store.append("a", &chunk); // a progress bar redrawn with \r, never \n
        }
        let text = store.read("a").unwrap();
        assert!(text.len() <= OUTPUT_BYTES, "kept {} bytes", text.len());
        assert!(text.len() > OUTPUT_BYTES - chunk.len());
    }

    #[test]
    fn long_lines_drop_oldest_by_bytes() {
        let store = OutputStore::default();
        let line = format!("{}\n", "y".repeat(1023));
        for i in 0..(OUTPUT_BYTES / 1024 + 10) {
            store.append("a", &format!("{i:08}{}", &line[8..]));
        }
        let text = store.read("a").unwrap();
        assert!(text.len() <= OUTPUT_BYTES);
        assert!(text.starts_with("00000010"), "oldest kept: {:?}", &text[..8]);
    }

    #[test]
    fn multibyte_text_is_cut_on_a_char_boundary() {
        let store = OutputStore::default();
        store.append("a", &"é".repeat(OUTPUT_BYTES)); // twice the limit in bytes
        assert!(store.read("a").unwrap().chars().all(|c| c == 'é'));
    }

    #[test]
    fn channels_workbench_first() {
        let store = OutputStore::default();
        for channel in ["b", WORKBENCH_CHANNEL, "a"] {
            store.append(channel, "x");
        }
        assert_eq!(store.channels(), [WORKBENCH_CHANNEL, "a", "b"]);
    }

    #[test]
    fn append_reports_new_channel() {
        let store = OutputStore::default();
        assert!(store.append("a", "x"));
        assert!(!store.append("a", "y"));
        assert!(store.read("nope").is_none());
    }

    #[test]
    fn clear_empties_but_keeps_channel() {
        let store = OutputStore::default();
        store.append("a", "x\n");
        assert!(store.clear("a"));
        assert_eq!(store.read("a").as_deref(), Some(""));
        assert_eq!(store.channels(), ["a"]);
        assert!(!store.clear("nope"));
    }
}
