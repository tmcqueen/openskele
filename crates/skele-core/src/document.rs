//! Text buffers backed by ropes. The editor in the webview is a view onto
//! these; saving goes through here so plugins hear about it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ropey::Rope;
use serde::Serialize;

use crate::{Error, Result};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DocumentSnapshot {
    pub uri: String,
    pub path: String,
    pub text: String,
    pub version: u32,
    pub dirty: bool,
}

struct Document {
    path: PathBuf,
    rope: Rope,
    version: u32,
    dirty: bool,
}

#[derive(Default)]
pub struct DocumentStore {
    documents: Mutex<HashMap<String, Document>>,
}

pub fn uri_for(path: &Path) -> String {
    format!("file://{}", path.display())
}

impl DocumentStore {
    /// Opens a file, or returns the live buffer if it is already open.
    pub fn open(&self, path: &Path) -> Result<DocumentSnapshot> {
        let path = std::fs::canonicalize(path)?;
        let uri = uri_for(&path);
        let mut documents = self.documents.lock().unwrap();
        if !documents.contains_key(&uri) {
            let rope = Rope::from_reader(std::io::BufReader::new(std::fs::File::open(&path)?))?;
            documents.insert(uri.clone(), Document { path, rope, version: 1, dirty: false });
        }
        let doc = &documents[&uri];
        Ok(DocumentSnapshot {
            uri,
            path: doc.path.display().to_string(),
            text: doc.rope.to_string(),
            version: doc.version,
            dirty: doc.dirty,
        })
    }

    /// Replaces the characters in `start..end` (char offsets) and returns the new version.
    pub fn edit(&self, uri: &str, start: usize, end: usize, text: &str) -> Result<u32> {
        self.with(uri, |doc| {
            if start > end || end > doc.rope.len_chars() {
                return Err(Error::Other(format!("edit range {start}..{end} is out of bounds")));
            }
            doc.rope.remove(start..end);
            doc.rope.insert(start, text);
            doc.version += 1;
            doc.dirty = true;
            Ok(doc.version)
        })
    }

    /// Replaces the whole buffer and returns the new version.
    pub fn set_text(&self, uri: &str, text: &str) -> Result<u32> {
        self.with(uri, |doc| {
            doc.rope = Rope::from_str(text);
            doc.version += 1;
            doc.dirty = true;
            Ok(doc.version)
        })
    }

    /// Writes the buffer to its file and returns the version that was saved.
    pub fn save(&self, uri: &str) -> Result<u32> {
        self.with(uri, |doc| {
            doc.rope.write_to(std::io::BufWriter::new(std::fs::File::create(&doc.path)?))?;
            doc.dirty = false;
            Ok(doc.version)
        })
    }

    pub fn close(&self, uri: &str) {
        self.documents.lock().unwrap().remove(uri);
    }

    fn with<T>(&self, uri: &str, f: impl FnOnce(&mut Document) -> Result<T>) -> Result<T> {
        let mut documents = self.documents.lock().unwrap();
        let doc = documents.get_mut(uri).ok_or_else(|| Error::UnknownDocument(uri.to_owned()))?;
        f(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_and_save_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "héllo world").unwrap();

        let store = DocumentStore::default();
        let doc = store.open(&path).unwrap();
        assert_eq!((doc.version, doc.dirty), (1, false));

        // Offsets are chars, not bytes: "world" starts at 6 despite the é.
        assert_eq!(store.edit(&doc.uri, 6, 11, "rope").unwrap(), 2);
        assert!(store.edit(&doc.uri, 6, 99, "x").is_err());
        assert!(store.open(&path).unwrap().dirty);

        assert_eq!(store.save(&doc.uri).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "héllo rope");
        assert!(!store.open(&path).unwrap().dirty);
    }
}
