#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() {
    let mut skele = tauri_plugin_skele::Builder::new();

    // Arguments: `[workspace] [--log-dir <path>]`, in any order.
    let mut positional = None;
    let mut log_dir = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--log-dir" {
            log_dir = args.next();
        } else if positional.is_none() {
            positional = Some(arg);
        }
    }

    // Workspace: first argument, else SKELE_WORKSPACE.
    let mut workspace = positional
        .or_else(|| std::env::var_os("SKELE_WORKSPACE"))
        .map(PathBuf::from)
        .filter(|path| path.is_dir());
    // Log files: --log-dir, else SKELE_LOG_DIR; either overrides the output.* settings.
    if let Some(dir) = log_dir.or_else(|| std::env::var_os("SKELE_LOG_DIR")) {
        skele = skele.log_dir(dir);
    }
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
