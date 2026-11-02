//! ZIP entry name decoding.
//!
//! Archives produced by DOS/Windows tools store entry names in an OEM codepage
//! without setting the UTF-8 flag (general purpose bit 11). APPNOTE declares
//! such names to be CP437, which is what the `zip` crate decodes them as — so
//! a Cyrillic name written in CP866 comes back as mojibake. `library.zip_codepage`
//! names the codepage to use instead.

use std::io::{Read, Seek};
use std::sync::OnceLock;

use encoding_rs::Encoding;
use zip::ZipArchive;

/// Resolve `library.zip_codepage` to an encoding.
///
/// Returns `None` when the `zip` crate's own decoding should stand: CP437
/// itself (which the crate already applies, and for which `encoding_rs` has no
/// label), or an unrecognised label.
pub fn resolve(codepage: &str) -> Option<&'static Encoding> {
    let label = codepage.trim();
    let is_cp437 = label.eq_ignore_ascii_case("cp437") || label.eq_ignore_ascii_case("ibm437");
    if label.is_empty() || is_cp437 {
        return None;
    }
    let encoding = Encoding::for_label(label.as_bytes());
    if encoding.is_none() {
        static WARNED: OnceLock<()> = OnceLock::new();
        WARNED.get_or_init(|| {
            tracing::warn!(
                "Unknown library.zip_codepage '{label}'; ZIP entry names will be read as CP437"
            );
        });
    }
    encoding
}

/// Decode a raw ZIP entry name with `encoding` (see [`resolve`]).
///
/// Returns `None` when the `zip` crate's own decoding already applies: the
/// entry is UTF-8 flagged, the name is pure ASCII (where every supported
/// codepage agrees with CP437), or no encoding was resolved.
pub fn decode(raw: &[u8], is_utf8: bool, encoding: Option<&'static Encoding>) -> Option<String> {
    if is_utf8 || raw.is_ascii() {
        return None;
    }
    let (decoded, had_errors) = encoding?.decode_without_bom_handling(raw);
    if had_errors {
        return None;
    }
    Some(decoded.into_owned())
}

/// Decode the name of an entry, falling back to the `zip` crate's decoding.
pub fn entry_name<R: Read>(
    entry: &zip::read::ZipFile<'_, R>,
    encoding: Option<&'static Encoding>,
) -> zip::result::ZipResult<String> {
    let is_utf8 = entry.flags().as_u16() & (1 << 11) != 0;
    if let Some(name) = decode(entry.name_raw(), is_utf8, encoding) {
        return Ok(name);
    }
    entry.name().map(|name| name.into_owned())
}

/// The last path component of a ZIP entry name.
fn basename(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// True when `name` identifies the entry the database refers to as `wanted`.
///
/// The scanner stores an entry's basename, so a nested entry matches on its
/// last path component as well as on its full name.
fn matches(name: &str, wanted: &str) -> bool {
    name == wanted || basename(name) == wanted
}

/// CP437 characters for bytes 0x80..=0xFF, in order. The `zip` crate decodes
/// unflagged names with this table but does not expose it.
const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅ\
    ÉæÆôöòûùÿÖÜ¢£¥₧ƒ\
    áíóúñÑªº¿⌐¬½¼¡«»\
    ░▒▓│┤╡╢╖╕╣║╗╝╜╛┐\
    └┴┬├─┼╞╟╚╔╩╦╠═╬╧\
    ╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀\
    αßΓπΣσµτΦΘΩδ∞φε∩\
    ≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

fn cp437_decode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x00..=0x7f => b as char,
            _ => CP437_HIGH.chars().nth((b - 0x80) as usize).unwrap(),
        })
        .collect()
}

/// The name the `zip` crate shows for an entry whose raw name bytes are
/// `wanted` encoded in `enc`: the CP437 decoding of those bytes.
///
/// Relies on `enc.encode` inverting the scan-time decode, which holds for
/// the error-free single-byte encodings this setting targets.
pub fn mangle(wanted: &str, enc: &'static Encoding) -> Option<String> {
    let (bytes, _, had_errors) = enc.encode(wanted);
    if had_errors {
        return None;
    }
    Some(cp437_decode(&bytes))
}

/// Locate the entry matching the filename stored in the database.
///
/// `ZipArchive::by_name` cannot be used here: it keys on the *raw* name bytes,
/// so a name that had to be decoded never matches. This only walks the
/// in-memory central directory — no per-entry IO.
pub fn find_entry_index<R: Read + Seek>(
    archive: &ZipArchive<R>,
    wanted: &str,
    encoding: Option<&'static Encoding>,
) -> Option<usize> {
    // Fast path: raw bytes are the name, i.e. a genuinely UTF-8 archive or a
    // name recorded before codepage support.
    if let Some(index) = archive.index_for_name(wanted) {
        return Some(index);
    }
    let mangled = encoding.and_then(|enc| mangle(wanted, enc));
    archive
        .file_names()
        .enumerate()
        .find(|(_, name)| {
            name.as_ref().is_ok_and(|name| {
                matches(name, wanted) || mangled.as_deref().is_some_and(|m| matches(name, m))
            })
        })
        .map(|(index, _)| index)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// `Стругацкий - Страна багровых туч.fb2` as stored by a DOS/Windows
    /// archiver in CP866.
    const CP866_NAME: &[u8] = &[
        0x91, 0xe2, 0xe0, 0xe3, 0xa3, 0xa0, 0xe6, 0xaa, 0xa8, 0xa9, 0x20, 0x2d, 0x20, 0x91, 0xe2,
        0xe0, 0xa0, 0xad, 0xa0, 0x20, 0xa1, 0xa0, 0xa3, 0xe0, 0xae, 0xa2, 0xeb, 0xe5, 0x20, 0xe2,
        0xe3, 0xe7, 0x2e, 0x66, 0x62, 0x32,
    ];
    /// The decoded name those bytes stand for.
    const UTF8_NAME: &str = "Стругацкий - Страна багровых туч.fb2";

    #[test]
    fn decodes_cp866_entry_name() {
        assert_eq!(
            decode(CP866_NAME, false, resolve("cp866")).as_deref(),
            Some(UTF8_NAME)
        );
    }

    #[test]
    fn keeps_crate_decoding_for_utf8_and_ascii_names() {
        // UTF-8 flagged names are already correct.
        assert_eq!(decode(UTF8_NAME.as_bytes(), true, resolve("cp866")), None);
        // ASCII agrees across codepages.
        assert_eq!(decode(b"book.fb2", false, resolve("cp866")), None);
    }

    #[test]
    fn keeps_crate_decoding_for_cp437_and_unknown_labels() {
        assert_eq!(decode(CP866_NAME, false, resolve("cp437")), None);
        assert_eq!(decode(CP866_NAME, false, resolve("not-a-codepage")), None);
        assert_eq!(decode(CP866_NAME, false, resolve("")), None);
    }

    #[test]
    fn decodes_other_common_codepages() {
        // windows-1251 "Страна.fb2"
        let cp1251 = &[0xd1, 0xf2, 0xf0, 0xe0, 0xed, 0xe0, 0x2e, 0x66, 0x62, 0x32];
        assert_eq!(
            decode(cp1251, false, resolve("windows-1251")).as_deref(),
            Some("Страна.fb2")
        );
        assert_eq!(
            decode(cp1251, false, resolve("cp1251")).as_deref(),
            Some("Страна.fb2")
        );
    }

    #[test]
    fn cp437_table_matches_crate_decoding() {
        // The mangled form compared against `file_names()` must be exactly
        // what the crate decoded the raw bytes to.
        let raw = zip_with_raw_name(CP866_NAME);
        let archive = ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
        let crate_name = archive.file_names().next().unwrap().unwrap();
        assert_eq!(cp437_decode(CP866_NAME), crate_name.as_ref());
        assert_eq!(
            mangle(UTF8_NAME, resolve("cp866").unwrap()).as_deref(),
            Some(crate_name.as_ref())
        );
    }

    /// Build an in-memory ZIP with one entry whose name is raw `bytes`.
    pub(crate) fn zip_with_raw_name(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        // The `zip` writer always encodes names as UTF-8, so craft the archive
        // by hand to get a non-UTF-8 name with the language-encoding flag clear.
        let mut buf = Vec::new();
        let data = b"content";
        let crc = crc32fast::hash(data);
        let name_len = bytes.len() as u16;

        // Local file header
        buf.write_all(&0x0403_4b50u32.to_le_bytes()).unwrap();
        buf.write_all(&[10, 0, 0, 0, 0, 0]).unwrap(); // version, flags (bit 11 clear), method=stored
        buf.write_all(&[0, 0, 0, 0]).unwrap(); // mod time/date
        buf.write_all(&crc.to_le_bytes()).unwrap();
        buf.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        buf.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        buf.write_all(&name_len.to_le_bytes()).unwrap();
        buf.write_all(&0u16.to_le_bytes()).unwrap(); // extra len
        buf.write_all(bytes).unwrap();
        buf.write_all(data).unwrap();

        // Central directory
        let cd_offset = buf.len() as u32;
        buf.write_all(&0x0201_4b50u32.to_le_bytes()).unwrap();
        buf.write_all(&[10, 0, 10, 0, 0, 0, 0, 0]).unwrap(); // versions, flags, method
        buf.write_all(&[0, 0, 0, 0]).unwrap(); // mod time/date
        buf.write_all(&crc.to_le_bytes()).unwrap();
        buf.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        buf.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        buf.write_all(&name_len.to_le_bytes()).unwrap();
        buf.write_all(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            .unwrap(); // extra/comment/disk/attrs
        buf.write_all(&0u32.to_le_bytes()).unwrap(); // local header offset
        buf.write_all(bytes).unwrap();
        let cd_size = buf.len() as u32 - cd_offset;

        // End of central directory
        buf.write_all(&0x0605_4b50u32.to_le_bytes()).unwrap();
        buf.write_all(&[0, 0, 0, 0]).unwrap(); // disk numbers
        buf.write_all(&1u16.to_le_bytes()).unwrap();
        buf.write_all(&1u16.to_le_bytes()).unwrap();
        buf.write_all(&cd_size.to_le_bytes()).unwrap();
        buf.write_all(&cd_offset.to_le_bytes()).unwrap();
        buf.write_all(&0u16.to_le_bytes()).unwrap(); // comment len
        buf
    }

    #[test]
    fn finds_entry_by_decoded_name() {
        let raw = zip_with_raw_name(CP866_NAME);
        let archive = ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
        assert_eq!(
            find_entry_index(&archive, UTF8_NAME, resolve("cp866")),
            Some(0)
        );
    }

    #[test]
    fn finds_nested_entry_by_basename() {
        let mut nested = b"books/".to_vec();
        nested.extend_from_slice(CP866_NAME);
        let raw = zip_with_raw_name(&nested);
        let archive = ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
        assert_eq!(
            find_entry_index(&archive, UTF8_NAME, resolve("cp866")),
            Some(0)
        );
    }

    #[test]
    fn missing_entry_returns_none() {
        let raw = zip_with_raw_name(CP866_NAME);
        let archive = ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
        assert_eq!(
            find_entry_index(&archive, "absent.fb2", resolve("cp866")),
            None
        );
    }

    #[test]
    fn finds_entry_in_utf8_archive() {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file("nested/книга.fb2", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"content").unwrap();
        let raw = writer.finish().unwrap().into_inner();
        let archive = ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
        assert_eq!(
            find_entry_index(&archive, "книга.fb2", resolve("cp866")),
            Some(0)
        );
    }
}
