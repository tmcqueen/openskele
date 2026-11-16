//! Wire contract between the openskele host and its sidecar plugins.
//!
//! Plugins are separate processes speaking JSON-RPC 2.0 over stdio with
//! LSP-style `Content-Length` framing. The types here are the single source
//! of truth: `protocol/openrpc.json` and `protocol/plugin-manifest.schema.json`
//! are generated from them (`cargo run -p skele-protocol --bin gen-protocol`)
//! and the language SDKs are generated from those documents.

pub mod jsonrpc;
pub mod manifest;
pub mod methods;
pub mod openrpc;
pub mod view;

/// Version of the plugin protocol. Host and plugin must agree on the major
/// component (and on the minor one while the major is 0).
pub const PROTOCOL_VERSION: &str = "0.1.0";

/// Whether a plugin speaking `theirs` can talk to a host speaking `ours`.
pub fn is_compatible(ours: &str, theirs: &str) -> bool {
    fn parts(v: &str) -> Option<(u64, u64)> {
        let mut it = v.split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next().unwrap_or("0").parse().ok()?;
        Some((major, minor))
    }
    match (parts(ours), parts(theirs)) {
        (Some((0, a)), Some((0, b))) => a == b,
        (Some((a, _)), Some((b, _))) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compatibility() {
        assert!(is_compatible("0.1.0", "0.1"));
        assert!(is_compatible("0.1.0", "0.1.7"));
        assert!(!is_compatible("0.1.0", "0.2.0"));
        assert!(is_compatible("1.4.0", "1.0.0"));
        assert!(!is_compatible("1.4.0", "2.0.0"));
        assert!(!is_compatible("1.0.0", "nope"));
    }

    #[test]
    fn manifest_parses_view_containers() {
        let m: manifest::Manifest = serde_json::from_value(serde_json::json!({
            "id": "a.b", "name": "B", "version": "0", "protocol": "0.1",
            "main": { "command": "x" },
            "contributes": {
                "viewContainers": [{ "id": "b.box", "title": "Box", "icon": "package" },
                                   { "id": "b.side", "title": "Side", "icon": "info", "bar": "secondary", "order": 3 }],
                "views": [{ "id": "b.v", "title": "V", "container": "b.box" },
                          { "id": "b.w", "title": "W", "location": "sidebar" }]
            }
        }))
        .unwrap();
        let c = &m.contributes;
        assert_eq!(c.view_containers[0].bar, manifest::BarId::Primary);
        assert_eq!(c.view_containers[1].bar, manifest::BarId::Secondary);
        assert_eq!(c.view_containers[1].order, 3);
        assert_eq!(c.views[0].container.as_deref(), Some("b.box"));
        assert_eq!(c.views[1].container, None); // old `location` is ignored
    }
}
