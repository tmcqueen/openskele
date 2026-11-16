//! Grants: who may call tools through the MCP server. A grant is a token plus
//! what it is for; tools see the grant, never the agent session behind it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::Workbench;

/// What a grant was issued for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantKind {
    /// An agent session the workbench started.
    AgentSession { session_id: String },
    /// An MCP client the workbench did not start. Not issued yet; it needs
    /// host-side confirmation of mutating tools first (ADR-0006).
    External,
}

#[derive(Clone)]
pub struct Grant {
    pub id: u64,
    pub label: String,
    /// 32 random bytes as lowercase hex. Presented by the bridge on connect.
    pub token: String,
    pub kind: GrantKind,
    /// Ask the user before tools that change things. Not read yet (ADR-0006).
    pub confirm_mutations: bool,
    pub expires: Option<Instant>,
}

impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grant")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("kind", &self.kind)
            .field("confirm_mutations", &self.confirm_mutations)
            .field("expires", &self.expires)
            .finish_non_exhaustive()
    }
}

pub struct GrantStore {
    by_token: Mutex<HashMap<String, Grant>>,
    next_id: AtomicU64,
    /// Ids of revoked grants, so connections using them can end at once.
    revoked: tokio::sync::broadcast::Sender<u64>,
}

impl Default for GrantStore {
    fn default() -> Self {
        Self {
            by_token: Default::default(),
            next_id: Default::default(),
            revoked: tokio::sync::broadcast::channel(64).0,
        }
    }
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the system random source is available");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl GrantStore {
    /// Issues a grant that lasts until revoked.
    pub fn issue(&self, kind: GrantKind, label: impl Into<String>) -> Grant {
        self.insert(kind, label.into(), None)
    }

    /// Issues a grant that stops working at `expires`.
    pub fn issue_until(&self, kind: GrantKind, label: impl Into<String>, expires: Instant) -> Grant {
        self.insert(kind, label.into(), Some(expires))
    }

    fn insert(&self, kind: GrantKind, label: String, expires: Option<Instant>) -> Grant {
        let grant = Grant {
            id: self.next_id.fetch_add(1, Ordering::Relaxed) + 1,
            label,
            token: new_token(),
            kind,
            confirm_mutations: false,
            expires,
        };
        self.by_token.lock().unwrap().insert(grant.token.clone(), grant.clone());
        grant
    }

    /// The live grant with this token, if any.
    pub fn check(&self, token: &str) -> Option<Grant> {
        let grants = self.by_token.lock().unwrap();
        grants.get(token).filter(|g| g.expires.is_none_or(|at| Instant::now() < at)).cloned()
    }

    pub fn revoke(&self, id: u64) {
        self.by_token.lock().unwrap().retain(|_, g| g.id != id);
        let _ = self.revoked.send(id);
    }

    /// Ids of grants as they are revoked. A lagging receiver should re-check
    /// its grant with [`check`](Self::check).
    pub fn revocations(&self) -> tokio::sync::broadcast::Receiver<u64> {
        self.revoked.subscribe()
    }

    /// Number of grants issued and not revoked, expired ones included.
    pub fn len(&self) -> usize {
        self.by_token.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Workbench {
    pub fn grants(&self) -> &GrantStore {
        &self.inner.grants
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn agent() -> GrantKind {
        GrantKind::AgentSession { session_id: "s1".into() }
    }

    #[test]
    fn issued_token_checks() {
        let store = GrantStore::default();
        let grant = store.issue(agent(), "Scripted session 1");
        let found = store.check(&grant.token).unwrap();
        assert_eq!(found.id, grant.id);
        assert_eq!(found.label, "Scripted session 1");
        assert!(!found.confirm_mutations);
        assert!(store.check("nope").is_none());
    }

    #[test]
    fn token_is_64_hex() {
        let grant = GrantStore::default().issue(agent(), "x");
        assert_eq!(grant.token.len(), 64);
        assert!(grant.token.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn two_grants_have_distinct_tokens() {
        let store = GrantStore::default();
        assert_ne!(store.issue(agent(), "a").token, store.issue(agent(), "b").token);
    }

    #[test]
    fn revoked_token_fails() {
        let store = GrantStore::default();
        let grant = store.issue(agent(), "x");
        store.revoke(grant.id);
        assert!(store.check(&grant.token).is_none());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn expired_token_fails() {
        let store = GrantStore::default();
        let grant = store.issue_until(agent(), "x", Instant::now() - Duration::from_secs(1));
        assert!(store.check(&grant.token).is_none());
    }

    #[test]
    fn debug_hides_the_token() {
        let grant = GrantStore::default().issue(agent(), "x");
        assert!(!format!("{grant:?}").contains(&grant.token));
    }
}
