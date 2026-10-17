//! Getting a backup off the host: a command the operator names, run after each backup (`--after-backup`).
//!
//! The orchestrator speaks to no cloud. It runs `rclone`, `scp`, `rsync`, `aws s3 cp`, or whatever the operator wrote, checks
//! that it exited with 0 and says so loudly if it did not. The file it hands over is already encrypted, so the destination
//! sees no customer data; the key (`backup.key`) is a separate matter (`backup-key`): sent along the same road it would put
//! the data and its key in one place.
//!
//! The command is split into arguments **before** anything is put in it, and there is no shell: `{file}`, `{customer}` and
//! `{name}` are replaced inside each argument, so a path with a space in it, or a customer id, never becomes part of a command.

use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context};
use async_trait::async_trait;
use tokio::process::Command;

/// Where a finished backup goes next.
#[async_trait]
pub trait BackupSink: Send + Sync {
    /// Hands `file`, the encrypted backup of `customer`, over. An error is a backup that is not off the host.
    async fn ship(&self, customer: &str, file: &Path) -> anyhow::Result<()>;
}

/// Runs the operator's command for each backup.
pub struct CommandSink {
    template: Vec<String>,
    timeout: Duration,
}

impl CommandSink {
    /// `command` is the text of `--after-backup`; `timeout` how long one run may take before it is killed.
    pub fn new(command: &str, timeout: Duration) -> anyhow::Result<Self> {
        let template = split_args(command).context("the --after-backup command is not well formed")?;
        anyhow::ensure!(!template.is_empty(), "the --after-backup command is empty");
        Ok(Self { template, timeout })
    }

    /// The program and arguments for one backup: the placeholders filled in, argument by argument.
    fn command_line(&self, customer: &str, file: &Path) -> (String, Vec<String>) {
        let path = file.to_string_lossy();
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let fill = |arg: &str| arg.replace("{file}", &path).replace("{customer}", customer).replace("{name}", &name);
        let mut filled = self.template.iter().map(|arg| fill(arg));
        let program = self.template[0].clone();
        filled.next(); // the program itself is taken as written
        (program, filled.collect())
    }
}

#[async_trait]
impl BackupSink for CommandSink {
    async fn ship(&self, customer: &str, file: &Path) -> anyhow::Result<()> {
        let (program, args) = self.command_line(customer, file);
        let child = Command::new(&program)
            .args(&args)
            .env("WARDEN_BACKUP_FILE", file)
            .env("WARDEN_BACKUP_CUSTOMER", customer)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            // Only the program is named: the arguments of a command that copies to a cloud may hold a key.
            .with_context(|| format!("could not run `{program}` (is it installed and on the PATH?)"))?;
        // A run that goes over the time is dropped here, and `kill_on_drop` ends the process.
        let output = match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
            Ok(output) => output?,
            Err(_) => bail!("`{program}` took more than {}s and was stopped", self.timeout.as_secs()),
        };
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).rev().take(5).collect();
        let tail = tail.into_iter().rev().collect::<Vec<_>>().join(" | ");
        let tail: String = tail.chars().take(500).collect();
        match output.status.code() {
            Some(code) => bail!("`{program}` exited with {code}{}", if tail.is_empty() { String::new() } else { format!(": {tail}") }),
            None => bail!("`{program}` was ended by a signal"),
        }
    }
}

/// Splits a command line into arguments the way a shell would for the simple cases: spaces separate, single quotes keep
/// everything inside as it is, double quotes keep spaces, and a backslash outside single quotes takes the next character
/// literally. No expansion of any kind. An open quote is an error.
pub fn split_args(text: &str) -> anyhow::Result<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false; // so that '' makes an empty argument
    let (mut single, mut double) = (false, false);
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' if !double => {
                single = !single;
                started = true;
            }
            '"' if !single => {
                double = !double;
                started = true;
            }
            '\\' if !single => {
                current.push(chars.next().context("the command ends in a lone backslash")?);
                started = true;
            }
            c if c.is_whitespace() && !single && !double => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if single || double {
        bail!("a quote is never closed");
    }
    if started {
        args.push(current);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sink(command: &str) -> CommandSink {
        CommandSink::new(command, Duration::from_secs(20)).unwrap()
    }

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("warden-orchestrator-ship-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_command_is_split_into_arguments_with_quotes_and_escapes() {
        assert_eq!(split_args("rclone copyto {file} remote:dir").unwrap(), ["rclone", "copyto", "{file}", "remote:dir"]);
        assert_eq!(split_args("  a   b  ").unwrap(), ["a", "b"]);
        assert_eq!(split_args(r#"sh -c 'cp "$1" /x' _"#).unwrap(), ["sh", "-c", r#"cp "$1" /x"#, "_"]);
        assert_eq!(split_args(r#"echo "two words" one\ word"#).unwrap(), ["echo", "two words", "one word"]);
        assert_eq!(split_args("a '' b").unwrap(), ["a", "", "b"], "an empty quoted argument is an argument");
        assert_eq!(split_args(r"a 'x\y'").unwrap(), ["a", r"x\y"], "nothing is special in single quotes");
        assert!(split_args("").unwrap().is_empty());
        assert!(split_args("a 'open").is_err());
        assert!(split_args(r#"a "open"#).is_err());
        assert!(split_args(r"a \").is_err());
    }

    #[test]
    fn an_empty_command_is_refused() {
        assert!(CommandSink::new("   ", Duration::from_secs(1)).is_err());
        assert!(CommandSink::new("a 'open", Duration::from_secs(1)).is_err());
    }

    #[test]
    fn placeholders_are_filled_inside_each_argument_and_never_split() {
        let sink = sink("scp {file} backup@host:/srv/{customer}/{name}");
        let (program, args) = sink.command_line("acme", Path::new("/var/lib/o/backups/acme/a file with spaces.wbk"));
        assert_eq!(program, "scp");
        assert_eq!(args, ["/var/lib/o/backups/acme/a file with spaces.wbk", "backup@host:/srv/acme/a file with spaces.wbk"]);
    }

    #[test]
    fn a_placeholder_in_the_program_is_not_filled() {
        let sink = sink("{file} x");
        let (program, _) = sink.command_line("acme", Path::new("/f.wbk"));
        assert_eq!(program, "{file}");
    }

    #[tokio::test]
    async fn the_command_runs_with_the_file_and_the_environment() {
        let dir = dir("runs");
        let file = dir.join("a backup.wbk");
        std::fs::write(&file, b"sealed bytes").unwrap();
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();

        // Copies by the placeholder, and writes the customer the command was told about through the environment.
        let command = format!(r#"sh -c 'cp "$1" "$2" && printf %s "$WARDEN_BACKUP_CUSTOMER:$WARDEN_BACKUP_FILE" > "$2.info"' _ {{file}} '{}/{{name}}'"#, out.display());
        sink(&command).ship("acme", &file).await.unwrap();

        assert_eq!(std::fs::read(out.join("a backup.wbk")).unwrap(), b"sealed bytes");
        let info = std::fs::read_to_string(out.join("a backup.wbk.info")).unwrap();
        assert_eq!(info, format!("acme:{}", file.display()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_command_that_fails_says_its_code_and_what_it_complained_of_but_not_its_arguments() {
        let err = sink("sh -c 'echo first >&2; echo no such bucket >&2; exit 7' secret-token-in-an-argument").ship("acme", Path::new("/f.wbk")).await.unwrap_err().to_string();
        assert!(err.contains("exited with 7") && err.contains("no such bucket"), "{err}");
        assert!(!err.contains("secret-token"), "the arguments may hold credentials: {err}");
    }

    #[tokio::test]
    async fn a_program_that_is_not_there_is_named_alone() {
        let err = sink("warden-no-such-program --key hunter2").ship("acme", Path::new("/f.wbk")).await.unwrap_err().to_string();
        assert!(err.contains("warden-no-such-program") && !err.contains("hunter2"), "{err}");
    }

    #[tokio::test]
    async fn a_command_that_takes_too_long_is_stopped() {
        let slow = CommandSink::new("sleep 30", Duration::from_millis(200)).unwrap();
        let started = std::time::Instant::now();
        let err = slow.ship("acme", Path::new("/f.wbk")).await.unwrap_err().to_string();
        assert!(err.contains("was stopped"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10), "it did not wait for the sleep");
    }
}
