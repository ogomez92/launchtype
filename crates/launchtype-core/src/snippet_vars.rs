//! `{{...}}` inside a snippet — the substitution that asks for its own text.
//!
//! A snippet is stored text that goes to the clipboard as it stands. That
//! covers a signature. It does not cover the mail you send twice a week with
//! two words changed, which without this is one snippet per report: "te envío
//! el informe de {{informe}}, solicitado el {{fecha de solicitud}}" is one
//! snippet, and the two words are asked for on the way to the clipboard.
//!
//! One rule decides which is which: **a name Launchtype knows is filled in, a
//! name it does not know is asked for.** What it knows is everything in
//! [`crate::portable::Vars`] — this machine's folders and browsers, the clock
//! placeholders below, and the names the user has defined for themselves
//! ([`crate::placeholders`]) — which is deliberately the same vocabulary a
//! command's path and arguments expand against. A name means the same thing
//! wherever it is written.
//!
//! `{{query}}` is the exception, and stays positional so that it means in a
//! snippet exactly what it means in a command (see [`crate::query`]): one
//! question each, however many there are. A name asks once however often it
//! appears, which is the point of naming it — `{{informe}}` twice in a letter
//! is one thing said twice, not two things to type.
//!
//! Questions are found through the user's own variables as well as in the
//! snippet itself, so a variable can be written in terms of another and a
//! variable holding `{{informe}}` still asks for it.
//!
//! Nothing here is encoded on the way out, and nothing is normalised. A
//! command's `{{query}}` may land inside a URL and has to survive it; a
//! snippet is prose going to the clipboard, and prose is passed through as it
//! was typed.

use chrono::{DateTime, Datelike, Local, Timelike};

use crate::i18n::tr;
use crate::portable::{next_placeholder, VarSpec, VarValue, Vars};

/// The placeholders answered from the clock, in the order the insert-variable
/// menu offers them.
///
/// Never suggested: [`crate::portable::suggest`] rewrites literal *paths* back
/// into placeholders, and a date is not a path.
pub const CLOCK_VARS: &[VarSpec] = &[
    VarSpec { name: "date", suggest: false },
    VarSpec { name: "fecha", suggest: false },
    VarSpec { name: "time", suggest: false },
    VarSpec { name: "hora", suggest: false },
];

/// What a clock placeholder reads at `now`, or `None` for any other name.
///
/// The name picks the convention rather than the value — the Spanish name
/// writes `25/08/2026`, the English one `08/25/2026` — because a snippet is
/// written in the language it will be *read* in, which is not always the
/// language the app is running in. Matched case-insensitively, like every
/// other placeholder.
pub fn clock_value(name: &str, now: DateTime<Local>) -> Option<String> {
    match key(name).as_str() {
        "date" => Some(format!("{:02}/{:02}/{}", now.month(), now.day(), now.year())),
        "fecha" => Some(format!("{:02}/{:02}/{}", now.day(), now.month(), now.year())),
        "time" => {
            let (pm, hour) = now.hour12();
            Some(format!("{}:{:02} {}", hour, now.minute(), if pm { "PM" } else { "AM" }))
        }
        "hora" => Some(format!("{:02}:{:02}", now.hour(), now.minute())),
        _ => None,
    }
}

/// The clock placeholders as [`Vars`] entries, for the caller that knows what
/// time it is. [`VarValue::Text`], because a date is prose: the separator
/// normalisation a path gets would write it `25\08\2026`.
pub fn clock_vars(now: DateTime<Local>) -> Vec<(String, VarValue)> {
    CLOCK_VARS
        .iter()
        .filter_map(|spec| {
            Some((spec.name.to_string(), VarValue::Text(clock_value(spec.name, now)?)))
        })
        .collect()
}

/// The one-line description shown next to a clock placeholder in the insert
/// menu, or the name itself for anything else.
///
/// A `match` over translated literals for the same reason
/// [`crate::portable::description`] is one: `scripts/check_msgids.py` has to
/// be able to see every msgid, and one it cannot see falls back to English.
pub fn description(name: &str) -> String {
    match name {
        "date" => tr("Today's date, month/day/year"),
        "fecha" => tr("Today's date, day/month/year"),
        "time" => tr("The time now, 12-hour"),
        "hora" => tr("The time now, 24-hour"),
        other => other.to_string(),
    }
}

/// One thing a snippet has to be told before it can be copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// `{{query}}`: one question per occurrence, in reading order.
    Positional,
    /// `{{informe}}`: asked once, then filled in everywhere it appears. The
    /// string is the name as it was written, because that is what gets read
    /// out when the question is put.
    Named(String),
}

impl Ask {
    /// The name to announce, or `None` for the anonymous `{{query}}`.
    pub fn name(&self) -> Option<&str> {
        match self {
            Ask::Positional => None,
            Ask::Named(name) => Some(name),
        }
    }
}

/// Every question `contents` asks, in the order they will be put — including
/// the ones inside the variables it names.
pub fn questions(contents: &str, vars: &Vars) -> Vec<Ask> {
    let mut asks = Vec::new();
    collect(contents, vars, &mut Vec::new(), &mut asks);
    asks
}

fn collect(template: &str, vars: &Vars, open: &mut Vec<String>, asks: &mut Vec<Ask>) {
    for name in names(template) {
        let folded = key(name);
        if folded == crate::query::NAME {
            asks.push(Ask::Positional);
            continue;
        }
        match vars.get(name) {
            // A variable of the user's own may hold questions of its own.
            Some(VarValue::Text(text)) if !open.contains(&folded) => {
                let text = text.clone();
                open.push(folded);
                collect(&text, vars, open, asks);
                open.pop();
            }
            // A folder, a browser, or a name that has reached itself: all
            // answered — or given up on — without asking anybody.
            Some(_) => {}
            None if named_index(asks, name).is_none() => asks.push(Ask::Named(name.to_string())),
            None => {}
        }
    }
}

/// Fill `contents` in for the clipboard: known names from `vars`, questions
/// from `answers`, which lines up entry for entry with [`questions`].
///
/// A question with no answer keeps its placeholder rather than vanishing, for
/// the reason [`crate::query::fill`] keeps its own: `{{informe}}` arriving in
/// somebody's inbox says what went wrong, a hole in the sentence does not.
pub fn fill(contents: &str, answers: &[String], vars: &Vars) -> String {
    let asks = questions(contents, vars);
    let mut out = String::with_capacity(contents.len());
    let mut positional = 0;
    substitute(contents, vars, &asks, answers, &mut positional, &mut Vec::new(), &mut out);
    out
}

/// `positional` counts the `{{query}}`s seen so far, and is threaded through
/// the recursion rather than restarted: the second `{{query}}` is the second
/// one in reading order whichever variable it turned up in, which is the order
/// [`collect`] put the questions in.
fn substitute(
    template: &str,
    vars: &Vars,
    asks: &[Ask],
    answers: &[String],
    positional: &mut usize,
    open: &mut Vec<String>,
    out: &mut String,
) {
    let mut rest = template;
    while let Some((range, name)) = next_placeholder(rest) {
        let (start, end) = (range.start, range.end);
        out.push_str(&rest[..start]);
        // Braces and all, for the cases with nothing to put in their place.
        let verbatim = &rest[start..end];
        let folded = key(name);
        if folded == crate::query::NAME {
            out.push_str(positional_answer(asks, answers, *positional).unwrap_or(verbatim));
            *positional += 1;
        } else {
            match vars.get(name) {
                Some(VarValue::Path(path)) => out.push_str(path),
                // Names a handler, not a path: contributes no text.
                Some(VarValue::DefaultOpener) => {}
                Some(VarValue::Text(text)) if !open.contains(&folded) => {
                    let text = text.clone();
                    open.push(folded);
                    substitute(&text, vars, asks, answers, positional, open, out);
                    open.pop();
                }
                // A name that has reached itself. It stops here, on screen,
                // which is the only ending the user can do anything about.
                Some(_) => out.push_str(verbatim),
                None => out.push_str(named_answer(asks, answers, name).unwrap_or(verbatim)),
            }
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
}

/// The answer given to the `nth` `{{query}}`.
fn positional_answer<'a>(asks: &[Ask], answers: &'a [String], nth: usize) -> Option<&'a str> {
    let index = asks
        .iter()
        .enumerate()
        .filter(|(_, ask)| **ask == Ask::Positional)
        .map(|(index, _)| index)
        .nth(nth)?;
    answers.get(index).map(String::as_str)
}

/// The answer given for `name`, whatever case each occurrence was written in.
fn named_answer<'a>(asks: &[Ask], answers: &'a [String], name: &str) -> Option<&'a str> {
    answers.get(named_index(asks, name)?).map(String::as_str)
}

/// Where `name` sits in `asks`, comparing the way [`key`] does.
fn named_index(asks: &[Ask], name: &str) -> Option<usize> {
    let folded = key(name);
    asks.iter().position(|ask| ask.name().is_some_and(|other| key(other) == folded))
}

/// How two spellings of one placeholder are compared: case folded, the way
/// [`crate::portable::placeholder_names`] folds them. Not
/// `eq_ignore_ascii_case` — these names are written in Spanish as often as in
/// English, and `Ñ` has to fold to `ñ`.
fn key(name: &str) -> String {
    name.to_lowercase()
}

/// Every placeholder in `template`, as written (trimmed), in order.
///
/// [`crate::portable::placeholder_names`] would nearly do, but it lowercases,
/// and a question has to be put using the name the person actually typed.
fn names(template: &str) -> impl Iterator<Item = &str> + '_ {
    let mut rest = template;
    std::iter::from_fn(move || {
        let (range, name) = next_placeholder(rest)?;
        rest = &rest[range.end..];
        Some(name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// 2026-08-25, 15:07 local — a day and an hour that both read differently
    /// on the two sides of the Atlantic, so a swapped format cannot pass.
    fn at_three_pm() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 8, 25, 15, 7, 0).unwrap()
    }

    /// What a snippet is filled against: the clock, one folder to prove the
    /// machine catalog is in scope too, and whatever the test defines.
    fn vars(own: &[(&str, &str)]) -> Vars {
        Vars::new(
            [("documents".to_string(), VarValue::Path(r"C:\Users\nitropc\Documents".to_string()))],
            true,
            '\\',
        )
        .with(clock_vars(at_three_pm()))
        .with(own.iter().map(|(n, t)| (n.to_string(), VarValue::Text(t.to_string()))))
    }

    /// A user who has defined nothing of their own, which is most of them.
    fn plain() -> Vars {
        vars(&[])
    }

    fn answers(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn named(names: &[&str]) -> Vec<Ask> {
        names.iter().map(|n| Ask::Named((*n).to_string())).collect()
    }

    /// The flagship case, from the user who asked for this: one stored mail,
    /// the two words that change asked for on the way to the clipboard.
    #[test]
    fn a_mail_template_asks_for_the_words_that_change() {
        let snippet = "Hola, te envío el informe de {{informe}}, solicitado el {{fecha}}.";
        assert_eq!(questions(snippet, &plain()), named(&["informe"]), "{{fecha}} is not a question");
        assert_eq!(
            fill(snippet, &answers(&["ventas de agosto"]), &plain()),
            "Hola, te envío el informe de ventas de agosto, solicitado el 25/08/2026."
        );
    }

    /// Naming a placeholder is what buys this over `{{query}}`: say it twice,
    /// answer it once.
    #[test]
    fn a_name_is_asked_once_and_filled_everywhere() {
        let snippet = "Dear {{name}},\n\nThanks, {{name}}!";
        assert_eq!(questions(snippet, &plain()), named(&["name"]));
        assert_eq!(
            fill(snippet, &answers(&["Agustín"]), &plain()),
            "Dear Agustín,\n\nThanks, Agustín!"
        );
    }

    /// Spelling and spacing are as forgiving as they are anywhere else, and
    /// the *first* spelling is the one the question is put in.
    #[test]
    fn one_name_written_two_ways_is_still_one_question() {
        let snippet = "{{Informe}} / {{ informe }} / {{INFORME}}";
        assert_eq!(questions(snippet, &plain()), named(&["Informe"]));
        assert_eq!(fill(snippet, &answers(&["x"]), &plain()), "x / x / x");
    }

    /// Accented names fold too: Spanish is a first-class language for these,
    /// not an afterthought, so `{{Año}}` and `{{año}}` are one question.
    #[test]
    fn folding_is_not_ascii_only() {
        assert_eq!(questions("{{Año}} {{año}}", &plain()), named(&["Año"]));
        assert_eq!(fill("{{Año}} {{año}}", &answers(&["2026"]), &plain()), "2026 2026");
    }

    /// `{{query}}` keeps the meaning it has in a command: each one is its own
    /// question, so two of them take two different answers.
    #[test]
    fn query_stays_positional_the_way_commands_use_it() {
        let snippet = "from {{query}} to {{query}}";
        assert_eq!(questions(snippet, &plain()), vec![Ask::Positional, Ask::Positional]);
        assert_eq!(
            fill(snippet, &answers(&["Madrid", "Bilbao"]), &plain()),
            "from Madrid to Bilbao"
        );
    }

    /// Mixed, because a real snippet mixes them: the answers arrive in the
    /// order the questions were put, and each kind finds its own.
    #[test]
    fn positional_and_named_share_one_answer_list() {
        let snippet = "{{query}} {{who}} {{query}} {{who}}";
        assert_eq!(
            questions(snippet, &plain()),
            vec![Ask::Positional, Ask::Named("who".into()), Ask::Positional]
        );
        assert_eq!(fill(snippet, &answers(&["a", "b", "c"]), &plain()), "a b c b");
    }

    /// The name picks the convention, not the language the app is running in:
    /// a Spanish speaker writing to a US client wants `{{date}}` in that one
    /// line and `{{fecha}}` everywhere else.
    #[test]
    fn date_and_time_are_written_the_way_their_name_is() {
        let now = at_three_pm();
        assert_eq!(clock_value("date", now).unwrap(), "08/25/2026");
        assert_eq!(clock_value("fecha", now).unwrap(), "25/08/2026");
        assert_eq!(clock_value("time", now).unwrap(), "3:07 PM");
        assert_eq!(clock_value("hora", now).unwrap(), "15:07");
        // Case folded like every other placeholder, and a name off the clock
        // has no reading at all.
        assert_eq!(clock_value("FECHA", now).unwrap(), "25/08/2026");
        assert_eq!(clock_value("informe", now), None);
    }

    /// Midnight and noon are where a 12-hour clock goes wrong.
    #[test]
    fn twelve_hour_time_names_midnight_and_noon() {
        let at = |h| Local.with_ymd_and_hms(2026, 8, 25, h, 0, 0).unwrap();
        assert_eq!(clock_value("time", at(0)).unwrap(), "12:00 AM");
        assert_eq!(clock_value("time", at(12)).unwrap(), "12:00 PM");
        assert_eq!(clock_value("hora", at(0)).unwrap(), "00:00");
    }

    /// A snippet with nothing to ask must not enter the prompt at all, and one
    /// that only wants the date must still come out filled in.
    #[test]
    fn plain_snippets_are_untouched_and_never_ask() {
        assert!(questions("my_email@example.com", &plain()).is_empty());
        assert_eq!(fill("my_email@example.com", &[], &plain()), "my_email@example.com");

        assert!(questions("Madrid, {{fecha}}", &plain()).is_empty(), "the computer knows the date");
        assert_eq!(fill("Madrid, {{fecha}}", &[], &plain()), "Madrid, 25/08/2026");
    }

    /// One vocabulary for the whole app: a name means the same in a snippet as
    /// it does in a command, so a folder is filled in rather than asked for.
    #[test]
    fn the_machine_catalog_is_in_scope_too() {
        assert!(questions("in {{documents}}", &plain()).is_empty());
        assert_eq!(fill("in {{documents}}", &[], &plain()), r"in C:\Users\nitropc\Documents");
    }

    /// A name of the user's own is a placeholder they have already answered,
    /// so it fills itself in and is never asked for.
    #[test]
    fn a_users_own_placeholder_answers_itself() {
        let vars = vars(&[("hi", "Hola, ¿qué tal?")]);
        let snippet = "{{HI}} {{informe}} {{hi}}";
        assert_eq!(questions(snippet, &vars), named(&["informe"]), "only the unknown one asks");
        assert_eq!(
            fill(snippet, &answers(&["ventas"]), &vars),
            "Hola, ¿qué tal? ventas Hola, ¿qué tal?"
        );
        // Without the definition the same name is simply a question.
        assert_eq!(questions(snippet, &plain()), named(&["HI", "informe"]));
    }

    /// A variable written out of other variables, which is what the user asked
    /// for: `{{saludo}}` holding a `{{fecha}}` gives today's date.
    #[test]
    fn a_placeholder_may_be_written_out_of_other_placeholders() {
        let vars = vars(&[("saludo", "Hola. Madrid, {{fecha}}. {{firma}}"), ("firma", "Un saludo")]);
        assert!(questions("{{saludo}}", &vars).is_empty());
        assert_eq!(fill("{{saludo}}", &[], &vars), "Hola. Madrid, 25/08/2026. Un saludo");
    }

    /// And a variable may hold a question, which the snippet using it then
    /// asks — including a `{{query}}`, counted in reading order across the lot.
    #[test]
    fn a_question_inside_a_variable_is_still_asked() {
        let vars = vars(&[("saludo", "Hola {{nombre}}, {{query}}")]);
        assert_eq!(
            questions("{{query}} {{saludo}} {{nombre}}", &vars),
            vec![Ask::Positional, Ask::Named("nombre".into()), Ask::Positional]
        );
        assert_eq!(
            fill("{{query}} {{saludo}} {{nombre}}", &answers(&["Buenas.", "Ana", "¿qué tal?"]), &vars),
            "Buenas. Hola Ana, ¿qué tal? Ana"
        );
    }

    /// A name that reaches itself has to stop somewhere, and stopping with the
    /// placeholder on screen is the only ending the user can act on.
    #[test]
    fn a_placeholder_that_reaches_itself_stops() {
        let vars = vars(&[("a", "a then {{b}}"), ("b", "b then {{a}}"), ("me", "me then {{me}}")]);
        assert!(questions("{{a}}", &vars).is_empty());
        assert_eq!(fill("{{a}}", &[], &vars), "a then b then {{a}}");
        assert_eq!(fill("{{me}} {{me}}", &[], &vars), "me then {{me}} me then {{me}}");
    }

    /// The built-ins win. A hand-edited file could define `{{fecha}}`, and the
    /// date it stood for would be wrong from the next day onwards.
    #[test]
    fn a_users_placeholder_cannot_shadow_a_builtin() {
        let vars = vars(&[("fecha", "whenever"), ("query", "whatever")]);
        assert_eq!(fill("{{fecha}}", &[], &vars), "25/08/2026");
        assert_eq!(questions("{{query}}", &vars), vec![Ask::Positional]);
    }

    /// Fewer answers than questions can only happen if a caller miscounts;
    /// leaving the placeholder visible makes that obvious instead of pasting a
    /// sentence with a hole in it.
    #[test]
    fn a_missing_answer_leaves_its_placeholder_in_place() {
        assert_eq!(fill("{{a}} {{b}}", &answers(&["one"]), &plain()), "one {{b}}");
        assert_eq!(fill("{{query}} {{query}}", &answers(&["one"]), &plain()), "one {{query}}");
    }

    /// A missing brace must stay where it was typed. Reported from a real
    /// snippet: `{{w111}` (one closing brace) sat two lines above a real
    /// `{{firma}}`, and pairing that first `{{` with the *last* `}}` turned
    /// the whole paragraph into one question — which is what the user saw,
    /// and had no way to understand.
    #[test]
    fn a_missing_brace_does_not_swallow_the_paragraph() {
        let vars = vars(&[("w111", "1.1.1 non text content")]);
        let snippet = "- {{w111}: Logo sin alt text\n\n{{firma}}";
        // Only the genuine placeholder asks, and it asks by its own name.
        assert_eq!(questions(snippet, &vars), named(&["firma"]));
        assert_eq!(
            fill(snippet, &answers(&["Un saludo"]), &vars),
            "- {{w111}: Logo sin alt text\n\nUn saludo",
            "the typo stays on screen where it can be seen and fixed"
        );
        // Typed properly, it is the variable it was meant to be.
        assert_eq!(
            fill("- {{w111}}: Logo sin alt text", &[], &vars),
            "- 1.1.1 non text content: Logo sin alt text"
        );
    }

    /// Text that merely looks like a placeholder is text.
    #[test]
    fn braces_that_are_not_placeholders_are_left_alone() {
        assert_eq!(questions("{{}} and {{", &plain()), Vec::new());
        assert_eq!(fill("{{}} and {{", &[], &plain()), "{{}} and {{");
        // JSON in a snippet keeps its single braces.
        assert_eq!(fill(r#"{"a": 1}"#, &[], &plain()), r#"{"a": 1}"#);
    }

    /// An answer is pasted as typed. A snippet is prose, so the encoding a
    /// command's `{{query}}` needs inside a URL would corrupt it here.
    #[test]
    fn answers_are_never_encoded() {
        assert_eq!(
            fill("q: {{query}}", &answers(&["cats & dogs, 50%"]), &plain()),
            "q: cats & dogs, 50%"
        );
    }
}
