//! Runs the workbench core without a window: loads plugins, executes one
//! command and prints the events it caused.
//!
//!     cargo run -p skele-core --example headless -- plugins hello.greet Ferris

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use skele_core::{Workbench, WorkbenchOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(plugins), Some(command)) = (args.next(), args.next()) else {
        eprintln!("usage: headless <plugins-dir> <command-id> [string args...]");
        std::process::exit(2);
    };

    let workbench = Workbench::new(WorkbenchOptions::default())?;
    let mut events = workbench.bus().subscribe();
    let installed = workbench.load_plugins(&PathBuf::from(plugins));
    println!("plugins: {installed:?}");
    for menu in workbench.menubar() {
        println!("menu {}: {}", menu.title, serde_json::to_string(&menu.items)?);
    }

    let result = workbench.execute_command(&command, args.map(Value::String).collect()).await;
    // Notifications are asynchronous; give trailing ones a moment to land.
    tokio::time::sleep(Duration::from_millis(100)).await;
    while let Ok(event) = events.try_recv() {
        println!("event: {}", serde_json::to_string(&event)?);
    }
    workbench.shutdown().await;
    println!("result: {}", serde_json::to_string(&result?)?);
    Ok(())
}
