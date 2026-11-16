//! Output channels mirrored to files: one folder per run, one file per channel.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// File name for a channel: characters other than letters, digits, `.`, `_`,
/// `-` and space become `_`.
pub fn log_file_name(channel: &str) -> String {
    let safe: String = channel
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ') { c } else { '_' })
        .collect();
    format!("{safe}.log")
}

pub struct LogSink {
    run: PathBuf,
    files: HashMap<String, File>,
}

impl LogSink {
    /// Creates `<base>/<YYYYMMDD-HHMMSS>` (local time), with a numeric suffix
    /// if that folder already exists.
    pub fn open(base: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(base)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let mut run = base.join(&stamp);
        let mut n = 1;
        loop {
            match std::fs::create_dir(&run) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    n += 1;
                    run = base.join(format!("{stamp}-{n}"));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(Self { run, files: HashMap::new() })
    }

    pub fn run_dir(&self) -> &Path {
        &self.run
    }

    /// Appends to the channel's file, creating it on first use. Unbuffered, so
    /// `tail -f` sees each write.
    pub fn write(&mut self, channel: &str, text: &str) -> std::io::Result<()> {
        let file = match self.files.get_mut(channel) {
            Some(file) => file,
            None => {
                let path = self.run.join(log_file_name(channel));
                let file = OpenOptions::new().create(true).append(true).open(path)?;
                self.files.entry(channel.to_owned()).or_insert(file)
            }
        };
        file.write_all(text.as_bytes())
    }
}
