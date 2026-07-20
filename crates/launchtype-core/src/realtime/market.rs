//! Market prices: CoinGecko (bitcoin/ethereum in euros), Frankfurter ECB
//! reference rates (EUR→USD) and the Yahoo Finance chart API (brent crude,
//! gold and the IBEX 35 index).

use crate::i18n::{format_args, tr, Arg};

use super::history::HistoryStore;
use super::number::{format_number, python_float};
use super::{parse_json_body, RealtimeError};

pub const COINGECKO_BITCOIN_ID: &str = "bitcoin";
pub const COINGECKO_ETHEREUM_ID: &str = "ethereum";
pub const FRANKFURTER_URL: &str = "https://api.frankfurter.dev/v1/latest?base=EUR&symbols=USD";
pub const BRENT_SYMBOL: &str = "BZ=F";
pub const GOLD_SYMBOL: &str = "GC=F";
pub const IBEX_SYMBOL: &str = "^IBEX";

/// CoinGecko simple-price URL for a coin id, in euros.
pub fn coingecko_price_url(coin_id: &str) -> String {
    format!("https://api.coingecko.com/api/v3/simple/price?ids={coin_id}&vs_currencies=eur")
}

/// Yahoo Finance chart URL for a symbol (percent-encoded like `urllib.parse.quote`).
pub fn yahoo_chart_url(symbol: &str) -> String {
    format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?range=1d&interval=1d",
        python_quote(symbol)
    )
}

/// `urllib.parse.quote` with its default safe set (letters, digits, `_.-~` and `/`).
fn python_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Extract `body[coin_id]["eur"]` from a CoinGecko simple-price response.
pub fn parse_coingecko_price(body: &str, coin_id: &str) -> Result<f64, RealtimeError> {
    let body = parse_json_body(body)?;
    body.get(coin_id)
        .and_then(|coin| coin.get("eur"))
        .and_then(python_float)
        .ok_or(RealtimeError::NotUnderstood)
}

/// Extract `chart.result[0].meta.regularMarketPrice` from a Yahoo chart response.
pub fn parse_yahoo_price(body: &str) -> Result<f64, RealtimeError> {
    let body = parse_json_body(body)?;
    body.get("chart")
        .and_then(|chart| chart.get("result"))
        .and_then(|result| result.get(0))
        .and_then(|first| first.get("meta"))
        .and_then(|meta| meta.get("regularMarketPrice"))
        .and_then(python_float)
        .ok_or(RealtimeError::NotUnderstood)
}

/// Extract `rates.USD` from a Frankfurter response.
pub fn parse_frankfurter_usd_rate(body: &str) -> Result<f64, RealtimeError> {
    let body = parse_json_body(body)?;
    body.get("rates")
        .and_then(|rates| rates.get("USD"))
        .and_then(python_float)
        .ok_or(RealtimeError::NotUnderstood)
}

fn append_comparison(
    history: &HistoryStore,
    sentence: String,
    key: &str,
    current: f64,
    unit: &str,
    now_epoch: f64,
) -> Result<String, RealtimeError> {
    history
        .with_comparison(sentence, key, current, unit, now_epoch)
        .map_err(|error| RealtimeError::Unexpected(error.to_string()))
}

/// Python `_fetch_bitcoin`, minus the HTTP call.
pub fn bitcoin_sentence(
    body: &str,
    history: &HistoryStore,
    now_epoch: f64,
) -> Result<String, RealtimeError> {
    let price = parse_coingecko_price(body, COINGECKO_BITCOIN_ID)?;
    let sentence = format_args(
        &tr("One bitcoin is {price} euros right now"),
        &[("price", Arg::Str(&format_number(price, 2)))],
    );
    append_comparison(history, sentence, "bitcoin", price, &tr("euros"), now_epoch)
}

/// Python `_fetch_ethereum`, minus the HTTP call.
pub fn ethereum_sentence(
    body: &str,
    history: &HistoryStore,
    now_epoch: f64,
) -> Result<String, RealtimeError> {
    let price = parse_coingecko_price(body, COINGECKO_ETHEREUM_ID)?;
    let sentence = format_args(
        &tr("One ethereum is {price} euros right now"),
        &[("price", Arg::Str(&format_number(price, 2)))],
    );
    append_comparison(history, sentence, "ethereum", price, &tr("euros"), now_epoch)
}

/// Python `_fetch_eur_usd`, minus the HTTP call.
pub fn eur_usd_sentence(
    body: &str,
    history: &HistoryStore,
    now_epoch: f64,
) -> Result<String, RealtimeError> {
    let rate = parse_frankfurter_usd_rate(body)?;
    let amount = rate * 1000.0;
    let sentence = format_args(
        &tr("1000 euros are {amount} us dollars, at a rate of {rate} dollars per euro"),
        &[
            ("amount", Arg::Str(&format_number(amount, 2))),
            ("rate", Arg::Str(&format_number(rate, 4))),
        ],
    );
    append_comparison(history, sentence, "eur_usd", amount, &tr("us dollars"), now_epoch)
}

/// Python `_fetch_brent`, minus the HTTP call.
pub fn brent_sentence(
    body: &str,
    history: &HistoryStore,
    now_epoch: f64,
) -> Result<String, RealtimeError> {
    let price = parse_yahoo_price(body)?;
    let sentence = format_args(
        &tr("A barrel of brent crude oil is {price} us dollars"),
        &[("price", Arg::Str(&format_number(price, 2)))],
    );
    append_comparison(history, sentence, "brent", price, &tr("us dollars"), now_epoch)
}

/// Python `_fetch_gold`, minus the HTTP call.
pub fn gold_sentence(
    body: &str,
    history: &HistoryStore,
    now_epoch: f64,
) -> Result<String, RealtimeError> {
    let price = parse_yahoo_price(body)?;
    let sentence = format_args(
        &tr("An ounce of gold is {price} us dollars"),
        &[("price", Arg::Str(&format_number(price, 2)))],
    );
    append_comparison(history, sentence, "gold", price, &tr("us dollars"), now_epoch)
}

/// Python `_fetch_ibex`, minus the HTTP call.
pub fn ibex_sentence(
    body: &str,
    history: &HistoryStore,
    now_epoch: f64,
) -> Result<String, RealtimeError> {
    let points = parse_yahoo_price(body)?;
    let sentence = format_args(
        &tr("The ibex 35 is at {points} points"),
        &[("points", Arg::Str(&format_number(points, 2)))],
    );
    append_comparison(history, sentence, "ibex", points, &tr("points"), now_epoch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realtime::history::HISTORY_FILE;

    fn history(dir: &tempfile::TempDir) -> HistoryStore {
        HistoryStore::new(dir.path().join(HISTORY_FILE))
    }

    #[test]
    fn bitcoin_first_and_second_reading() {
        let dir = tempfile::tempdir().unwrap();
        let store = history(&dir);
        let body = r#"{"bitcoin": {"eur": 91234.56}}"#;
        assert_eq!(
            bitcoin_sentence(body, &store, 1000.0).unwrap(),
            "One bitcoin is 91234.56 euros right now"
        );
        let body = r#"{"bitcoin": {"eur": 91334.56}}"#;
        assert_eq!(
            bitcoin_sentence(body, &store, 1120.0).unwrap(),
            "One bitcoin is 91334.56 euros right now, up 100 euros (0.11 percent) since 2 minutes ago"
        );
    }

    #[test]
    fn ethereum_sentence_exact() {
        let dir = tempfile::tempdir().unwrap();
        let store = history(&dir);
        let body = r#"{"ethereum": {"eur": 3000}}"#;
        assert_eq!(
            ethereum_sentence(body, &store, 0.0).unwrap(),
            "One ethereum is 3000 euros right now"
        );
    }

    #[test]
    fn eur_usd_sentence_exact() {
        let dir = tempfile::tempdir().unwrap();
        let store = history(&dir);
        let body = r#"{"amount": 1.0, "base": "EUR", "rates": {"USD": 1.0856}}"#;
        assert_eq!(
            eur_usd_sentence(body, &store, 0.0).unwrap(),
            "1000 euros are 1085.6 us dollars, at a rate of 1.0856 dollars per euro"
        );
        // Python float() also accepts a numeric string rate.
        let body = r#"{"rates": {"USD": "1.25"}}"#;
        assert_eq!(
            eur_usd_sentence(body, &store, 60.0).unwrap(),
            "1000 euros are 1250 us dollars, at a rate of 1.25 dollars per euro, \
             up 164.4 us dollars (15.14 percent) since 1 minute ago"
        );
    }

    #[test]
    fn yahoo_sentences_exact() {
        let dir = tempfile::tempdir().unwrap();
        let store = history(&dir);
        let body = r#"{"chart": {"result": [{"meta": {"regularMarketPrice": 78.32}}]}}"#;
        assert_eq!(
            brent_sentence(body, &store, 0.0).unwrap(),
            "A barrel of brent crude oil is 78.32 us dollars"
        );
        let body = r#"{"chart": {"result": [{"meta": {"regularMarketPrice": 3391.9}}]}}"#;
        assert_eq!(
            gold_sentence(body, &store, 0.0).unwrap(),
            "An ounce of gold is 3391.9 us dollars"
        );
        let body = r#"{"chart": {"result": [{"meta": {"regularMarketPrice": 14321.8}}]}}"#;
        assert_eq!(
            ibex_sentence(body, &store, 0.0).unwrap(),
            "The ibex 35 is at 14321.8 points"
        );
    }

    #[test]
    fn missing_fields_are_not_understood() {
        let dir = tempfile::tempdir().unwrap();
        let store = history(&dir);
        for body in [
            "not json",
            "{}",
            r#"{"bitcoin": {}}"#,
            r#"{"bitcoin": {"eur": null}}"#,
            r#"{"bitcoin": {"eur": {}}}"#,
        ] {
            let error = bitcoin_sentence(body, &store, 0.0).unwrap_err();
            assert_eq!(error, RealtimeError::NotUnderstood);
            assert_eq!(error.to_string(), "The server returned data that could not be understood.");
        }
        for body in [
            "{}",
            r#"{"chart": {}}"#,
            r#"{"chart": {"result": []}}"#,
            r#"{"chart": {"result": [{"meta": {}}]}}"#,
        ] {
            assert_eq!(parse_yahoo_price(body).unwrap_err(), RealtimeError::NotUnderstood);
        }
        assert_eq!(
            parse_frankfurter_usd_rate(r#"{"rates": {}}"#).unwrap_err(),
            RealtimeError::NotUnderstood
        );
    }

    #[test]
    fn urls_match_python() {
        assert_eq!(
            coingecko_price_url("bitcoin"),
            "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies=eur"
        );
        assert_eq!(
            yahoo_chart_url(BRENT_SYMBOL),
            "https://query1.finance.yahoo.com/v8/finance/chart/BZ%3DF?range=1d&interval=1d"
        );
        assert_eq!(
            yahoo_chart_url(IBEX_SYMBOL),
            "https://query1.finance.yahoo.com/v8/finance/chart/%5EIBEX?range=1d&interval=1d"
        );
        assert_eq!(
            yahoo_chart_url(GOLD_SYMBOL),
            "https://query1.finance.yahoo.com/v8/finance/chart/GC%3DF?range=1d&interval=1d"
        );
    }
}
