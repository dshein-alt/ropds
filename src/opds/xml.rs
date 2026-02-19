use std::io::Cursor;

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::writer::Writer;

/// OPDS Atom content types.
pub const ATOM_XML: &str = "application/atom+xml; charset=utf-8";
pub const NAV_TYPE: &str = "application/atom+xml;profile=opds-catalog;kind=navigation";
pub const ACQ_TYPE: &str = "application/atom+xml;profile=opds-catalog";
pub const OPENSEARCH_TYPE: &str = "application/opensearchdescription+xml";

/// OPDS link relations.
pub const REL_ACQUISITION: &str = "http://opds-spec.org/acquisition/open-access";
pub const REL_IMAGE: &str = "http://opds-spec.org/image";
pub const REL_THUMBNAIL: &str = "http://opds-spec.org/thumbnail";

/// Book format MIME types.
pub fn mime_for_format(format: &str) -> &'static str {
    match format {
        "fb2" => "application/fb2+xml",
        "epub" => "application/epub+zip",
        "mobi" => "application/x-mobipocket-ebook",
        "pdf" => "application/pdf",
        "doc" | "docx" => "application/msword",
        "djvu" => "image/vnd.djvu",
        "txt" => "text/plain",
        "rtf" => "text/rtf",
        _ => "application/octet-stream",
    }
}

/// Formats that should NOT be offered as zipped downloads.
pub fn is_nozip_format(format: &str) -> bool {
    matches!(format, "epub" | "mobi")
}

/// MIME type for zipped book download.
pub fn mime_for_zip(format: &str) -> String {
    match format {
        "fb2" => "application/fb2+zip".to_string(),
        _ => format!("{}+zip", mime_for_format(format)),
    }
}

/// An OPDS Atom feed builder.
pub struct FeedBuilder {
    writer: Writer<Cursor<Vec<u8>>>,
}

/// A link to include in the feed or entry.
pub struct Link {
    pub href: String,
    pub rel: String,
    pub link_type: String,
    pub title: Option<String>,
}

/// An author element.
pub struct Author {
    pub name: String,
}

/// A category element.
pub struct Category {
    pub term: String,
    pub label: String,
}

impl FeedBuilder {
    pub fn new() -> Self {
        let buf = Cursor::new(Vec::new());
        let writer = Writer::new_with_indent(buf, b' ', 2);
        Self { writer }
    }

    /// Write the XML declaration and open the <feed> element with namespaces.
    pub fn begin_feed(
        &mut self,
        id: &str,
        title: &str,
        subtitle: &str,
        updated: &str,
        self_href: &str,
        start_href: &str,
    ) -> Result<(), quick_xml::Error> {
        self.writer
            .write_event(Event::Decl(BytesDecl::new("1.0", Some("utf-8"), None)))?;

        let mut feed = BytesStart::new("feed");
        feed.push_attribute(("xmlns", "http://www.w3.org/2005/Atom"));
        feed.push_attribute(("xmlns:dcterms", "http://purl.org/dc/terms"));
        feed.push_attribute(("xmlns:opds", "http://opds-spec.org/2010/catalog"));
        self.writer.write_event(Event::Start(feed))?;

        self.write_text_element("id", id)?;
        self.write_text_element("title", title)?;
        if !subtitle.is_empty() {
            self.write_text_element("subtitle", subtitle)?;
        }
        self.write_text_element("updated", updated)?;

        // Self link
        self.write_link(self_href, "self", NAV_TYPE, None)?;
        // Start link
        self.write_link(start_href, "start", NAV_TYPE, None)?;

        Ok(())
    }

    /// Write search links (OpenSearch).
    pub fn write_search_links(
        &mut self,
        search_href: &str,
        template_href: &str,
    ) -> Result<(), quick_xml::Error> {
        self.write_link(search_href, "search", OPENSEARCH_TYPE, None)?;
        self.write_link(template_href, "search", "application/atom+xml", None)?;
        Ok(())
    }

    /// Write pagination links.
    pub fn write_pagination(
        &mut self,
        prev_href: Option<&str>,
        next_href: Option<&str>,
    ) -> Result<(), quick_xml::Error> {
        if let Some(prev) = prev_href {
            self.write_link(prev, "prev", ACQ_TYPE, Some("Previous Page"))?;
        }
        if let Some(next) = next_href {
            self.write_link(next, "next", ACQ_TYPE, Some("Next Page"))?;
        }
        Ok(())
    }

    /// Begin a navigation entry (catalog, author, genre, series link).
    pub fn write_nav_entry(
        &mut self,
        id: &str,
        title: &str,
        href: &str,
        content: &str,
        updated: &str,
    ) -> Result<(), quick_xml::Error> {
        self.writer
            .write_event(Event::Start(BytesStart::new("entry")))?;
        self.write_text_element("id", id)?;
        self.write_text_element("title", title)?;
        self.write_link(href, "subsection", NAV_TYPE, None)?;
        self.write_text_element("updated", updated)?;
        if !content.is_empty() {
            self.write_content_text(content)?;
        }
        self.writer
            .write_event(Event::End(BytesEnd::new("entry")))?;
        Ok(())
    }

    /// Begin a book acquisition entry.
    pub fn begin_entry(
        &mut self,
        id: &str,
        title: &str,
        updated: &str,
    ) -> Result<(), quick_xml::Error> {
        self.writer
            .write_event(Event::Start(BytesStart::new("entry")))?;
        self.write_text_element("id", id)?;
        self.write_text_element("title", title)?;
        self.write_text_element("updated", updated)?;
        Ok(())
    }

    /// Write book acquisition links (download original, zipped, cover, thumbnail).
    pub fn write_acquisition_links(
        &mut self,
        book_id: i64,
        format: &str,
        has_cover: bool,
    ) -> Result<(), quick_xml::Error> {
        let dl_href = format!("/opds/download/{book_id}/0/");
        let mime = mime_for_format(format);

        // Original format download
        self.write_link(&dl_href, REL_ACQUISITION, mime, None)?;

        // Zipped download (if applicable)
        if !is_nozip_format(format) {
            let zip_href = format!("/opds/download/{book_id}/1/");
            let zip_mime = mime_for_zip(format);
            self.write_link(&zip_href, REL_ACQUISITION, &zip_mime, None)?;
        }

        // Cover and thumbnail
        if has_cover {
            let cover_href = format!("/opds/cover/{book_id}/");
            let thumb_href = format!("/opds/thumb/{book_id}/");
            self.write_link(&cover_href, REL_IMAGE, "image/jpeg", None)?;
            self.write_link(&thumb_href, REL_THUMBNAIL, "image/jpeg", None)?;
        }

        Ok(())
    }

    /// Write HTML content (book description).
    pub fn write_content_html(&mut self, html: &str) -> Result<(), quick_xml::Error> {
        let mut el = BytesStart::new("content");
        el.push_attribute(("type", "text/html"));
        self.writer.write_event(Event::Start(el))?;
        self.writer
            .write_event(Event::Text(BytesText::from_escaped(html)))?;
        self.writer
            .write_event(Event::End(BytesEnd::new("content")))?;
        Ok(())
    }

    /// Write plain text content.
    pub fn write_content_text(&mut self, text: &str) -> Result<(), quick_xml::Error> {
        let mut el = BytesStart::new("content");
        el.push_attribute(("type", "text"));
        self.writer.write_event(Event::Start(el))?;
        self.writer.write_event(Event::Text(BytesText::new(text)))?;
        self.writer
            .write_event(Event::End(BytesEnd::new("content")))?;
        Ok(())
    }

    /// Write an <author> element from a typed model.
    pub fn write_author_obj(&mut self, author: &Author) -> Result<(), quick_xml::Error> {
        self.writer
            .write_event(Event::Start(BytesStart::new("author")))?;
        self.write_text_element("name", &author.name)?;
        self.writer
            .write_event(Event::End(BytesEnd::new("author")))?;
        Ok(())
    }

    /// Write a <category> element from a typed model.
    pub fn write_category_obj(&mut self, category: &Category) -> Result<(), quick_xml::Error> {
        let mut el = BytesStart::new("category");
        el.push_attribute(("term", category.term.as_str()));
        el.push_attribute(("label", category.label.as_str()));
        self.writer.write_event(Event::Empty(el))?;
        Ok(())
    }

    /// End the current <entry>.
    pub fn end_entry(&mut self) -> Result<(), quick_xml::Error> {
        self.writer
            .write_event(Event::End(BytesEnd::new("entry")))?;
        Ok(())
    }

    /// Close the </feed> and return the complete XML as bytes.
    pub fn finish(mut self) -> Result<Vec<u8>, quick_xml::Error> {
        self.writer.write_event(Event::End(BytesEnd::new("feed")))?;
        Ok(self.writer.into_inner().into_inner())
    }

    /// Write a <link> element.
    pub fn write_link(
        &mut self,
        href: &str,
        rel: &str,
        link_type: &str,
        title: Option<&str>,
    ) -> Result<(), quick_xml::Error> {
        let link = Link {
            href: href.to_string(),
            rel: rel.to_string(),
            link_type: link_type.to_string(),
            title: title.map(str::to_string),
        };
        self.write_link_obj(&link)
    }

    /// Write a <link> element from a typed model.
    pub fn write_link_obj(&mut self, link: &Link) -> Result<(), quick_xml::Error> {
        let mut el = BytesStart::new("link");
        el.push_attribute(("href", link.href.as_str()));
        el.push_attribute(("rel", link.rel.as_str()));
        el.push_attribute(("type", link.link_type.as_str()));
        if let Some(t) = &link.title {
            el.push_attribute(("title", t.as_str()));
        }
        self.writer.write_event(Event::Empty(el))?;
        Ok(())
    }

    fn write_text_element(&mut self, tag: &str, text: &str) -> Result<(), quick_xml::Error> {
        self.writer
            .write_event(Event::Start(BytesStart::new(tag)))?;
        self.writer.write_event(Event::Text(BytesText::new(text)))?;
        self.writer.write_event(Event::End(BytesEnd::new(tag)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mime_helpers() {
        assert_eq!(mime_for_format("fb2"), "application/fb2+xml");
        assert_eq!(mime_for_format("unknown"), "application/octet-stream");
        assert!(is_nozip_format("epub"));
        assert!(!is_nozip_format("fb2"));
        assert_eq!(mime_for_zip("fb2"), "application/fb2+zip");
        assert_eq!(mime_for_zip("pdf"), "application/pdf+zip");
    }

    #[test]
    fn test_feed_builder_basic_feed_and_entries() {
        let mut fb = FeedBuilder::new();
        fb.begin_feed(
            "tag:test",
            "Test Feed",
            "Subtitle",
            "2024-01-01T00:00:00Z",
            "/opds/test/",
            "/opds/",
        )
        .unwrap();
        fb.write_search_links("/opds/search/", "/opds/search/{searchTerms}/")
            .unwrap();
        fb.write_nav_entry("n:1", "Node", "/opds/node/", "Desc", "2024-01-01T00:00:00Z")
            .unwrap();
        fb.write_pagination(Some("/opds/test/1/"), Some("/opds/test/3/"))
            .unwrap();
        let xml = String::from_utf8(fb.finish().unwrap()).unwrap();

        assert!(xml.contains("<feed"));
        assert!(xml.contains("Test Feed"));
        assert!(xml.contains("rel=\"self\""));
        assert!(xml.contains("rel=\"start\""));
        assert!(xml.contains("rel=\"search\""));
        assert!(xml.contains("rel=\"prev\""));
        assert!(xml.contains("rel=\"next\""));
        assert!(xml.contains("Node"));
    }

    #[test]
    fn test_feed_builder_book_entry_helpers() {
        let mut fb = FeedBuilder::new();
        fb.begin_feed(
            "tag:books",
            "Books",
            "",
            "2024-01-01T00:00:00Z",
            "/opds/",
            "/opds/",
        )
        .unwrap();
        fb.begin_entry("b:1", "Book One", "2024-01-01T00:00:00Z")
            .unwrap();
        fb.write_acquisition_links(1, "fb2", true).unwrap();
        fb.write_author_obj(&Author {
            name: "Author A".to_string(),
        })
        .unwrap();
        fb.write_category_obj(&Category {
            term: "sf".to_string(),
            label: "Sci-Fi".to_string(),
        })
        .unwrap();
        fb.write_content_html("<p>anno</p>").unwrap();
        fb.end_entry().unwrap();
        let xml = String::from_utf8(fb.finish().unwrap()).unwrap();

        assert!(xml.contains("/opds/download/1/0/"));
        assert!(xml.contains("/opds/download/1/1/"));
        assert!(xml.contains(REL_IMAGE));
        assert!(xml.contains(REL_THUMBNAIL));
        assert!(xml.contains("Author A"));
        assert!(xml.contains("term=\"sf\""));
        assert!(xml.contains("type=\"text/html\""));
        assert!(xml.contains("anno"));
    }

    #[test]
    fn test_write_acquisition_links_skips_zip_for_nozip_formats() {
        let mut fb = FeedBuilder::new();
        fb.begin_feed(
            "tag:books",
            "Books",
            "",
            "2024-01-01T00:00:00Z",
            "/opds/",
            "/opds/",
        )
        .unwrap();
        fb.begin_entry("b:2", "EPUB", "2024-01-01T00:00:00Z")
            .unwrap();
        fb.write_acquisition_links(2, "epub", false).unwrap();
        fb.end_entry().unwrap();
        let xml = String::from_utf8(fb.finish().unwrap()).unwrap();
        assert!(xml.contains("/opds/download/2/0/"));
        assert!(!xml.contains("/opds/download/2/1/"));
    }
}
