//! Writes `protocol/openrpc.json` and `protocol/plugin-manifest.schema.json`.

use std::path::Path;

use skele_protocol::openrpc::{manifest_schema, openrpc, render, MANIFEST_SCHEMA_PATH, OPENRPC_PATH};

fn main() -> std::io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (path, value) in [(OPENRPC_PATH, openrpc()), (MANIFEST_SCHEMA_PATH, manifest_schema())] {
        std::fs::write(root.join(path), render(&value))?;
        println!("wrote {path}");
    }
    Ok(())
}
