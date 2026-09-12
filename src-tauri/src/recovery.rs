use crate::{photographs, storage::{Journal, Page, WritingSession, read_bytes}};
use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Write, ops::Range, path::{Path, PathBuf}, sync::LazyLock};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PendingImage {
    pub bytes: Vec<u8>,
    pub from: usize,
    pub to: usize,
    pub at: String,
    pub corrected: bool,
}

#[derive(Clone, Deserialize, Serialize)]
struct Asset { root: PathBuf, date: NaiveDate, path: String, bytes: Vec<u8> }
#[derive(Deserialize, Serialize)]
struct Record {
    root: PathBuf, date: NaiveDate, raw: String, base: Option<String>,
    at: String, token: String, assets: Vec<Asset>, pending: Vec<PendingImage>, pending_input: Option<serde_json::Value>,
}
#[derive(Serialize)]
pub struct SaveResult {
    pub saved: bool, pub recovered: bool, pub base: Option<String>,
    pub cause: Option<String>, pub conflicts: Vec<PathBuf>,
}

#[derive(Deserialize)]
pub struct CopyAsset { pub path: String, pub bytes: Vec<u8> }

pub struct RecoveryStore { directory: PathBuf }

fn atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing destination folder.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temp.write_all(bytes).and_then(|_| temp.as_file().sync_all()).map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    fs::File::open(parent).and_then(|f| f.sync_all()).map_err(|e| e.to_string())?;
    if let Some(ancestor) = parent.parent() { fs::File::open(ancestor).and_then(|f| f.sync_all()).map_err(|e| e.to_string())?; }
    Ok(())
}

// Match the editor's Image nodes and supported caption/destination syntax, excluding literal code.
fn image_references(markdown: &str) -> Vec<(Range<usize>, String)> {
    static IMAGE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"^!\[((?:\\.|[^\]\\])*)\]\(([^\s()]+)\)$").unwrap());
    pulldown_cmark::Parser::new(markdown).into_offset_iter().filter_map(|(event, span)| {
        if !matches!(event, pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { .. })) { return None; }
        let captures = IMAGE.captures(&markdown[span.clone()])?;
        let path = captures.get(2)?;
        if path.as_str().contains([':', '\\']) || path.as_str().starts_with('/') || path.as_str().split('/').any(|p| p == "..") { return None; }
        Some((span.start + path.start()..span.start + path.end(), path.as_str().into()))
    }).collect()
}
fn image_paths(markdown: &str) -> Vec<String> {
    image_references(markdown).into_iter().map(|(_, path)| path).collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub date: String, pub id: String, pub base: Option<String>, pub sessions: Vec<WritingSession>,
    pub last_end: Option<String>, pub label: String, pub pending: Vec<PendingImage>, pub pending_input: Option<serde_json::Value>,
}

impl RecoveryStore {
    pub fn new(directory: PathBuf) -> Self { Self { directory } }
    fn asset_path(&self, root: &Path, date: NaiveDate, path: &str) -> PathBuf {
        let key = serde_json::to_vec(&(root, date, path)).unwrap();
        self.directory.join("assets").join(format!("{:x}.json", Sha256::digest(key)))
    }
    fn cache(&self, asset: &Asset) -> Result<(), String> {
        atomic(&self.asset_path(&asset.root, asset.date, &asset.path), &serde_json::to_vec(asset).map_err(|e| e.to_string())?)
    }
    fn asset(&self, root: &Path, date: NaiveDate, path: &str) -> Result<Asset, String> {
        if let Ok(bytes) = photographs::read_bytes(root, date, path) {
            let asset = Asset { root: root.into(), date, path: path.into(), bytes };
            // The durable page record also embeds these bytes; the cache lets an unavailable journal still display them.
            let _ = self.cache(&asset);
            return Ok(asset);
        }
        let bytes = match fs::read(self.asset_path(root, date, path)) {
            Ok(bytes) => bytes,
            Err(error) => {
                for (_, record) in self.records(root)?.into_iter().rev() {
                    if let Some(asset) = record.assets.into_iter().find(|a| a.date == date && a.path == path && a.root == root) { photographs::format(&asset.bytes)?; return Ok(asset); }
                }
                return Err(error.to_string());
            }
        };
        let asset: Asset = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if asset.root != root || asset.date != date || asset.path != path { return Err("Photograph recovery identity differs.".into()); }
        photographs::format(&asset.bytes)?;
        Ok(asset)
    }
    pub fn read(&self, root: &Path, date: NaiveDate, path: &str) -> Result<String, String> {
        photographs::data_url(&self.asset(root, date, path)?.bytes)
    }
    pub fn import(&self, root: &Path, date: NaiveDate, bytes: &[u8]) -> Result<String, String> {
        let (extension, _) = photographs::format(bytes).map_err(|e| format!("Unsupported photograph. {e}"))?;
        let path = format!("{date}/{}.{}", ulid::Ulid::new(), extension);
        let cached = self.cache(&Asset { root: root.into(), date, path: path.clone(), bytes: bytes.into() });
        let journal = photographs::store_named(root, date, &path, bytes);
        if let (Err(recovery), Err(journal)) = (cached, journal) {
            return Err(format!("Journal: {journal}. Recovery: {recovery}. The image remains in memory."));
        }
        Ok(path)
    }
    fn checkpoint_path(&self, root: &Path) -> PathBuf {
        let key = serde_json::to_vec(root).unwrap();
        self.directory.join("journals").join(format!("{:x}.json", Sha256::digest(key)))
    }
    fn read_records(&self, root: &Path) -> Result<BTreeMap<String, Record>, String> {
        match fs::read(self.checkpoint_path(root)) {
            Ok(bytes) => return serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read recovery: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(e) => return Err(format!("Cannot read recovery: {e}")),
        }
        // Individual page records from earlier versions remain readable until the first journal checkpoint.
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(format!("Cannot read recovery: {e}")),
        };
        let mut records = BTreeMap::new();
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_file() || entry.path().extension().is_none_or(|e| e != "json") { continue; }
            let record: Record = serde_json::from_slice(&fs::read(entry.path()).map_err(|e| e.to_string())?).map_err(|e| format!("Cannot read recovery {}: {e}", entry.path().display()))?;
            if record.root == root { records.insert(entry.path().file_stem().unwrap().to_string_lossy().into_owned(), record); }
        }
        Ok(records)
    }
    fn write_records(&self, root: &Path, records: &BTreeMap<String, Record>) -> Result<(), String> {
        // ponytail: rewrites all pending pages; separate immutable records if recovery size becomes a bottleneck.
        atomic(&self.checkpoint_path(root), &serde_json::to_vec(records).map_err(|e| e.to_string())?)
    }
    fn records(&self, root: &Path) -> Result<Vec<(String, Record)>, String> {
        let mut records: Vec<_> = self.read_records(root)?.into_iter().collect();
        records.sort_by(|a, b| a.1.token.cmp(&b.1.token));
        Ok(records)
    }
    fn record(&self, journal: &Journal, sessions: &[WritingSession], last_end: Option<&str>, label: &str, pending: &[PendingImage], pending_input: Option<serde_json::Value>) -> Result<(Record, Option<String>), String> {
        let raw = journal.draft(sessions, last_end, label)?;
        let paths: std::collections::BTreeSet<_> = sessions.iter().flat_map(|s| image_paths(&s.content)).collect();
        let mut assets = Vec::new(); let mut cause = None;
        for path in paths {
            match self.asset(&journal.root, journal.date, &path) {
                Ok(asset) => assets.push(asset),
                Err(error) => cause = Some(error),
            }
        }
        Ok((Record { root: journal.root.clone(), date: journal.date, raw, base: journal.raw_base().map(str::to_owned), at: Local::now().to_rfc3339(), token: ulid::Ulid::new().to_string(), assets, pending: pending.to_vec(), pending_input }, cause))
    }
    pub fn checkpoint(&self, root: &Path, drafts: &[Draft]) -> Result<(), String> {
        let mut records = self.read_records(root)?;
        for draft in drafts {
            let date = NaiveDate::parse_from_str(&draft.date, "%Y-%m-%d").map_err(|e| e.to_string())?;
            let journal = Journal::at_base(root, date, draft.base.clone(), Some(&draft.id))?;
            let (record, _) = self.record(&journal, &draft.sessions, draft.last_end.as_deref(), &draft.label, &draft.pending, draft.pending_input.clone())?;
            records.insert(draft.id.clone(), record);
        }
        // Both sides of rollover change ownership in this single atomic replacement.
        self.write_records(root, &records)
    }
    fn restore_record(&self, record: &Record) -> Result<Page, String> {
        for asset in &record.assets { let _ = self.cache(asset); }
        let mut page = Journal::recovered(&record.root, record.date, record.base.clone(), &record.raw)?.page();
        page.recovered = Some(record.at.clone()); page.pending = record.pending.clone(); page.pending_input = record.pending_input.clone();
        Ok(page)
    }
    pub fn restored(&self, root: &Path) -> Result<Vec<Page>, String> {
        let mut pages: Vec<Page> = Vec::new();
        for (_, record) in self.records(root)? {
            let path = crate::storage::page_path(root, record.date);
            if record.pending.is_empty() && record.pending_input.is_none() && path.and_then(|p| read_bytes(&p)).ok().flatten().as_deref() == Some(record.raw.as_bytes()) { continue; }
            let page = self.restore_record(&record)?;
            pages.retain(|p| p.date != page.date); pages.push(page);
        }
        Ok(pages)
    }
    pub fn open(&self, root: &Path, date: NaiveDate) -> Result<Journal, String> {
        if let Some((_, record)) = self.records(root)?.into_iter().rev().find(|(_, r)| r.date == date) {
            let path = crate::storage::page_path(root, date);
            if record.pending.is_empty() && record.pending_input.is_none() && path.and_then(|p| read_bytes(&p)).ok().flatten().as_deref() == Some(record.raw.as_bytes()) { return Journal::open(root, date); }
            self.restore_record(&record)?;
            return Journal::recovered(root, date, record.base, &record.raw);
        }
        Journal::open(root, date)
    }
    pub fn save(&self, journal: &mut Journal, sessions: &[WritingSession], last_end: Option<&str>, label: &str, pending: &[PendingImage]) -> Result<SaveResult, String> {
        self.save_with(journal, sessions, last_end, label, pending, None, &mut |_| Ok(()))
    }
    pub fn save_input(&self, journal: &mut Journal, sessions: &[WritingSession], last_end: Option<&str>, label: &str, pending: &[PendingImage], input: Option<serde_json::Value>) -> Result<SaveResult, String> {
        self.save_with(journal, sessions, last_end, label, pending, input, &mut |_| Ok(()))
    }
    fn save_with(&self, journal: &mut Journal, sessions: &[WritingSession], last_end: Option<&str>, label: &str, pending: &[PendingImage], pending_input: Option<serde_json::Value>, fault: &mut dyn FnMut(&str) -> Result<(), String>) -> Result<SaveResult, String> {
        let (record, asset_error) = self.record(journal, sessions, last_end, label, pending, pending_input)?;
        let raw = record.raw.clone();
        let id = journal.page().id;
        let mut records = match self.read_records(&journal.root) {
            Ok(records) => records,
            Err(cause) => return Ok(SaveResult { saved: false, recovered: false, base: record.base, cause: Some(format!("Recovery could not be read: {cause}.")), conflicts: Vec::new() }),
        };
        records.insert(id.clone(), record);
        let checkpoint = fault("recovery-write").and_then(|_| self.write_records(&journal.root, &records));
        let record = records.get(&id).unwrap();
        let recovered = checkpoint.is_ok();
        // No canonical replacement may precede a successful recovery checkpoint.
        if let Err(cause) = checkpoint {
            return Ok(SaveResult { saved: false, recovered: false, base: record.base.clone(), cause: Some(format!("Recovery could not be written: {cause}.")), conflicts: Vec::new() });
        }
        *journal = Journal::recovered(&record.root, record.date, record.base.clone(), &raw)?;
        let saved = (|| {
            if let Some(cause) = asset_error { return Err(format!("Writing is in recovery, but an image could not be copied: {cause}")); }
            if !pending.is_empty() || record.pending_input.is_some() { return Err("Photographs are still waiting to be inserted. Try again.".into()); }
            for asset in &record.assets {
                fault("image-write")?;
                // Existing local references may use another dated directory in the same year.
                let date = asset.path.split('/').next().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).ok_or("Invalid photograph directory.")?;
                if date.format("%Y").to_string() != journal.date.format("%Y").to_string() { return Err("Photograph directory is outside this journal year.".into()); }
                photographs::store_named(&journal.root, date, &asset.path, &asset.bytes)?;
            }
            journal.persist(&raw, fault)
        })();
        match saved {
            Ok(conflicts) => {
                let cleanup = (|| {
                    fault("recovery-delete")?;
                    // Remove only the checkpoint this save owns, never a newer revision.
                    let mut current = self.read_records(&journal.root)?;
                    if current.get(&id).is_some_and(|current| current.token == record.token) {
                        current.remove(&id);
                        self.write_records(&journal.root, &current)?;
                    }
                    Ok::<(), String>(())
                })();
                Ok(SaveResult { saved: true, recovered: cleanup.is_err(), base: Some(raw), cause: cleanup.err().map(|e| format!("Saved to the journal; an older recovery copy remains: {e}")), conflicts })
            }
            Err(cause) => Ok(SaveResult { saved: false, recovered, base: record.base.clone(), cause: Some(cause), conflicts: Vec::new() }),
        }
    }
    pub fn copy(&self, journal: &Journal, raw: &str, destination: &Path, pending: &[CopyAsset]) -> Result<(), String> {
        let parent = destination.parent().ok_or("Missing copy destination.")?;
        let folder = format!("photographs-{}", ulid::Ulid::new());
        let snapshot = Journal::recovered(&journal.root, journal.date, journal.raw_base().map(str::to_owned), raw)?;
        let mut page = snapshot.page();
        let mut copied_paths = BTreeMap::new();
        for session in &mut page.sessions {
            for (range, path) in image_references(&session.content).into_iter().rev() {
                let relative = if let Some(relative) = copied_paths.get(&path) { relative } else {
                    let bytes = if let Some(asset) = pending.iter().find(|a| a.path == path) { asset.bytes.clone() } else { self.asset(&journal.root, journal.date, &path)?.bytes };
                    let (extension, _) = photographs::format(&bytes)?;
                    let relative = format!("{folder}/{}.{extension}", copied_paths.len());
                    atomic(&parent.join(&relative), &bytes)?;
                    copied_paths.entry(path).or_insert(relative)
                };
                session.content.replace_range(range, relative);
            }
        }
        let copied = snapshot.draft(&page.sessions, page.last_end_input.as_deref(), &page.label)?;
        atomic(destination, copied.as_bytes())
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
