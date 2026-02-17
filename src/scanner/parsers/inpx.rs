use std::io::{BufRead, BufReader, Read, Seek};
use std::path::Path;

use super::{BookMeta, strip_meta};

const INPX_SEPARATOR: u8 = 0x04;

/// Field indices in the default INP format.
const I_AUTHOR: usize = 0;
const I_GENRE: usize = 1;
const I_TITLE: usize = 2;
const I_SERIES: usize = 3;
const I_SERNO: usize = 4;
const I_FILE: usize = 5;
const I_SIZE: usize = 6;
// const I_LIBID: usize = 7;
const I_DEL: usize = 8;
const I_EXT: usize = 9;
const I_DATE: usize = 10;
const I_LANG: usize = 11;
const MIN_FIELDS: usize = 12;

/// A single book record parsed from an INPX line.
#[derive(Debug, Clone)]
pub struct InpxRecord {
    pub filename: String,
    pub folder: String,
    pub format: String,
    pub size: i64,
    pub meta: BookMeta,
}

/// Parse all book records from an INPX archive.
/// `inpx_reader` should be a seekable reader over the .inpx ZIP file.
pub fn parse<R: Read + Seek>(inpx_reader: R) -> Result<Vec<InpxRecord>, InpxError> {
    let mut archive = zip::ZipArchive::new(inpx_reader)?;
    let mut records = Vec::new();

    // Collect .inp entry names first (borrow issue with ZipArchive)
    let inp_names: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_string();
            if name.ends_with(".inp") {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    for inp_name in &inp_names {
        let folder = default_folder(inp_name);
        let entry = archive.by_name(inp_name)?;
        let reader = BufReader::new(entry);
        parse_inp(reader, &folder, &mut records);
    }

    Ok(records)
}

/// Parse a single .inp text file, appending records to `out`.
fn parse_inp(reader: impl BufRead, folder: &str, out: &mut Vec<InpxRecord>) {
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line.split(INPX_SEPARATOR as char).collect();
        if fields.len() < MIN_FIELDS {
            continue;
        }

        // Skip deleted records
        let del = fields[I_DEL].trim();
        if !del.is_empty() && del != "0" {
            continue;
        }

        let file_stem = fields[I_FILE].trim();
        let ext = fields[I_EXT].trim();
        let filename = format!("{file_stem}.{ext}");
        let format = ext.to_lowercase();

        let title = strip_meta(fields[I_TITLE]);
        let lang = strip_meta(fields[I_LANG]);
        let docdate = strip_meta(fields[I_DATE]);

        // Authors: colon-separated, commas replaced with spaces
        let authors: Vec<String> = fields[I_AUTHOR]
            .split(':')
            .map(|a| {
                a.replace(',', " ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|a| !a.is_empty())
            .collect();

        // Genres: colon-separated, lowercased
        let genres: Vec<String> = fields[I_GENRE]
            .split(':')
            .map(|g| strip_meta(g).to_lowercase())
            .filter(|g| !g.is_empty())
            .collect();

        // Series: first item from colon-separated list
        let series_title = fields[I_SERIES]
            .split(':')
            .next()
            .map(|s| strip_meta(s))
            .filter(|s| !s.is_empty());

        let series_index = fields[I_SERNO].trim().parse::<i32>().unwrap_or(0);

        let size = fields[I_SIZE].trim().parse::<i64>().unwrap_or(0);

        let meta = BookMeta {
            title,
            authors,
            genres,
            lang,
            docdate,
            series_title,
            series_index,
            annotation: String::new(),
            cover_data: None,
            cover_type: String::new(),
        };

        out.push(InpxRecord {
            filename,
            folder: folder.to_string(),
            format,
            size,
            meta,
        });
    }
}

/// Default folder for an .inp entry: strip the .inp extension, append .zip.
/// e.g. "fb2-000001-000500.inp" → "fb2-000001-000500.zip"
fn default_folder(inp_name: &str) -> String {
    let stem = Path::new(inp_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(inp_name);
    format!("{stem}.zip")
}

#[derive(Debug, thiserror::Error)]
pub enum InpxError {
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
