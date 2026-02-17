use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_SCALE_TO: u32 = 600;
const DEFAULT_JPEG_QUALITY: u8 = 85;

pub fn pdftoppm_available() -> bool {
    Command::new("pdftoppm")
        .arg("-h")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

pub fn pdfinfo_available() -> bool {
    Command::new("pdfinfo")
        .arg("-h")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

#[derive(Debug, Clone, Default)]
pub struct PdfMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
}

pub fn render_first_page_jpeg_from_path(path: &Path) -> Result<Vec<u8>, PdfRenderError> {
    let pdf_data = std::fs::read(path).map_err(PdfRenderError::ReadInput)?;
    render_first_page_jpeg_from_bytes(&pdf_data)
}

pub fn render_first_page_jpeg_from_bytes(pdf_data: &[u8]) -> Result<Vec<u8>, PdfRenderError> {
    let temp_dir = temp_work_dir();
    std::fs::create_dir_all(&temp_dir).map_err(PdfRenderError::CreateTempDir)?;
    let _cleanup = TempDirCleanup(temp_dir.clone());

    let input_pdf = temp_dir.join("input.pdf");
    let output_base = temp_dir.join("page");
    let output_jpg = temp_dir.join("page.jpg");

    std::fs::write(&input_pdf, pdf_data).map_err(PdfRenderError::WriteInput)?;

    let jpegopt = format!("quality={DEFAULT_JPEG_QUALITY}");
    let status = Command::new("pdftoppm")
        .arg("-f")
        .arg("1")
        .arg("-singlefile")
        .arg("-jpeg")
        .arg("-jpegopt")
        .arg(jpegopt)
        .arg("-scale-to")
        .arg(DEFAULT_SCALE_TO.to_string())
        .arg(&input_pdf)
        .arg(&output_base)
        .status()
        .map_err(PdfRenderError::Spawn)?;

    if !status.success() {
        return Err(PdfRenderError::ExitStatus(status.code()));
    }

    std::fs::read(&output_jpg).map_err(PdfRenderError::ReadOutput)
}

pub fn extract_metadata_from_path(path: &Path) -> Result<PdfMetadata, PdfInfoError> {
    let output = Command::new("pdfinfo")
        .arg(path)
        .output()
        .map_err(PdfInfoError::Spawn)?;

    if !output.status.success() {
        return Err(PdfInfoError::ExitStatus(output.status.code()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_pdfinfo_stdout(&stdout))
}

pub fn extract_metadata_from_bytes(pdf_data: &[u8]) -> Result<PdfMetadata, PdfInfoError> {
    let temp_dir = temp_work_dir();
    std::fs::create_dir_all(&temp_dir).map_err(PdfInfoError::CreateTempDir)?;
    let _cleanup = TempDirCleanup(temp_dir.clone());

    let input_pdf = temp_dir.join("input.pdf");
    std::fs::write(&input_pdf, pdf_data).map_err(PdfInfoError::WriteInput)?;

    extract_metadata_from_path(&input_pdf)
}

fn parse_pdfinfo_stdout(stdout: &str) -> PdfMetadata {
    let mut meta = PdfMetadata::default();

    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            let value = normalize_pdfinfo_value(value);
            match key.as_str() {
                "title" => meta.title = value,
                "author" => meta.author = value,
                _ => {}
            }
        }
    }

    meta
}

fn normalize_pdfinfo_value(value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("(null)") {
        None
    } else {
        Some(v.to_string())
    }
}

fn temp_work_dir() -> std::path::PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("ropds-pdfthumb-{}-{now}", std::process::id()))
}

struct TempDirCleanup(std::path::PathBuf);

impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            tracing::debug!("Failed to cleanup temp PDF dir {:?}: {}", self.0, e);
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PdfRenderError {
    #[error("failed to read input PDF: {0}")]
    ReadInput(std::io::Error),
    #[error("failed to create temp dir: {0}")]
    CreateTempDir(std::io::Error),
    #[error("failed to write temp input PDF: {0}")]
    WriteInput(std::io::Error),
    #[error("failed to start pdftoppm: {0}")]
    Spawn(std::io::Error),
    #[error("pdftoppm exited with status {0:?}")]
    ExitStatus(Option<i32>),
    #[error("failed to read rendered JPEG: {0}")]
    ReadOutput(std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum PdfInfoError {
    #[error("failed to create temp dir: {0}")]
    CreateTempDir(std::io::Error),
    #[error("failed to write temp input PDF: {0}")]
    WriteInput(std::io::Error),
    #[error("failed to start pdfinfo: {0}")]
    Spawn(std::io::Error),
    #[error("pdfinfo exited with status {0:?}")]
    ExitStatus(Option<i32>),
}
