// SPDX-License-Identifier: Apache-2.0
//! Signed reviews, kept apart from the changes they review.
//!
//! Where the ledger names reviewers, a review is made on a commit: the
//! part's files must be committed as they are, and the reviewer must not be
//! the author (or a co-author) of any commit that changed them since the
//! part's last signed review that was kept apart in the same way. The
//! review is signed by the ledger's `sign` command with the reviewer's key,
//! and every `status` checks each part's signature with `check-signature`
//! and the separation again from git, so that a record added or edited by
//! hand is caught rather than trusted.
//!
//! A reviewer the ledger names with `self-reviewer` may review changes they
//! wrote: the review is signed and stands, `status` says it is a
//! self-review, and the changes it covered stay to be covered by the next
//! review kept apart, should the ledger name them `reviewer` again.
//!
//! Steward signs nothing itself: the commands do, given the message in a
//! file. With Basis's token the key stays on the token, under its PIN:
//!
//! ```text
//! sign = basis-token sign "$STEWARD_REVIEWER" "$STEWARD_MESSAGE"
//! check-signature = basis-token verify "$STEWARD_PUBLIC" "$STEWARD_MESSAGE" "$STEWARD_SIGNATURE"
//! ```
//!
//! `sign` runs with the terminal's input, so that it can ask for a PIN, and
//! prints the signature; `check-signature` exits 0 for a good one.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ledger::{Ledger, Reviewer};

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The commit checked out.
pub fn head(dir: &Path) -> Result<String, String> {
    Ok(git(dir, &["rev-parse", "--verify", "HEAD"])?.trim().to_string())
}

/// The files among `files` that differ from the commit checked out, or are
/// not in it.
pub fn uncommitted(dir: &Path, files: &[String]) -> Result<Vec<String>, String> {
    let mut args = vec!["status", "--porcelain", "--untracked-files=all", "--"];
    args.extend(files.iter().map(String::as_str));
    Ok(git(dir, &args)?.lines().map(|l| l.get(3..).unwrap_or(l).to_string()).collect())
}

/// The email addresses of the authors and co-authors of the commits that
/// changed `files`, after `since` (from the first commit, if none) up to
/// `upto`, lowercased.
pub fn authors(dir: &Path, since: Option<&str>, upto: &str, files: &[String]) -> Result<BTreeSet<String>, String> {
    let range = match since {
        Some(s) => format!("{s}..{upto}"),
        None => upto.to_string(),
    };
    let mut args = vec!["log", "--format=%ae%n%(trailers:key=Co-authored-by,valueonly)", range.as_str(), "--"];
    args.extend(files.iter().map(String::as_str));
    let mut out = BTreeSet::new();
    for line in git(dir, &args)?.lines() {
        let line = line.trim();
        // A trailer is `Name <email>`; %ae the email alone.
        let email = match (line.find('<'), line.rfind('>')) {
            (Some(a), Some(b)) if a < b => &line[a + 1..b],
            _ => line,
        };
        if !email.is_empty() {
            out.insert(email.to_ascii_lowercase());
        }
    }
    Ok(out)
}

/// A file holding `text`, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(text: &str) -> Result<Scratch, String> {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("steward-{}-{}.msg", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        f.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Scratch(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `message` signed by `reviewer`, through the ledger's `sign` command.
pub fn sign(dir: &Path, ledger: &Ledger, reviewer: &Reviewer, message: &str) -> Result<String, String> {
    let cmd = ledger.sign.as_deref().ok_or("the ledger has no `sign` command")?;
    let file = Scratch::new(message)?;
    let out = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .env("STEWARD_REVIEWER", &reviewer.name)
        .env("STEWARD_MESSAGE", &file.0)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("sign: {e}"))?;
    if !out.status.success() {
        return Err(format!("`{cmd}` failed"));
    }
    let signature = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if signature.is_empty() || signature.contains(char::is_whitespace) {
        return Err(format!("`{cmd}` printed no signature (one word expected)"));
    }
    Ok(signature)
}

/// Whether `signature` is `reviewer`'s over `message`, by the ledger's
/// `check-signature` command.
pub fn check(dir: &Path, ledger: &Ledger, reviewer: &Reviewer, message: &str, signature: &str) -> Result<bool, String> {
    let cmd = ledger.check_signature.as_deref().ok_or("the ledger has no `check-signature` command")?;
    let file = Scratch::new(message)?;
    let status = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .env("STEWARD_PUBLIC", dir.join(&reviewer.public))
        .env("STEWARD_MESSAGE", &file.0)
        .env("STEWARD_SIGNATURE", signature)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("check-signature: {e}"))?;
    Ok(status.success())
}
