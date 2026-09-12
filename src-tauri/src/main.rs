#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{fs, path::PathBuf, sync::Mutex};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    Emitter, Manager,
};
use tauri_plugin_dialog::DialogExt;
use the_page::storage::{self, Journal, Page};

#[derive(Default)]
struct Session {
    root: Option<PathBuf>,
    journal: Option<Journal>,
    allow_exit: bool,
}

fn read_page(state: &mut Session) -> Result<Option<Page>, String> {
    let Some(root) = &state.root else {
        return Ok(None);
    };
    let date = storage::journal_day(chrono::Local::now().fixed_offset());
    let path = storage::page_path(root, date)?;
    match Journal::open(root, date) {
        Ok(journal) => {
            let page = journal.page();
            state.journal = Some(journal);
            Ok(Some(page))
        }
        Err(error) => {
            state.journal = None;
            Ok(Some(Page {
                date: date.to_string(),
                content: fs::read_to_string(path).unwrap_or_default(),
                created: None,
                last_end_input: None,
                label: String::new(),
                error: Some(error),
            }))
        }
    }
}

#[tauri::command]
fn open_page(
    app: tauri::AppHandle,
    state: tauri::State<Mutex<Session>>,
) -> Result<Option<Page>, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let settings = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("journal.json");
    if state.root.is_none() && settings.exists() {
        state.root = Some(
            serde_json::from_slice(&fs::read(settings).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Cannot read journal settings: {e}"))?,
        );
    }
    read_page(&mut state)
}

#[tauri::command]
async fn choose_folder(app: tauri::AppHandle) -> Result<Option<Page>, String> {
    let picker_app = app.clone();
    let choice = tauri::async_runtime::spawn_blocking(move || {
        picker_app
            .dialog()
            .file()
            .set_title("Choose your journal folder")
            .blocking_pick_folder()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(choice) = choice else {
        return Ok(None);
    };
    let root = choice
        .into_path()
        .map_err(|e| e.to_string())?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let mut state = app
        .state::<Mutex<Session>>()
        .inner()
        .lock()
        .map_err(|e| e.to_string())?;
    state.root = Some(root.clone());
    let page = read_page(&mut state)?;
    let settings_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&settings_dir).map_err(|e| e.to_string())?;
    fs::write(
        settings_dir.join("journal.json"),
        serde_json::to_vec(&root).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(page)
}

#[tauri::command]
fn save_page(
    state: tauri::State<Mutex<Session>>,
    content: String,
    started: String,
    last_end: String,
) -> Result<(), String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .journal
        .as_mut()
        .ok_or("There is no writable page.")?
        .save(&content, &started, &last_end)
}

#[tauri::command]
fn exit_now(app: tauri::AppHandle, state: tauri::State<Mutex<Session>>) {
    if let Ok(mut state) = state.lock() {
        state.allow_exit = true;
    }
    app.exit(0);
}

fn main() {
    let app = tauri::Builder::default()
        .manage(Mutex::new(Session::default()))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            open_page,
            choose_folder,
            save_page,
            exit_now
        ])
        .setup(|app| {
            let save = MenuItem::with_id(app, "save", "Save Now", true, Some("CmdOrCtrl+S"))?;
            let reveal = MenuItem::with_id(app, "reveal", "Reveal in Finder", true, None::<&str>)?;
            let application = Submenu::with_items(
                app,
                "The Page",
                true,
                &[
                    &PredefinedMenuItem::about(app, Some("About The Page"), None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &MenuItem::with_id(app, "quit", "Quit The Page", true, Some("CmdOrCtrl+Q"))?,
                ],
            )?;
            let file = Submenu::with_items(app, "File", true, &[&save, &reveal])?;
            let edit = Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &MenuItem::with_id(app, "undo", "Undo", true, Some("CmdOrCtrl+Z"))?,
                    &MenuItem::with_id(app, "redo", "Redo", true, Some("CmdOrCtrl+Shift+Z"))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ],
            )?;
            app.set_menu(Menu::with_items(app, &[&application, &file, &edit])?)?;
            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quit" => {
                let _ = app.emit("exit-requested", ());
            }
            "undo" | "redo" => {
                let _ = app.emit("edit-requested", event.id().as_ref());
            }
            "save" => {
                let _ = app.emit("save-requested", ());
            }
            "reveal" => {
                let state = app.state::<Mutex<Session>>();
                if let Ok(state) = state.lock() {
                    if let Some(root) = &state.root {
                        let result = std::process::Command::new("/usr/bin/open")
                            .arg(root)
                            .spawn();
                        if let Err(e) = result {
                            let _ = app.emit("native-error", e.to_string());
                        }
                    }
                };
            }
            _ => {}
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.emit("exit-requested", ());
            }
        })
        .build(tauri::generate_context!())
        .expect("Cannot start The Page");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            let allowed = app
                .state::<Mutex<Session>>()
                .lock()
                .map(|s| s.allow_exit)
                .unwrap_or(false);
            if !allowed {
                api.prevent_exit();
                let _ = app.emit("exit-requested", ());
            }
        }
    });
}
