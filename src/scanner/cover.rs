use super::*;
use image::DynamicImage;
use image::GenericImageView;
use image::codecs::jpeg::JpegEncoder;
use std::io::Cursor;

pub(crate) fn normalize_cover_for_storage_with_options(
    data: &[u8],
    mime: &str,
    cover_cfg: CoverImageConfig,
) -> (Vec<u8>, String) {
    let max_dimension_px = cover_cfg.scale_to();
    let jpeg_quality = cover_cfg.jpeg_quality();

    let Ok(img) = image::load_from_memory(data) else {
        // Keep original bytes if decoder can't parse this format.
        return (data.to_vec(), normalize_mime(mime).to_string());
    };

    let (w, h) = img.dimensions();
    let is_jpeg = matches!(mime, "image/jpeg" | "image/jpg" | "image/pjpeg");
    let needs_resize = w.max(h) > max_dimension_px;
    let needs_format_conversion = !is_jpeg;

    if !needs_resize && !needs_format_conversion {
        return (data.to_vec(), normalize_mime(mime).to_string());
    }

    let processed = if needs_resize {
        img.resize(
            max_dimension_px,
            max_dimension_px,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };

    // Store decodable covers as JPEG to ensure uniform quality/size handling.
    if let Some(bytes) = encode_jpeg(&processed, jpeg_quality) {
        return (bytes, "image/jpeg".to_string());
    }

    // Fallback if encoding fails for any reason.
    (data.to_vec(), normalize_mime(mime).to_string())
}

/// Save cover image bytes to disk using hierarchical cover storage.
pub fn save_cover(
    covers_path: &Path,
    book_id: i64,
    data: &[u8],
    mime: &str,
    cover_cfg: CoverImageConfig,
) -> Result<(), std::io::Error> {
    let (normalized_data, normalized_mime) =
        normalize_cover_for_storage_with_options(data, mime, cover_cfg);
    let ext = mime_to_ext(&normalized_mime);
    let path = cover_storage_path(covers_path, book_id, ext);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, normalized_data)
}

/// Return hierarchical storage path for a cover file.
/// Layout: `{covers_dir}/{bucket_thousands}/{book_id}.{ext}`.
pub fn cover_storage_path(covers_path: &Path, book_id: i64, ext: &str) -> PathBuf {
    let id = book_id.unsigned_abs();
    let bucket_thousands = (id / 1_000) % 1_000;
    covers_path
        .join(format!("{bucket_thousands:03}"))
        .join(format!("{book_id}.{ext}"))
}

/// Return old two-level hierarchical storage path for a cover file.
/// Layout: `{covers_dir}/{bucket_millions}/{bucket_thousands}/{book_id}.{ext}`.
pub fn two_level_cover_storage_path(covers_path: &Path, book_id: i64, ext: &str) -> PathBuf {
    let id = book_id.unsigned_abs();
    let bucket_millions = (id / 1_000_000) % 1_000;
    let bucket_thousands = (id / 1_000) % 1_000;
    covers_path
        .join(format!("{bucket_millions:03}"))
        .join(format!("{bucket_thousands:03}"))
        .join(format!("{book_id}.{ext}"))
}

/// Return legacy flat storage path for a cover file.
pub fn legacy_cover_storage_path(covers_path: &Path, book_id: i64, ext: &str) -> PathBuf {
    covers_path.join(format!("{book_id}.{ext}"))
}

pub(super) fn mime_to_ext(mime: &str) -> &str {
    match mime {
        "image/png" => "png", // legacy/decode-fallback covers
        "image/gif" => "gif", // legacy/decode-fallback covers
        _ => "jpg",
    }
}

fn normalize_mime(mime: &str) -> &str {
    match mime {
        "image/png" => "image/png",
        "image/gif" => "image/gif",
        _ => "image/jpeg",
    }
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
    encoder.encode_image(img).ok()?;
    Some(out.into_inner())
}

/// Remove cover file for a book (tries all known extensions and layouts).
pub(super) fn delete_cover(covers_path: &Path, book_id: i64) {
    for ext in &["jpg", "png", "gif"] {
        for path in [
            cover_storage_path(covers_path, book_id, ext),
            two_level_cover_storage_path(covers_path, book_id, ext),
            legacy_cover_storage_path(covers_path, book_id, ext),
        ] {
            if path.exists() {
                match fs::remove_file(&path) {
                    Ok(()) => remove_empty_cover_dirs(covers_path, &path),
                    Err(e) => warn!("Failed to remove cover {}: {e}", path.display()),
                }
            }
        }
    }
}

fn remove_empty_cover_dirs(covers_path: &Path, file_path: &Path) {
    let Some(dir) = file_path.parent() else {
        return;
    };
    if dir == covers_path {
        return;
    }
    let _ = fs::remove_dir(dir);
    if let Some(parent) = dir.parent()
        && parent != covers_path
    {
        let _ = fs::remove_dir(parent);
    }
}
