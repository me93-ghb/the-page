#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{collections::HashMap, fs, path::PathBuf, sync::Mutex};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    Emitter, Manager,
};
use tauri_plugin_dialog::DialogExt;
use the_page::storage::{self, Journal, Page, WritingSession};

#[derive(Default)]
struct Session {
    root: Option<PathBuf>,
    journals: HashMap<chrono::NaiveDate, Journal>,
    allow_exit: bool,
}

fn read_page(state: &mut Session, date: chrono::NaiveDate) -> Result<Option<Page>, String> {
    let Some(root) = &state.root else {
        return Ok(None);
    };
    if let Some(journal) = state.journals.get(&date) {
        return Ok(Some(journal.page()));
    }
    let path = storage::page_path(root, date)?;
    match Journal::open(root, date) {
        Ok(journal) => {
            let page = journal.page();
            state.journals.insert(date, journal);
            Ok(Some(page))
        }
        Err(error) => Ok(Some(Page {
            sessions: Vec::new(),
            date: date.to_string(),
            content: fs::read_to_string(path).unwrap_or_default(),
            created: None,
            last_end_input: None,
            label: String::new(),
            error: Some(error),
        })),
    }
}

#[tauri::command]
fn open_page(
    app: tauri::AppHandle,
    state: tauri::State<Mutex<Session>>,
    date: Option<String>,
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
    let date = date
        .map(|date| chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d"))
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| storage::journal_day(chrono::Local::now().fixed_offset()));
    read_page(&mut state, date)
}

#[tauri::command]
fn list_pages(state: tauri::State<Mutex<Session>>) -> Result<Vec<String>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let Some(root) = &state.root else { return Ok(Vec::new()); };
    Ok(storage::page_dates(root)?.iter().map(ToString::to_string).collect())
}

#[tauri::command]
fn load_pages(state: tauri::State<Mutex<Session>>, dates: Vec<String>) -> Result<Vec<Page>, String> {
    if dates.len() > 8 { return Err("History loads at most eight pages at a time.".into()); }
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let mut pages = Vec::new();
    for date in dates {
        let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
        let root = state.root.as_ref().ok_or("Choose a journal folder first.")?;
        if storage::page_path(root, date)?.try_exists().map_err(|e| e.to_string())? {
            if let Some(page) = read_page(&mut state, date)? { pages.push(page); }
        }
    }
    Ok(pages)
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
    state.journals.clear();
    let page = read_page(
        &mut state,
        storage::journal_day(chrono::Local::now().fixed_offset()),
    )?;
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
    date: String,
    sessions: Vec<WritingSession>,
    last_end: Option<String>,
    label: String,
) -> Result<(), String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .journals
        .get_mut(&chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?)
        .ok_or("There is no writable page.")?
        .save_sessions(&sessions, last_end.as_deref(), &label)
}

#[tauri::command]
fn store_photograph(state: tauri::State<Mutex<Session>>, date: String, bytes: Vec<u8>) -> Result<String, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let journal = state.journals.get(&date).ok_or("There is no writable page.")?;
    the_page::photographs::store(&journal.root, date, &bytes)
}

#[tauri::command]
fn read_photograph(state: tauri::State<Mutex<Session>>, date: String, path: String) -> Result<String, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let root = state.root.as_ref().ok_or("Choose a journal folder first.")?;
    the_page::photographs::read(root, date, &path)
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
            list_pages,
            load_pages,
            choose_folder,
            save_page,
            store_photograph,
            read_photograph,
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
                    &MenuItem::with_id(app, "label", "Label This Page", true, Some("CmdOrCtrl+Shift+L"))?,
                ],
            )?;
            let go = Submenu::with_items(app, "Go", true, &[
                &MenuItem::with_id(app, "today", "Today", true, Some("CmdOrCtrl+T"))?,
                &MenuItem::with_id(app, "previous", "Previous Page", true, Some("CmdOrCtrl+Alt+Up"))?,
                &MenuItem::with_id(app, "next", "Next Page", true, Some("CmdOrCtrl+Alt+Down"))?,
            ])?;
            app.set_menu(Menu::with_items(app, &[&application, &file, &edit, &go])?)?;
            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quit" => {
                let _ = app.emit("exit-requested", ());
            }
            "undo" | "redo" => {
                let _ = app.emit("edit-requested", event.id().as_ref());
            }
            "today" | "previous" | "next" => {
                let _ = app.emit("navigate-requested", event.id().as_ref());
            }
            "label" => { let _ = app.emit("label-requested", ()); }
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
