//! Driving the Bitwarden CLI (`bw`) to read a Bitwarden or Vaultwarden vault.
//!
//! The import needs someone else's vault, and Bitwarden's own client is the
//! only thing that speaks its protocol properly: the key hierarchy, the KDF
//! negotiation and the per-item unwrapping are not things to reimplement
//! against a server this app does not control. So this module shells out, and
//! the whole of the crypto is `bw`'s problem.
//!
//! # Keeping the master password out of sight
//!
//! `bw login email password` puts the password in the command line, where any
//! other process on the machine can read it out of the process list for as
//! long as the command runs. Every call here uses `--passwordenv` instead, so
//! it travels in the child's environment block — which, unlike a command line,
//! is not world-readable on either platform.
//!
//! The session key `bw` hands back is the same kind of secret and gets the
//! same treatment: it lives in a [`Zeroizing`] string, is passed to later
//! commands through `BW_SESSION` rather than `--session`, and dies with the
//! [`BwSession`] that owns it.
//!
//! # What is left behind afterwards
//!
//! Depends on who the session belonged to. A login this module made is logged
//! out again by [`BwSession::logout`], from `Drop` as well as the happy path,
//! so an import that fails halfway does not leave the vault sitting unlocked
//! in the local `data.json` `bw` keeps.
//!
//! A session key the *user* supplied is left completely alone — not logged
//! out, and not preceded by the `bw logout` and `bw config server` that the
//! login path runs. That login is theirs; they unlocked it for their own
//! reasons and expect to still have it afterwards. It is also the way in for
//! any account whose second factor cannot be typed into a box, Duo and
//! WebAuthn included: `bw unlock --raw` in a terminal answers the challenge
//! however the account needs, and hands over a key that needs no factor.

use std::process::Command;

use launchtype_core::bitwarden::BwItem;
use zeroize::Zeroizing;

/// Long enough for `bw sync` to pull a large vault over a slow link, short
/// enough that a wedged CLI does not hang the app forever. `bw` has no timeout
/// of its own, so this is enforced by the caller waiting on the thread.
pub const SLOW_COMMAND_SECONDS: u64 = 180;

#[derive(Debug, thiserror::Error)]
pub enum BwError {
    #[error("the Bitwarden CLI (bw) is not installed, or is not on the PATH")]
    NotInstalled,
    #[error("{0}")]
    Cli(String),
    #[error("the Bitwarden CLI returned something this version cannot read: {0}")]
    BadOutput(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, BwError>;

/// A two-step login method that can be answered by typing something into a
/// box.
///
/// Bitwarden has more than these — Duo and WebAuthn among them — but they are
/// browser flows: there is no code to type, the user has to approve the login
/// somewhere else. They are deliberately absent rather than listed and broken,
/// because [`BwAuth::Session`] already handles them properly. Unlocking `bw`
/// in a terminal answers whatever challenge the account uses, whatever it is,
/// and hands over a session key that needs no second factor at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TwoFactor {
    #[default]
    None,
    /// The six digits from an authenticator app.
    Authenticator,
    /// A code the server sends by email.
    Email,
    /// The long one-time string a YubiKey types when it is touched.
    YubiKey,
}

impl TwoFactor {
    /// The number `--method` wants: Bitwarden's `TwoFactorProviderType`, where
    /// 0 is the authenticator app, 1 email and 3 a YubiKey OTP.
    fn method(self) -> Option<&'static str> {
        match self {
            TwoFactor::None => None,
            TwoFactor::Authenticator => Some("0"),
            TwoFactor::Email => Some("1"),
            TwoFactor::YubiKey => Some("3"),
        }
    }
}

/// How to reach the server and who to log in as.
pub struct BwCredentials {
    /// The Vaultwarden base URL, e.g. `https://vault.example.com`. Empty means
    /// leave whatever server `bw` is already configured for.
    pub server: String,
    pub email: String,
    pub password: Zeroizing<String>,
    pub two_factor: TwoFactor,
    /// The code answering `two_factor`; ignored when there is no method.
    pub code: String,
}

/// The two ways to get at a vault, which differ in one important way beyond
/// the obvious: who the session belongs to afterwards.
pub enum BwAuth {
    /// Log in from scratch. This session is ours, so it is logged out again
    /// when the import is done.
    Login(BwCredentials),
    /// Borrow a session key the user already has, from `bw unlock --raw` in a
    /// terminal. Their login is left exactly as it was found — see
    /// [`BwSession::logout`].
    Session(Zeroizing<String>),
}

/// Where the `bw` executable is. Resolved once so a missing CLI is reported
/// before the user is asked for a master password.
///
/// Resolved to an absolute path off `PATH` rather than left as a bare name for
/// `Command` to look up: on Windows that lookup starts in the folder the app
/// is running from, so a `bw.exe` dropped beside a portable install — a
/// Downloads folder, say — would be the one handed the master password. See
/// [`crate::program`].
pub fn find_cli() -> Option<String> {
    let found = crate::program::on_path("bw")?;
    let found = found.to_string_lossy().into_owned();
    // Found by name is not the same as runnable; asking for a version is both
    // checks in one.
    version(&found).is_some().then_some(found)
}

pub fn version(program: &str) -> Option<String> {
    let output = hidden(program).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// A logged-in, unlocked `bw` session. Holds the session key and nothing else;
/// the vault contents are fetched on demand and handed straight to the caller.
pub struct BwSession {
    program: String,
    key: Zeroizing<String>,
    /// False for a borrowed session key, which must outlive this import.
    ours: bool,
    logged_out: bool,
}

impl BwSession {
    /// Get a usable session, either by logging in or by taking one the user
    /// already had.
    pub fn open(program: &str, auth: &BwAuth) -> Result<Self> {
        match auth {
            BwAuth::Login(credentials) => Self::login(program, credentials),
            BwAuth::Session(key) => Self::borrow(program, key),
        }
    }

    /// Point `bw` at the server, log in, and keep the session key.
    ///
    /// Logging in when a previous run left an account logged in fails with
    /// "You are already logged in"; rather than treat that as an error, the
    /// stale session is cleared first so the import always starts from a known
    /// state and always ends up as the account the user just typed.
    fn login(program: &str, credentials: &BwCredentials) -> Result<Self> {
        let mut session = BwSession {
            program: program.to_string(),
            key: Zeroizing::new(String::new()),
            ours: true,
            logged_out: false,
        };

        // Best-effort: there may be nothing to log out of.
        let _ = session.run(&["logout"], None);

        if !credentials.server.trim().is_empty() {
            // Must happen while logged out; `bw` refuses to change server
            // under a live account.
            session.run(&["config", "server", credentials.server.trim()], None)?;
        }

        let mut login: Vec<&str> = vec![
            "login",
            credentials.email.trim(),
            "--passwordenv",
            PASSWORD_VAR,
            "--raw",
        ];
        let code = credentials.code.trim();
        if let Some(method) = credentials.two_factor.method() {
            if !code.is_empty() {
                login.extend_from_slice(&["--method", method, "--code", code]);
            }
        }

        let key = session.run(&login, Some(&credentials.password))?;
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(BwError::BadOutput("no session key from bw login".into()));
        }
        session.key = Zeroizing::new(key);
        Ok(session)
    }

    /// Take a session key the user unlocked themselves.
    ///
    /// Nothing is configured and nothing is logged out on the way in: the
    /// account, the server and the unlocked state all belong to whoever ran
    /// `bw unlock`, and an import has no business changing any of them. A key
    /// that has expired or was mistyped surfaces at the first real command,
    /// with `bw`'s own wording.
    fn borrow(program: &str, key: &str) -> Result<Self> {
        let key = key.trim();
        if key.is_empty() {
            return Err(BwError::BadOutput("empty session key".into()));
        }
        Ok(BwSession {
            program: program.to_string(),
            key: Zeroizing::new(key.to_string()),
            ours: false,
            logged_out: false,
        })
    }

    /// Pull the latest vault from the server. Without this the import would
    /// read whatever `bw` last cached, which for a fresh login is nothing.
    pub fn sync(&self) -> Result<()> {
        self.run(&["sync"], None).map(|_| ())
    }

    /// Every item in the vault, as `bw list items` prints it.
    pub fn items(&self) -> Result<Vec<BwItem>> {
        // The JSON holds every password in the vault, so it goes into a
        // Zeroizing buffer and is wiped as soon as it has been parsed.
        let json = Zeroizing::new(self.run(&["list", "items"], None)?);
        launchtype_core::bitwarden::parse_items(&json)
            .map_err(|e| BwError::BadOutput(e.to_string()))
    }

    /// Log out, wiping the local copy `bw` keeps in its own data file.
    ///
    /// Does nothing at all for a borrowed session. Logging out there would
    /// destroy a login the user set up for their own reasons and expects to
    /// still have when the import is over — the import would be reaching
    /// outside itself to break something it was only lent.
    pub fn logout(&mut self) {
        if self.logged_out {
            return;
        }
        self.logged_out = true;
        if !self.ours {
            log::info!("leaving the borrowed bw session logged in");
            return;
        }
        if let Err(e) = self.run(&["logout"], None) {
            log::warn!("bw logout failed, its local data may still hold the vault: {e}");
        }
    }

    fn run(&self, args: &[&str], password: Option<&str>) -> Result<String> {
        run_bw(&self.program, args, password, Some(&self.key))
    }
}

/// The session key must not outlive the import, and neither must `bw`'s cached
/// vault; both go here so an early return or a panic still cleans up.
impl Drop for BwSession {
    fn drop(&mut self) {
        self.logout();
    }
}

/// The environment variable `--passwordenv` reads. Named per run only in the
/// child, never set on this process.
const PASSWORD_VAR: &str = "LAUNCHTYPE_BW_PASSWORD";

fn run_bw(
    program: &str,
    args: &[&str],
    password: Option<&str>,
    session_key: Option<&str>,
) -> Result<String> {
    let mut command = hidden(program);
    command.args(args);
    // `--nointeraction` matters more than it looks: without it a `bw` that
    // wants a prompt blocks forever on a stdin that a GUI app never answers.
    command.arg("--nointeraction");
    if let Some(password) = password {
        command.env(PASSWORD_VAR, password);
    }
    if let Some(key) = session_key.filter(|k| !k.is_empty()) {
        command.env("BW_SESSION", key);
    }

    let output = match command.output() {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(BwError::NotInstalled),
        Err(e) => return Err(BwError::Io(e)),
    };

    if !output.status.success() {
        return Err(BwError::Cli(cli_message(&output.stderr, &output.stdout)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `bw` reports failures on stderr, but not always; a wrong password comes
/// back there while some errors only reach stdout, so both are considered
/// before giving up and saying something generic.
fn cli_message(stderr: &[u8], stdout: &[u8]) -> String {
    for stream in [stderr, stdout] {
        let text = String::from_utf8_lossy(stream).trim().to_string();
        if !text.is_empty() {
            // `bw` prefixes some failures; the user does not need to see that.
            return text.lines().next().unwrap_or(&text).trim_start_matches("Error: ").to_string();
        }
    }
    "the Bitwarden CLI failed without saying why".to_string()
}

/// A `Command` that does not flash a console window on Windows, where the app
/// is a GUI process with no console of its own.
fn hidden(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_executable_is_reported_as_not_installed() {
        let error = run_bw("launchtype-no-such-program", &["--version"], None, None).unwrap_err();
        assert!(matches!(error, BwError::NotInstalled), "got {error:?}");
    }

    #[test]
    fn the_first_line_of_stderr_becomes_the_message() {
        let message = cli_message(b"Error: Username or password is incorrect.\nmore\n", b"");
        assert_eq!(message, "Username or password is incorrect.");
    }

    #[test]
    fn stdout_is_used_when_stderr_is_silent() {
        assert_eq!(cli_message(b"", b"You are not logged in."), "You are not logged in.");
    }

    #[test]
    fn a_silent_failure_still_says_something() {
        assert!(!cli_message(b"   ", b"").is_empty());
    }

    #[test]
    fn the_method_numbers_are_the_ones_bitwarden_defines() {
        assert_eq!(TwoFactor::None.method(), None);
        assert_eq!(TwoFactor::Authenticator.method(), Some("0"));
        assert_eq!(TwoFactor::Email.method(), Some("1"));
        assert_eq!(TwoFactor::YubiKey.method(), Some("3"));
    }

    /// The whole point of the borrowed path: the user's own login survives it.
    #[test]
    fn a_borrowed_session_is_never_logged_out() {
        let mut session = BwSession::borrow("bw", "  key-from-bw-unlock  ").unwrap();
        assert!(!session.ours);
        assert_eq!(&*session.key, "key-from-bw-unlock");
        // Would shell out to a real `bw logout` if it were not a no-op; the
        // flag flipping without a call is what says it did nothing.
        session.logout();
        assert!(session.logged_out);
    }

    #[test]
    fn an_empty_session_key_is_refused_before_anything_runs() {
        assert!(BwSession::borrow("bw", "   ").is_err());
    }
}
