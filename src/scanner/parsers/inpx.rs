use std::io::{BufRead, BufReader, Read, Seek};
use std::path::Path;

use super::{BookMeta, strip_meta};

const INPX_SEPARATOR: u8 = 0x04;

#[derive(Debug, Clone)]
struct InpxFieldIndex {
    author: usize,
    genre: usize,
    title: usize,
    series: usize,
    ser_no: usize,
    file: usize,
    size: usize,
    del: Option<usize>,
    ext: usize,
    date: usize,
    lang: usize,
    min_fields: usize,
}

impl InpxFieldIndex {
    fn default_layout() -> Self {
        Self {
            author: 0,
            genre: 1,
            title: 2,
            series: 3,
            ser_no: 4,
            file: 5,
            size: 6,
            del: Some(8),
            ext: 9,
            date: 10,
            lang: 11,
            min_fields: 12,
        }
    }
}

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
    let field_index =
        parse_structure_info(&mut archive).unwrap_or_else(InpxFieldIndex::default_layout);

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
        parse_inp(reader, &folder, &field_index, &mut records);
    }

    Ok(records)
}

/// Parse a single .inp text file, appending records to `out`.
fn parse_inp(reader: impl BufRead, folder: &str, idx: &InpxFieldIndex, out: &mut Vec<InpxRecord>) {
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
        if fields.len() < idx.min_fields {
            continue;
        }

        // Skip deleted records
        if let Some(del_idx) = idx.del {
            let del = fields[del_idx].trim();
            if !del.is_empty() && del != "0" {
                continue;
            }
        }

        let file_stem = fields[idx.file].trim();
        let ext = fields[idx.ext].trim();
        let filename = format!("{file_stem}.{ext}");
        let format = ext.to_lowercase();

        let title = strip_meta(fields[idx.title]);
        let lang = strip_meta(fields[idx.lang]);
        let docdate = strip_meta(fields[idx.date]);

        // Authors: colon-separated, commas replaced with spaces
        let authors: Vec<String> = fields[idx.author]
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
        let genres: Vec<String> = fields[idx.genre]
            .split(':')
            .map(|g| strip_meta(g).to_lowercase())
            .filter(|g| !g.is_empty())
            .collect();

        // Series: first item from colon-separated list
        let series_title = fields[idx.series]
            .split(':')
            .next()
            .map(strip_meta)
            .filter(|s| !s.is_empty());

        let series_index = fields[idx.ser_no].trim().parse::<i32>().unwrap_or(0);

        let size = fields[idx.size].trim().parse::<i64>().unwrap_or(0);

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

/// Parse INPX `structure.info` and build field index mapping.
/// Falls back to default layout if missing, malformed, or incomplete.
fn parse_structure_info<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<InpxFieldIndex> {
    let mut structure_entry_name: Option<String> = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).ok()?;
        let name = entry.name().to_string();
        if name.eq_ignore_ascii_case("structure.info")
            || name
                .rsplit('/')
                .next()
                .is_some_and(|n| n.eq_ignore_ascii_case("structure.info"))
        {
            structure_entry_name = Some(name);
            break;
        }
    }
    let structure_entry_name = structure_entry_name?;
    let entry = archive.by_name(&structure_entry_name).ok()?;
    let mut reader = BufReader::new(entry);
    let mut first_line = String::new();
    if reader.read_line(&mut first_line).ok()? == 0 {
        return None;
    }

    let columns: Vec<String> = first_line
        .trim()
        .split(';')
        .map(|v| v.trim().to_uppercase())
        .filter(|v| !v.is_empty())
        .collect();
    if columns.is_empty() {
        return None;
    }

    let find_idx = |name: &str| columns.iter().position(|c| c == name);
    let author = find_idx("AUTHOR")?;
    let genre = find_idx("GENRE")?;
    let title = find_idx("TITLE")?;
    let series = find_idx("SERIES")?;
    let ser_no = find_idx("SERNO")?;
    let file = find_idx("FILE")?;
    let size = find_idx("SIZE")?;
    let ext = find_idx("EXT")?;
    let date = find_idx("DATE")?;
    let lang = find_idx("LANG")?;
    let del = find_idx("DEL");
    let min_fields = columns.len();

    Some(InpxFieldIndex {
        author,
        genre,
        title,
        series,
        ser_no,
        file,
        size,
        del,
        ext,
        date,
        lang,
        min_fields,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum InpxError {
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_inpx_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, content) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn inpx_line(
        author: &str,
        genre: &str,
        title: &str,
        series: &str,
        ser_no: &str,
        file: &str,
        size: &str,
        del: &str,
        ext: &str,
        date: &str,
        lang: &str,
    ) -> String {
        let sep = INPX_SEPARATOR as char;
        format!(
            "{author}{sep}{genre}{sep}{title}{sep}{series}{sep}{ser_no}{sep}{file}{sep}{size}{sep}lib{sep}{del}{sep}{ext}{sep}{date}{sep}{lang}"
        )
    }

    #[test]
    fn test_parse_inpx_archive_records() {
        let good = inpx_line(
            "Asimov,Isaac:Clarke,Arthur",
            "sf:space_opera",
            "Foundation",
            "Series:Ignored",
            "2",
            "foundation",
            "12345",
            "0",
            "fb2",
            "1951",
            "en",
        );
        let deleted = inpx_line(
            "Nobody", "sf", "Deleted", "", "0", "deleted", "1", "1", "fb2", "", "en",
        );
        let zip_data = make_inpx_zip(&[(
            "pack-0001.inp",
            &format!(
                "{good}\n{deleted}\nshort{sep}\n",
                sep = INPX_SEPARATOR as char
            ),
        )]);

        let records = parse(std::io::Cursor::new(zip_data)).unwrap();
        assert_eq!(records.len(), 1);

        let r = &records[0];
        assert_eq!(r.filename, "foundation.fb2");
        assert_eq!(r.folder, "pack-0001.zip");
        assert_eq!(r.format, "fb2");
        assert_eq!(r.size, 12345);
        assert_eq!(r.meta.title, "Foundation");
        assert_eq!(
            r.meta.authors,
            vec!["Asimov Isaac".to_string(), "Clarke Arthur".to_string()]
        );
        assert_eq!(
            r.meta.genres,
            vec!["sf".to_string(), "space_opera".to_string()]
        );
        assert_eq!(r.meta.series_title, Some("Series".to_string()));
        assert_eq!(r.meta.series_index, 2);
        assert_eq!(r.meta.docdate, "1951");
        assert_eq!(r.meta.lang, "en");
    }

    #[test]
    fn test_parse_inpx_archive_with_structure_info_mapping() {
        let sep = INPX_SEPARATOR as char;
        let structure = "FILE;EXT;AUTHOR;GENRE;TITLE;SERIES;SERNO;SIZE;DEL;DATE;LANG";
        let line = format!(
            "book{sep}fb2{sep}Asimov,Isaac{sep}sf{sep}Foundation{sep}Series{sep}2{sep}123{sep}0{sep}1951{sep}en"
        );
        let zip_data = make_inpx_zip(&[
            ("structure.info", structure),
            ("pack-0001.inp", &format!("{line}\n")),
        ]);

        let records = parse(std::io::Cursor::new(zip_data)).unwrap();
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.filename, "book.fb2");
        assert_eq!(r.folder, "pack-0001.zip");
        assert_eq!(r.meta.title, "Foundation");
        assert_eq!(r.meta.authors, vec!["Asimov Isaac".to_string()]);
        assert_eq!(r.meta.genres, vec!["sf".to_string()]);
        assert_eq!(r.meta.series_title, Some("Series".to_string()));
        assert_eq!(r.meta.series_index, 2);
        assert_eq!(r.size, 123);
    }

    #[test]
    fn test_default_folder() {
        assert_eq!(default_folder("fb2-000001.inp"), "fb2-000001.zip");
        assert_eq!(default_folder("nested/file.inp"), "file.zip");
    }

    #[test]
    fn test_parse_invalid_zip_error() {
        let err = parse(std::io::Cursor::new(b"not-a-zip".to_vec())).unwrap_err();
        assert!(matches!(err, InpxError::Zip(_)));
    }
}
