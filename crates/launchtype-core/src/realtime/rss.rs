//! RSS headlines, replicating `xml.etree.ElementTree` extraction semantics:
//! every non-namespaced `<item>` element in pre-order, each item's first
//! direct `<title>` child, the text before any nested markup, stripped; the
//! first five non-empty titles are spoken.

use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;

use crate::i18n::{format_args, tr, Arg};

use super::RealtimeError;

/// How many headlines are spoken (Python `HEADLINE_COUNT`).
pub const HEADLINE_COUNT: usize = 5;

/// A feed the app knows about: its URL and its spoken source name (the name
/// is a proper noun and stays untranslated, as in Python).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RssSource {
    pub url: &'static str,
    pub name: &'static str,
}

pub const ELPAIS: RssSource = RssSource {
    url: "https://feeds.elpais.com/mrss-s/pages/ep/site/elpais.com/portada",
    name: "El País",
};
pub const BBC: RssSource =
    RssSource { url: "https://feeds.bbci.co.uk/news/world/rss.xml", name: "BBC" };
pub const CATALUNYA: RssSource = RssSource {
    url: "https://www.lavanguardia.com/rss/local/catalunya.xml",
    name: "La Vanguardia Catalunya",
};
pub const VILAWEB: RssSource =
    RssSource { url: "https://www.vilaweb.cat/feed/", name: "VilaWeb" };

struct XmlNode {
    /// Local name when the element is in no namespace — the only kind a plain
    /// ElementTree tag like `"item"` can match; `None` for namespaced ones.
    plain_name: Option<String>,
    /// Text before the first child element (ElementTree `.text`).
    text: String,
    children: Vec<XmlNode>,
}

fn new_node(resolve: ResolveResult<'_>, local_name: &[u8]) -> XmlNode {
    let plain_name = match resolve {
        ResolveResult::Unbound => Some(String::from_utf8_lossy(local_name).into_owned()),
        _ => None,
    };
    XmlNode { plain_name, text: String::new(), children: Vec::new() }
}

/// Build a DOM the way `ElementTree.fromstring` would, or `None` on anything
/// it would reject (malformed markup, junk outside the root, no root at all).
fn parse_document(xml: &str) -> Option<XmlNode> {
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    let mut reader = NsReader::from_str(xml);
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;

    loop {
        let (resolve, event) = reader.read_resolved_event().ok()?;
        match event {
            Event::Start(start) => {
                if stack.is_empty() && root.is_some() {
                    return None; // junk after document element
                }
                stack.push(new_node(resolve, start.local_name().as_ref()));
            }
            Event::Empty(start) => {
                let node = new_node(resolve, start.local_name().as_ref());
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => {
                        if root.is_some() {
                            return None;
                        }
                        root = Some(node);
                    }
                }
            }
            Event::End(_) => {
                // The reader validates that end names match their start tag.
                let node = stack.pop()?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Event::Text(text) => {
                let text = text.unescape().ok()?;
                match stack.last_mut() {
                    Some(open) => {
                        if open.children.is_empty() {
                            open.text.push_str(&text);
                        }
                    }
                    None => {
                        if !text.trim().is_empty() {
                            return None; // ElementTree: junk outside the root
                        }
                    }
                }
            }
            Event::CData(cdata) => {
                let bytes = cdata.into_inner();
                let text = std::str::from_utf8(&bytes).ok()?;
                match stack.last_mut() {
                    Some(open) => {
                        if open.children.is_empty() {
                            open.text.push_str(text);
                        }
                    }
                    None => return None, // CDATA outside the root
                }
            }
            Event::Eof => break,
            _ => {} // declaration, comments, processing instructions, doctype
        }
    }
    if !stack.is_empty() {
        return None; // unclosed elements
    }
    root
}

/// Pre-order walk mirroring `root.iter("item")` + `item.findtext("title")`.
fn collect_titles(node: &XmlNode, titles: &mut Vec<String>) {
    if titles.len() >= HEADLINE_COUNT {
        return;
    }
    if node.plain_name.as_deref() == Some("item") {
        if let Some(title) =
            node.children.iter().find(|child| child.plain_name.as_deref() == Some("title"))
        {
            let trimmed = title.text.trim();
            if !trimmed.is_empty() {
                titles.push(trimmed.to_string());
            }
        }
        if titles.len() >= HEADLINE_COUNT {
            return;
        }
    }
    for child in &node.children {
        collect_titles(child, titles);
        if titles.len() >= HEADLINE_COUNT {
            return;
        }
    }
}

/// Extract up to five headline titles the way the Python fetcher does.
/// Malformed XML maps to the "could not be understood" error.
pub fn parse_rss_titles(xml: &str) -> Result<Vec<String>, RealtimeError> {
    let root = parse_document(xml).ok_or(RealtimeError::NotUnderstood)?;
    let mut titles = Vec::new();
    collect_titles(&root, &mut titles);
    Ok(titles)
}

/// Python `_fetch_rss_headlines`, minus the HTTP call: the spoken headline
/// sentence for a feed body.
pub fn rss_headline_sentence(xml: &str, source: &str) -> Result<String, RealtimeError> {
    let titles = parse_rss_titles(xml)?;
    if titles.is_empty() {
        return Err(RealtimeError::NoHeadlines);
    }
    Ok(format_args(
        &tr("Latest headlines from {source}: {headlines}"),
        &[("source", Arg::Str(source)), ("headlines", Arg::Str(&titles.join(". ")))],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/">
  <channel>
    <title>El País: el periódico global</title>
    <item><title>  First headline  </title><link>https://example.com/1</link></item>
    <item><title><![CDATA[Second headline]]></title></item>
    <item><title></title><description>skipped: empty title</description></item>
    <item><description>skipped: no title at all</description></item>
    <item><media:title>skipped: namespaced title only</media:title></item>
    <item><title>Third &amp; fourth</title></item>
    <item><title/></item>
    <item><title>Fourth</title></item>
    <item><title>Fifth</title></item>
    <item><title>Sixth is never reached</title></item>
  </channel>
</rss>"#;

    #[test]
    fn extracts_top_five_titles_with_elementtree_semantics() {
        let titles = parse_rss_titles(FEED).unwrap();
        assert_eq!(
            titles,
            vec![
                "First headline",
                "Second headline",
                "Third & fourth",
                "Fourth",
                "Fifth",
            ]
        );
    }

    #[test]
    fn sentence_matches_python_wording() {
        assert_eq!(
            rss_headline_sentence(FEED, "El País").unwrap(),
            "Latest headlines from El País: First headline. Second headline. \
             Third & fourth. Fourth. Fifth"
        );
    }

    #[test]
    fn title_text_stops_at_nested_markup() {
        let xml = "<rss><channel><item><title>Breaking <b>news</b> tail</title></item></channel></rss>";
        assert_eq!(parse_rss_titles(xml).unwrap(), vec!["Breaking"]);
    }

    #[test]
    fn only_the_first_title_child_counts() {
        // findtext returns the first <title>; an empty first one hides a
        // non-empty second one.
        let xml = "<rss><item><title> </title><title>Ignored</title></item></rss>";
        assert_eq!(parse_rss_titles(xml).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn default_namespace_hides_items_like_elementtree() {
        let xml = r#"<rss xmlns="urn:example"><item><title>Hi</title></item></rss>"#;
        assert_eq!(parse_rss_titles(xml).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn malformed_xml_is_not_understood() {
        for xml in [
            "",
            "not xml at all",
            "<rss><item>",
            "<rss></wrong>",
            "<a/><b/>",
            "<rss>&undefined;</rss>",
        ] {
            let error = parse_rss_titles(xml).unwrap_err();
            assert_eq!(error, RealtimeError::NotUnderstood, "for {xml:?}");
        }
    }

    #[test]
    fn feed_without_headlines_has_its_own_error() {
        let xml = "<rss><channel><item><title>  </title></item></channel></rss>";
        let error = rss_headline_sentence(xml, "BBC").unwrap_err();
        assert_eq!(error, RealtimeError::NoHeadlines);
        assert_eq!(error.to_string(), "The news feed contained no headlines.");
    }

    #[test]
    fn feed_sources_match_python() {
        assert_eq!(ELPAIS.url, "https://feeds.elpais.com/mrss-s/pages/ep/site/elpais.com/portada");
        assert_eq!(ELPAIS.name, "El País");
        assert_eq!(BBC.url, "https://feeds.bbci.co.uk/news/world/rss.xml");
        assert_eq!(BBC.name, "BBC");
        assert_eq!(CATALUNYA.url, "https://www.lavanguardia.com/rss/local/catalunya.xml");
        assert_eq!(CATALUNYA.name, "La Vanguardia Catalunya");
        assert_eq!(VILAWEB.url, "https://www.vilaweb.cat/feed/");
        assert_eq!(VILAWEB.name, "VilaWeb");
    }
}
