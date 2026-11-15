//! Writes the typed IPC bindings the webview imports.

use std::path::Path;

use specta_typescript::Typescript;

fn main() {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/workbench/src/bindings.ts");
    tauri_plugin_skele::specta_builder()
        .export(Typescript::default(), &target)
        .expect("failed to export TypeScript bindings");
    println!("wrote apps/workbench/src/bindings.ts");
}
