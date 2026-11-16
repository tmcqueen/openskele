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
        let mut nonce = [0u8; 8];
        getrandom::fill(&mut nonce).map_err(|e| Error::Other(format!("random: {e}")))?;
        let nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
        let name = format!("openskele-{}-{n}-{nonce}", std::process::id());
        let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let path = private_dir(&base.join(name))
            .map(|dir| dir.join("mcp.sock"))
            .map_err(|e| Error::Other(format!("mcp socket folder under {}: {e}", base.display())))?;
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

    /// Stops accepting and removes the socket and its folder.
    pub fn close(&self) {
        self.accept.abort();
        let _ = std::fs::remove_file(&self.path);
        #[cfg(unix)]
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

impl Drop for McpListener {
    fn drop(&mut self) {
        self.close();
    }
}

/// A new folder only the owner can enter (0700), with an unguessable name, so
/// no other user can reach, pre-create or squat the socket inside it. This is
/// what keeps the socket private on every Unix: macOS cannot set a socket's
/// own mode.
#[cfg(unix)]
fn private_dir(dir: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(dir)?;
    Ok(dir.to_owned())
}

#[cfg(windows)]
fn private_dir(dir: &Path) -> std::io::Result<PathBuf> {
    // Named pipes live in their own namespace; the name alone is used.
    Ok(dir.to_owned())
}

#[cfg(unix)]
fn listen(path: &Path) -> std::io::Result<LocalSocketListener> {
    use interprocess::local_socket::GenericFilePath;
    let options = ListenerOptions::new().name(path.to_fs_name::<GenericFilePath>()?);
    // On Linux the socket itself is 0600 as well, set before bind (no umask race).
    #[cfg(target_os = "linux")]
    let options = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        options.mode(0o600)
    };
    options.create_tokio()
}

#[cfg(windows)]
fn listen(path: &Path) -> std::io::Result<LocalSocketListener> {
    use interprocess::local_socket::GenericNamespaced;
    let name = path.parent().and_then(Path::file_name).unwrap().to_string_lossy().into_owned();
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
