use super::*;

fn day() -> NaiveDate { NaiveDate::from_ymd_opt(2026, 9, 12).unwrap() }
const START: &str = "2026-09-12T08:13:00+01:00";
fn writing(text: &str) -> Vec<WritingSession> { vec![WritingSession { start: START.into(), previous_end: None, content: text.into() }] }
fn setup(root: &Path) -> Journal {
    let mut page = Journal::open(root, day()).unwrap();
    page.save_sessions(&writing("base"), Some(START), "label").unwrap(); page
}
fn png() -> Vec<u8> {
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 3).write_to(&mut png, image::ImageFormat::Png).unwrap(); png.into_inner()
}

#[test]
fn failure_at_each_write_boundary_keeps_the_canonical_page_and_truthful_recovery() {
    for point in ["recovery-write", "temporary-write", "conflict-copy", "rename"] {
        let root = tempfile::tempdir().unwrap(); let support = tempfile::tempdir().unwrap();
        let recovery = RecoveryStore::new(support.path().join("recovery"));
        let mut journal = setup(root.path());
        let path = root.path().join("2026/2026-09-12.md");
        if point == "conflict-copy" { fs::write(&path, "external").unwrap(); }
        let before = fs::read(&path).unwrap();
        let result = recovery.save_with(&mut journal, &writing("local"), Some(START), "label", &[], None,
            &mut |at| if at == point { Err(format!("injected {point}")) } else { Ok(()) }).unwrap();
        assert!(!result.saved, "{point}");
        assert_eq!(result.recovered, point != "recovery-write", "{point}");
        assert_eq!(fs::read(&path).unwrap(), before, "{point}");
        if result.recovered {
            assert_eq!(recovery.open(root.path(), day()).unwrap().content(), "local");
            assert!(recovery.save(&mut journal, &writing("local"), Some(START), "label", &[]).unwrap().saved);
        }
    }
    let root = tempfile::tempdir().unwrap(); let support = tempfile::tempdir().unwrap();
    let mut page = setup(root.path()); let recovery = RecoveryStore::new(support.path().join("occupied"));
    fs::write(&recovery.directory, "not a directory").unwrap();
    let result = recovery.save(&mut page, &writing("local"), Some(START), "label", &[]).unwrap();
    assert!(!result.recovered && !result.saved);
    assert_eq!(Journal::open(root.path(), day()).unwrap().content(), "base");
}

#[test]
fn recheck_preserves_every_observed_external_version_before_retry() {
    let root = tempfile::tempdir().unwrap(); let support = tempfile::tempdir().unwrap();
    let recovery = RecoveryStore::new(support.path().join("recovery")); let mut journal = setup(root.path());
    let path = root.path().join("2026/2026-09-12.md");
    fs::write(&path, "external one").unwrap();
    let result = recovery.save_with(&mut journal, &writing("local"), Some(START), "label", &[], None,
        &mut |at| { if at == "before-recheck" { fs::write(&path, "external two").unwrap(); } Ok(()) }).unwrap();
    assert!(!result.saved && result.recovered);
    assert_eq!(fs::read(&path).unwrap(), b"external two");
    let versions: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().filter_map(|e| {
        let e = e.unwrap(); e.file_name().to_string_lossy().ends_with(".conflict.md").then(|| fs::read(e.path()).unwrap())
    }).collect();
    assert!(versions.contains(&b"external one".to_vec())); assert!(versions.contains(&b"external two".to_vec()));
    assert!(recovery.save(&mut journal, &writing("local"), Some(START), "label", &[]).unwrap().saved);
}

#[test]
fn stale_base_and_missing_page_restore_without_discarding_external_bytes() {
    for missing in [false, true] {
        let root = tempfile::tempdir().unwrap(); let support = tempfile::tempdir().unwrap();
        let recovery = RecoveryStore::new(support.path().join("recovery")); let mut journal = setup(root.path());
        let path = root.path().join("2026/2026-09-12.md");
        let base = fs::read_to_string(&path).unwrap();
        recovery.save_with(&mut journal, &writing("recovered"), Some(START), "label", &[], None,
            &mut |at| if at == "rename" { Err("rename failed".into()) } else { Ok(()) }).unwrap();
        if missing { fs::remove_file(&path).unwrap(); } else { fs::write(&path, "divergent bytes").unwrap(); }
        let mut restored = recovery.open(root.path(), day()).unwrap();
        assert_eq!(restored.raw_base(), Some(base.as_str()));
        assert_eq!(restored.content(), "recovered");
        let result = recovery.save(&mut restored, &writing("recovered"), Some(START), "label", &[]).unwrap();
        assert!(result.saved);
        if !missing { assert_eq!(fs::read(&result.conflicts[0]).unwrap(), b"divergent bytes"); }
        assert_eq!(Journal::open(root.path(), day()).unwrap().page().last_end_input.as_deref(), Some(START));
    }
}

#[test]
fn older_completion_never_deletes_a_newer_checkpoint_and_cleanup_failure_is_not_data_loss() {
    for newer in [true, false] {
        let root = tempfile::tempdir().unwrap(); let support = tempfile::tempdir().unwrap();
        let recovery = RecoveryStore::new(support.path().join("recovery")); let mut journal = setup(root.path());
        let id = journal.page().id;
        let path = recovery.checkpoint_path(root.path());
        let result = recovery.save_with(&mut journal, &writing("first"), Some(START), "label", &[], None, &mut |at| {
            if at == "recovery-delete" {
                if !newer { return Err("cannot remove recovery".into()); }
                let mut records = recovery.read_records(root.path()).unwrap();
                let record = records.get_mut(&id).unwrap();
                record.base = Some(record.raw.clone()); record.raw = record.raw.replace("first", "newer"); record.token = ulid::Ulid::new().to_string();
                recovery.write_records(root.path(), &records).unwrap();
            }
            Ok(())
        }).unwrap();
        assert!(result.saved);
        assert!(path.exists());
        assert_eq!(Journal::open(root.path(), day()).unwrap().content(), "first");
        assert_eq!(recovery.open(root.path(), day()).unwrap().content(), if newer { "newer" } else { "first" });
    }
}

#[test]
fn pending_bytes_and_rollover_ownership_are_in_the_recovery_record() {
    let root = tempfile::tempdir().unwrap(); let support = tempfile::tempdir().unwrap();
    let recovery = RecoveryStore::new(support.path().join("recovery")); let mut journal = setup(root.path());
    let pending = PendingImage { bytes: png(), from: 4, to: 4, at: START.into(), corrected: true };
    let input = serde_json::json!({"from":4,"to":7,"batch":{"date":"2026-09-13","at":"2026-09-13T04:00:00+01:00","lastEnd":"2026-09-13T04:00:00+01:00","completed":false}});
    let result = recovery.save_input(&mut journal, &writing("basenew"), Some(START), "label", &[pending.clone()], Some(input.clone())).unwrap();
    assert!(result.recovered && !result.saved);
    let pages = recovery.restored(root.path()).unwrap();
    assert_eq!(pages[0].pending[0].bytes, pending.bytes); assert_eq!(pages[0].pending[0].from, 4);
    assert_eq!(pages[0].pending_input, Some(input));
    assert_eq!(Journal::open(root.path(), day()).unwrap().content(), "base");
}

#[test]
fn export_is_usable_with_pending_photographs_and_leaves_recovery_untouched() {
    let root = tempfile::tempdir().unwrap(); let support = tempfile::tempdir().unwrap(); let exports = tempfile::tempdir().unwrap();
    let recovery = RecoveryStore::new(support.path().join("recovery")); let mut journal = setup(root.path());
    let image = CopyAsset { path: "2026-09-12/pending.png".into(), bytes: png() };
    let pending = PendingImage { bytes: image.bytes.clone(), from: 4, to: 4, at: START.into(), corrected: true };
    recovery.save(&mut journal, &writing("base"), Some(START), "label", &[pending]).unwrap();
    let checkpoint = recovery.checkpoint_path(root.path()); let before = fs::read(&checkpoint).unwrap();
    let raw = journal.draft(&writing(&format!("base\n![caption]({})\n[link](https://example.com)", image.path)), Some(START), "label").unwrap();
    let destination = exports.path().join("A usable copy.md");
    recovery.copy(&journal, &raw, &destination, &[image]).unwrap();
    let copied = fs::read_to_string(destination).unwrap();
    let paths = image_paths(&copied); assert_eq!(paths.len(), 1); assert!(!paths[0].contains(' '));
    assert_eq!(fs::read(exports.path().join(&paths[0])).unwrap(), png());
    assert!(copied.contains(START)); assert!(copied.contains("[link](https://example.com)"));
    assert_eq!(fs::read(checkpoint).unwrap(), before);
    assert_eq!(Journal::open(root.path(), day()).unwrap().content(), "base");
}

#[test]
fn inline_and_quoted_images_are_exported_but_literal_examples_are_not_assets() {
    let root = tempfile::tempdir().unwrap(); let support = tempfile::tempdir().unwrap(); let exports = tempfile::tempdir().unwrap();
    let recovery = RecoveryStore::new(support.path().join("recovery")); let mut journal = setup(root.path());
    let path = recovery.import(root.path(), day(), &png()).unwrap();
    for content in [format!("Memory ![caption]({path})"), format!("> ![caption]({path})")] {
        let raw = journal.draft(&writing(&content), Some(START), "label").unwrap();
        let dest = exports.path().join("copy.md");
        recovery.copy(&journal, &raw, &dest, &[]).unwrap();
        let copied = fs::read_to_string(&dest).unwrap();
        assert!(!copied.contains(&path), "export did not rewrite {content}");
        let relative = copied.split("](photographs-").nth(1).unwrap().split(')').next().unwrap();
        assert_eq!(fs::read(exports.path().join(format!("photographs-{relative}"))).unwrap(), png());
    }
    for content in ["```md\n![example](2026-09-12/missing.png)\n```", "`![example](2026-09-12/missing.png)`", "    ![example](2026-09-12/missing.png)"] {
        let result = recovery.save(&mut journal, &writing(content), Some(START), "label", &[]).unwrap();
        assert!(result.saved, "literal code blocked save: {:?}", result.cause);
    }
}

#[test]
fn rollover_checkpoint_failure_preserves_source_and_success_transfers_ownership_atomically() {
    use std::os::unix::fs::PermissionsExt;
    for backwards in [false, true] {
        let root = tempfile::tempdir().unwrap(); let support = tempfile::tempdir().unwrap();
        let recovery = RecoveryStore::new(support.path().join("recovery")); let mut source = setup(root.path());
        let target_date = if backwards { day().pred_opt().unwrap() } else { day().succ_opt().unwrap() };
        let target = Journal::open(root.path(), target_date).unwrap();
        let at = format!("{target_date}T04:00:00+01:00");
        let pending = serde_json::json!({"from":4,"to":7,"batch":{"date":target_date,"at":at,"lastEnd":at,"completed":false}});
        recovery.save_input(&mut source, &writing("baseNEW"), Some(START), "label", &[], Some(pending.clone())).unwrap();
        let mut drafts = vec![
            Draft { date: day().to_string(), id: source.page().id, base: source.raw_base().map(str::to_owned), sessions: writing("base"), last_end: Some(START.into()), label: "label".into(), pending: vec![], pending_input: None },
            Draft { date: target_date.to_string(), id: target.page().id, base: None, sessions: vec![WritingSession { start: at.clone(), previous_end: None, content: "NEW".into() }], last_end: Some(at), label: "".into(), pending: vec![], pending_input: None },
        ];
        // Fail after the source draft is prepared, then fail the shared checkpoint's real file write.
        drafts[1].label = "invalid\nlabel".into();
        assert!(recovery.checkpoint(root.path(), &drafts).is_err());
        drafts[1].label.clear();
        let checkpoint = recovery.checkpoint_path(root.path());
        let original = fs::read(&checkpoint).unwrap();
        let parent = checkpoint.parent().unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o555)).unwrap();
        let result = recovery.checkpoint(root.path(), &drafts);
        fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read(&checkpoint).unwrap(), original);
        assert_eq!(recovery.open(root.path(), day()).unwrap().content(), "baseNEW");
        assert_eq!(recovery.restored(root.path()).unwrap()[0].pending_input, Some(pending));
        assert_eq!(recovery.open(root.path(), target_date).unwrap().content(), "");
        recovery.checkpoint(root.path(), &drafts).unwrap();
        let pages = recovery.restored(root.path()).unwrap();
        let source_page = recovery.open(root.path(), day()).unwrap();
        assert_eq!(source_page.content(), "base");
        assert!(pages.iter().filter(|p| p.date == day().to_string()).all(|p| p.pending_input.is_none()));
        assert_eq!(recovery.open(root.path(), target_date).unwrap().content(), "NEW");
        // Acknowledging the source page cannot delete the target checkpoint.
        let mut source_page = source_page;
        assert!(recovery.save(&mut source_page, &writing("base"), Some(START), "label", &[]).unwrap().saved);
        assert_eq!(recovery.open(root.path(), target_date).unwrap().content(), "NEW");
    }
}

#[test]
fn legacy_page_recovery_migrates_once_without_reviving_after_acknowledgement() {
    let root = tempfile::tempdir().unwrap(); let support = tempfile::tempdir().unwrap();
    let recovery = RecoveryStore::new(support.path().join("recovery")); let mut journal = setup(root.path());
    let (record, _) = recovery.record(&journal, &writing("legacy local"), Some(START), "label", &[], None).unwrap();
    let path = recovery.directory.join(format!("{}.json", journal.page().id));
    atomic(&path, &serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(recovery.open(root.path(), day()).unwrap().content(), "legacy local");
    assert!(recovery.save(&mut journal, &writing("legacy local"), Some(START), "label", &[]).unwrap().saved);
    assert!(recovery.restored(root.path()).unwrap().is_empty());
    assert!(recovery.read_records(root.path()).unwrap().is_empty());
    assert!(path.exists()); // The new empty checkpoint prevents replaying the retained legacy record.
}

#[test]
fn export_rewrites_only_rendered_images_and_parses_each_session_independently() {
    let root = tempfile::tempdir().unwrap(); let support = tempfile::tempdir().unwrap(); let exports = tempfile::tempdir().unwrap();
    let recovery = RecoveryStore::new(support.path().join("recovery")); let mut journal = setup(root.path());
    let path = recovery.import(root.path(), day(), &png()).unwrap();
    let mut sessions = writing("```md\n![literal](2026-09-12/missing.png)");
    sessions.push(WritingSession { start: "2026-09-12T09:00:00+01:00".into(), previous_end: Some(START.into()), content: format!("![actual]({path})\n\n```md\n![example]({path})\n```\n\n[ordinary link]({path})") });
    assert!(recovery.save(&mut journal, &sessions, Some(START), "label", &[]).unwrap().saved);
    let raw = journal.raw_base().unwrap(); let dest = exports.path().join("copy.md");
    recovery.copy(&journal, raw, &dest, &[]).unwrap();
    let copied = fs::read_to_string(dest).unwrap();
    assert!(copied.contains("![actual](photographs-"));
    assert!(copied.contains(&format!("![example]({path})")));
    assert!(copied.contains(&format!("[ordinary link]({path})")));
    assert!(copied.contains("![literal](2026-09-12/missing.png)"));
}
