//! "Does it work?" mode (`"`): what was pasted, and what the answer means.
//!
//! This half decides what the text is and turns what the network or the disk
//! said into a sentence; [`launchtype_services::check`] does the asking. Every
//! sentence leads with its verdict, because that is the part a screen reader
//! should get to before the user arrows away.

use crate::i18n::{format_args, tr, Arg};
use crate::paths;
use crate::portable::is_absolute_location;

/// Something that can be checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// An http or https address, scheme included.
    Link(String),
    /// An email address. Only the domain can be asked about, so only the
    /// domain is kept: the rest of the address never leaves the machine.
    Email { domain: String },
    /// An absolute path on this machine.
    Path(String),
}

/// Why pasted text cannot be checked at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    Unrecognised,
    Link,
    Email,
    /// Not http or https; holds the scheme as it was written.
    Scheme(String),
    /// A `\\host\share`. See [`classify`] for why it is refused.
    NetworkPath,
}

impl Invalid {
    pub fn message(&self) -> String {
        match self {
            Invalid::Unrecognised => {
                tr("Invalid: that is not a link, an email address or a full file path.")
            }
            Invalid::Link => tr("Invalid: that is not a valid link."),
            Invalid::Email => tr("Invalid: that is not a valid email address."),
            Invalid::Scheme(scheme) => format_args(
                &tr("Invalid: only http and https links can be checked, not {scheme}."),
                &[("scheme", Arg::Str(scheme))],
            ),
            Invalid::NetworkPath => {
                tr("Invalid: only paths on this computer can be checked, not network shares.")
            }
        }
    }
}

/// What a check found. `works` is true only when the thing was confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub works: bool,
    pub sentence: String,
}

impl Outcome {
    fn works(sentence: String) -> Self {
        Outcome { works: true, sentence }
    }

    pub fn failed(sentence: String) -> Self {
        Outcome { works: false, sentence }
    }
}

/// One row of the mode's list: the text as it was pasted, so Enter can check
/// it again, and how it went.
#[derive(Debug, Clone)]
pub struct Checked {
    pub text: String,
    pub outcome: Outcome,
}

/// Work out what pasted text is.
///
/// Network shares are refused rather than probed. Asking Windows whether
/// `\\host\share` exists is an outbound SMB login to `host`, which hands
/// whoever answers the user's account name and a response derived from their
/// password, and pasted text often comes from a web page or a message (see
/// [`paths::parse_paths`], which refuses them for the same reason).
pub fn classify(text: &str) -> Result<Target, Invalid> {
    let text = unwrap(text);
    if text.is_empty() {
        return Err(Invalid::Unrecognised);
    }
    if text.get(..7).is_some_and(|prefix| prefix.eq_ignore_ascii_case("mailto:")) {
        let address = text[7..].split('?').next().unwrap_or_default();
        return email_domain(address)
            .map(|domain| Target::Email { domain })
            .ok_or(Invalid::Email);
    }

    let cleaned = paths::clean(text);
    // Before the absolute test: that one does not count `\/host` as absolute,
    // and Windows still reads it as a share.
    if paths::is_network_location(&cleaned) {
        return Err(Invalid::NetworkPath);
    }
    if is_absolute_location(&cleaned) {
        return Ok(Target::Path(cleaned));
    }

    if let Some((scheme, rest)) = text.split_once("://") {
        if is_scheme(scheme) {
            if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
                return Err(Invalid::Scheme(scheme.to_string()));
            }
            if rest.split(['/', '?', '#']).next().unwrap_or_default().is_empty() {
                return Err(Invalid::Link);
            }
            return Ok(Target::Link(text.to_string()));
        }
    }

    if text.chars().any(char::is_whitespace) {
        return Err(Invalid::Unrecognised);
    }
    if text.contains('@') && !text.contains('/') {
        return email_domain(text)
            .map(|domain| Target::Email { domain })
            .ok_or(Invalid::Email);
    }
    // `github.com/ogomez92` is how plenty of links get copied.
    let host = text.split(['/', '?', '#']).next().unwrap_or_default();
    let host = match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    if is_domain(host) {
        return Ok(Target::Link(format!("https://{text}")));
    }
    Err(Invalid::Unrecognised)
}

/// Peel what copying tends to leave around the text: spaces, a pair of quotes,
/// and the name in `Oscar <oscar@example.com>`.
fn unwrap(text: &str) -> &str {
    let text = text.trim();
    if let Some(inner) = text.strip_suffix('>') {
        if let Some((_, address)) = inner.rsplit_once('<') {
            return address.trim();
        }
    }
    for quote in ['"', '\''] {
        if let Some(inner) = text.strip_prefix(quote).and_then(|t| t.strip_suffix(quote)) {
            return inner.trim();
        }
    }
    text
}

fn is_scheme(text: &str) -> bool {
    text.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// The domain of an email address, or `None` if it is not shaped like one.
fn email_domain(address: &str) -> Option<String> {
    let (local, domain) = address.rsplit_once('@')?;
    let usable = !local.is_empty()
        && !local.contains('@')
        && !local.chars().any(char::is_whitespace)
        && is_domain(domain);
    usable.then(|| domain.to_string())
}

/// At least two dotted labels and a top level that is not a number, so `1.5`,
/// `e.g` and `localhost` are not taken for addresses. Letters may be accented:
/// `españa.es` is a real domain.
fn is_domain(text: &str) -> bool {
    let labels: Vec<&str> = text.split('.').collect();
    let label_ok = |label: &&str| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_alphanumeric() || c == '-')
    };
    let Some(top) = labels.last() else { return false };
    labels.len() >= 2
        && labels.iter().all(label_ok)
        && top.chars().count() >= 2
        && !top.chars().all(|c| c.is_ascii_digit())
}

/// What the server's final status code says about the link. `requested` and
/// `landed` are the address asked for and the one the redirects ended on.
pub fn link_outcome(code: u16, requested: &str, landed: &str) -> Outcome {
    let status = Arg::Int(i64::from(code));
    match code {
        200..=299 if same_place(requested, landed) => Outcome::works(tr("Works.")),
        // Worth saying: a dead page sent to the home page, or a private one
        // sent to a login, also finishes with a 200.
        200..=299 => Outcome::works(format_args(
            &tr("Works, but it takes you to {address}."),
            &[("address", Arg::Str(without_query(landed)))],
        )),
        404 | 410 => Outcome::failed(format_args(
            &tr("Broken: that page does not exist (status {status})."),
            &[("status", status)],
        )),
        401 | 407 => Outcome::failed(format_args(
            &tr("Could not confirm: that page asks for a login (status {status})."),
            &[("status", status)],
        )),
        // Sites turn automated requests away with all of these, so none of
        // them says the page is gone.
        403 | 405 | 406 | 429 | 451 | 503 | 999 => Outcome::failed(format_args(
            &tr("Could not confirm: the server refused to answer (status {status}). It may still open in a browser."),
            &[("status", status)],
        )),
        500..=599 => Outcome::failed(format_args(
            &tr("Broken: the server has a problem (status {status})."),
            &[("status", status)],
        )),
        _ => Outcome::failed(format_args(
            &tr("Could not confirm: the server answered with status {status}."),
            &[("status", status)],
        )),
    }
}

fn without_query(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// Whether a redirect only tidied the address up: https for http, `www.` added
/// or dropped, a trailing slash, or a change of query string.
fn same_place(a: &str, b: &str) -> bool {
    fn key(url: &str) -> String {
        let url = without_query(url).to_lowercase();
        let url = url.split_once("://").map_or(url.as_str(), |(_, rest)| rest);
        let url = url.strip_prefix("www.").unwrap_or(url);
        url.trim_end_matches('/').to_string()
    }
    key(a) == key(b)
}

/// What DNS said about an email address's domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailAnswer {
    Receives,
    /// A null MX record (RFC 7505): the domain states that it takes no mail.
    Refuses,
    NoMailServer,
    NoSuchDomain,
    /// No usable answer, which says nothing about the domain.
    Unknown,
}

/// Read a DNS-over-HTTPS JSON answer to an MX query.
pub fn parse_mail_answer(body: &str) -> MailAnswer {
    const MX: u64 = 15;
    const NO_ERROR: u64 = 0;
    const NAME_ERROR: u64 = 3;
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return MailAnswer::Unknown;
    };
    match json.get("Status").and_then(|status| status.as_u64()) {
        Some(NAME_ERROR) => return MailAnswer::NoSuchDomain,
        Some(NO_ERROR) => {}
        _ => return MailAnswer::Unknown,
    }
    let records = json.get("Answer").and_then(|answer| answer.as_array());
    // An answer can also hold the CNAMEs it went through, so filter by type.
    let exchanges: Vec<&str> = records
        .into_iter()
        .flatten()
        .filter(|record| record.get("type").and_then(|t| t.as_u64()) == Some(MX))
        .filter_map(|record| record.get("data").and_then(|data| data.as_str()))
        .filter_map(|data| data.split_whitespace().nth(1))
        .collect();
    if exchanges.is_empty() {
        MailAnswer::NoMailServer
    } else if exchanges.iter().all(|exchange| *exchange == ".") {
        MailAnswer::Refuses
    } else {
        MailAnswer::Receives
    }
}

pub fn mail_outcome(domain: &str, answer: MailAnswer) -> Outcome {
    let args = [("domain", Arg::Str(domain))];
    match answer {
        MailAnswer::Receives => Outcome::works(format_args(
            &tr("Works: {domain} receives email, but there is no way to tell from here whether this address exists."),
            &args,
        )),
        MailAnswer::Refuses => Outcome::failed(format_args(
            &tr("Broken: {domain} does not accept email."),
            &args,
        )),
        MailAnswer::NoSuchDomain => Outcome::failed(format_args(
            &tr("Broken: the domain {domain} does not exist."),
            &args,
        )),
        MailAnswer::NoMailServer => Outcome::failed(format_args(
            &tr("Broken: {domain} exists but has no mail server."),
            &args,
        )),
        MailAnswer::Unknown => Outcome::failed(format_args(
            &tr("Could not confirm: there was no answer about {domain}. Check your connection."),
            &args,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(url: &str) -> Result<Target, Invalid> {
        Ok(Target::Link(url.to_string()))
    }

    fn email(domain: &str) -> Result<Target, Invalid> {
        Ok(Target::Email { domain: domain.to_string() })
    }

    fn path(path: &str) -> Result<Target, Invalid> {
        Ok(Target::Path(path.to_string()))
    }

    #[test]
    fn links_are_recognised_with_or_without_a_scheme() {
        assert_eq!(classify("https://example.com/a?b=c"), link("https://example.com/a?b=c"));
        assert_eq!(classify("  HTTP://example.com  "), link("HTTP://example.com"));
        assert_eq!(classify("github.com/ogomez92"), link("https://github.com/ogomez92"));
        assert_eq!(classify("example.com:8080/x"), link("https://example.com:8080/x"));
        assert_eq!(classify("\"https://example.com\""), link("https://example.com"));
        assert_eq!(classify("<https://example.com>"), link("https://example.com"));
    }

    /// A password in a link has an `@` in it, and that does not make it mail.
    #[test]
    fn a_login_in_a_link_does_not_make_it_an_email() {
        assert_eq!(classify("https://me:pw@example.com/"), link("https://me:pw@example.com/"));
    }

    #[test]
    fn other_schemes_and_empty_hosts_are_invalid() {
        assert_eq!(classify("ftp://example.com"), Err(Invalid::Scheme("ftp".to_string())));
        assert_eq!(classify("https://"), Err(Invalid::Link));
        assert_eq!(classify("https:///path"), Err(Invalid::Link));
    }

    #[test]
    fn email_addresses_keep_only_their_domain() {
        assert_eq!(classify("me@example.com"), email("example.com"));
        assert_eq!(classify("mailto:me@example.com?subject=hi"), email("example.com"));
        assert_eq!(classify("MAILTO:me@example.com"), email("example.com"));
        assert_eq!(classify("Oscar <me@example.com>"), email("example.com"));
        assert_eq!(classify("\"O G\" <me@example.com>"), email("example.com"));
        assert_eq!(classify("yo@españa.es"), email("españa.es"));
    }

    #[test]
    fn malformed_email_addresses_are_invalid() {
        for text in ["@example.com", "me@", "me@example", "me@example.", "a@b@c.com", "mailto:"] {
            assert_eq!(classify(text), Err(Invalid::Email), "{text}");
        }
    }

    #[test]
    fn absolute_paths_are_recognised() {
        assert_eq!(classify(r"C:\Users\me\a b.txt"), path(r"C:\Users\me\a b.txt"));
        assert_eq!(classify(r#""C:\Users\me\a b.txt""#), path(r"C:\Users\me\a b.txt"));
        assert_eq!(classify("C:/Users/me"), path("C:/Users/me"));
        assert_eq!(classify("/Users/me/a.txt"), path("/Users/me/a.txt"));
        assert_eq!(classify("file:///C:/a%20b.txt"), path("C:/a b.txt"));
    }

    /// Probing any of these would log in to `host` with the user's account.
    #[test]
    fn network_shares_are_refused_however_they_are_written() {
        for text in [
            r"\\host\share",
            "//host/share",
            r"/\host\share",
            r"\/host\share",
            "file://host/share",
            r#""\\host\share\a.txt""#,
        ] {
            assert_eq!(classify(text), Err(Invalid::NetworkPath), "{text}");
        }
    }

    #[test]
    fn anything_else_is_invalid() {
        for text in ["", "   ", "hello there", "1.5", "e.g", "localhost", "notes", r".\relative"] {
            assert_eq!(classify(text), Err(Invalid::Unrecognised), "{text:?}");
        }
    }

    #[test]
    fn only_a_success_status_counts_as_working() {
        let url = "https://example.com/";
        assert!(link_outcome(200, url, url).works);
        assert!(link_outcome(204, url, url).works);
        for code in [301, 400, 401, 403, 404, 410, 429, 500, 503, 999] {
            assert!(!link_outcome(code, url, url).works, "{code}");
        }
    }

    #[test]
    fn the_status_decides_how_a_failure_is_worded() {
        let url = "https://example.com/";
        assert!(link_outcome(404, url, url).sentence.starts_with("Broken"));
        assert!(link_outcome(500, url, url).sentence.starts_with("Broken"));
        assert!(link_outcome(403, url, url).sentence.starts_with("Could not confirm"));
        assert!(link_outcome(503, url, url).sentence.starts_with("Could not confirm"));
    }

    #[test]
    fn only_a_real_redirect_is_mentioned() {
        let works = tr("Works.");
        for (asked, landed) in [
            ("http://example.com", "https://example.com/"),
            ("https://example.com", "https://www.example.com/"),
            ("https://www.example.com/a/", "https://example.com/a"),
            ("https://example.com/a", "https://example.com/a?from=b"),
        ] {
            assert_eq!(link_outcome(200, asked, landed).sentence, works, "{asked}");
        }
        let moved = link_outcome(200, "https://a.com/doc", "https://login.a.com/in?next=doc");
        assert!(moved.works);
        assert!(moved.sentence.contains("https://login.a.com/in"), "{}", moved.sentence);
        assert!(!moved.sentence.contains("next=doc"), "{}", moved.sentence);
    }

    #[test]
    fn dns_answers_are_read() {
        let gmail = r#"{"Status":0,"Answer":[{"name":"gmail.com","type":15,"data":"5 gmail-smtp-in.l.google.com."}]}"#;
        assert_eq!(parse_mail_answer(gmail), MailAnswer::Receives);
        let null_mx = r#"{"Status":0,"Answer":[{"name":"example.com","type":15,"data":"0 ."}]}"#;
        assert_eq!(parse_mail_answer(null_mx), MailAnswer::Refuses);
        assert_eq!(parse_mail_answer(r#"{"Status":3}"#), MailAnswer::NoSuchDomain);
        assert_eq!(parse_mail_answer(r#"{"Status":0}"#), MailAnswer::NoMailServer);
        let only_cname = r#"{"Status":0,"Answer":[{"type":5,"data":"other.example.com."}]}"#;
        assert_eq!(parse_mail_answer(only_cname), MailAnswer::NoMailServer);
        assert_eq!(parse_mail_answer(r#"{"Status":2}"#), MailAnswer::Unknown);
        assert_eq!(parse_mail_answer("not json"), MailAnswer::Unknown);
    }

    #[test]
    fn only_a_domain_with_a_mail_server_counts_as_working() {
        assert!(mail_outcome("a.com", MailAnswer::Receives).works);
        for answer in [
            MailAnswer::Refuses,
            MailAnswer::NoMailServer,
            MailAnswer::NoSuchDomain,
            MailAnswer::Unknown,
        ] {
            assert!(!mail_outcome("a.com", answer).works, "{answer:?}");
        }
    }
}
