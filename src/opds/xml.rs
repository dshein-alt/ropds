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
    pub fn begin_entry(&mut self, id: &str, title: &str, updated: &str) -> Result<(), quick_xml::Error> {
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
        self.writer
            .write_event(Event::Text(BytesText::new(text)))?;
        self.writer
            .write_event(Event::End(BytesEnd::new("content")))?;
        Ok(())
    }

    /// Write an <author> element.
    pub fn write_author(&mut self, name: &str) -> Result<(), quick_xml::Error> {
        self.writer
            .write_event(Event::Start(BytesStart::new("author")))?;
        self.write_text_element("name", name)?;
        self.writer
            .write_event(Event::End(BytesEnd::new("author")))?;
        Ok(())
    }

    /// Write a <category> element.
    pub fn write_category(&mut self, term: &str, label: &str) -> Result<(), quick_xml::Error> {
        let mut el = BytesStart::new("category");
        el.push_attribute(("term", term));
        el.push_attribute(("label", label));
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
        self.writer
            .write_event(Event::End(BytesEnd::new("feed")))?;
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
        let mut el = BytesStart::new("link");
        el.push_attribute(("href", href));
        el.push_attribute(("rel", rel));
        el.push_attribute(("type", link_type));
        if let Some(t) = title {
            el.push_attribute(("title", t));
        }
        self.writer.write_event(Event::Empty(el))?;
        Ok(())
    }

    fn write_text_element(&mut self, tag: &str, text: &str) -> Result<(), quick_xml::Error> {
        self.writer
            .write_event(Event::Start(BytesStart::new(tag)))?;
        self.writer
            .write_event(Event::Text(BytesText::new(text)))?;
        self.writer
            .write_event(Event::End(BytesEnd::new(tag)))?;
        Ok(())
    }
}
