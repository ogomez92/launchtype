//! "Does it work?" mode (`"`): asking the network and the disk. What the text
//! is and how the answers are worded live in [`launchtype_core::check`].

use std::error::Error as _;
use std::io;
use std::path::Path;
use std::time::Duration;

use launchtype_core::check::{
    link_outcome, mail_outcome, parse_mail_answer, MailAnswer, Outcome, Target,
};
use launchtype_core::i18n::{format_args, tr, Arg};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_REDIRECTS: u32 = 10;

/// Cloudflare's DNS-over-HTTPS endpoint. Rust's standard library can only look
/// up addresses, not mail servers, and this needs no new dependency.
const DNS_OVER_HTTPS: &str = "https://cloudflare-dns.com/dns-query";

/// Run the check. Blocks for as long as the network takes, so call it off the
/// UI thread.
pub fn run(target: &Target) -> Outcome {
    match target {
        Target::Link(url) => check_link(url),
        Target::Email { domain } => check_email(domain),
        Target::Path(path) => check_path(Path::new(path)),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout(TIMEOUT)
        .redirects(MAX_REDIRECTS)
        .user_agent(crate::USER_AGENT)
        .build()
}

/// A GET rather than a HEAD, because plenty of servers answer HEAD with an
/// error for pages that are fine. Only the headers are read: dropping the
/// response closes the connection, so a large file is not downloaded.
fn check_link(url: &str) -> Outcome {
    match agent().get(url).set("accept", "*/*").call() {
        Ok(response) => link_outcome(response.status(), url, response.get_url()),
        Err(ureq::Error::Status(code, response)) => link_outcome(code, url, response.get_url()),
        Err(ureq::Error::Transport(transport)) => transport_outcome(url, &transport),
    }
}

fn transport_outcome(url: &str, transport: &ureq::Transport) -> Outcome {
    // After a redirect the host that failed may not be the one that was pasted.
    let host = transport
        .url()
        .and_then(|failed| failed.host_str())
        .unwrap_or(url)
        .to_string();
    let args = [("host", Arg::Str(&host))];
    let details = transport.to_string().to_lowercase();
    let sentence = match transport.kind() {
        ureq::ErrorKind::InvalidUrl | ureq::ErrorKind::UnknownScheme => {
            tr("Invalid: that is not a valid link.")
        }
        ureq::ErrorKind::Dns => format_args(
            &tr("Broken: could not find the server {host}. Check the address and your connection."),
            &args,
        ),
        ureq::ErrorKind::TooManyRedirects => tr("Broken: the link keeps redirecting and never arrives."),
        _ if timed_out(transport) => {
            format_args(&tr("Broken: {host} took too long to answer."), &args)
        }
        _ if details.contains("certificate") => format_args(
            &tr("Broken: the security certificate of {host} is not valid."),
            &args,
        ),
        ureq::ErrorKind::ConnectionFailed => {
            format_args(&tr("Broken: could not connect to {host}."), &args)
        }
        _ => format_args(
            &tr("Broken: the connection to {host} failed: {reason}"),
            &[("host", Arg::Str(&host)), ("reason", Arg::Str(&transport.kind().to_string()))],
        ),
    };
    Outcome::failed(sentence)
}

fn timed_out(transport: &ureq::Transport) -> bool {
    transport
        .source()
        .and_then(|source| source.downcast_ref::<io::Error>())
        .is_some_and(|error| {
            matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)
        })
}

/// Whether the domain has a mail server. That is as far as this can go: the
/// only way to ask about the mailbox itself is an SMTP conversation on port
/// 25, which home connections block and big mail providers will not answer
/// truthfully. Only the domain is sent, never the whole address.
fn check_email(domain: &str) -> Outcome {
    // DNS names are ASCII, so `españa.es` goes out as `xn--espaa-rta.es`.
    // Parsing it as a URL does that conversion.
    let name = ureq::get(&format!("https://{domain}/"))
        .request_url()
        .map_or_else(|_| domain.to_string(), |url| url.host().to_string());
    let answer = agent()
        .get(DNS_OVER_HTTPS)
        .query("name", &name)
        .query("type", "MX")
        .set("accept", "application/dns-json")
        .call()
        .ok()
        .and_then(|response| response.into_string().ok())
        .map_or(MailAnswer::Unknown, |body| parse_mail_answer(&body));
    mail_outcome(domain, answer)
}

fn check_path(path: &Path) -> Outcome {
    match std::fs::metadata(path) {
        Ok(found) if found.is_dir() => Outcome { works: true, sentence: tr("Works: that folder exists.") },
        Ok(_) => Outcome { works: true, sentence: tr("Works: that file exists.") },
        Err(error) => Outcome::failed(match error.kind() {
            io::ErrorKind::NotFound => tr("Broken: there is nothing at that path."),
            io::ErrorKind::PermissionDenied => {
                tr("Could not confirm: Launchtype is not allowed to read that path.")
            }
            _ => format_args(
                &tr("Could not confirm: {reason}"),
                &[("reason", Arg::Str(&error.to_string()))],
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A server that answers each request by its path, for as long as the test
    /// runs. Returns its address, without a trailing slash.
    fn serve() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let own_address = address.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut request = [0u8; 2048];
                let read = stream.read(&mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..read]);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or(path);
                let head = match path {
                    "/ok" => "200 OK\r\n".to_string(),
                    "/forbidden" => "403 Forbidden\r\n".to_string(),
                    "/down" => "500 Internal Server Error\r\n".to_string(),
                    "/moved" => format!("302 Found\r\nLocation: {own_address}/login?next=x\r\n"),
                    "/login" => "200 OK\r\n".to_string(),
                    "/dead-end" => format!("302 Found\r\nLocation: {own_address}/gone\r\n"),
                    "/loop" => format!("302 Found\r\nLocation: {own_address}/loop\r\n"),
                    _ => "404 Not Found\r\n".to_string(),
                };
                let _ = write!(stream, "HTTP/1.1 {head}Content-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
        address
    }

    fn broken_sentence(url: &str) -> String {
        let outcome = check_link(url);
        assert!(!outcome.works, "{url}: {}", outcome.sentence);
        outcome.sentence
    }

    #[test]
    fn a_page_that_answers_works() {
        let server = serve();
        let outcome = check_link(&format!("{server}/ok"));
        assert!(outcome.works);
        assert_eq!(outcome.sentence, tr("Works."));
    }

    #[test]
    fn a_missing_page_is_broken() {
        let server = serve();
        assert!(broken_sentence(&format!("{server}/nope")).contains("404"));
    }

    #[test]
    fn a_refusal_and_a_server_error_do_not_count_as_working() {
        let server = serve();
        assert!(broken_sentence(&format!("{server}/forbidden")).contains("403"));
        assert!(broken_sentence(&format!("{server}/down")).contains("500"));
    }

    #[test]
    fn a_redirect_says_where_it_landed() {
        let server = serve();
        let outcome = check_link(&format!("{server}/moved"));
        assert!(outcome.works);
        assert!(outcome.sentence.contains(&format!("{server}/login")), "{}", outcome.sentence);
    }

    #[test]
    fn a_redirect_to_a_missing_page_is_broken() {
        let server = serve();
        assert!(broken_sentence(&format!("{server}/dead-end")).contains("404"));
    }

    #[test]
    fn a_redirect_loop_is_broken() {
        let server = serve();
        assert_eq!(
            broken_sentence(&format!("{server}/loop")),
            tr("Broken: the link keeps redirecting and never arrives.")
        );
    }

    #[test]
    fn a_server_that_is_not_listening_is_broken() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let sentence = broken_sentence(&format!("http://{address}/"));
        assert!(sentence.contains("127.0.0.1"), "{sentence}");
    }

    #[test]
    fn a_host_that_does_not_exist_is_broken() {
        // `.invalid` is reserved and never resolves (RFC 2606).
        let sentence = broken_sentence("https://launchtype.invalid/");
        assert!(sentence.contains("launchtype.invalid"), "{sentence}");
    }

    #[test]
    fn paths_are_checked_on_disk() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();

        let found = check_path(&file);
        assert!(found.works);
        assert_eq!(found.sentence, tr("Works: that file exists."));

        let found = check_path(folder.path());
        assert!(found.works);
        assert_eq!(found.sentence, tr("Works: that folder exists."));

        let missing = check_path(&folder.path().join("missing.txt"));
        assert!(!missing.works);
        assert_eq!(missing.sentence, tr("Broken: there is nothing at that path."));
    }

    /// Needs the network: Cloudflare, gmail.com's and example.com's records.
    #[test]
    #[ignore]
    fn email_domains_are_asked_about_for_real() {
        assert!(check_email("gmail.com").works);
        assert!(!check_email("example.com").works);
        assert!(!check_email("launchtype-no-such-domain-a1b2c3.com").works);
    }

    /// Needs the network, and real TLS.
    #[test]
    #[ignore]
    fn real_sites_are_checked_for_real() {
        assert!(check_link("https://example.com/").works);
        assert!(broken_sentence("https://expired.badssl.com/").contains("certificate"));
    }
}
