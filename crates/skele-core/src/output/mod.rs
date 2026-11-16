//! Output channels: what plugins, agents and the host write for people to read
//! while debugging. Each channel keeps its most recent lines in memory.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use skele_protocol::methods::MessageLevel;

use crate::events::HostEvent;
use crate::{Error, Result, Workbench};

/// Lines kept per channel; older ones are dropped.
pub const OUTPUT_LINES: usize = 10_000;

/// The host's own channel.
pub const WORKBENCH_CHANNEL: &str = "Workbench";

/// Lines of one channel, each with its line ending. The last may be partial.
#[derive(Default)]
struct Buffer {
    lines: VecDeque<String>,
}

impl Buffer {
    fn push(&mut self, text: &str) {
        for piece in text.split_inclusive('\n') {
            match self.lines.back_mut() {
                Some(last) if !last.ends_with('\n') => last.push_str(piece),
                _ => self.lines.push_back(piece.to_owned()),
            }
        }
        while self.lines.len() > OUTPUT_LINES {
            self.lines.pop_front();
        }
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
                buffer.lines.clear();
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

impl Workbench {
    /// Writes to an output channel, creating it on first use.
    pub fn output(&self, channel: &str, text: &str) {
        if self.inner.output.append(channel, text) {
            self.inner.bus.emit(HostEvent::OutputChannelsChanged);
        }
        self.inner.bus.emit(HostEvent::OutputAppended { channel: channel.to_owned(), text: text.to_owned() });
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
