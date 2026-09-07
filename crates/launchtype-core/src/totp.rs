//! Time-based one-time passwords (RFC 6238), for the `*` mode entries that
//! hold a second factor rather than a password.
//!
//! # Why the vault generates the code itself
//!
//! What a site actually wants is the six digits, not the seed they come from,
//! so an entry that copied its seed would leave the user to find another app
//! to finish the job. Generating here keeps the seed where it already is —
//! sealed in the vault, decrypted for the moment it takes to run the HMAC —
//! and means the codes keep working with no network and no second program
//! installed.
//!
//! # What a seed looks like coming in
//!
//! Bitwarden hands over either a bare base32 seed or a whole `otpauth://` URI,
//! and the URI may override the digit count, the step length and the hash. Both
//! are accepted by [`Totp::parse`], because which one an entry holds depends on
//! how it was typed into Bitwarden years ago and the user should not have to
//! care.
//!
//! The decoded seed lives in a [`Zeroizing`] buffer and the generated code is
//! handed back in one too: a `Totp` is built for a single copy and dropped
//! straight after, so neither outlives the keypress that asked for it.

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use zeroize::Zeroizing;

/// The default step in seconds, and what every issuer worth the name uses.
const DEFAULT_PERIOD: u64 = 30;
const DEFAULT_DIGITS: u32 = 6;

/// Digit counts outside this range are refused rather than silently clamped:
/// RFC 4226 only defines the truncation for 6 to 8, and a "code" of 20 digits
/// is a sign the URI was mangled, not a preference to honour.
const MIN_DIGITS: u32 = 6;
const MAX_DIGITS: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TotpError {
    #[error("the authenticator secret is empty")]
    Empty,
    #[error("the authenticator secret is not valid base32")]
    BadSecret,
    #[error("that otpauth link has no secret in it")]
    NoSecret,
    #[error("that authenticator code length is not supported")]
    BadDigits,
    #[error("that authenticator time step is not supported")]
    BadPeriod,
    #[error("that authenticator hash algorithm is not supported")]
    BadAlgorithm,
}

type Result<T> = std::result::Result<T, TotpError>;

/// The hash inside the HMAC. SHA-1 is the default and, in practice, almost
/// always the answer; the other two exist because the URI can ask for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    fn parse(name: &str) -> Result<Self> {
        match name.trim().to_ascii_uppercase().as_str() {
            "SHA1" | "SHA-1" => Ok(Algorithm::Sha1),
            "SHA256" | "SHA-256" => Ok(Algorithm::Sha256),
            "SHA512" | "SHA-512" => Ok(Algorithm::Sha512),
            _ => Err(TotpError::BadAlgorithm),
        }
    }
}

/// A parsed authenticator seed, ready to produce codes.
pub struct Totp {
    secret: Zeroizing<Vec<u8>>,
    digits: u32,
    period: u64,
    algorithm: Algorithm,
}

/// Deliberately hand-written rather than derived: a derived `Debug` would put
/// the decoded seed into any log line or panic message that ever formatted a
/// `Totp`, which is the one thing this type exists to keep hold of.
impl std::fmt::Debug for Totp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Totp")
            .field("secret", &"<redacted>")
            .field("digits", &self.digits)
            .field("period", &self.period)
            .field("algorithm", &self.algorithm)
            .finish()
    }
}

impl Totp {
    /// Accept either a bare base32 seed (`"JBSWY3DPEHPK3PXP"`, spaces and
    /// lowercase and all) or a full `otpauth://totp/...` URI.
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if input.is_empty() {
            return Err(TotpError::Empty);
        }
        if input.len() >= 8 && input[..8].eq_ignore_ascii_case("otpauth:") {
            Self::parse_uri(input)
        } else {
            Self::from_parts(input, DEFAULT_DIGITS, DEFAULT_PERIOD, Algorithm::Sha1)
        }
    }

    fn parse_uri(uri: &str) -> Result<Self> {
        // Only the query matters: the label before it names the account, which
        // the vault entry already has a better version of.
        let query = uri.split_once('?').map(|(_, q)| q).unwrap_or("");
        let mut secret = None;
        let mut digits = DEFAULT_DIGITS;
        let mut period = DEFAULT_PERIOD;
        let mut algorithm = Algorithm::Sha1;

        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = percent_decode(value);
            match key.to_ascii_lowercase().as_str() {
                "secret" => secret = Some(value),
                "digits" => digits = value.parse().map_err(|_| TotpError::BadDigits)?,
                "period" => period = value.parse().map_err(|_| TotpError::BadPeriod)?,
                "algorithm" => algorithm = Algorithm::parse(&value)?,
                // `issuer`, `counter` and anything an issuer invented are not
                // ours to object to; an unknown parameter is not a broken URI.
                _ => {}
            }
        }

        let secret = secret.ok_or(TotpError::NoSecret)?;
        Self::from_parts(&secret, digits, period, algorithm)
    }

    fn from_parts(secret: &str, digits: u32, period: u64, algorithm: Algorithm) -> Result<Self> {
        if !(MIN_DIGITS..=MAX_DIGITS).contains(&digits) {
            return Err(TotpError::BadDigits);
        }
        if period == 0 {
            return Err(TotpError::BadPeriod);
        }
        let secret = decode_base32(secret)?;
        if secret.is_empty() {
            return Err(TotpError::Empty);
        }
        Ok(Totp { secret, digits, period, algorithm })
    }

    /// The code for the step containing `unix_seconds`, zero-padded to the
    /// configured width.
    ///
    /// Times before the epoch have no step number to speak of; they can only
    /// come from a clock set absurdly wrong, and are treated as step zero
    /// rather than panicking on the way to a code nobody can use anyway.
    pub fn code_at(&self, unix_seconds: i64) -> Zeroizing<String> {
        let step = (unix_seconds.max(0) as u64) / self.period;
        let counter = step.to_be_bytes();
        let digest = match self.algorithm {
            Algorithm::Sha1 => self.hmac::<Sha1>(&counter),
            Algorithm::Sha256 => self.hmac::<Sha256>(&counter),
            Algorithm::Sha512 => self.hmac::<Sha512>(&counter),
        };
        Zeroizing::new(truncate(&digest, self.digits))
    }

    /// How much of the current step is left, in seconds. Always 1..=period, so
    /// "0 seconds left" never gets announced for a code that still works.
    pub fn seconds_remaining(&self, unix_seconds: i64) -> u64 {
        let elapsed = (unix_seconds.max(0) as u64) % self.period;
        self.period - elapsed
    }

    pub fn digits(&self) -> u32 {
        self.digits
    }

    pub fn period(&self) -> u64 {
        self.period
    }

    fn hmac<D>(&self, message: &[u8]) -> Zeroizing<Vec<u8>>
    where
        D: hmac::digest::block_api::EagerHash,
        Hmac<D>: KeyInit + Mac,
    {
        let mut mac = <Hmac<D> as KeyInit>::new_from_slice(&self.secret)
            .expect("HMAC accepts a key of any length");
        mac.update(message);
        Zeroizing::new(mac.finalize().into_bytes().to_vec())
    }
}

/// RFC 4226 dynamic truncation: the low nibble of the last byte picks where to
/// read the four bytes that become the code.
fn truncate(digest: &[u8], digits: u32) -> String {
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let binary = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    let modulus = 10u32.pow(digits);
    format!("{:0width$}", binary % modulus, width = digits as usize)
}

/// Decode RFC 4648 base32, forgiving the things people paste: lowercase, the
/// spaces authenticator apps put in for readability, and `=` padding.
fn decode_base32(input: &str) -> Result<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(Vec::with_capacity(input.len() * 5 / 8 + 1));
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;

    for c in input.chars() {
        if c == '=' || c.is_whitespace() || c == '-' {
            continue;
        }
        let value = match c.to_ascii_uppercase() {
            'A'..='Z' => c.to_ascii_uppercase() as u32 - 'A' as u32,
            '2'..='7' => c as u32 - '2' as u32 + 26,
            _ => return Err(TotpError::BadSecret),
        };
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Ok(out)
}

/// The little of URI escaping that turns up in an `otpauth://` query. Anything
/// that is not a well-formed `%XX` is left alone, which is what a user who put
/// a literal `%` in an issuer name would want.
fn percent_decode(input: &str) -> String {
    if !input.contains('%') {
        return input.replace('+', " ");
    }
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RFC 6238 test vectors, whose seed is the ASCII "12345678901234567890"
    /// base32-encoded. Getting these right is the whole contract.
    const RFC_SEED: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    fn code(seed: &str, at: i64) -> String {
        Totp::parse(seed).unwrap().code_at(at).to_string()
    }

    #[test]
    fn rfc_6238_sha1_vectors() {
        let totp = Totp {
            secret: decode_base32(RFC_SEED).unwrap(),
            digits: 8,
            period: 30,
            algorithm: Algorithm::Sha1,
        };
        assert_eq!(totp.code_at(59).to_string(), "94287082");
        assert_eq!(totp.code_at(1111111109).to_string(), "07081804");
        assert_eq!(totp.code_at(1111111111).to_string(), "14050471");
        assert_eq!(totp.code_at(1234567890).to_string(), "89005924");
        assert_eq!(totp.code_at(2000000000).to_string(), "69279037");
    }

    #[test]
    fn a_bare_base32_seed_defaults_to_six_sha1_digits_every_thirty_seconds() {
        let totp = Totp::parse("JBSWY3DPEHPK3PXP").unwrap();
        assert_eq!(totp.digits(), 6);
        assert_eq!(totp.period(), 30);
        assert_eq!(totp.algorithm, Algorithm::Sha1);
        assert_eq!(totp.code_at(0).len(), 6);
    }

    #[test]
    fn a_seed_pasted_with_the_spaces_and_lowercase_an_app_showed_still_works() {
        // The same seed three ways; an authenticator app displays the spaced
        // form, so that is what gets pasted into Bitwarden.
        let plain = code("JBSWY3DPEHPK3PXP", 1111111109);
        assert_eq!(code("jbswy3dp ehpk3pxp", 1111111109), plain);
        assert_eq!(code("JBSW Y3DP EHPK 3PXP", 1111111109), plain);
        assert_eq!(code("JBSWY3DPEHPK3PXP====", 1111111109), plain);
    }

    #[test]
    fn an_otpauth_uri_overrides_the_defaults_it_names() {
        let totp =
            Totp::parse("otpauth://totp/GitHub:oriol?secret=JBSWY3DPEHPK3PXP&digits=8&period=60&algorithm=SHA256")
                .unwrap();
        assert_eq!(totp.digits(), 8);
        assert_eq!(totp.period(), 60);
        assert_eq!(totp.algorithm, Algorithm::Sha256);
        assert_eq!(totp.code_at(0).len(), 8);
    }

    #[test]
    fn an_otpauth_uri_with_only_a_secret_is_the_same_as_the_bare_seed() {
        let bare = code("JBSWY3DPEHPK3PXP", 1234567890);
        let uri = code("otpauth://totp/Amazon?secret=JBSWY3DPEHPK3PXP&issuer=Amazon", 1234567890);
        assert_eq!(uri, bare);
    }

    #[test]
    fn the_scheme_is_matched_whatever_its_case() {
        assert!(Totp::parse("OTPAUTH://TOTP/x?secret=JBSWY3DPEHPK3PXP").is_ok());
    }

    #[test]
    fn a_percent_escaped_label_does_not_confuse_the_query() {
        let totp = Totp::parse(
            "otpauth://totp/Work%20VPN:oriol%40example.com?issuer=Work%20VPN&secret=JBSWY3DPEHPK3PXP",
        )
        .unwrap();
        assert_eq!(totp.code_at(59).len(), 6);
    }

    #[test]
    fn the_code_is_padded_to_the_full_width() {
        // A truncation that lands under 100000 must still read as six digits,
        // or the user types five and the site rejects it.
        let totp = Totp {
            secret: decode_base32("JBSWY3DPEHPK3PXP").unwrap(),
            digits: 6,
            period: 30,
            algorithm: Algorithm::Sha1,
        };
        for step in 0..500i64 {
            assert_eq!(totp.code_at(step * 30).len(), 6);
        }
    }

    #[test]
    fn the_countdown_runs_from_the_full_period_down_to_one() {
        let totp = Totp::parse("JBSWY3DPEHPK3PXP").unwrap();
        assert_eq!(totp.seconds_remaining(0), 30);
        assert_eq!(totp.seconds_remaining(1), 29);
        assert_eq!(totp.seconds_remaining(29), 1);
        // A new step, so the countdown starts over rather than reaching zero.
        assert_eq!(totp.seconds_remaining(30), 30);
    }

    #[test]
    fn the_code_only_changes_when_the_step_does() {
        let totp = Totp::parse("JBSWY3DPEHPK3PXP").unwrap();
        // The step runs [1111111110, 1111111140): 1111111110 is exactly
        // 37037037 * 30.
        let inside = totp.code_at(1111111110).to_string();
        assert_eq!(totp.code_at(1111111139).to_string(), inside);
        assert_ne!(totp.code_at(1111111140).to_string(), inside);
    }

    #[test]
    fn rubbish_is_refused_rather_than_producing_a_code_that_never_works() {
        assert_eq!(Totp::parse("").unwrap_err(), TotpError::Empty);
        assert_eq!(Totp::parse("   ").unwrap_err(), TotpError::Empty);
        assert_eq!(Totp::parse("not base32!").unwrap_err(), TotpError::BadSecret);
        assert_eq!(Totp::parse("otpauth://totp/x?issuer=x").unwrap_err(), TotpError::NoSecret);
        assert_eq!(
            Totp::parse("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&digits=20").unwrap_err(),
            TotpError::BadDigits
        );
        assert_eq!(
            Totp::parse("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&period=0").unwrap_err(),
            TotpError::BadPeriod
        );
        assert_eq!(
            Totp::parse("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&algorithm=MD5").unwrap_err(),
            TotpError::BadAlgorithm
        );
    }

    #[test]
    fn a_clock_set_before_the_epoch_gives_a_code_instead_of_a_panic() {
        let totp = Totp::parse("JBSWY3DPEHPK3PXP").unwrap();
        assert_eq!(totp.code_at(-1).to_string(), totp.code_at(0).to_string());
        assert_eq!(totp.seconds_remaining(-1), 30);
    }

    /// Codes for the cases the RFC does not cover — the other hashes, other
    /// digit counts and a non-default period — computed independently with
    /// Python's stdlib `hmac`/`hashlib` rather than by this code, so the test
    /// can actually disagree with the implementation it is checking.
    #[test]
    fn the_other_algorithms_and_periods_match_an_independent_implementation() {
        let cases = [
            ("otpauth://x?secret=JBSWY3DPEHPK3PXP", 0i64, "282760"),
            ("otpauth://x?secret=JBSWY3DPEHPK3PXP", 1234567890, "742275"),
            ("otpauth://x?secret=JBSWY3DPEHPK3PXP", 1767225600, "260025"),
            ("otpauth://x?secret=JBSWY3DPEHPK3PXP&digits=8&period=60&algorithm=SHA256", 1234567890, "45806924"),
            ("otpauth://x?secret=JBSWY3DPEHPK3PXP&algorithm=SHA512", 1234567890, "136418"),
            // An odd-length seed, so the base32 padding path is exercised too.
            ("otpauth://x?secret=MZXW6YTBOI======", 1600000000, "687398"),
        ];
        for (uri, at, expected) in cases {
            let totp = Totp::parse(uri).unwrap();
            assert_eq!(totp.code_at(at).to_string(), expected, "{uri} at {at}");
        }
    }

    #[test]
    fn base32_decodes_the_known_answer() {
        assert_eq!(decode_base32(RFC_SEED).unwrap().as_slice(), b"12345678901234567890");
        assert_eq!(decode_base32("MZXW6===").unwrap().as_slice(), b"foo");
    }
}
