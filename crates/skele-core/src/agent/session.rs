//! Agent sessions: one agent process per session, driven over ACP (stable v1).
//! Each session gets its own grant, handed to the agent as the bridge's
//! environment, so the agent's tool calls run under it and end with it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::schema::{
    CancelNotification, ContentBlock, EnvVariable, InitializeRequest, McpServer, McpServerStdio, NewSessionRequest,
    PromptRequest, ReadTextFileRequest, ReadTextFileResponse, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionId, SessionNotification, TextContent,
    WriteTextFileRequest, WriteTextFileResponse,
};
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::{Agent, ByteStreams, Client};
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::oneshot;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use super::transcript::{Entry, PermissionOption, Transcript};
use super::AgentEntry;
use crate::events::HostEvent;
use crate::grants::GrantKind;
use crate::workbench::Inner;
use crate::{Error, Result, Workbench};

/// How long an agent may take to start and answer `initialize` / `session/new`.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);

static NEXT_SESSION: AtomicU64 = AtomicU64::new(0);
static NEXT_PERMISSION: AtomicU64 = AtomicU64::new(0);

/// Why a prompt turn ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    EndTurn,
    Cancelled,
    /// The agent failed or exited during the turn; the transcript says why.
    Error,
    /// Another ACP stop reason, such as `max_tokens` or `refusal`.
    Other(String),
}

type Answer = Box<dyn FnOnce(Option<String>) + Send>;

pub(crate) struct AgentSession {
    agent_name: String,
    conn: ConnectionTo<Agent>,
    acp_session: SessionId,
    transcript: Mutex<Transcript>,
    /// Permission requests waiting for the user, by our request id.
    permissions: Mutex<HashMap<String, Answer>>,
    grant_id: u64,
    /// Kills the agent process.
    kill: Mutex<Option<oneshot::Sender<()>>>,
    /// Ends the ACP connection task.
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

#[derive(Default)]
pub(crate) struct AgentSessions {
    sessions: Mutex<HashMap<String, Arc<AgentSession>>>,
}

impl AgentSessions {
    fn get(&self, id: &str) -> Option<Arc<AgentSession>> {
        self.sessions.lock().unwrap().get(id).cloned()
    }
}

fn agent_error(agent: &str, message: impl std::fmt::Display) -> Error {
    Error::Agent { agent: agent.to_owned(), message: message.to_string() }
}

/// `"end_turn"` and friends, from the ACP enum's wire name.
fn stop_reason(value: impl Serialize) -> StopReason {
    match serde_json::to_value(value).ok().as_ref().and_then(Value::as_str) {
        Some("end_turn") => StopReason::EndTurn,
        Some("cancelled") => StopReason::Cancelled,
        Some(other) => StopReason::Other(other.to_owned()),
        None => StopReason::Other("unknown".to_owned()),
    }
}

impl Workbench {
    fn session(&self, id: &str) -> Result<Arc<AgentSession>> {
        self.inner.agent_sessions.get(id).ok_or_else(|| Error::UnknownAgentSession(id.to_owned()))
    }

    /// Changes a session's transcript and tells the front end which entry changed.
    fn update_transcript(
        &self,
        id: &str,
        session: &AgentSession,
        change: impl FnOnce(&mut Transcript) -> Option<usize>,
    ) {
        let changed = {
            let mut transcript = session.transcript.lock().unwrap();
            change(&mut transcript).map(|index| (index, transcript.entries[index].clone()))
        };
        if let Some((index, entry)) = changed {
            self.inner.bus.emit(HostEvent::AgentUpdate { session: id.to_owned(), index: index as u32, entry });
        }
    }

    /// Starts the agent and opens an ACP session with the workbench's tools.
    /// Returns the session id the other agent calls take.
    pub async fn new_agent_session(&self, agent_id: &str) -> Result<String> {
        let entry = self.inner.agents.get(agent_id).ok_or_else(|| Error::UnknownAgent(agent_id.to_owned()))?;
        let n = NEXT_SESSION.fetch_add(1, Ordering::Relaxed) + 1;
        let id = format!("agent-session-{n}");
        let label = format!("{} session {n}", entry.info.name);
        let grant = self.inner.grants.issue(GrantKind::AgentSession { session_id: id.clone() }, label);
        match self.start_session(&entry, &id, &grant).await {
            Ok(session) => {
                self.inner.agent_sessions.sessions.lock().unwrap().insert(id.clone(), session);
                Ok(id)
            }
            Err(e) => {
                self.inner.grants.revoke(grant.id);
                Err(e)
            }
        }
    }

    async fn start_session(
        &self,
        entry: &AgentEntry,
        id: &str,
        grant: &crate::grants::Grant,
    ) -> Result<Arc<AgentSession>> {
        let name = entry.info.name.clone();
        let agent = &entry.contribution;
        // A bare name is looked up on PATH; anything with a separator is relative to the plugin.
        let program = if Path::new(&agent.command).components().count() > 1 {
            entry.dir.join(&agent.command)
        } else {
            PathBuf::from(&agent.command)
        };
        let mut child = tokio::process::Command::new(&program)
            .args(&agent.args)
            .envs(&agent.env)
            .current_dir(&entry.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| agent_error(&name, format!("failed to start `{}`: {e}", agent.command)))?;

        // stdout carries ACP, so stderr is the agent's log, in its output channel.
        let stderr = BufReader::new(child.stderr.take().expect("stderr is piped"));
        let (weak, channel) = (Arc::downgrade(&self.inner), name.clone());
        tokio::spawn(async move {
            let mut lines = stderr.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Some(inner) = weak.upgrade() else { break };
                Workbench { inner }.output(&channel, &format!("{line}\r\n"));
            }
        });
        let transport = ByteStreams::new(
            child.stdin.take().expect("stdin is piped").compat_write(),
            child.stdout.take().expect("stdout is piped").compat(),
        );

        // The process: waits for it to exit, or kills it when the session closes.
        let (kill_tx, kill_rx) = oneshot::channel::<()>();
        let (weak, key) = (Arc::downgrade(&self.inner), id.to_owned());
        tokio::spawn(async move {
            let reason = tokio::select! {
                status = child.wait() => match status {
                    Ok(status) => format!("the agent exited ({status})"),
                    Err(e) => format!("the agent could not be waited on: {e}"),
                },
                _ = kill_rx => {
                    let _ = child.kill().await;
                    return;
                }
            };
            if let Some(inner) = weak.upgrade() {
                Workbench { inner }.agent_session_ended(&key, &reason, true);
            }
        });

        // The ACP connection lives inside connect_with; it hands the connection
        // out and stays open until the session closes.
        let (conn_tx, conn_rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        // The client side of ACP for this session: updates, permission requests and file access.
        let (on_update, on_permission, on_read, on_write) =
            (self.handle(id), self.handle(id), self.handle(id), self.handle(id));
        let agent_name = name.clone();
        tokio::spawn(async move {
            let result = Client
                .v1()
                .on_receive_notification(
                    async move |notification: SessionNotification, _cx| {
                        if let Some((workbench, id, session)) = on_update.get() {
                            let update = serde_json::to_value(&notification.update).unwrap_or_default();
                            workbench.update_transcript(&id, &session, |t| t.apply(&update));
                        }
                        Ok(())
                    },
                    agent_client_protocol::on_receive_notification!(),
                )
                .on_receive_request(
                    async move |request: RequestPermissionRequest, responder, _cx| {
                        // Stored, not awaited: the dispatch loop must not wait for the user.
                        let Some((workbench, id, session)) = on_permission.get() else {
                            return responder
                                .respond(RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled));
                        };
                        let json = serde_json::to_value(&request).unwrap_or_default();
                        let request_id = format!("permission-{}", NEXT_PERMISSION.fetch_add(1, Ordering::Relaxed) + 1);
                        let title =
                            json["toolCall"]["title"].as_str().unwrap_or("The agent asks permission").to_owned();
                        let options = json["options"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|o| PermissionOption {
                                id: o["optionId"].as_str().unwrap_or_default().to_owned(),
                                name: o["name"].as_str().unwrap_or_default().to_owned(),
                                kind: o["kind"].as_str().unwrap_or_default().to_owned(),
                            })
                            .collect();
                        let answer: Answer = Box::new(move |choice: Option<String>| {
                            let outcome = match choice {
                                Some(option) => {
                                    RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option))
                                }
                                None => RequestPermissionOutcome::Cancelled,
                            };
                            let _ = responder.respond(RequestPermissionResponse::new(outcome));
                        });
                        session.permissions.lock().unwrap().insert(request_id.clone(), answer);
                        let entry = Entry::PermissionRequest { id: request_id, title, options, answer: None };
                        workbench.update_transcript(&id, &session, |t| Some(t.push(entry)));
                        Ok(())
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |request: ReadTextFileRequest, responder, _cx| match on_read.workbench() {
                        Some(workbench) => match workbench.read_for_agent(&request.path, request.line, request.limit) {
                            Ok(text) => responder.respond(ReadTextFileResponse::new(text)),
                            Err(e) => responder
                                .respond_with_error(agent_client_protocol::Error::internal_error().data(e.to_string())),
                        },
                        None => responder.respond_with_error(agent_client_protocol::Error::internal_error()),
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |request: WriteTextFileRequest, responder, _cx| match on_write.workbench() {
                        Some(workbench) => match workbench.write_for_agent(&request.path, &request.content) {
                            Ok(()) => responder.respond(WriteTextFileResponse::new()),
                            Err(e) => responder
                                .respond_with_error(agent_client_protocol::Error::internal_error().data(e.to_string())),
                        },
                        None => responder.respond_with_error(agent_client_protocol::Error::internal_error()),
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .connect_with(transport, async move |conn: ConnectionTo<Agent>| {
                    let _ = conn_tx.send(conn);
                    let _ = shutdown_rx.await;
                    Ok(())
                })
                .await;
            if let Err(e) = result {
                tracing_free_log(&agent_name, &e);
            }
        });

        let setup = async {
            let conn = conn_rx.await.map_err(|_| agent_error(&name, "the agent closed before connecting"))?;
            conn.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await
                .map_err(|e| agent_error(&name, format!("initialize failed: {e}")))?;
            let launch = self.mcp_launch(grant).await?;
            let env = launch.env.into_iter().map(|(k, v)| EnvVariable::new(k, v)).collect();
            let tools = McpServer::Stdio(McpServerStdio::new("openskele", launch.command).args(launch.args).env(env));
            let cwd = self.workspace().map(Path::to_owned).or_else(|| std::env::current_dir().ok()).unwrap_or_default();
            let created = conn
                .send_request(NewSessionRequest::new(cwd).mcp_servers(vec![tools]))
                .block_task()
                .await
                .map_err(|e| agent_error(&name, format!("session/new failed: {e}")))?;
            Ok::<_, Error>((conn, created.session_id))
        };
        let (conn, acp_session) = match tokio::time::timeout(STARTUP_TIMEOUT, setup).await {
            Ok(Ok(started)) => started,
            Ok(Err(e)) => {
                let _ = kill_tx.send(());
                let _ = shutdown_tx.send(());
                return Err(e);
            }
            Err(_) => {
                let _ = kill_tx.send(());
                let _ = shutdown_tx.send(());
                return Err(agent_error(&name, "the agent did not start in time"));
            }
        };

        Ok(Arc::new(AgentSession {
            agent_name: name,
            conn,
            acp_session,
            transcript: Mutex::new(Transcript::default()),
            permissions: Mutex::new(HashMap::new()),
            grant_id: grant.id,
            kill: Mutex::new(Some(kill_tx)),
            shutdown: Mutex::new(Some(shutdown_tx)),
        }))
    }

    fn handle(&self, id: &str) -> SessionHandle {
        SessionHandle { inner: Arc::downgrade(&self.inner), id: id.to_owned() }
    }

    /// What the agent reads: the open buffer if the file is open (unsaved
    /// edits included), else the file on disk. `line` is 1-based.
    fn read_for_agent(&self, path: &Path, line: Option<u32>, limit: Option<u32>) -> Result<String> {
        let canonical = std::fs::canonicalize(path)?;
        let text = match self.documents().text(&crate::document::uri_for(&canonical)) {
            Ok(text) => text,
            Err(_) => std::fs::read_to_string(&canonical)?,
        };
        if line.is_none() && limit.is_none() {
            return Ok(text);
        }
        let skip = line.map_or(0, |l| l.saturating_sub(1) as usize);
        let take = limit.map_or(usize::MAX, |l| l as usize);
        Ok(text.split_inclusive('\n').skip(skip).take(take).collect())
    }

    /// What the agent writes goes through the document store, so open editors
    /// show it and saving is announced like any other save.
    fn write_for_agent(&self, path: &Path, content: &str) -> Result<()> {
        if !path.exists() {
            std::fs::write(path, content)?;
            return Ok(());
        }
        let doc = self.documents().open(path)?;
        self.documents().set_text(&doc.uri, content)?;
        self.save_document(&doc.uri)?;
        Ok(())
    }

    /// Sends a prompt and waits for the turn to end.
    pub async fn agent_prompt(&self, id: &str, text: String) -> Result<StopReason> {
        let session = self.session(id)?;
        self.update_transcript(id, &session, |t| Some(t.push(Entry::UserMessage { text: text.clone() })));
        let request = PromptRequest::new(session.acp_session.clone(), vec![ContentBlock::Text(TextContent::new(text))]);
        match session.conn.send_request(request).block_task().await {
            Ok(response) => Ok(stop_reason(response.stop_reason)),
            Err(e) => {
                let message = format!("{}: {e}", session.agent_name);
                self.update_transcript(id, &session, |t| Some(t.push(Entry::Error { message })));
                Ok(StopReason::Error)
            }
        }
    }

    /// Asks the agent to stop the current turn; pending permission requests are cancelled.
    pub async fn agent_cancel(&self, id: &str) -> Result<()> {
        let session = self.session(id)?;
        self.cancel_permissions(id, &session);
        session
            .conn
            .send_notification(CancelNotification::new(session.acp_session.clone()))
            .map_err(|e| agent_error(&session.agent_name, e))
    }

    /// Answers a permission request with one of its option ids, or `None` to cancel it.
    pub fn agent_answer_permission(&self, id: &str, request: &str, option: Option<String>) -> Result<()> {
        let session = self.session(id)?;
        let answer = session
            .permissions
            .lock()
            .unwrap()
            .remove(request)
            .ok_or_else(|| Error::UnknownPermissionRequest(request.to_owned()))?;
        let recorded = option.clone().unwrap_or_else(|| "cancelled".to_owned());
        self.mark_answer(id, &session, request, recorded);
        answer(option);
        Ok(())
    }

    fn mark_answer(&self, id: &str, session: &AgentSession, request: &str, recorded: String) {
        self.update_transcript(id, session, |t| {
            let index =
                t.entries.iter().position(|e| matches!(e, Entry::PermissionRequest { id, .. } if id == request))?;
            if let Entry::PermissionRequest { answer, .. } = &mut t.entries[index] {
                *answer = Some(recorded);
            }
            Some(index)
        });
    }

    fn cancel_permissions(&self, id: &str, session: &AgentSession) {
        let pending: Vec<_> = session.permissions.lock().unwrap().drain().collect();
        for (request, answer) in pending {
            self.mark_answer(id, session, &request, "cancelled".to_owned());
            answer(None);
        }
    }

    pub fn agent_transcript(&self, id: &str) -> Result<Transcript> {
        Ok(self.session(id)?.transcript.lock().unwrap().clone())
    }

    /// Ends a session: stops the agent and revokes its grant.
    pub async fn close_agent_session(&self, id: &str) {
        self.agent_session_ended(id, "closed", false);
    }

    /// Ends every session, for shutdown.
    pub(crate) fn close_agent_sessions(&self) {
        let ids: Vec<String> = self.inner.agent_sessions.sessions.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.agent_session_ended(&id, "closed", false);
        }
    }

    /// Tears a session down once, whoever notices first: the user closing it
    /// or the agent process exiting (`unexpected`, recorded in the transcript).
    fn agent_session_ended(&self, id: &str, reason: &str, unexpected: bool) {
        let Some(session) = self.inner.agent_sessions.sessions.lock().unwrap().remove(id) else { return };
        self.inner.grants.revoke(session.grant_id);
        self.cancel_permissions(id, &session);
        if unexpected {
            let message = format!("{}: {reason}", session.agent_name);
            self.update_transcript(id, &session, |t| Some(t.push(Entry::Error { message })));
        }
        if let Some(kill) = session.kill.lock().unwrap().take() {
            let _ = kill.send(());
        }
        if let Some(shutdown) = session.shutdown.lock().unwrap().take() {
            let _ = shutdown.send(());
        }
        self.inner.bus.emit(HostEvent::AgentSessionEnded { session: id.to_owned(), reason: reason.to_owned() });
    }
}

/// What a handler needs to find its session again, without keeping it alive.
struct SessionHandle {
    inner: Weak<Inner>,
    id: String,
}

impl SessionHandle {
    fn workbench(&self) -> Option<Workbench> {
        Some(Workbench { inner: self.inner.upgrade()? })
    }

    fn get(&self) -> Option<(Workbench, String, Arc<AgentSession>)> {
        let workbench = self.workbench()?;
        let session = workbench.inner.agent_sessions.get(&self.id)?;
        Some((workbench, self.id.clone(), session))
    }
}

/// The connection ending with an error usually just means the agent went
/// away; it is noted in the agent's channel for debugging.
fn tracing_free_log(agent: &str, error: &agent_client_protocol::Error) {
    eprintln!("agent {agent}: connection ended: {error}");
}
