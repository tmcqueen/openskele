//! The workbench end of the private transport: a local socket (a Unix socket
//! only its owner can open, or a named pipe on Windows). Each connection
//! starts with a grant token on one line; a token that does not check out
//! closes the connection.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::ListenerOptions;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::task::JoinHandle;

use crate::workbench::Inner;
use crate::{Error, Result, Workbench};

/// How long a connection may take to present its token.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// A token line longer than this is not a token.
const MAX_TOKEN_LINE: usize = 256;

static NEXT: AtomicU32 = AtomicU32::new(0);

pub struct McpListener {
    path: PathBuf,
    accept: JoinHandle<()>,
}

impl McpListener {
    /// Listens on a new socket and serves every connection that presents a live grant.
    pub(crate) fn start(workbench: &Workbench) -> Result<Self> {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let file = format!("openskele-{}-{n}.sock", std::process::id());
        let path = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join(file);
        let listener = listen(&path).map_err(|e| Error::Other(format!("mcp socket {}: {e}", path.display())))?;

        let weak: Weak<Inner> = Arc::downgrade(&workbench.inner);
        let accept = tokio::spawn(async move {
            while let Ok(stream) = listener.accept().await {
                let Some(inner) = weak.upgrade() else { break };
                tokio::spawn(handle(Workbench { inner }, stream));
            }
        });
        Ok(Self { path, accept })
    }

    /// Where agents' bridges connect.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Stops accepting and removes the socket file.
    pub fn close(&self) {
        self.accept.abort();
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for McpListener {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(unix)]
fn listen(path: &Path) -> std::io::Result<LocalSocketListener> {
    use interprocess::local_socket::GenericFilePath;
    use interprocess::os::unix::local_socket::ListenerOptionsExt;
    let _ = std::fs::remove_file(path);
    // The mode is set before bind, so there is no moment the socket is open to others.
    ListenerOptions::new().name(path.to_fs_name::<GenericFilePath>()?).mode(0o600).create_tokio()
}

#[cfg(windows)]
fn listen(path: &Path) -> std::io::Result<LocalSocketListener> {
    use interprocess::local_socket::GenericNamespaced;
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    ListenerOptions::new().name(name.to_ns_name::<GenericNamespaced>()?).create_tokio()
}

async fn handle(workbench: Workbench, stream: LocalSocketStream) {
    let (reader, writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let mut limited = (&mut reader).take(MAX_TOKEN_LINE as u64);
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, limited.read_line(&mut line)).await {
        Ok(Ok(_)) if line.ends_with('\n') => {}
        _ => return,
    }
    let Some(grant) = workbench.grants().check(line.trim_end()) else { return };
    let _ = super::service::serve(workbench, grant, reader, writer).await;
}
