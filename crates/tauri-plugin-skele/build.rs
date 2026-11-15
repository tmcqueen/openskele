const COMMANDS: &[&str] = &[
    "snapshot",
    "subscribe",
    "execute_command",
    "view_children",
    "config_sections",
    "config_set",
    "terminal_open",
    "terminal_write",
    "terminal_resize",
    "terminal_close",
    "document_open",
    "document_set_text",
    "document_save",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
