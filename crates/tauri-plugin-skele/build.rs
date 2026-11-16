const COMMANDS: &[&str] = &[
    "snapshot",
    "subscribe",
    "execute_command",
    "view_children",
    "config_sections",
    "config_set",
    "terminal_attach",
    "terminal_write",
    "terminal_resize",
    "terminal_close",
    "document_open",
    "document_set_text",
    "document_save",
    "output_channels",
    "output_read",
    "output_clear",
    "layout_set_active_container",
    "layout_set_active_panel",
    "layout_close_panel",
    "chrome",
    "window_start_drag",
    "window_minimize",
    "window_toggle_maximize",
    "window_close",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
