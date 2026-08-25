//! `{{query}}` — the one placeholder this machine cannot answer.
//!
//! Every other placeholder in [`crate::portable`] is resolved from the
//! computer the command runs on. `{{query}}` is resolved from the person
//! running it: a web search is stored once as
//! `https://www.google.com/search?q={{query}}` and asks for the words each
//! time, instead of being one saved command per thing you ever searched for.
//!
//! A command may hold several. They are filled in the order they appear —
//! path first, then arguments — which is the order the user is asked for
//! them, so "parameter 1" always means the first `{{query}}` you would read.
//!
//! Answers landing inside a URL are percent-encoded, because that is where
//! this is used and a space would otherwise produce an address no opener can
//! follow. Everywhere else the answer is passed through exactly as typed: a
//! `{{query}}` standing in for a file name must stay a file name.

use crate::portable::{looks_like_url, next_placeholder, placeholder_names};

/// The placeholder name, without its braces.
pub const NAME: &str = "query";

/// `"{{query}}"` — what the insert-variable menu drops into a field.
pub fn placeholder() -> String {
    crate::portable::placeholder(NAME)
}

/// How many answers a command's `path` and `args` ask for between them.
pub fn count(path: &str, args: &str) -> usize {
    count_in(path) + count_in(args)
}

fn count_in(template: &str) -> usize {
    placeholder_names(template).filter(|name| name == NAME).count()
}

/// Substitute `answers` into a command's two fields, in order.
///
/// Every other placeholder is left untouched for [`crate::portable::expand`]
/// to deal with at launch. A `{{query}}` with no answer left is also left
/// alone rather than dropped — a visible `{{query}}` in whatever opens says
/// what went wrong, an empty search box does not.
pub fn fill(path: &str, args: &str, answers: &[String]) -> (String, String) {
    let mut answers = answers.iter();
    let path = fill_in(path, &mut answers);
    let args = fill_in(args, &mut answers);
    (path, args)
}

fn fill_in<'a>(template: &str, answers: &mut impl Iterator<Item = &'a String>) -> String {
    let mut out = String::with_capacity(template.len());
    // Bytes of `template` already written out. Tracked as an absolute offset
    // rather than as a shrinking slice so the prefix below is easy to take.
    let mut consumed = 0;
    while let Some((range, name)) = next_placeholder(&template[consumed..]) {
        let (start, end) = (consumed + range.start, consumed + range.end);
        out.push_str(&template[consumed..start]);
        if !name.eq_ignore_ascii_case(NAME) {
            // Somebody else's placeholder: copy it through braces and all.
            out.push_str(&template[start..end]);
        } else {
            match answers.next() {
                // What precedes the placeholder decides the encoding, and it
                // is read off the template rather than off `out`: an answer
                // already substituted could otherwise change how the next one
                // is read.
                Some(answer) => {
                    let value = if inside_url(&template[..start]) {
                        encode(answer)
                    } else {
                        answer.clone()
                    };
                    out.push_str(&value);
                }
                None => out.push_str(&placeholder()),
            }
        }
        consumed = end;
    }
    out.push_str(&template[consumed..]);
    out
}

/// Whether the placeholder following `prefix` sits inside a URL.
///
/// What matters is the token the placeholder is part of, which is whatever
/// follows the last comma, space or quote: arguments are one comma-separated
/// string, and no URL contains any of those characters.
///
/// The token is not required to *start* with the scheme. A scheme routinely
/// sits behind something else — `--app=https://example.com/{{query}}` is how
/// Chrome is told to open a site as an app — so the `://` is looked for
/// inside the token, with [`looks_like_url`] still covering the schemes that
/// have no slashes (`mailto:`).
fn inside_url(prefix: &str) -> bool {
    let token = prefix.rsplit([',', ' ', '\t', '"', '\'']).next().unwrap_or(prefix);
    token.contains("://") || looks_like_url(token)
}

/// Percent-encode everything outside RFC 3986's unreserved set.
///
/// Deliberately strict, and space becomes `%20` rather than `+`: the same
/// answer has to work in a query string (`?q={{query}}`) and in a path
/// (`/wiki/{{query}}`), and `+` only means a space in the first.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn counts_every_occurrence_across_both_fields() {
        assert_eq!(count("{{browser}}", "https://example.com/?q={{query}}"), 1);
        assert_eq!(count("{{query}}", "{{query}} {{query}}"), 3);
        // Spelling and spacing are as forgiving as they are for any other
        // placeholder, because `placeholder_names` trims and folds case.
        assert_eq!(count("", "{{ QUERY }}"), 1);
        // Nothing to ask for: an ordinary command must not enter the prompt.
        assert_eq!(count(r"C:\tools\a.exe", "--flag, {{home}}\\notes.txt"), 0);
    }

    /// The flagship case: one stored command, a different search every time.
    #[test]
    fn a_google_search_command_takes_its_words_at_launch() {
        let (path, args) = fill(
            "{{browser}}",
            "https://www.google.com/search?q={{query}}",
            &answers(["screen reader reviews"].as_slice()),
        );
        assert_eq!(path, "{{browser}}", "other placeholders are left for expand()");
        assert_eq!(args, "https://www.google.com/search?q=screen%20reader%20reviews");
    }

    /// Wikipedia puts the answer in the path rather than in a query string,
    /// which is why the encoding cannot use `+` for a space.
    #[test]
    fn a_wikipedia_article_lands_in_the_url_path() {
        let (_, args) = fill(
            "{{firefox}}",
            "https://en.wikipedia.org/wiki/{{query}}",
            &answers(["Screen reader"].as_slice()),
        );
        assert_eq!(args, "https://en.wikipedia.org/wiki/Screen%20reader");

        // Accents and punctuation survive the round trip as UTF-8 bytes.
        let (_, args) = fill("", "https://es.wikipedia.org/wiki/{{query}}", &answers(&["Añejo"]));
        assert_eq!(args, "https://es.wikipedia.org/wiki/A%C3%B1ejo");
    }

    #[test]
    fn several_parameters_are_filled_in_reading_order() {
        let (path, args) = fill(
            r"C:\tools\{{query}}.exe",
            "--from, {{query}}, --to, {{query}}",
            &answers(&["convert", "png", "jpg"]),
        );
        assert_eq!(path, r"C:\tools\convert.exe", "the path is asked for first");
        assert_eq!(args, "--from, png, --to, jpg");
    }

    /// A `{{query}}` that is not part of a URL is a file name, a search term
    /// for a desktop app, a note — encoding it would corrupt it.
    #[test]
    fn only_urls_are_percent_encoded() {
        let (_, args) = fill("", "-o, {{query}}", &answers(&["My Report.pdf"]));
        assert_eq!(args, "-o, My Report.pdf");

        // The decision is per segment, so one argument list can hold both.
        let (_, args) =
            fill("", "-o, {{query}}, https://example.com/s?q={{query}}", &answers(&["a b", "a b"]));
        assert_eq!(args, "-o, a b, https://example.com/s?q=a%20b");

        // A quoted URL argument is still a URL.
        let (_, args) = fill("", "\"https://example.com/s?q={{query}}\"", &answers(&["a b"]));
        assert_eq!(args, "\"https://example.com/s?q=a%20b\"");
    }

    /// A URL does not have to start its argument. Chrome's site-as-an-app
    /// switch puts one behind a flag, and a command that hands a URL to a
    /// shell puts one behind the program name.
    #[test]
    fn a_url_is_recognised_wherever_it_starts() {
        let (_, args) = fill("", "--app=https://example.com/s?q={{query}}", &answers(&["a b"]));
        assert_eq!(args, "--app=https://example.com/s?q=a%20b");

        let (_, args) =
            fill("", "/c, echo https://example.com/s?q={{query}}> out.txt", &answers(&["a b"]));
        assert_eq!(args, "/c, echo https://example.com/s?q=a%20b> out.txt");

        // A path that merely sits next to a word is still not a URL.
        let (_, args) = fill("", "/c, echo {{query}}> out.txt", &answers(&["a b"]));
        assert_eq!(args, "/c, echo a b> out.txt");
    }

    /// A Windows path is not a URL, however much `C:` looks like a scheme.
    #[test]
    fn a_drive_letter_is_not_a_scheme() {
        let (path, _) = fill(r"C:\tools\{{query}}", "", &answers(&["my file.txt"]));
        assert_eq!(path, r"C:\tools\my file.txt");
    }

    /// Fewer answers than placeholders can only happen if a caller miscounts;
    /// leaving the placeholder visible makes that obvious instead of silently
    /// searching for nothing.
    #[test]
    fn a_missing_answer_leaves_its_placeholder_in_place() {
        let (_, args) = fill("", "{{query}} and {{query}}", &answers(&["first"]));
        assert_eq!(args, "first and {{query}}");
    }

    #[test]
    fn an_unterminated_placeholder_is_left_alone() {
        let (_, args) = fill("", "{{query", &answers(&["x"]));
        assert_eq!(args, "{{query");
    }

    #[test]
    fn encoding_covers_the_characters_a_url_would_reinterpret() {
        assert_eq!(encode("a b"), "a%20b");
        assert_eq!(encode("rust&c++"), "rust%26c%2B%2B");
        assert_eq!(encode("50%"), "50%25");
        assert_eq!(encode("a/b?c#d"), "a%2Fb%3Fc%23d");
        // Unreserved characters are never touched.
        assert_eq!(encode("A-Za-z0-9_.~"), "A-Za-z0-9_.~");
    }
}
