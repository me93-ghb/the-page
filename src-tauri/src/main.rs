#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{collections::HashMap, fs, path::PathBuf, sync::Mutex};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    Emitter, Manager,
};
use tauri_plugin_dialog::DialogExt;
use the_page::storage::{self, Journal, Page, WritingSession};
use the_page::recovery::{RecoveryStore, PendingImage, CopyAsset, SaveResult, Draft};

#[derive(Default)]
struct Session {
    root: Option<PathBuf>,
    journals: HashMap<chrono::NaiveDate, Journal>,
    allow_exit: bool,
}

fn recovery(app: &tauri::AppHandle) -> Result<RecoveryStore, String> {
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let data = if app.config().identifier == "com.thepage.journal" { data.parent().ok_or("Missing Application Support folder.")?.join("The Page") } else { data };
    Ok(RecoveryStore::new(data.join("recovery")))
}

fn read_page(state: &mut Session, date: chrono::NaiveDate, recovery: &RecoveryStore) -> Result<Option<Page>, String> {
    let Some(root) = &state.root else {
        return Ok(None);
    };
    if let Some(journal) = state.journals.get(&date) {
        return Ok(Some(journal.page()));
    }
    match recovery.open(root, date) {
        Ok(journal) => {
            let page = recovery.restored(root)?.into_iter().find(|p| p.date == date.to_string()).unwrap_or_else(|| journal.page());
            state.journals.insert(date, journal);
            Ok(Some(page))
        }
        Err(error) => Ok(Some(Page {
            id: String::new(), base: None, recovered: None, pending: Vec::new(), pending_input: None,
            sessions: Vec::new(),
            date: date.to_string(),
            content: storage::page_path(root, date).and_then(|path| fs::read_to_string(path).map_err(|e| e.to_string())).unwrap_or_default(),
            created: None,
            last_end_input: None,
            label: String::new(),
            error: Some(error),
        })),
    }
}

#[tauri::command]
async fn open_page(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Session>>,
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
    read_page(&mut state, date, &recovery(&app)?)
}

#[tauri::command]
async fn list_pages(state: tauri::State<'_, Mutex<Session>>) -> Result<Vec<String>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let Some(root) = &state.root else { return Ok(Vec::new()); };
    Ok(storage::page_dates(root)?.iter().map(ToString::to_string).collect())
}

#[tauri::command]
async fn load_pages(app: tauri::AppHandle, state: tauri::State<'_, Mutex<Session>>, dates: Vec<String>) -> Result<Vec<Page>, String> {
    if dates.len() > 8 { return Err("History loads at most eight pages at a time.".into()); }
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let mut pages = Vec::new();
    for date in dates {
        let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
        let root = state.root.as_ref().ok_or("Choose a journal folder first.")?;
        if storage::page_path(root, date)?.try_exists().map_err(|e| e.to_string())? {
            if let Some(page) = read_page(&mut state, date, &recovery(&app)?)? { pages.push(page); }
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
        &recovery(&app)?,
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
async fn recovered_pages(app: tauri::AppHandle, state: tauri::State<'_, Mutex<Session>>) -> Result<Vec<Page>, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let Some(root) = state.root.clone() else { return Ok(Vec::new()); };
    let recovery = recovery(&app)?;
    let pages = recovery.restored(&root)?;
    for page in &pages {
        let date = chrono::NaiveDate::parse_from_str(&page.date, "%Y-%m-%d").map_err(|e| e.to_string())?;
        state.journals.insert(date, recovery.open(&root, date)?);
    }
    Ok(pages)
}

#[tauri::command]
async fn refresh_pages(state: tauri::State<'_, Mutex<Session>>, dates: Vec<String>) -> Result<Vec<Page>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let root = state.root.as_ref().ok_or("Choose a journal folder first.")?;
    dates.into_iter().map(|date| {
        let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
        match Journal::open(root, date) {
            Ok(journal) => Ok(journal.page()),
            Err(error) => {
                let path = storage::page_path(root, date)?;
                let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
                Ok(Page { id: String::new(), base: Some(raw.clone()), recovered: None, pending: Vec::new(), pending_input: None,
                    sessions: Vec::new(), date: date.to_string(), content: raw, created: None, last_end_input: None, label: String::new(), error: Some(error) })
            }
        }
    }).collect()
}

#[tauri::command]
async fn checkpoint_pages(app: tauri::AppHandle, state: tauri::State<'_, Mutex<Session>>, drafts: Vec<Draft>) -> Result<(), String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let root = state.root.as_ref().ok_or("Choose a journal folder first.")?;
    recovery(&app)?.checkpoint(root, &drafts)
}

#[tauri::command]
async fn save_page(app: tauri::AppHandle, state: tauri::State<'_, Mutex<Session>>, date: String, id: String, base: Option<String>,
    sessions: Vec<WritingSession>, last_end: Option<String>, label: String, pending: Vec<PendingImage>, pending_input: Option<serde_json::Value>,
) -> Result<SaveResult, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let journal = state.journals.get_mut(&date).ok_or("There is no writable page.")?;
    if journal.raw_base() != base.as_deref() || journal.page().id != id {
        *journal = Journal::at_base(&journal.root, date, base, Some(&id))?;
    }
    recovery(&app)?.save_input(journal, &sessions, last_end.as_deref(), &label, &pending, pending_input)
}

#[tauri::command]
async fn store_photograph(app: tauri::AppHandle, state: tauri::State<'_, Mutex<Session>>, date: String, bytes: Vec<u8>) -> Result<String, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let journal = state.journals.get(&date).ok_or("There is no writable page.")?;
    recovery(&app)?.import(&journal.root, date, &bytes)
}

#[tauri::command]
async fn read_photograph(app: tauri::AppHandle, state: tauri::State<'_, Mutex<Session>>, date: String, path: String) -> Result<String, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let root = state.root.as_ref().ok_or("Choose a journal folder first.")?;
    recovery(&app)?.read(root, date, &path)
}

#[tauri::command]
async fn save_copy(app: tauri::AppHandle, date: String, id: String, base: Option<String>, sessions: Vec<WritingSession>, last_end: Option<String>, label: String, assets: Vec<CopyAsset>) -> Result<bool, String> {
    let (journal, raw) = {
        let state = app.state::<Mutex<Session>>();
        let state = state.lock().map_err(|e| e.to_string())?;
        let root = state.root.as_ref().ok_or("Choose a journal folder first.")?;
        let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
        let journal = match state.journals.get(&date) {
            Some(journal) if journal.raw_base() == base.as_deref() && journal.page().id == id => journal.clone(),
            _ => Journal::at_base(root, date, base, Some(&id))?,
        };
        let raw = journal.draft(&sessions, last_end.as_deref(), &label)?;
        (journal, raw)
    };
    let picker_app = app.clone();
    let choice = tauri::async_runtime::spawn_blocking(move || picker_app.dialog().file().set_title("Save a copy").add_filter("Markdown", &["md"]).set_file_name(format!("{date}.md")).blocking_save_file()).await.map_err(|e| e.to_string())?;
    let Some(choice) = choice else { return Ok(false); };
    let path = choice.into_path().map_err(|e| e.to_string())?;
    recovery(&app)?.copy(&journal, &raw, &path, &assets)?;
    Ok(true)
}

#[tauri::command]
async fn export_card(app: tauri::AppHandle, bytes: Vec<u8>, copy: bool, date: String) -> Result<bool, String> {
    the_page::press::validate(&bytes)?;
    if copy {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
        use objc2_foundation::NSData;
        let pasteboard = NSPasteboard::generalPasteboard();
        let data = NSData::with_bytes(&bytes);
        pasteboard.clearContents();
        // SAFETY: AppKit supplies the static PNG pasteboard type; NSData owns the validated bytes.
        if !pasteboard.setData_forType(Some(&data), unsafe { NSPasteboardTypePNG }) { return Err("The system clipboard could not accept this image.".into()); }
        return Ok(true);
    }
    let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let picker = app.clone();
    let choice = tauri::async_runtime::spawn_blocking(move || picker.dialog().file().set_title("Save pressed card").add_filter("PNG image", &["png"]).set_file_name(format!("{date}-pressed.png")).blocking_save_file()).await.map_err(|e| e.to_string())?;
    let destination = choice.map(|choice| choice.into_path().map_err(|e| e.to_string())).transpose()?;
    the_page::press::save(&bytes, destination.as_deref())
}

#[tauri::command]
fn exit_now(app: tauri::AppHandle, state: tauri::State<'_, Mutex<Session>>) {
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
            checkpoint_pages,
            recovered_pages,
            refresh_pages,
            save_copy,
            store_photograph,
            read_photograph,
            exit_now,
            export_card
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
            let copy = MenuItem::with_id(app, "save-copy", "Save a Copy…", true, None::<&str>)?;
            let press = MenuItem::with_id(app, "press", "Press Line…", true, Some("CmdOrCtrl+Shift+P"))?;
            let file = Submenu::with_items(app, "File", true, &[&save, &copy, &press, &reveal])?;
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
            #[cfg(target_os = "macos")]
            {
                use objc2_app_kit::{NSWorkspace, NSWorkspaceWillSleepNotification};
                let handle = app.handle().clone();
                let callback = block2::RcBlock::new(move |_: std::ptr::NonNull<objc2_foundation::NSNotification>| { let _ = handle.emit("save-requested", ()); });
                // SAFETY: the block captures only a sendable AppHandle; the observer lives for the app's lifetime.
                let observer = unsafe { NSWorkspace::sharedWorkspace().notificationCenter().addObserverForName_object_queue_usingBlock(Some(NSWorkspaceWillSleepNotification), None, None, &callback) };
                std::mem::forget(observer);
            }
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
            "press" => { let _ = app.emit("press-requested", ()); }
            "label" => { let _ = app.emit("label-requested", ()); }
            "save-copy" => { let _ = app.emit("save-copy-requested", ()); }
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
            if matches!(event, tauri::WindowEvent::Focused(false)) { let _ = window.emit("save-requested", ()); }
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

#[cfg(test)]
mod recovery_boundary_tests {
    use super::*;
    #[test]
    fn rejected_symlinks_do_not_read_outside_journal_but_malformed_pages_remain_visible() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap(); let outside = tempfile::tempdir().unwrap();
        let recovery = RecoveryStore::new(outside.path().join("recovery"));
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let mut state = Session { root: Some(root.path().into()), ..Default::default() };
        fs::write(outside.path().join("secret"), "private outside content").unwrap();
        fs::create_dir(root.path().join("2026")).unwrap();
        let path = root.path().join("2026/2026-09-12.md");
        symlink(outside.path().join("secret"), &path).unwrap();
        let page = read_page(&mut state, date, &recovery);
        assert!(page.is_err() || page.unwrap().unwrap().content.is_empty());
        fs::remove_file(&path).unwrap();
        fs::remove_dir(root.path().join("2026")).unwrap();
        symlink(outside.path(), root.path().join("2026")).unwrap();
        fs::write(outside.path().join("2026-09-12.md"), "private outside content").unwrap();
        let page = read_page(&mut state, date, &recovery);
        assert!(page.is_err() || page.unwrap().unwrap().content.is_empty());
        fs::remove_file(root.path().join("2026")).unwrap();
        fs::create_dir(root.path().join("2026")).unwrap();
        fs::write(&path, "---\ninvalid: [\n---\nlocal malformed writing").unwrap();
        let page = read_page(&mut state, date, &recovery).unwrap().unwrap();
        assert!(page.error.is_some());
        assert!(page.content.contains("local malformed writing"));
    }
}
