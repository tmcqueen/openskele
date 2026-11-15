#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() {
    let mut skele = tauri_plugin_skele::Builder::new();

    // Workspace: first CLI argument, else SKELE_WORKSPACE.
    let mut workspace = std::env::args_os()
        .nth(1)
        .or_else(|| std::env::var_os("SKELE_WORKSPACE"))
        .map(PathBuf::from)
        .filter(|path| path.is_dir());
    if let Some(dir) = std::env::var_os("SKELE_PLUGINS_DIR") {
        skele = skele.plugin_dir(dir);
    }

    // Development builds run against the repository: its sample plugins, and
    // itself as the workspace when none was given.
    #[cfg(debug_assertions)]
    {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        skele = skele.plugin_dir(repo.join("plugins"));
        workspace = workspace.or(Some(repo));
    }

    if let Some(workspace) = workspace {
        skele = skele.workspace(workspace);
    }

    tauri::Builder::default()
        .plugin(skele.build())
        .run(tauri::generate_context!())
        .expect("error while running the workbench");
}
