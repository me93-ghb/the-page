use chrono::{DateTime, FixedOffset, Local, NaiveDate, Timelike};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use std::{
    ffi::CString,
    fs,
    io::Write,
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
};

pub fn journal_day(now: DateTime<FixedOffset>) -> NaiveDate {
    let date = now.date_naive();
    if now.hour() < 4 {
        date.pred_opt().expect("date has a previous day")
    } else {
        date
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WritingSession {
    pub start: String,
    pub previous_end: Option<String>,
    pub content: String,
}

#[derive(Clone, Serialize)]
pub struct Page {
    pub id: String,
    pub base: Option<String>,
    pub recovered: Option<String>,
    pub pending: Vec<crate::recovery::PendingImage>,
    pub pending_input: Option<serde_json::Value>,
    pub sessions: Vec<WritingSession>,
    pub date: String,
    pub content: String,
    pub created: Option<String>,
    pub last_end_input: Option<String>,
    pub label: String,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Journal {
    pub root: PathBuf,
    pub date: NaiveDate,
    base: Option<String>,
    metadata: Mapping,
    content: String,
    sessions: Vec<WritingSession>,
}

fn text(meta: &Mapping, key: &str) -> Option<String> {
    meta.get(Value::String(key.into()))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn replace_page(
    temporary: tempfile::NamedTempFile,
    path: &Path,
    base: Option<&[u8]>,
) -> Result<(), String> {
    if base.is_none() {
        temporary
            .persist_noclobber(path)
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let from = CString::new(temporary.path().as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let to = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // Keep ownership before swapping: neither errors nor process exit may unlink the displaced file.
    let (_file, displaced) = temporary.keep().map_err(|e| e.to_string())?;
    // SAFETY: both C strings remain valid for the call; the OS atomically exchanges the paths.
    const RENAME_NOFOLLOW_ANY: libc::c_uint = 0x10; // macOS sys/stdio.h; not yet exposed by libc.
    let result = unsafe {
        libc::renamex_np(
            from.as_ptr(),
            to.as_ptr(),
            libc::RENAME_SWAP | RENAME_NOFOLLOW_ANY,
        )
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        return Err(format!("Cannot safely replace this page: {error}"));
    }
    // Retain the displaced inode because another editor may still write through an open handle.
    let previous = fs::read(&displaced).map_err(|e| {
        format!(
            "Cannot check the previous file at {}: {e}",
            displaced.display()
        )
    })?;
    if Some(previous.as_slice()) != base {
        let conflict = conflict_copy(path, &previous)?;
        return Err(format!("This page changed during saving. The displaced version is retained at {} (and {}). Review both files before trying again.", conflict.display(), displaced.display()));
    }
    Ok(())
}

pub(crate) fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn conflict_copy(path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    let copy = path.with_file_name(format!("{}.{}.conflict.md", path.file_stem().unwrap().to_string_lossy(), ulid::Ulid::new()));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&copy).map_err(|e| format!("Cannot preserve a conflict copy: {e}"))?;
    file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
    fs::File::open(path.parent().unwrap()).and_then(|f| f.sync_all()).map_err(|e| e.to_string())?;
    Ok(copy)
}

pub fn page_path(root: &Path, date: NaiveDate) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    if !root.is_dir() {
        return Err("The journal folder is unavailable.".into());
    }
    let year = root.join(date.format("%Y").to_string());
    let path = year.join(format!("{date}.md"));
    for candidate in [&year, &path] {
        if let Ok(meta) = fs::symlink_metadata(candidate) {
            if meta.file_type().is_symlink() {
                return Err("Journal pages and year folders cannot be symbolic links.".into());
            }
        }
    }
    Ok(path)
}

pub fn page_dates(root: &Path) -> Result<Vec<NaiveDate>, String> {
    let mut dates = Vec::new();
    for year in fs::read_dir(root).map_err(|e| e.to_string())? {
        let year = year.map_err(|e| e.to_string())?;
        let name = year.file_name().to_string_lossy().into_owned();
        if name.len() != 4 || !name.bytes().all(|c| c.is_ascii_digit())
            || !year.file_type().map_err(|e| e.to_string())?.is_dir() { continue; }
        for entry in fs::read_dir(year.path()).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_file() { continue; }
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some(stem) = file.strip_suffix(".md") else { continue; };
            let Ok(date) = NaiveDate::parse_from_str(stem, "%Y-%m-%d") else { continue; };
            if date.to_string() == stem && date.format("%Y").to_string() == name { dates.push(date); }
        }
    }
    dates.sort_unstable();
    Ok(dates)
}

impl Journal {
    pub fn open(root: &Path, date: NaiveDate) -> Result<Self, String> {
        let path = page_path(root, date)?;
        let base = match fs::read_to_string(&path) {
            Ok(raw) => Some(raw),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        };
        Self::at_base(root, date, base, None)
    }
    pub fn at_base(root: &Path, date: NaiveDate, base: Option<String>, id: Option<&str>) -> Result<Self, String> {
        if let Some(id) = id { ulid::Ulid::from_string(id).map_err(|_| "Invalid page ID.")?; }
        let mut journal = Self {
            root: root.to_path_buf(),
            date,
            base: base.clone(),
            metadata: Mapping::new(),
            content: String::new(),
            sessions: Vec::new(),
        };
        if let Some(raw) = base {
            let (yaml, body) = raw
                .strip_prefix("---\n")
                .and_then(|s| s.split_once("\n---\n"))
                .ok_or("This page has malformed front matter. Its original text is protected.")?;
            journal.metadata =
                serde_yaml_ng::from_str(yaml).map_err(|e| format!("Invalid page metadata: {e}"))?;
            let id = text(&journal.metadata, "id").ok_or("Missing page ID.")?;
            ulid::Ulid::from_string(&id).map_err(|_| "Invalid page ID.")?;
            for key in ["created", "updated", "last_end_input"] {
                if key == "last_end_input"
                    && !journal.metadata.contains_key(Value::String(key.into()))
                {
                    continue;
                }
                DateTime::parse_from_rfc3339(
                    &text(&journal.metadata, key).ok_or(format!("Missing {key}."))?,
                )
                .map_err(|_| format!("Invalid {key} timestamp."))?;
            }
            let label = text(&journal.metadata, "label").unwrap_or_default();
            let heading = journal.heading(&label);
            let remaining = body
                .strip_prefix(&format!("\n{heading}\n\n"))
                .ok_or("The generated page heading is malformed.")?;
            journal.sessions = parse_sessions(remaining, date)?;
            journal.content = journal
                .sessions
                .iter()
                .map(|s| s.content.as_str())
                .collect();
        }
        if journal.base.is_none() {
            journal.metadata.insert("id".into(), id.map(str::to_owned).unwrap_or_else(|| ulid::Ulid::new().to_string()).into());
        }
        Ok(journal)
    }
    pub(crate) fn recovered(root: &Path, date: NaiveDate, base: Option<String>, raw: &str) -> Result<Self, String> {
        let mut journal = Self::at_base(root, date, Some(raw.into()), None)?;
        journal.base = base;
        Ok(journal)
    }
    pub fn raw_base(&self) -> Option<&str> { self.base.as_deref() }
    fn heading(&self, label: &str) -> String {
        let mut heading = format!("# {}", self.date.format("%A, %-d %B %Y"));
        if !label.is_empty() {
            heading.push_str(&format!(" · {label}"));
        }
        heading
    }
    pub fn content(&self) -> &str {
        &self.content
    }
    pub fn page(&self) -> Page {
        Page {
            id: text(&self.metadata, "id").unwrap(),
            base: self.base.clone(), recovered: None, pending: Vec::new(), pending_input: None,
            sessions: self.sessions.clone(),
            date: self.date.to_string(),
            content: self.content.clone(),
            created: text(&self.metadata, "created"),
            last_end_input: text(&self.metadata, "last_end_input")
                .or_else(|| self.sessions.last().map(|s| s.start.clone())),
            label: text(&self.metadata, "label").unwrap_or_default(),
            error: None,
        }
    }
    pub fn draft(
        &self,
        sessions: &[WritingSession],
        last_end: Option<&str>,
        label: &str,
    ) -> Result<String, String> {
        let mut sessions = sessions.to_vec();
        while sessions.len() > 1 && sessions.last().unwrap().content.trim().is_empty() {
            sessions.pop();
        }
        if label.contains(['\n', '\r', '\0']) { return Err("A label must fit on one line.".into()); }
        let started = sessions.first().map(|s| s.start.clone()).unwrap_or_else(|| Local::now().to_rfc3339());
        let mut blocks = Vec::new();
        for (index, session) in sessions.iter().enumerate() {
            if (index == 0) != session.previous_end.is_none() {
                return Err("Invalid session boundary.".into());
            }
            let heading = session_heading(&session.start, self.date)?;
            let previous = if let Some(previous) = &session.previous_end {
                DateTime::parse_from_rfc3339(previous)
                    .map_err(|_| "Invalid preceding end-input time.")?;
                format!("; previous-end: {previous}")
            } else {
                String::new()
            };
            blocks.push(format!(
                "{heading}\n<!-- session: {}{previous} -->\n\n{}",
                session.start, session.content
            ));
        }
        if let Some(last_end) = last_end {
            DateTime::parse_from_rfc3339(last_end).map_err(|_| "Invalid end-input time.")?;
        } else if !sessions.is_empty() { return Err("Missing end-input time.".into()); }
        let mut meta = self.metadata.clone();
        if !meta.contains_key(Value::String("created".into())) {
            meta.insert("created".into(), started.into());
        }
        meta.insert("updated".into(), Local::now().to_rfc3339().into());
        if let Some(last_end) = last_end { meta.insert("last_end_input".into(), last_end.into()); }
        meta.insert("label".into(), label.into());
        let yaml = serde_yaml_ng::to_string(&meta).map_err(|e| e.to_string())?;
        let raw = format!(
            "---\n{yaml}---\n\n{}\n\n{}",
            self.heading(label),
            blocks.join("\n\n")
        );
        Ok(raw)
    }
    pub fn save_sessions(&mut self, sessions: &[WritingSession], last_end: Option<&str>, label: &str) -> Result<(), String> {
        let raw = self.draft(sessions, last_end, label)?;
        self.persist(&raw, &mut |_| Ok(())).map(|_| ())
    }
    pub(crate) fn persist(&mut self, raw: &str, fault: &mut dyn FnMut(&str) -> Result<(), String>) -> Result<Vec<PathBuf>, String> {
        let next = Self::at_base(&self.root, self.date, Some(raw.into()), None)?;
        let path = page_path(&self.root, self.date)?;
        let parent = path.parent().ok_or("Missing year folder.")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        fs::File::open(&self.root).and_then(|f| f.sync_all()).map_err(|e| e.to_string())?;
        page_path(&self.root, self.date)?;
        fault("temporary-write")?;
        let mut temporary = tempfile::Builder::new().prefix(&format!(".the-page-{}-", self.date)).suffix(".md").tempfile_in(parent).map_err(|e| e.to_string())?;
        temporary.write_all(raw.as_bytes()).and_then(|_| temporary.as_file().sync_all()).map_err(|e| e.to_string())?;
        let mut conflicts = Vec::new();
        let current = read_bytes(&path)?;
        if current.as_deref() != self.base.as_deref().map(str::as_bytes) {
            if let Some(bytes) = &current {
                fault("conflict-copy")?;
                conflicts.push(conflict_copy(&path, bytes)?);
            }
        }
        fault("before-recheck")?;
        let latest = read_bytes(&path)?;
        if latest != current {
            if let Some(bytes) = &latest { fault("conflict-copy")?; conflicts.push(conflict_copy(&path, bytes)?); }
            return Err(format!("The page changed again while saving. Preserved copies: {}. Try again.", conflicts.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")));
        }
        page_path(&self.root, self.date)?;
        fault("rename")?;
        replace_page(temporary, &path, current.as_deref())?;
        fs::File::open(parent).and_then(|f| f.sync_all()).map_err(|e| e.to_string())?;
        *self = next;
        Ok(conflicts)
    }

}

fn session_heading(start: &str, date: NaiveDate) -> Result<String, String> {
    let parsed = DateTime::parse_from_rfc3339(start).map_err(|_| "Malformed session timestamp.")?;
    Ok(parsed
        .format(if parsed.date_naive() > date {
            "## %Y-%m-%d %H:%M"
        } else {
            "## %H:%M"
        })
        .to_string())
}

fn parse_sessions(body: &str, date: NaiveDate) -> Result<Vec<WritingSession>, String> {
    if body.is_empty() { return Ok(Vec::new()); }
    let mut boundaries = vec![0];
    for (index, _) in body.match_indices("\n\n## ") {
        if body[index + 2..]
            .lines()
            .nth(1)
            .is_some_and(|line| line.starts_with("<!-- session:"))
        {
            boundaries.push(index + 2);
        }
    }
    let mut sessions = Vec::new();
    for (index, start) in boundaries.iter().copied().enumerate() {
        let end = boundaries
            .get(index + 1)
            .map_or(body.len(), |next| next - 2);
        let (header, content) = body[start..end]
            .split_once("\n\n")
            .ok_or("Missing session boundary.")?;
        let (heading, comment) = header
            .split_once('\n')
            .ok_or("Missing session timestamp.")?;
        let stamp = comment
            .strip_prefix("<!-- session: ")
            .and_then(|s| s.strip_suffix(" -->"))
            .ok_or("Malformed session comment.")?;
        let (start, previous_end) = stamp
            .split_once("; previous-end: ")
            .map_or((stamp, None), |(start, end)| (start, Some(end)));
        if session_heading(start, date)? != heading {
            return Err("Session time and timestamp disagree.".into());
        }
        if (index == 0) != previous_end.is_none() {
            return Err("Invalid session boundary.".into());
        }
        if let Some(end) = previous_end {
            DateTime::parse_from_rfc3339(end).map_err(|_| "Invalid preceding end-input time.")?;
        }
        sessions.push(WritingSession {
            start: start.into(),
            previous_end: previous_end.map(str::to_owned),
            content: content.into(),
        });
    }
    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn labels_and_captions_round_trip_without_changing_session_clocks() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = Journal::open(root.path(), day()).unwrap();
        journal.save_sessions(&[], None, "quiet morning").unwrap();
        let mut journal = Journal::open(root.path(), day()).unwrap();
        assert!(journal.page().sessions.is_empty());
        assert_eq!(journal.page().last_end_input, None);
        assert_eq!(journal.page().label, "quiet morning");
        let sessions = one("![the shelf](2026-09-12/photo.jpg)\n");
        journal.save_sessions(&sessions, Some(START), "a phrase: 🌿").unwrap();
        journal.save_sessions(&sessions, Some(START), "new label").unwrap();
        assert!(journal.save_sessions(&sessions, Some(START), "bad\nlabel").is_err());
        let reopened = Journal::open(root.path(), day()).unwrap().page();
        assert_eq!(reopened.sessions, sessions);
        assert_eq!(reopened.label, "new label");
        assert_eq!(reopened.last_end_input.as_deref(), Some(START));
        let raw = fs::read_to_string(root.path().join("2026/2026-09-12.md")).unwrap();
        assert!(raw.contains("# Saturday, 12 September 2026 · new label"));
    }

    #[test]
    fn history_lists_only_dated_files_in_real_year_directories() {
        let root = tempfile::tempdir().unwrap();
        let year = root.path().join("2026");
        fs::create_dir(&year).unwrap();
        for day in 1..=17 {
            fs::write(year.join(format!("2026-09-{day:02}.md")), "fixture").unwrap();
        }
        fs::write(year.join("2026-09-25.md"), "future").unwrap();
        for name in [".the-page-hidden.md", "2026-02-30.md", "2025-09-20.md", "2026-09-2.md"] {
            fs::write(year.join(name), "ignore").unwrap();
        }
        std::os::unix::fs::symlink(&year, root.path().join("2025")).unwrap();
        std::os::unix::fs::symlink(year.join("2026-09-01.md"), year.join("2026-09-30.md")).unwrap();
        let dates = page_dates(root.path()).unwrap();
        assert_eq!(dates.len(), 18);
        assert_eq!(dates.first().unwrap().to_string(), "2026-09-01");
        assert_eq!(dates.last().unwrap().to_string(), "2026-09-25");
        assert!(page_dates(&root.path().join("unavailable")).is_err());
    }

    fn instant(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }
    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 12).unwrap()
    }
    const START: &str = "2026-09-12T09:42:03+01:00";
    fn one(content: &str) -> Vec<WritingSession> {
        vec![WritingSession {
            start: START.into(),
            previous_end: None,
            content: content.into(),
        }]
    }

    #[test]
    fn sessions_round_trip_and_missing_clock_uses_last_session_not_updated() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("2026/2026-09-12.md");
        let sessions = vec![
            WritingSession {
                start: START.into(),
                previous_end: None,
                content: "# Authored\n\n## 08:30\n\nFirst 😊\n".into(),
            },
            WritingSession {
                start: "2026-09-13T01:30:00+01:00".into(),
                previous_end: Some(START.into()),
                content: "Later writing".into(),
            },
        ];
        let mut journal = Journal::open(root.path(), day()).unwrap();
        let end = "2026-09-13T01:35:00+01:00";
        journal.save_sessions(&sessions, Some(end), "").unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("## 2026-09-13 01:30\n<!-- session: 2026-09-13T01:30:00+01:00; previous-end: 2026-09-12T09:42:03+01:00 -->"));
        assert_eq!(
            Journal::open(root.path(), day()).unwrap().page().sessions,
            sessions
        );
        let mut corrected = sessions.clone();
        corrected[0].content.push_str("Correction");
        journal.save_sessions(&corrected, Some(end), "").unwrap();
        assert_eq!(
            Journal::open(root.path(), day())
                .unwrap()
                .page()
                .last_end_input
                .as_deref(),
            Some(end)
        );
        let raw = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("last_end_input:"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, raw).unwrap();
        assert_eq!(
            Journal::open(root.path(), day())
                .unwrap()
                .page()
                .last_end_input
                .as_deref(),
            Some(sessions[1].start.as_str())
        );
    }

    #[test]
    fn empty_final_sessions_are_omitted_and_malformed_boundaries_are_protected() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("2026/2026-09-12.md");
        let sessions = vec![
            WritingSession {
                start: START.into(),
                previous_end: None,
                content: "Words".into(),
            },
            WritingSession {
                start: "2026-09-12T12:00:00+01:00".into(),
                previous_end: Some(START.into()),
                content: String::new(),
            },
        ];
        let mut journal = Journal::open(root.path(), day()).unwrap();
        journal
            .save_sessions(&sessions, Some(&sessions[1].start), "")
            .unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert_eq!(raw.matches("<!-- session:").count(), 1);
        fs::write(
            &path,
            format!("{raw}\n\n## 12:00\n<!-- session: nonsense; previous-end: {START} -->\n\ntext"),
        )
        .unwrap();
        assert!(Journal::open(root.path(), day()).is_err());
    }

    #[test]
    fn four_am_uses_calendar_date_not_elapsed_hours() {
        assert_eq!(
            journal_day(instant("2026-03-29T04:00:00+01:00")).to_string(),
            "2026-03-29"
        );
        assert_eq!(
            journal_day(instant("2026-03-29T03:59:59+01:00")).to_string(),
            "2026-03-28"
        );
        assert_eq!(
            journal_day(instant("2026-10-25T04:00:00+00:00")).to_string(),
            "2026-10-25"
        );
    }
    #[test]
    fn round_trip_preserves_authored_headings_unknown_metadata_and_offsets() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("2026/2026-09-12.md");
        let mut journal = Journal::open(root.path(), day()).unwrap();
        assert!(!path.exists());
        let content = "# My heading\n\n## 08:30\n\n**Hello**\n\n末尾\n";
        journal.save_sessions(&one(content), Some(START), "").unwrap();
        let raw = fs::read_to_string(&path).unwrap().replacen(
            "---\n",
            "---\ncustom: {nested: [one, two]}\n",
            1,
        );
        fs::write(&path, raw).unwrap();
        let mut reopened = Journal::open(root.path(), day()).unwrap();
        assert_eq!(reopened.content(), content);
        reopened.save_sessions(&one(content), Some(START), "").unwrap();
        let raw = fs::read_to_string(path).unwrap();
        assert!(raw.contains("nested:"));
        assert!(raw.contains(START));
        assert!(raw.contains("## 08:30"));
    }
    #[test]
    fn replacement_boundary_retains_racing_external_writes() {
        for base in [Some(b"base".as_slice()), None] {
            let root = tempfile::tempdir().unwrap();
            let directory = root.path().canonicalize().unwrap();
            let path = directory.join("page.md");
            let mut temporary = tempfile::NamedTempFile::new_in(&directory).unwrap();
            temporary.write_all(b"mine").unwrap();
            // The external write arrives after the caller's last base comparison.
            fs::write(&path, "external").unwrap();
            assert!(replace_page(temporary, &path, base).is_err());
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                if base.is_some() { "mine" } else { "external" }
            );
            assert!(fs::read_dir(root.path())
                .unwrap()
                .any(|entry| { fs::read(entry.unwrap().path()).unwrap() == b"external" }));
        }
    }

    #[test]
    fn an_external_open_handle_remains_recoverable_after_replacement() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().canonicalize().unwrap();
        let path = directory.join("page.md");
        fs::write(&path, "base").unwrap();
        let mut external = fs::OpenOptions::new().write(true).open(&path).unwrap();
        let mut temporary = tempfile::NamedTempFile::new_in(&directory).unwrap();
        temporary.write_all(b"mine").unwrap();
        replace_page(temporary, &path, Some(b"base")).unwrap();
        external.write_all(b"late external").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"mine");
        assert!(fs::read_dir(root.path())
            .unwrap()
            .any(|entry| { fs::read(entry.unwrap().path()).unwrap() == b"late external" }));
    }

    #[test]
    fn present_invalid_end_input_is_not_a_missing_compatibility_field() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("2026/2026-09-12.md");
        let mut journal = Journal::open(root.path(), day()).unwrap();
        journal.save_sessions(&one("saved words"), Some(START), "").unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        for invalid in ["42", "null", "[]", "{}", "not-a-timestamp"] {
            let malformed = raw
                .lines()
                .map(|line| {
                    if line.starts_with("last_end_input:") {
                        format!("last_end_input: {invalid}")
                    } else {
                        line.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(&path, &malformed).unwrap();
            assert!(
                Journal::open(root.path(), day()).is_err(),
                "accepted {invalid}"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), malformed);
        }
    }

    #[test]
    fn malformed_files_are_protected_and_conflicting_bytes_are_copied() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("2026/2026-09-12.md");
        let mut journal = Journal::open(root.path(), day()).unwrap();
        journal.save_sessions(&one("mine"), Some(START), "").unwrap();
        fs::write(&path, "external writing").unwrap();
        assert!(Journal::open(root.path(), day()).is_err());
        journal.save_sessions(&one("new mine"), Some(START), "").unwrap();
        assert_eq!(Journal::open(root.path(), day()).unwrap().content(), "new mine");
        assert!(fs::read_dir(path.parent().unwrap()).unwrap().any(|f| {
            let f = f.unwrap(); f.file_name().to_string_lossy().ends_with(".conflict.md") && fs::read(f.path()).unwrap() == b"external writing"
        }));
    }
    #[test]
    fn a_failed_temporary_write_keeps_the_saved_page() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let mut journal = Journal::open(root.path(), day()).unwrap();
        journal.save_sessions(&one("saved words"), Some(START), "").unwrap();
        let year = root.path().join("2026");
        fs::set_permissions(&year, fs::Permissions::from_mode(0o555)).unwrap();
        let result = journal.save_sessions(&one("unsaved words"), Some(START), "");
        fs::set_permissions(&year, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert_eq!(
            Journal::open(root.path(), day()).unwrap().content(),
            "saved words"
        );
        assert_eq!(journal.content(), "saved words");
    }

    #[test]
    fn unavailable_parent_and_symlink_escape_do_not_lose_data() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = Journal::open(root.path(), day()).unwrap();
        fs::write(root.path().join("2026"), "occupied").unwrap();
        assert!(journal.save_sessions(&one("unsaved"), Some(START), "").is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("2026")).unwrap(),
            "occupied"
        );
        fs::remove_file(root.path().join("2026")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("2026")).unwrap();
        assert!(journal.save_sessions(&one("unsaved"), Some(START), "").is_err());
        assert!(!outside.path().join("2026-09-12.md").exists());
    }
}
