use std::{fs, io::{Cursor, Write}, path::Path};

pub fn validate(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > 16 * 1024 * 1024 || image::guess_format(bytes).ok() != Some(image::ImageFormat::Png) { return Err("Expected a pressed-card PNG.".into()); }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1200); limits.max_image_height = Some(2000); limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| format!("Cannot read the pressed image: {e}"))?;
    if image.width() != 1200 || image.height() != 2000 { return Err("Pressed cards must be 1200 × 2000 pixels.".into()); }
    Ok(())
}

pub fn save(bytes: &[u8], destination: Option<&Path>) -> Result<bool, String> {
    let Some(path) = destination else { return Ok(false); };
    validate(bytes)?;
    let parent = path.parent().ok_or("Choose a folder for the image.")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary.write_all(bytes).and_then(|_| temporary.as_file().sync_all()).map_err(|e| e.to_string())?;
    temporary.persist(path).map_err(|e| e.to_string())?;
    fs::File::open(parent).and_then(|file| file.sync_all()).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_valid_card_pngs_reach_the_chosen_destination_and_cancel_writes_nothing() {
        let folder = tempfile::tempdir().unwrap(); let path = folder.path().join("card.png");
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1200, 2000).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let bytes = png.into_inner();
        assert!(!save(&bytes, None).unwrap());
        assert!(fs::read_dir(folder.path()).unwrap().next().is_none());
        assert!(save(&bytes, Some(&path)).unwrap());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(save(b"not a PNG", Some(&path)).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(save(&bytes, Some(&folder.path().join("missing/card.png"))).is_err());
        let mut small = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(12, 20).write_to(&mut small, image::ImageFormat::Png).unwrap();
        assert!(validate(small.get_ref()).is_err());
    }
}
