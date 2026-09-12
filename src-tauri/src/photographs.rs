use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::NaiveDate;
use std::{ffi::CString, fs::{File, OpenOptions}, io::{Cursor, Read, Write}, os::{fd::{AsRawFd, FromRawFd}, unix::fs::OpenOptionsExt}, path::{Component, Path}};

const MAX_BYTES: usize = 32 * 1024 * 1024;

pub(crate) fn format(bytes: &[u8]) -> Result<(&'static str, &'static str), String> {
    if bytes.len() > MAX_BYTES { return Err("Photographs must be at most 32 MB.".into()); }
    let format = image::guess_format(bytes).map_err(|_| "Use a PNG or JPEG photograph.")?;
    let kind = match format {
        image::ImageFormat::Png => ("png", "image/png"),
        image::ImageFormat::Jpeg => ("jpg", "image/jpeg"),
        _ => return Err("Use a PNG or JPEG photograph.".into()),
    };
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|e| format!("Cannot decode this photograph: {e}"))?;
    Ok(kind)
}

// Open each component relative to its directory handle so symlink swaps cannot escape the journal.
fn directory(parent: &File, name: &str, create: bool) -> Result<File, String> {
    let name = CString::new(name).map_err(|e| e.to_string())?;
    if create {
        // SAFETY: the parent handle and component string remain valid throughout the call.
        let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
        if result != 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    // SAFETY: openat returns a new owned descriptor and does not follow symbolic links.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
    if fd < 0 { return Err("Cannot open the photograph folder. Symbolic links are not allowed.".into()); }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn year(root: &Path, date: NaiveDate, create: bool) -> Result<File, String> {
    let root = OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW).open(root).map_err(|e| e.to_string())?;
    directory(&root, &date.format("%Y").to_string(), create)
}

pub fn store(root: &Path, date: NaiveDate, bytes: &[u8]) -> Result<String, String> {
    let (extension, _) = format(bytes)?;
    let path = format!("{date}/{}.{}", ulid::Ulid::new(), extension);
    store_named(root, date, &path, bytes)?;
    Ok(path)
}

pub(crate) fn store_named(root: &Path, date: NaiveDate, path: &str, bytes: &[u8]) -> Result<(), String> {
    format(bytes)?;
    let name = path.strip_prefix(&format!("{date}/")).ok_or("Invalid photograph destination.")?;
    if name.is_empty() || name.contains(['/', '\\', ':', '\0']) || matches!(name, "." | "..") { return Err("Invalid photograph filename.".into()); }
    let year = year(root, date, true)?;
    let folder = directory(&year, &date.to_string(), true)?;
    let c_name = CString::new(name).map_err(|e| e.to_string())?;
    let temporary = CString::new(format!(".the-page-{}", ulid::Ulid::new())).unwrap();
    // SAFETY: the directory handle and string are valid; O_EXCL forbids overwriting a collision.
    let fd = unsafe { libc::openat(folder.as_raw_fd(), temporary.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600) };
    if fd < 0 { return Err(std::io::Error::last_os_error().to_string()); }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = (|| {
        file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
        // SAFETY: both names are single components of the verified directory; never replace an existing image.
        let renamed = unsafe { libc::renameatx_np(folder.as_raw_fd(), temporary.as_ptr(), folder.as_raw_fd(), c_name.as_ptr(), libc::RENAME_EXCL) };
        if renamed != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::AlreadyExists || read_bytes(root, date, path)? != bytes { return Err(error.to_string()); }
        }
        folder.sync_all().and_then(|_| year.sync_all()).and_then(|_| File::open(root)?.sync_all()).map_err(|e| e.to_string())
    })();
    // SAFETY: this is our unique temporary name, never a user image.
    unsafe { libc::unlinkat(folder.as_raw_fd(), temporary.as_ptr(), 0); }
    result

}

pub(crate) fn data_url(bytes: &[u8]) -> Result<String, String> {
    let (_, mime) = format(bytes)?;
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

pub fn read(root: &Path, date: NaiveDate, relative: &str) -> Result<String, String> {
    data_url(&read_bytes(root, date, relative)?)
}

pub(crate) fn read_bytes(root: &Path, date: NaiveDate, relative: &str) -> Result<Vec<u8>, String> {
    if relative.contains([':', '\\', '\0']) || relative.starts_with('/') {
        return Err("Photographs must use a local path inside the journal.".into());
    }
    let parts = Path::new(relative).components().map(|part| match part {
        Component::Normal(name) => name.to_str().ok_or("Invalid photograph path."),
        _ => Err("Photograph paths cannot escape the journal."),
    }).collect::<Result<Vec<_>, _>>()?;
    let (name, parents) = parts.split_last().ok_or("Missing photograph path.")?;
    let mut folder = year(root, date, false)?;
    for part in parents { folder = directory(&folder, part, false)?; }
    let name = CString::new(*name).map_err(|e| e.to_string())?;
    // SAFETY: open relative to the verified directory and reject a symbolic-link file.
    let fd = unsafe { libc::openat(folder.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC) };
    if fd < 0 { return Err("Cannot read this photograph. Missing files and symbolic links are not opened.".into()); }
    let file = unsafe { File::from_raw_fd(fd) };
    if !file.metadata().map_err(|e| e.to_string())?.is_file() { return Err("The photograph is not a file.".into()); }
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    format(&bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(3, 2).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }
    #[test]
    fn imports_keep_original_bytes_and_never_replace_other_images() {
        let root = tempfile::tempdir().unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let bytes = png();
        let a = store(root.path(), date, &bytes).unwrap();
        let b = store(root.path(), date, &bytes).unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read(root.path().join("2026").join(&a)).unwrap(), bytes);
        assert!(read(root.path(), date, &a).unwrap().starts_with("data:image/png;base64,"));
        assert!(store(root.path(), date, b"not an image").is_err());
        assert!(store(root.path(), date, &bytes[..20]).is_err());
        assert_eq!(std::fs::read_dir(root.path().join("2026/2026-09-12")).unwrap().count(), 2);
    }
    #[test]
    fn remote_traversal_symlinks_and_failed_writes_are_rejected() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let a = store(root.path(), date, &png()).unwrap();
        for path in ["https://example.com/a.png", "../outside.png", "/tmp/a.png", "2026-09-12/../../a.png"] { assert!(read(root.path(), date, path).is_err()); }
        symlink(outside.path(), root.path().join("2026/escape")).unwrap();
        symlink(root.path().join("2026").join(&a), root.path().join("2026/link.png")).unwrap();
        assert!(read(root.path(), date, "escape/a.png").is_err());
        assert!(read(root.path(), date, "link.png").is_err());
        let folder = root.path().join("2026/2026-09-12");
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o500)).unwrap();
        assert!(store(root.path(), date, &png()).is_err());
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        let next = date.succ_opt().unwrap();
        symlink(outside.path(), root.path().join(format!("2026/{next}"))).unwrap();
        assert!(store(root.path(), next, &png()).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
