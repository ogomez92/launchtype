//! The emoji catalogue behind `:` mode: every emoji with the name a screen
//! reader would give it, found by typing that name or anything close to it.
//!
//! The table is compiled into the binary rather than read from the OS. Neither
//! platform will hand us one: Windows ships no emoji-name table at all, and
//! the macOS names live inside a private framework. Baking in CLDR's — the
//! same data every emoji picker uses — also keeps Windows and macOS saying
//! exactly the same thing, and needs no network at build or run time.
//!
//! Regenerate with `python scripts/gen_emoji.py`.

use std::sync::OnceLock;

/// One tab-separated row per emoji, in the order emoji keyboards list them
/// (smileys first): `emoji`, then a name and its keywords per language.
const TABLE: &str = include_str!("../data/emoji.txt");

/// The languages [`TABLE`] carries, in column order: language `i` has its name
/// in column `1 + i * 2` and its keywords in the column after that. Adding one
/// means adding it to `LANGUAGES` in `scripts/gen_emoji.py` too.
const LANGUAGES: [&str; 2] = ["en", "es"];

const COLUMNS: usize = 1 + LANGUAGES.len() * 2;

#[derive(Debug)]
pub struct Emoji {
    /// The characters to put on the clipboard.
    pub emoji: &'static str,
    /// CLDR's short name — what a screen reader calls this emoji.
    pub name: &'static str,
    /// [`fold`]ed `name`, for matching against a folded query.
    name_key: String,
    /// CLDR's keywords, folded and space separated. Searched but never shown,
    /// so that typing "laugh" can still list "face with tears of joy".
    keywords_key: String,
}

/// Every emoji, named in `language`, falling back to English for a language
/// the table has no column for. Parsed once per language, on first use.
pub fn table(language: &str) -> &'static [Emoji] {
    static TABLES: [OnceLock<Vec<Emoji>>; LANGUAGES.len()] =
        [const { OnceLock::new() }; LANGUAGES.len()];
    let index = LANGUAGES.iter().position(|&code| code == language).unwrap_or(0);
    TABLES[index].get_or_init(|| parse(index)).as_slice()
}

/// The emoji `query` describes, best match first. An empty query is every
/// emoji, in table order.
///
/// This does not use [`crate::search::fuzzy_search`]. That scorer is tuned for
/// short command names — it pays most for a match near the start of the text —
/// and emoji are searched against a long tail of keywords, where the wanted
/// word is usually nowhere near the front. Ranked by it, "laugh" put five
/// unrelated emoji above "face with tears of joy" and "coffee" never reached
/// "hot beverage" at all.
pub fn search(language: &str, query: &str) -> Vec<&'static Emoji> {
    let table = table(language);
    let query = fold(query);
    let query = query.trim();
    if query.is_empty() {
        return table.iter().collect();
    }
    let mut hits: Vec<(u8, usize, &'static Emoji)> = table
        .iter()
        .filter_map(|emoji| {
            rank(emoji, query).map(|tier| (tier, emoji.name.chars().count(), emoji))
        })
        .collect();
    // Shorter name first within a tier: it is the plainer, more canonical
    // emoji ("red heart" ahead of "heart with arrow"). Counted in characters,
    // not bytes, or an accent would cost a Spanish name a place. Sorting is
    // stable, so an exact tie keeps the palette order emoji keyboards use.
    hits.sort_by_key(|&(tier, length, _)| (tier, length));
    hits.into_iter().map(|(_, _, emoji)| emoji).collect()
}

/// How well `query` (already folded and trimmed) describes `emoji`, as a tier
/// number where lower is better, or `None` if it does not describe it at all.
fn rank(emoji: &Emoji, query: &str) -> Option<u8> {
    if emoji.name_key == query {
        return Some(0);
    }
    if starts_a_word(&emoji.name_key, query) {
        return Some(1);
    }
    if emoji.name_key.contains(query) {
        return Some(2);
    }
    if starts_a_word(&emoji.keywords_key, query) {
        return Some(3);
    }
    if emoji.keywords_key.contains(query) {
        return Some(4);
    }
    // Last resort, for a description whose words are spread across the name
    // and the keywords ("crying cat", "red car").
    let words_found = query
        .split_whitespace()
        .all(|word| emoji.name_key.contains(word) || emoji.keywords_key.contains(word));
    words_found.then_some(5)
}

/// Whether any word of `text` starts with `prefix`, so that "heart" finds
/// "red heart" and "laugh" finds "rolling on the floor laughing".
fn starts_a_word(text: &str, prefix: &str) -> bool {
    text.split(' ').any(|word| word.starts_with(prefix))
}

/// Lowercase, and strip the diacritics a Spanish speaker will not type: the
/// table says "corazón" and "cañón", people search "corazon" and "canon".
fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

fn parse(language: usize) -> Vec<Emoji> {
    let name_column = 1 + language * 2;
    TABLE
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            // A short row would mean a corrupt generated file; drop it rather
            // than panic, so a bad regeneration cannot stop the app booting.
            if fields.len() != COLUMNS {
                log::warn!("emoji table row has {} fields, want {COLUMNS}", fields.len());
                return None;
            }
            Some(Emoji {
                emoji: fields[0],
                name: fields[name_column],
                name_key: fold(fields[name_column]),
                keywords_key: fold(fields[name_column + 1]),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(language: &str, emoji: &str) -> &'static Emoji {
        table(language)
            .iter()
            .find(|e| e.emoji == emoji)
            .unwrap_or_else(|| panic!("{emoji} missing from the {language} table"))
    }

    /// Where `emoji` lands in the results for `query`, or `None` if missing.
    fn rank_of(language: &str, query: &str, emoji: &str) -> Option<usize> {
        search(language, query).iter().position(|e| e.emoji == emoji)
    }

    fn top(language: &str, query: &str) -> &'static str {
        search(language, query).first().expect("no results").emoji
    }

    #[test]
    fn every_row_parses() {
        assert_eq!(table("en").len(), TABLE.lines().count());
        assert!(table("en").len() > 1500, "suspiciously small table");
    }

    #[test]
    fn names_are_the_screen_reader_ones() {
        assert_eq!(find("en", "😀").name, "grinning face");
        assert_eq!(find("en", "😂").name, "face with tears of joy");
        assert_eq!(find("es", "😀").name, "cara sonriendo");
    }

    /// Multi-codepoint emoji must keep the exact sequence that renders, or
    /// what lands on the clipboard is not what the list showed.
    #[test]
    fn sequences_keep_their_variation_selectors() {
        assert_eq!(find("en", "\u{2764}\u{fe0f}").name, "red heart");
        assert_eq!(find("en", "\u{1f1ea}\u{1f1f8}").name, "flag: Spain");
        assert!(table("en").iter().any(|e| e.emoji.contains('\u{200d}')), "no ZWJ sequences");
    }

    /// Skin-tone variants are filtered out at generation: six near-identical
    /// rows per emoji would bury the plain forms.
    #[test]
    fn no_skin_tone_variants() {
        let toned = table("en")
            .iter()
            .find(|e| e.emoji.chars().any(|c| ('\u{1f3fb}'..='\u{1f3ff}').contains(&c)));
        assert!(toned.is_none(), "{toned:?}");
    }

    #[test]
    fn unknown_language_falls_back_to_english() {
        assert_eq!(find("de", "😀").name, "grinning face");
    }

    /// The clipboard payload is the emoji alone — no stray name or whitespace.
    #[test]
    fn rows_are_well_formed() {
        for emoji in table("en") {
            assert!(!emoji.emoji.is_empty());
            assert_eq!(emoji.emoji.trim(), emoji.emoji, "{:?} has padding", emoji.emoji);
            assert!(!emoji.name.is_empty(), "{:?} has no name", emoji.emoji);
        }
    }

    #[test]
    fn empty_query_is_the_whole_table_in_palette_order() {
        let all = search("en", "");
        assert_eq!(all.len(), table("en").len());
        assert_eq!(all[0].emoji, "😀", "smileys come first in a palette");
    }

    #[test]
    fn the_plain_name_wins() {
        assert_eq!(top("en", "fire"), "🔥");
        assert_eq!(top("en", "rocket"), "🚀");
        assert_eq!(top("en", "thumbs up"), "👍");
        assert_eq!(top("en", "ok hand"), "👌");
        // Ahead of "heart with arrow", "heart with ribbon" and the rest.
        assert_eq!(top("en", "heart"), "\u{2764}\u{fe0f}");
    }

    /// The whole point of carrying keywords: the word people would type is
    /// often not in the name at all.
    #[test]
    fn keywords_find_what_the_name_does_not_say() {
        assert_eq!(top("en", "coffee"), "☕", "\"hot beverage\" says no coffee");
        assert_eq!(top("en", "tada"), "🎉", "\"party popper\" says no tada");
        assert!(rank_of("en", "laugh", "😂") < Some(5), "{:?}", rank_of("en", "laugh", "😂"));
    }

    #[test]
    fn a_partly_typed_description_still_finds_it() {
        assert!(rank_of("en", "grin", "😀") < Some(3));
        assert!(rank_of("en", "smil", "\u{263a}\u{fe0f}") < Some(5));
    }

    #[test]
    fn spanish_names_are_searched_in_spanish() {
        assert_eq!(top("es", "fuego"), "🔥");
        assert_eq!(top("es", "cohete"), "🚀");
        assert!(rank_of("es", "risa", "😂") < Some(5), "{:?}", rank_of("es", "risa", "😂"));
    }

    /// Nobody reaches for the accent keys mid-search, so typing the name
    /// without them has to rank exactly as typing it with them.
    #[test]
    fn accents_are_optional() {
        let bare: Vec<&str> = search("es", "corazon").iter().map(|e| e.emoji).collect();
        let accented: Vec<&str> = search("es", "corazón").iter().map(|e| e.emoji).collect();
        assert_eq!(bare, accented);
        assert_eq!(top("es", "canon de confeti"), "🎉");
        // "corazón rojo" and "corazón roto" are the same length, so which of
        // ❤️ and 💔 leads is down to palette order; both are right there.
        assert!(rank_of("es", "corazon", "\u{2764}\u{fe0f}") < Some(3));
    }

    #[test]
    fn a_description_spread_across_name_and_keywords_still_matches() {
        assert!(rank_of("en", "crying cat", "😿").is_some());
        assert!(rank_of("en", "not an emoji at all", "😀").is_none());
    }

    #[test]
    fn no_match_is_empty_rather_than_everything() {
        assert!(search("en", "zzzzqqqq").is_empty());
    }

    /// Every example the READMEs promise, in both languages.
    #[test]
    fn the_documented_examples_work() {
        let heart = "\u{2764}\u{fe0f}";
        for &(language, query, wanted) in &[
            ("en", "grinning face", "😀"),
            ("en", "red heart", heart),
            ("en", "coffee", "☕"),
            ("en", "tada", "🎉"),
            ("es", "cara sonriendo", "😀"),
            ("es", "corazón rojo", heart),
            ("es", "risa", "😂"),
            ("es", "café", "☕"),
        ] {
            let found = top(language, query);
            assert_eq!(found, wanted, "{language} {query:?} found {found} first");
        }
        // "laugh" is not a first-place claim: 🤣 says "laughing" in its name,
        // and several shorter-named smileys share the keyword. 😂 lands a few
        // rows down, which is all "reaches" promises.
        assert!(rank_of("en", "laugh", "😂") < Some(5));
    }
}
