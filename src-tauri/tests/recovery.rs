use std::fs;
use chrono::NaiveDate;
use the_page::{storage::{Journal, WritingSession}, recovery::RecoveryStore};

fn day() -> NaiveDate { NaiveDate::from_ymd_opt(2026, 9, 12).unwrap() }
const START: &str = "2026-09-12T08:13:00+01:00";
fn writing(text: &str) -> Vec<WritingSession> {
    vec![WritingSession { start: START.into(), previous_end: None, content: text.into() }]
}

#[test]
fn unavailable_journal_recovers_text_and_pending_image_with_original_base() {
    let root = tempfile::tempdir().unwrap();
    let support = tempfile::tempdir().unwrap();
    let recovery = RecoveryStore::new(support.path().join("recovery"));
    let mut page = Journal::open(root.path(), day()).unwrap();
    page.save_sessions(&writing("base"), Some(START), "label").unwrap();
    let base = fs::read(root.path().join("2026/2026-09-12.md")).unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 3).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let unavailable = root.path().join("2026-away");
    fs::rename(root.path().join("2026"), &unavailable).unwrap();
    fs::write(root.path().join("2026"), "unavailable").unwrap();
    let photograph = recovery.import(root.path(), day(), png.get_ref()).unwrap();
    let local = format!("local\n![caption]({photograph})");
    let result = recovery.save(&mut page, &writing(&local), Some(START), "label", &[]).unwrap();
    assert!(!result.saved && result.recovered);
    let mut restored = recovery.open(root.path(), day()).unwrap();
    assert_eq!(restored.content(), local);
    assert_eq!(restored.page().last_end_input.as_deref(), Some(START));
    assert_eq!(restored.page().base.as_deref().unwrap().as_bytes(), base);
    fs::remove_file(root.path().join("2026")).unwrap();
    fs::rename(unavailable, root.path().join("2026")).unwrap();
    assert!(recovery.save(&mut restored, &writing(&local), Some(START), "label", &[]).unwrap().saved);
    assert_eq!(Journal::open(root.path(), day()).unwrap().content(), local);
    assert_eq!(fs::read(root.path().join("2026").join(photograph)).unwrap(), *png.get_ref());
    assert!(recovery.restored(root.path()).unwrap().is_empty());
}

#[test]
fn conflicts_preserve_each_disk_version_and_never_become_daily_pages() {
    let root = tempfile::tempdir().unwrap();
    let support = tempfile::tempdir().unwrap();
    let recovery = RecoveryStore::new(support.path().join("recovery"));
    let mut page = Journal::open(root.path(), day()).unwrap();
    page.save_sessions(&writing("base"), Some(START), "").unwrap();
    let path = root.path().join("2026/2026-09-12.md");
    let times = fs::FileTimes::new().set_modified(fs::metadata(&path).unwrap().modified().unwrap());
    let mut copies = Vec::new();
    for external in ["external one", "external two"] {
        fs::write(&path, external).unwrap();
        fs::File::options().write(true).open(&path).unwrap().set_times(times).unwrap();
        let result = recovery.save(&mut page, &writing("local"), Some(START), "", &[]).unwrap();
        assert!(result.saved);
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(fs::read_to_string(&result.conflicts[0]).unwrap(), external);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&result.conflicts[0]).unwrap().permissions().mode() & 0o777, 0o600);
        copies.extend(result.conflicts);
    }
    assert_ne!(copies[0], copies[1]);
    assert_eq!(the_page::storage::page_dates(root.path()).unwrap(), vec![day()]);
}
