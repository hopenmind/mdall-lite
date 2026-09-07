//! Read-only extraction of Word review data (tracked changes + comments) from a
//! DOCX, for the supervisor-feedback workflow: a researcher exports a DOCX, a
//! supervisor annotates it in Word (insertions, deletions, margin comments), and
//! re-imports it here to READ the feedback in-app instead of opening Word.
//!
//! This is purely additive and never touches `source_embed.rs` or the DOCX wire
//! format - it only parses `word/document.xml` and `word/comments.xml`. Our own
//! equation-recovery comments (authored `MD-TO-ALL`, body `LaTeX: ...`) are filtered
//! out so only genuine human feedback surfaces.

use std::path::Path;

/// The kind of review annotation a Word reviewer left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewKind {
    /// Tracked insertion (`<w:ins>`): text the reviewer added.
    Insertion,
    /// Tracked deletion (`<w:del>`): text the reviewer removed.
    Deletion,
    /// Margin comment (`word/comments.xml`).
    Comment,
}

impl ReviewKind {
    pub fn label(self) -> &'static str {
        match self {
            ReviewKind::Insertion => "Insertion",
            ReviewKind::Deletion => "Deletion",
            ReviewKind::Comment => "Comment",
        }
    }
}

/// One reviewer annotation recovered from a DOCX.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReviewItem {
    pub kind: ReviewKind,
    pub author: String,
    pub date: String,
    /// The inserted/deleted text, or the comment body.
    pub text: String,
    /// For comments: the document text the comment is anchored to. Empty for
    /// tracked changes (whose `text` is itself the changed run).
    pub context: String,
}

/// Extract every reviewer annotation from a DOCX file, in document order
/// (tracked changes first as they appear, then comments). Returns an empty list
/// if the file has no review data. Errors only on unreadable/invalid archives.
pub fn extract_review_items(path: &Path) -> Result<Vec<ReviewItem>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    let doc_xml = read_entry(&mut archive, "word/document.xml")?;
    let comments_xml = read_entry(&mut archive, "word/comments.xml").unwrap_or_default();

    let mut items = parse_revisions(&doc_xml);
    items.extend(parse_comments(&comments_xml, &doc_xml));
    Ok(items)
}

fn read_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<String, String> {
    use std::io::Read;
    let mut entry = archive.by_name(name).map_err(|e| e.to_string())?;
    let mut s = String::new();
    entry.read_to_string(&mut s).map_err(|e| e.to_string())?;
    Ok(s)
}

/// Parse tracked insertions (`<w:ins>`) and deletions (`<w:del>`) from
/// `word/document.xml`. Insertion text is in `<w:t>`; deletion text in
/// `<w:delText>`.
pub fn parse_revisions(doc_xml: &str) -> Vec<ReviewItem> {
    let mut items = Vec::new();
    collect_blocks(doc_xml, "w:ins", "w:t", ReviewKind::Insertion, &mut items);
    collect_blocks(doc_xml, "w:del", "w:delText", ReviewKind::Deletion, &mut items);
    items
}

/// Walk every `<w:{tag} ...> ... </w:{tag}>` block, pull author/date from the opening
/// tag and the changed text from `<{text_tag}>` runs inside.
fn collect_blocks(
    xml: &str,
    tag: &str,
    text_tag: &str,
    kind: ReviewKind,
    out: &mut Vec<ReviewItem>,
) {
    let open = format!("<{} ", tag);
    let close = format!("</{}>", tag);
    let mut rest = xml;
    while let Some(p) = rest.find(&open) {
        rest = &rest[p..];
        let Some(block_end) = rest.find(&close) else { break };
        let block = &rest[..block_end + close.len()];
        // Attributes live in the opening tag only (up to the first '>').
        let header = &block[..block.find('>').unwrap_or(block.len())];
        let text = xml_unescape(&extract_tag_text(block, text_tag));
        if !text.trim().is_empty() {
            out.push(ReviewItem {
                kind,
                author: attr_value(header, "w:author").unwrap_or("Unknown").to_string(),
                date: attr_value(header, "w:date").unwrap_or("").to_string(),
                text,
                context: String::new(),
            });
        }
        // Advance past this opening tag to find the next sibling.
        rest = &rest[open.len()..];
    }
}

/// Parse `word/comments.xml` into review items, skipping our own equation-recovery
/// comments. The anchored document text is recovered from `document.xml` via the
/// `<w:commentRangeStart/End>` pair matching the comment id.
pub fn parse_comments(comments_xml: &str, doc_xml: &str) -> Vec<ReviewItem> {
    let mut items = Vec::new();
    let mut rest = comments_xml;
    while let Some(p) = rest.find("<w:comment ") {
        rest = &rest[p..];
        let close = "</w:comment>";
        let Some(end) = rest.find(close) else { break };
        let block = &rest[..end + close.len()];
        let header = &block[..block.find('>').unwrap_or(block.len())];

        let author = attr_value(header, "w:author").unwrap_or("Unknown");
        let body = xml_unescape(&extract_tag_text(block, "w:t"));

        // Skip our own equation-recovery comments (not human feedback).
        let is_ours = author == "MD-TO-ALL" || body.starts_with("LaTeX: ");
        if !is_ours && !body.trim().is_empty() {
            let context = attr_value(header, "w:id")
                .map(|id| comment_anchor(doc_xml, id))
                .unwrap_or_default();
            items.push(ReviewItem {
                kind: ReviewKind::Comment,
                author: author.to_string(),
                date: attr_value(header, "w:date").unwrap_or("").to_string(),
                text: body,
                context,
            });
        }
        rest = &rest[1..];
    }
    items
}

/// Recover the text a comment is anchored to, between the range markers.
fn comment_anchor(doc_xml: &str, id: &str) -> String {
    let start_pat = format!("<w:commentRangeStart w:id=\"{}\"", id);
    let end_pat = format!("<w:commentRangeEnd w:id=\"{}\"", id);
    if let (Some(s), Some(e)) = (doc_xml.find(&start_pat), doc_xml.find(&end_pat)) {
        if e > s {
            return xml_unescape(&extract_tag_text(&doc_xml[s..e], "w:t"));
        }
    }
    String::new()
}

/// Concatenate the text content of every `<{tag}>...</{tag}>` element in `xml`.
fn extract_tag_text(xml: &str, tag: &str) -> String {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut out = String::new();
    let mut rest = xml;
    while let Some(p) = rest.find(&open) {
        rest = &rest[p + open.len()..];
        // Ensure we matched the whole tag name (next char ends the name).
        if !rest.starts_with('>') && !rest.starts_with(' ') && !rest.starts_with('/') {
            continue;
        }
        let Some(tag_end) = rest.find('>') else { break };
        // Self-closing element carries no text.
        if rest.as_bytes().get(tag_end.wrapping_sub(1)) == Some(&b'/') {
            rest = &rest[tag_end + 1..];
            continue;
        }
        rest = &rest[tag_end + 1..];
        if let Some(c) = rest.find(&close) {
            out.push_str(&rest[..c]);
            rest = &rest[c + close.len()..];
        } else {
            break;
        }
    }
    out
}

/// Read an XML attribute value from a tag string (handles `"` and `'` quoting).
fn attr_value<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    for q in ['"', '\''] {
        let needle = format!("{}={}", attr, q);
        if let Some(p) = xml.find(&needle) {
            let start = p + needle.len();
            let rest = &xml[start..];
            if let Some(end) = rest.find(q) {
                return Some(&rest[..end]);
            }
        }
    }
    None
}

/// Decode the five predefined XML entities (Word escapes `&` and `<` in text).
fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tracked_insertion_and_deletion() {
        let doc = r#"<w:p>
            <w:r><w:t>The term is </w:t></w:r>
            <w:del w:id="1" w:author="Dr Smith" w:date="2026-06-15T10:00:00Z">
              <w:r><w:delText>30</w:delText></w:r>
            </w:del>
            <w:ins w:id="2" w:author="Dr Smith" w:date="2026-06-15T10:01:00Z">
              <w:r><w:t>60</w:t></w:r>
            </w:ins>
            <w:r><w:t> days.</w:t></w:r>
        </w:p>"#;
        let items = parse_revisions(doc);
        assert_eq!(items.len(), 2);
        let ins = items.iter().find(|i| i.kind == ReviewKind::Insertion).unwrap();
        assert_eq!(ins.text, "60");
        assert_eq!(ins.author, "Dr Smith");
        let del = items.iter().find(|i| i.kind == ReviewKind::Deletion).unwrap();
        assert_eq!(del.text, "30");
    }

    #[test]
    fn parses_human_comment_with_anchor_and_skips_ours() {
        let comments = r#"
            <w:comment w:id="0" w:author="MD-TO-ALL" w:date="2026-06-15T09:00:00Z">
              <w:p><w:r><w:t>LaTeX: E = mc^2</w:t></w:r></w:p>
            </w:comment>
            <w:comment w:id="1" w:author="Dr Smith" w:date="2026-06-15T10:05:00Z">
              <w:p><w:r><w:t>Clarify this claim &amp; add a citation.</w:t></w:r></w:p>
            </w:comment>"#;
        let doc = r#"<w:p>
            <w:commentRangeStart w:id="1"/>
            <w:r><w:t>quantum advantage</w:t></w:r>
            <w:commentRangeEnd w:id="1"/>
            <w:r><w:commentReference w:id="1"/></w:r>
        </w:p>"#;
        let items = parse_comments(comments, doc);
        assert_eq!(items.len(), 1, "our own MD-TO-ALL equation comment must be filtered out");
        let c = &items[0];
        assert_eq!(c.kind, ReviewKind::Comment);
        assert_eq!(c.author, "Dr Smith");
        assert_eq!(c.text, "Clarify this claim & add a citation.");
        assert_eq!(c.context, "quantum advantage");
    }

    #[test]
    fn no_review_data_yields_empty() {
        let doc = r#"<w:p><w:r><w:t>Plain unannotated paragraph.</w:t></w:r></w:p>"#;
        assert!(parse_revisions(doc).is_empty());
        assert!(parse_comments("", doc).is_empty());
    }

    #[test]
    fn extract_from_minimal_docx_zip_round_trips() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let path = std::env::temp_dir().join("mdall_review_extract_test.docx");
        let doc = r#"<?xml version="1.0"?><w:document><w:body>
            <w:p>
              <w:commentRangeStart w:id="1"/>
              <w:r><w:t>quantum advantage</w:t></w:r>
              <w:commentRangeEnd w:id="1"/>
              <w:r><w:commentReference w:id="1"/></w:r>
            </w:p>
            <w:p>
              <w:del w:id="2" w:author="Dr Smith" w:date="2026-06-15T10:00:00Z">
                <w:r><w:delText>old wording</w:delText></w:r>
              </w:del>
              <w:ins w:id="3" w:author="Dr Smith" w:date="2026-06-15T10:01:00Z">
                <w:r><w:t>new wording</w:t></w:r>
              </w:ins>
            </w:p>
        </w:body></w:document>"#;
        let comments = r#"<w:comments>
            <w:comment w:id="1" w:author="Dr Smith" w:date="2026-06-15T10:05:00Z">
              <w:p><w:r><w:t>Add a citation here.</w:t></w:r></w:p>
            </w:comment>
        </w:comments>"#;
        {
            let f = std::fs::File::create(&path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opt = SimpleFileOptions::default();
            z.start_file("word/document.xml", opt).unwrap();
            z.write_all(doc.as_bytes()).unwrap();
            z.start_file("word/comments.xml", opt).unwrap();
            z.write_all(comments.as_bytes()).unwrap();
            z.finish().unwrap();
        }

        let items = extract_review_items(&path).unwrap();
        assert_eq!(items.len(), 3, "insertion + deletion + comment");
        assert!(items.iter().any(|i| i.kind == ReviewKind::Insertion && i.text == "new wording"));
        assert!(items.iter().any(|i| i.kind == ReviewKind::Deletion && i.text == "old wording"));
        let c = items.iter().find(|i| i.kind == ReviewKind::Comment).unwrap();
        assert_eq!(c.text, "Add a citation here.");
        assert_eq!(c.context, "quantum advantage");
        assert_eq!(c.author, "Dr Smith");

        let _ = std::fs::remove_file(&path);
    }
}
