//! One end of a JSON-RPC connection: correlates responses with requests and
//! hands incoming calls to a [`Handler`].

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use skele_protocol::jsonrpc::{Id, Message, RpcError};
use skele_protocol::methods::{Notification, Request};
use tokio::io::{AsyncBufRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};

use super::transport::{read_frame, write_frame};
use crate::{Error, Result};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub trait Handler: Send + Sync + 'static {
    /// Requests run concurrently, each on its own task.
    fn request(&self, method: String, params: Value) -> BoxFuture<Result<Value, RpcError>>;
    /// Notifications are delivered in wire order and must not block.
    fn notification(&self, method: String, params: Value);
    /// The remote end closed the connection.
    fn closed(&self);
}

type Pending = HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>;

pub struct Peer {
    outgoing: mpsc::UnboundedSender<Message>,
    /// `None` once the connection is closed, so late requests fail instead of hanging.
    pending: Mutex<Option<Pending>>,
    next_id: AtomicI64,
}

impl Peer {
    pub fn spawn<R, W>(mut reader: R, mut writer: W, handler: Arc<dyn Handler>) -> Arc<Self>
    where
        R: AsyncBufRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (outgoing, mut queue) = mpsc::unbounded_channel::<Message>();
        let peer = Arc::new(Self {
            outgoing: outgoing.clone(),
            pending: Mutex::new(Some(HashMap::new())),
            next_id: AtomicI64::new(1),
        });

        tokio::spawn(async move {
            while let Some(message) = queue.recv().await {
                if write_frame(&mut writer, &message.encode()).await.is_err() {
                    break;
                }
            }
        });

        let reading = peer.clone();
        tokio::spawn(async move {
            while let Ok(Some(frame)) = read_frame(&mut reader).await {
                match Message::decode(&frame) {
                    Ok(Message::Request { id, method, params }) => {
                        let (handler, outgoing) = (handler.clone(), outgoing.clone());
                        tokio::spawn(async move {
                            let result = handler.request(method, params).await;
                            let _ = outgoing.send(Message::Response { id, result });
                        });
                    }
                    Ok(Message::Notification { method, params }) => handler.notification(method, params),
                    Ok(Message::Response { id: Id::Num(id), result }) => {
                        let waiter = reading.pending.lock().unwrap().as_mut().and_then(|p| p.remove(&id));
                        if let Some(waiter) = waiter {
                            let _ = waiter.send(result);
                        }
                    }
                    // We only issue numeric ids, and an undecodable frame has no id to answer.
                    Ok(Message::Response { .. }) | Err(_) => {}
                }
            }
            // Dropping the waiters fails every in-flight request.
            reading.pending.lock().unwrap().take();
            handler.closed();
        });

        peer
    }

    pub fn is_closed(&self) -> bool {
        self.pending.lock().unwrap().is_none()
    }

    /// Sends a request and waits for its response, optionally bounded by `timeout`.
    pub async fn request<R: Request>(&self, params: R::Params, timeout: Option<Duration>) -> Result<R::Result> {
        let closed = || Error::Other(format!("connection closed during {}", R::METHOD));
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().as_mut().ok_or_else(closed)?.insert(id, tx);

        let message =
            Message::Request { id: Id::Num(id), method: R::METHOD.to_owned(), params: serde_json::to_value(params)? };
        self.outgoing.send(message).map_err(|_| closed())?;

        let response = match timeout {
            None => rx.await,
            Some(limit) => match tokio::time::timeout(limit, rx).await {
                Ok(response) => response,
                Err(_) => {
                    if let Some(pending) = self.pending.lock().unwrap().as_mut() {
                        pending.remove(&id);
                    }
                    return Err(Error::Other(format!("{} timed out after {limit:?}", R::METHOD)));
                }
            },
        };
        let value = response.map_err(|_| closed())??;
        Ok(serde_json::from_value(value)?)
    }

    pub fn notify<N: Notification>(&self, params: N::Params) -> Result<()> {
        let message = Message::Notification { method: N::METHOD.to_owned(), params: serde_json::to_value(params)? };
        self.outgoing.send(message).map_err(|_| Error::Other(format!("connection closed during {}", N::METHOD)))
    }
}
