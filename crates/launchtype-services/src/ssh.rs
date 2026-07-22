//! SSH mode ($) transport: one long-lived connection driving one long-lived
//! remote shell, driven from the UI thread without blocking it.
//!
//! The whole russh/tokio world lives on a dedicated worker thread running a
//! current-thread runtime. The UI talks to it over a channel and gets answers
//! back through callbacks, which fire *on the worker thread* — callers are
//! expected to hop back to the UI thread themselves (`wxdragon::call_after`).
//!
//! Every command runs in a single login shell opened at connect time (a
//! `shell` request with no PTY), so `cd`, exported variables and anything the
//! login scripts set up persist between commands. Right after the shell
//! starts, `.zshrc`/`.bashrc` is sourced so aliases and PATH additions from
//! the interactive setup work too; the stdout of that startup (banners, rc
//! chatter) is discarded, and its stderr is handed to `on_connect` so broken
//! login scripts get surfaced exactly once.
//!
//! A shared shell has no per-command exit signal, so each command is chased
//! by two `printf`s that write a per-command sentinel line to stdout
//! (carrying `$?`) and to stderr; everything before the sentinels is the
//! command's output. No PTY also means no prompt, no echo and no ANSI noise
//! — but also no terminal: full-screen programs and password prompts will
//! not work, same as before.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::{client, Channel, ChannelMsg, Disconnect};
use tokio::sync::mpsc::{self, error::SendError, UnboundedSender};

/// Keeps NAT/firewall state alive between commands typed minutes apart.
const KEEPALIVE_SECS: u64 = 30;
/// Give up rather than hanging the mode forever on an unreachable host.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// A login script that blocks (say, waiting for a TTY that will never come)
/// fails the connection instead of wedging it forever.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);

/// Runs as command #0 in the fresh shell. sshd already started it as a login
/// shell (profile files loaded); this pulls in the interactive rc file as
/// well, because that is where people actually keep their aliases and PATH.
/// POSIX syntax on purpose — it has to parse in whatever /etc/passwd names.
///
/// Bash needs two extra pushes to behave like the interactive shell the rc
/// file expects: `expand_aliases` (off in non-interactive bash, so defined
/// aliases would never fire) and a non-empty PS1, which defeats the classic
/// `[ -z "$PS1" ] && return` guard. The modern `case $- in *i*)` guard
/// cannot be defeated from outside — rc files behind it stay skipped, same
/// as with `ssh host command`.
const SOURCE_RC: &str = concat!(
    r#"if [ -n "$ZSH_VERSION" ] && [ -f "${ZDOTDIR:-$HOME}/.zshrc" ]; then . "${ZDOTDIR:-$HOME}/.zshrc"; "#,
    r#"elif [ -n "$BASH_VERSION" ]; then shopt -s expand_aliases; "#,
    r#"if [ -f "$HOME/.bashrc" ]; then PS1="${PS1:-launchtype}"; . "$HOME/.bashrc"; fi; fi"#
);

#[derive(Debug, Clone)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Path to a private key (OpenSSH or PEM). Preferred over the password.
    pub key_path: String,
    /// Password authentication, and the passphrase for an encrypted key.
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_status: Option<u32>,
}

/// A failure worth showing the user, already flattened to a message.
#[derive(Debug, Clone)]
pub struct SshError {
    pub message: String,
}

impl SshError {
    fn new(message: impl Into<String>) -> Self {
        SshError { message: message.into() }
    }
}

type Callback<T> = Box<dyn FnOnce(Result<T, SshError>) + Send + 'static>;

enum Job {
    Exec { command: String, reply: Callback<CommandOutput> },
    Close,
}

/// Handle to the worker thread. Dropping it disconnects.
pub struct SshSession {
    tx: UnboundedSender<Job>,
}

impl SshSession {
    /// Start connecting in the background. `on_connect` reports the handshake
    /// result — on success it carries whatever the login scripts wrote to
    /// stderr, which the caller should show once. Commands queued before it
    /// lands simply run afterwards (or fail with the same error if the
    /// connection never came up).
    pub fn connect(
        config: SshConfig,
        on_connect: impl FnOnce(Result<String, SshError>) + Send + 'static,
    ) -> SshSession {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(e) => {
                    on_connect(Err(SshError::new(e.to_string())));
                    return;
                }
            };
            runtime.block_on(async move {
                let (handle, mut shell, startup_stderr) = match start_session(&config).await {
                    Ok(parts) => parts,
                    Err(e) => {
                        on_connect(Err(e.clone()));
                        // Drain the queue so pending commands don't hang.
                        while let Some(job) = rx.recv().await {
                            match job {
                                Job::Exec { reply, .. } => reply(Err(e.clone())),
                                Job::Close => break,
                            }
                        }
                        return;
                    }
                };
                on_connect(Ok(startup_stderr));
                // Startup was sequence number 0.
                let mut seq: u64 = 0;
                while let Some(job) = rx.recv().await {
                    match job {
                        Job::Exec { command, reply } => {
                            seq += 1;
                            reply(run_command(&mut shell, seq, &command).await);
                        }
                        Job::Close => break,
                    }
                }
                let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
            });
        });
        SshSession { tx }
    }

    /// Queue a command. `on_done` runs on the worker thread once it finishes.
    pub fn exec(
        &self,
        command: &str,
        on_done: impl FnOnce(Result<CommandOutput, SshError>) + Send + 'static,
    ) {
        let job = Job::Exec { command: command.to_string(), reply: Box::new(on_done) };
        // The worker died; answer immediately so the UI never waits.
        if let Err(SendError(Job::Exec { reply, .. })) = self.tx.send(job) {
            reply(Err(SshError::new("the SSH connection is closed")));
        }
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        let _ = self.tx.send(Job::Close);
    }
}

struct Client;

impl client::Handler for Client {
    type Error = russh::Error;

    /// Launchtype keeps no known_hosts file, so every host key is accepted.
    /// The mode is a convenience console for servers the user already owns,
    /// not a hardened SSH client.
    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Connect, authenticate, start the login shell and run its rc files.
/// Returns the connection, the shell channel, and the startup stderr.
async fn start_session(
    config: &SshConfig,
) -> Result<(client::Handle<Client>, Channel<client::Msg>, String), SshError> {
    let handle = connect_and_authenticate(config).await?;
    let mut shell = handle
        .channel_open_session()
        .await
        .map_err(|e| SshError::new(e.to_string()))?;
    shell.request_shell(true).await.map_err(|e| SshError::new(e.to_string()))?;
    let startup = tokio::time::timeout(STARTUP_TIMEOUT, run_command(&mut shell, 0, SOURCE_RC))
        .await
        .map_err(|_| SshError::new("the login shell did not respond"))??;
    Ok((handle, shell, startup.stderr))
}

async fn connect_and_authenticate(config: &SshConfig) -> Result<client::Handle<Client>, SshError> {
    let client_config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(KEEPALIVE_SECS)),
        ..Default::default()
    });
    let address = (config.host.trim().to_string(), config.port);
    let connecting = client::connect(client_config, address, Client);
    let mut handle = match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
        Ok(Ok(handle)) => handle,
        Ok(Err(e)) => return Err(SshError::new(e.to_string())),
        Err(_) => return Err(SshError::new("timed out while connecting")),
    };

    let user = config.user.trim();
    let key_path = config.key_path.trim();
    let mut key_error = None;
    if !key_path.is_empty() {
        match load_key(Path::new(key_path), &config.password) {
            Ok(key) => {
                let hash = handle
                    .best_supported_rsa_hash()
                    .await
                    .map_err(|e| SshError::new(e.to_string()))?
                    .flatten();
                let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash);
                match handle.authenticate_publickey(user, key).await {
                    Ok(result) if result.success() => return Ok(handle),
                    Ok(_) => key_error = Some(SshError::new("the server rejected the key")),
                    Err(e) => key_error = Some(SshError::new(e.to_string())),
                }
            }
            Err(e) => key_error = Some(e),
        }
    }

    // The key is preferred, but a password falls back for it when present.
    if !config.password.is_empty() {
        match handle.authenticate_password(user, &config.password).await {
            Ok(result) if result.success() => return Ok(handle),
            Ok(_) => {}
            Err(e) => return Err(SshError::new(e.to_string())),
        }
    }

    Err(key_error.unwrap_or_else(|| SshError::new("authentication failed")))
}

/// Load a private key, trying the configured password as its passphrase when
/// the key turns out to be encrypted.
fn load_key(path: &Path, password: &str) -> Result<russh::keys::PrivateKey, SshError> {
    match load_secret_key(path, None) {
        Ok(key) => Ok(key),
        Err(first) => {
            if password.is_empty() {
                return Err(SshError::new(format!("{}: {first}", path.display())));
            }
            load_secret_key(path, Some(password))
                .map_err(|e| SshError::new(format!("{}: {e}", path.display())))
        }
    }
}

/// Run one command in the shared shell and collect its output up to the
/// sentinels. `seq` must be unique per command so a sentinel left over from a
/// desynchronised earlier command can never terminate this one.
async fn run_command(
    shell: &mut Channel<client::Msg>,
    seq: u64,
    command: &str,
) -> Result<CommandOutput, SshError> {
    let out_prefix = format!("__LT_EOC_{seq}_");
    let err_sentinel = format!("__LT_ERR_{seq}__");
    // The leading \n puts the sentinel on its own line even when the command's
    // output does not end with a newline; the buffer strips it back out. The
    // first printf must run before the second so `$?` is still the command's.
    let payload = format!(
        "{command}\nprintf '\\n{out_prefix}%d__\\n' $?\nprintf '\\n{err_sentinel}\\n' >&2\n"
    );
    shell.data(payload.as_bytes()).await.map_err(|e| SshError::new(e.to_string()))?;

    let mut stdout = StreamBuf::new();
    let mut stderr = StreamBuf::new();
    let mut exit_status = None;
    while exit_status.is_none() || !stderr.done {
        let Some(msg) = shell.wait().await else {
            // The shell exited (`exit`, a syntax error that aborted it, a
            // dropped connection); this session cannot run anything more.
            return Err(SshError::new("the remote shell exited"));
        };
        match msg {
            ChannelMsg::Data { ref data } => {
                stdout.push(data);
                if exit_status.is_none() {
                    exit_status = stdout.take_status_sentinel(&out_prefix);
                }
            }
            // ext 1 is stderr; other extended data types don't exist in practice.
            ChannelMsg::ExtendedData { ref data, ext: 1 } => {
                stderr.push(data);
                stderr.take_sentinel(&err_sentinel);
            }
            _ => {}
        }
    }
    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&stdout.data).into_owned(),
        stderr: String::from_utf8_lossy(&stderr.data).into_owned(),
        exit_status,
    })
}

/// One output stream of the shared shell: accumulates bytes until its
/// sentinel shows up, then truncates itself to just the command's output.
struct StreamBuf {
    data: Vec<u8>,
    /// Where the next sentinel scan resumes, so megabytes of output aren't
    /// rescanned on every incoming packet. Always backed off far enough that
    /// a sentinel cut in half by a packet boundary is still found whole.
    scanned: usize,
    done: bool,
}

impl StreamBuf {
    fn new() -> StreamBuf {
        StreamBuf { data: Vec::new(), scanned: 0, done: false }
    }

    /// Anything arriving after the sentinel belongs to no command (stray
    /// background-job output); a terminal would show it, this drops it.
    fn push(&mut self, chunk: &[u8]) {
        if !self.done {
            self.data.extend_from_slice(chunk);
        }
    }

    /// Look for `prefix` + digits + `__`; on a hit, cut the buffer back to
    /// the command's output and return the parsed exit status.
    fn take_status_sentinel(&mut self, prefix: &str) -> Option<u32> {
        if self.done {
            return None;
        }
        match find_status_sentinel(&self.data, self.scanned, prefix) {
            Some((start, status)) => {
                self.finish(start);
                Some(status)
            }
            None => {
                // Digits and terminator are at most u32's 10 digits + 2.
                self.scanned = self.data.len().saturating_sub(prefix.len() + 12);
                None
            }
        }
    }

    /// Same, for the fixed stderr sentinel (which carries no status).
    fn take_sentinel(&mut self, sentinel: &str) {
        if self.done {
            return;
        }
        match find(&self.data[self.scanned..], sentinel.as_bytes()) {
            Some(pos) => self.finish(self.scanned + pos),
            None => self.scanned = self.data.len().saturating_sub(sentinel.len()),
        }
    }

    fn finish(&mut self, sentinel_start: usize) {
        self.data.truncate(sentinel_start);
        // Drop the newline the sentinel printf injected before itself.
        if self.data.last() == Some(&b'\n') {
            self.data.pop();
        }
        self.done = true;
    }
}

/// Find `prefix` followed by decimal digits and a closing `__`. Returns the
/// index of the prefix and the parsed status only once the whole sentinel has
/// arrived — one cut in half by a packet boundary is found on a later call.
/// Command output that merely mentions the prefix is skipped over.
fn find_status_sentinel(data: &[u8], mut from: usize, prefix: &str) -> Option<(usize, u32)> {
    while let Some(pos) = find(&data[from..], prefix.as_bytes()) {
        let start = from + pos;
        let rest = &data[start + prefix.len()..];
        let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        if rest.len() < digits + 2 {
            // The tail of the buffer; decide when the rest arrives.
            return None;
        }
        if digits > 0 && &rest[digits..digits + 2] == b"__" {
            if let Ok(status) = std::str::from_utf8(&rest[..digits]).unwrap_or("").parse() {
                return Some((start, status));
            }
        }
        from = start + prefix.len();
    }
    None
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Split command output into the lines the results list shows: trailing
/// newlines are dropped and `\r\n` is normalised, but blank lines inside the
/// output are kept so the shape of e.g. a `df` table survives.
pub fn output_lines(text: &str) -> Vec<String> {
    text.trim_end_matches(['\n', '\r'])
        .split('\n')
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_lines_drops_only_the_trailing_newline() {
        assert_eq!(output_lines("a\r\nb\n"), vec!["a", "b"]);
        assert_eq!(output_lines("a\n\nb"), vec!["a", "", "b"]);
        assert_eq!(output_lines(""), vec![""]);
    }

    fn buf_with(chunks: &[&str], prefix: &str) -> (StreamBuf, Option<u32>) {
        let mut buf = StreamBuf::new();
        let mut status = None;
        for chunk in chunks {
            buf.push(chunk.as_bytes());
            if status.is_none() {
                status = buf.take_status_sentinel(prefix);
            }
        }
        (buf, status)
    }

    #[test]
    fn sentinel_terminates_output_and_carries_the_status() {
        let (buf, status) = buf_with(&["hello\n\n__LT_EOC_3_0__\n"], "__LT_EOC_3_");
        assert_eq!(status, Some(0));
        assert_eq!(buf.data, b"hello\n");
    }

    #[test]
    fn output_without_trailing_newline_survives_intact() {
        let (buf, status) = buf_with(&["no newline\n__LT_EOC_1_2__\n"], "__LT_EOC_1_");
        assert_eq!(status, Some(2));
        assert_eq!(buf.data, b"no newline");
    }

    #[test]
    fn sentinel_split_across_packets_is_still_found() {
        let (buf, status) = buf_with(&["a\n\n__LT_EO", "C_7_12", "7__\n"], "__LT_EOC_7_");
        assert_eq!(status, Some(127));
        assert_eq!(buf.data, b"a\n");
    }

    #[test]
    fn output_mentioning_the_prefix_is_not_a_sentinel() {
        let (buf, status) =
            buf_with(&["__LT_EOC_5_x fake\n\n__LT_EOC_5_1__\n"], "__LT_EOC_5_");
        assert_eq!(status, Some(1));
        assert_eq!(buf.data, b"__LT_EOC_5_x fake\n");
    }

    #[test]
    fn stderr_sentinel_truncates_and_marks_done() {
        let mut buf = StreamBuf::new();
        buf.push(b"warning\n\n__LT_ERR_2__\nignored after");
        buf.take_sentinel("__LT_ERR_2__");
        assert!(buf.done);
        assert_eq!(buf.data, b"warning\n");
        buf.push(b"more ignored");
        assert_eq!(buf.data, b"warning\n");
    }

    #[test]
    fn empty_output_reduces_to_nothing() {
        // A silent command still produces the injected "\n" + sentinel.
        let (buf, status) = buf_with(&["\n__LT_EOC_9_0__\n"], "__LT_EOC_9_");
        assert_eq!(status, Some(0));
        assert_eq!(buf.data, b"");
    }
}
