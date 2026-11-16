//! The stdio MCP server an agent launches. It connects to the workbench's
//! private socket (`SKELE_MCP_SOCKET`), presents its grant token
//! (`SKELE_MCP_TOKEN`) on the first line, then pipes stdin and stdout through.

use std::process::ExitCode;

use interprocess::local_socket::tokio::prelude::*;
use tokio::io::AsyncWriteExt;

const SOCKET_ENV: &str = "SKELE_MCP_SOCKET";
const TOKEN_ENV: &str = "SKELE_MCP_TOKEN";

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("skele-mcp-bridge: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let socket = std::env::var_os(SOCKET_ENV).ok_or(format!("{SOCKET_ENV} is not set"))?;
    let token = std::env::var(TOKEN_ENV).map_err(|_| format!("{TOKEN_ENV} is not set"))?;
    let stream = LocalSocketStream::connect(name(&socket).map_err(|e| e.to_string())?)
        .await
        .map_err(|e| format!("cannot connect to {}: {e}", socket.to_string_lossy()))?;
    let (mut from_host, mut to_host) = tokio::io::split(stream);
    to_host.write_all(format!("{token}\n").as_bytes()).await.map_err(|e| e.to_string())?;

    let (mut stdin, mut stdout) = (tokio::io::stdin(), tokio::io::stdout());
    let upstream = async {
        tokio::io::copy(&mut stdin, &mut to_host).await?;
        to_host.shutdown().await
    };
    let downstream = async {
        tokio::io::copy(&mut from_host, &mut stdout).await?;
        stdout.flush().await
    };
    // Either side closing ends the session.
    tokio::select! {
        result = upstream => result,
        result = downstream => result,
    }
    .map_err(|e| e.to_string())
}

#[cfg(unix)]
fn name(socket: &std::ffi::OsStr) -> std::io::Result<interprocess::local_socket::Name<'_>> {
    use interprocess::local_socket::GenericFilePath;
    socket.to_fs_name::<GenericFilePath>()
}

#[cfg(windows)]
fn name(socket: &std::ffi::OsStr) -> std::io::Result<interprocess::local_socket::Name<'static>> {
    use interprocess::local_socket::GenericNamespaced;
    let file = std::path::Path::new(socket).file_name().unwrap_or(socket).to_string_lossy().into_owned();
    file.to_ns_name::<GenericNamespaced>()
}
