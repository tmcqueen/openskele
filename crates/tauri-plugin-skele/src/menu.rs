//! Native menu bar built from the menu registry.

use skele_core::menus::MenuNode;
use skele_core::protocol::methods::MessageLevel;
use skele_core::Workbench;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};

/// Menu item ids for commands are the command id behind this prefix.
const COMMAND_PREFIX: &str = "command:";

/// Sets the app menu and routes clicks on command items to the workbench.
pub fn install<R: Runtime>(app: &AppHandle<R>, workbench: &Workbench) -> tauri::Result<()> {
    app.set_menu(build(app, workbench)?)?;

    let workbench = workbench.clone();
    app.on_menu_event(move |_app, event| {
        let Some(command) = event.id().as_ref().strip_prefix(COMMAND_PREFIX) else { return };
        let (workbench, command) = (workbench.clone(), command.to_owned());
        tauri::async_runtime::spawn(async move {
            if let Err(e) = workbench.execute_command(&command, Vec::new()).await {
                workbench.bus().message(MessageLevel::Error, e.to_string(), None);
            }
        });
    });
    Ok(())
}

/// Rebuilds the app menu after contributions changed.
pub fn refresh<R: Runtime>(app: &AppHandle<R>, workbench: &Workbench) {
    let (handle, workbench) = (app.clone(), workbench.clone());
    // Native menus must be touched from the main thread.
    let _ = app.run_on_main_thread(move || {
        if let Ok(menu) = build(&handle, &workbench) {
            let _ = handle.set_menu(menu);
        }
    });
}

fn build<R: Runtime>(app: &AppHandle<R>, workbench: &Workbench) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;

    // macOS expects the first submenu to be the app menu, and routes the
    // clipboard shortcuts of text fields through an Edit menu.
    #[cfg(target_os = "macos")]
    {
        let name = app.package_info().name.clone();
        menu.append(&Submenu::with_items(
            app,
            name,
            true,
            &[
                &PredefinedMenuItem::hide(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::quit(app, None)?,
            ],
        )?)?;
        menu.append(&Submenu::with_items(
            app,
            "Edit",
            true,
            &[
                &PredefinedMenuItem::undo(app, None)?,
                &PredefinedMenuItem::redo(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::cut(app, None)?,
                &PredefinedMenuItem::copy(app, None)?,
                &PredefinedMenuItem::paste(app, None)?,
                &PredefinedMenuItem::select_all(app, None)?,
            ],
        )?)?;
    }

    for contributed in workbench.menubar() {
        let submenu = Submenu::new(app, &contributed.title, true)?;
        for node in contributed.items {
            match node {
                MenuNode::Separator => submenu.append(&PredefinedMenuItem::separator(app)?)?,
                MenuNode::Item { command, title, accelerator } => {
                    let id = format!("{COMMAND_PREFIX}{command}");
                    let accelerator = accelerator.map(|chord| chord.replace("Mod", "CmdOrCtrl"));
                    // A chord the platform cannot express should cost the shortcut hint, not the item.
                    let item = MenuItem::with_id(app, &id, &title, true, accelerator.as_deref())
                        .or_else(|_| MenuItem::with_id(app, &id, &title, true, None::<&str>))?;
                    submenu.append(&item)?;
                }
            }
        }
        menu.append(&submenu)?;
    }
    Ok(menu)
}
