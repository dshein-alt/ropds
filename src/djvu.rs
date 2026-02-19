use std::io::Cursor;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_SCALE_TO: u32 = 600;
const DEFAULT_JPEG_QUALITY: u8 = 85;

pub fn ddjvu_available() -> bool {
    Command::new("ddjvu")
        .arg("-h")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

pub fn render_first_page_jpeg_from_path(path: &Path) -> Result<Vec<u8>, DjvuRenderError> {
    let djvu_data = std::fs::read(path).map_err(DjvuRenderError::ReadInput)?;
    render_first_page_jpeg_from_bytes(&djvu_data)
}

pub fn render_first_page_jpeg_from_bytes(djvu_data: &[u8]) -> Result<Vec<u8>, DjvuRenderError> {
    let temp_dir = temp_work_dir();
    std::fs::create_dir_all(&temp_dir).map_err(DjvuRenderError::CreateTempDir)?;
    let _cleanup = TempDirCleanup(temp_dir.clone());

    let input_djvu = temp_dir.join("input.djvu");
    std::fs::write(&input_djvu, djvu_data).map_err(DjvuRenderError::WriteInput)?;

    let output = Command::new("ddjvu")
        .arg("-page=1")
        .arg(format!("-size={}x{}", DEFAULT_SCALE_TO, DEFAULT_SCALE_TO))
        .arg("-format=ppm")
        .arg(&input_djvu)
        .arg("-")
        .output()
        .map_err(DjvuRenderError::Spawn)?;

    if !output.status.success() {
        return Err(DjvuRenderError::ExitStatus(output.status.code()));
    }

    let image = image::load_from_memory_with_format(&output.stdout, image::ImageFormat::Pnm)
        .map_err(DjvuRenderError::DecodeOutput)?;

    let mut jpeg = Cursor::new(Vec::new());
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, DEFAULT_JPEG_QUALITY);
    encoder
        .encode_image(&image)
        .map_err(DjvuRenderError::EncodeJpeg)?;

    Ok(jpeg.into_inner())
}

fn temp_work_dir() -> std::path::PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("ropds-djvuthumb-{}-{now}", std::process::id()))
}

struct TempDirCleanup(std::path::PathBuf);

impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            tracing::debug!("Failed to cleanup temp DJVU dir {:?}: {}", self.0, e);
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DjvuRenderError {
    #[error("failed to read input DJVU: {0}")]
    ReadInput(std::io::Error),
    #[error("failed to create temp dir: {0}")]
    CreateTempDir(std::io::Error),
    #[error("failed to write temp input DJVU: {0}")]
    WriteInput(std::io::Error),
    #[error("failed to start ddjvu: {0}")]
    Spawn(std::io::Error),
    #[error("ddjvu exited with status {0:?}")]
    ExitStatus(Option<i32>),
    #[error("failed to decode ddjvu output: {0}")]
    DecodeOutput(image::ImageError),
    #[error("failed to encode JPEG: {0}")]
    EncodeJpeg(image::ImageError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temp_work_dir_shape() {
        let p1 = temp_work_dir();
        let p2 = temp_work_dir();
        assert_ne!(p1, p2);
        let s1 = p1.to_string_lossy();
        assert!(s1.contains("ropds-djvuthumb-"));
    }

    #[test]
    fn test_render_first_page_from_missing_path() {
        let err = render_first_page_jpeg_from_path(Path::new("/definitely/missing/file.djvu"))
            .unwrap_err();
        assert!(matches!(err, DjvuRenderError::ReadInput(_)));
    }

    #[test]
    fn test_render_first_page_from_invalid_bytes_errors() {
        let err = render_first_page_jpeg_from_bytes(b"not a djvu").unwrap_err();
        assert!(matches!(
            err,
            DjvuRenderError::Spawn(_)
                | DjvuRenderError::ExitStatus(_)
                | DjvuRenderError::DecodeOutput(_)
        ));
    }
}
