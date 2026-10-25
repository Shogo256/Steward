// SPDX-License-Identifier: Apache-2.0
//! The ledger, `steward.ledger`: what a project writes itself and how each
//! part of it is kept. A strict line format, read here:
//!
//! ```text
//! # A comment: a line that starts with #.
//! project = Basis
//! own = host/src/**/*.rs        # (repeatable) files the project writes
//! exempt = host/src/bin/*.rs    # (repeatable) owned, but no component's
//! lock = host/Cargo.lock        # (repeatable) must hold path packages only
//! toolchain = rustc --version   # (repeatable) what identifies the tools
//! stale = 90                    # days after which a verification is old
//!
//! # Signed reviews, kept apart from the changes they review (optional):
//! reviewer = alice keys/alice.pub alice@example.org   # (repeatable) name,
//! #   public key file, and the git emails the reviewer writes code as
//! sign = basis-token sign "$STEWARD_REVIEWER" "$STEWARD_MESSAGE"
//! check-signature = basis-token verify "$STEWARD_PUBLIC" "$STEWARD_MESSAGE" "$STEWARD_SIGNATURE"
//!
//! [sha3]
//! files = host/src/hash.rs      # (repeatable) globs
//! standard = FIPS 202 (August 2015)
//! verify = cd host && cargo test --lib hash   # (repeatable) sh -c, in the project
//! recheck = 2027-04-01          # by when to read the standard's errata again
//! ```
//!
//! A value is the rest of its line after `= `; a `#` in it is part of it
//! (comments are whole lines). Anything else is an error with its line.

use crate::date::Date;

#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub name: String,
    pub files: Vec<String>,
    pub standards: Vec<String>,
    pub verify: Vec<String>,
    pub recheck: Option<Date>,
    pub line: usize,
}

/// Someone who may review: a name, a file holding their public key, and the
/// email addresses git records them by as an author.
#[derive(Debug, Clone, PartialEq)]
pub struct Reviewer {
    pub name: String,
    pub public: String,
    pub emails: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ledger {
    pub project: String,
    pub own: Vec<String>,
    pub exempt: Vec<String>,
    pub locks: Vec<String>,
    pub toolchain: Vec<String>,
    pub stale_days: i64,
    pub components: Vec<Component>,
    pub reviewers: Vec<Reviewer>,
    /// The command that signs a review, and the one that checks a
    /// signature; both or neither, and with reviewers.
    pub sign: Option<String>,
    pub check_signature: Option<String>,
}

impl Ledger {
    /// Whether reviews are signed and kept apart from their changes.
    pub fn signed(&self) -> bool {
        !self.reviewers.is_empty()
    }
}

pub fn parse(text: &str) -> Result<Ledger, String> {
    let mut l = Ledger { stale_days: 90, ..Ledger::default() };
    let mut current: Option<Component> = None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let name = rest.strip_suffix(']').ok_or(format!("line {n}: a section is [name]"))?.trim();
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
                return Err(format!("line {n}: a component's name is letters, digits, - _ and ."));
            }
            if l.components.iter().chain(current.as_ref()).any(|c| c.name == name) {
                return Err(format!("line {n}: component [{name}] twice"));
            }
            if let Some(c) = current.take() {
                l.components.push(finish(c)?);
            }
            current = Some(Component { name: name.to_string(), files: vec![], standards: vec![], verify: vec![], recheck: None, line: n });
            continue;
        }
        let (key, value) = line.split_once('=').ok_or(format!("line {n}: expected `key = value`"))?;
        let (key, value) = (key.trim(), value.trim());
        if value.is_empty() {
            return Err(format!("line {n}: `{key}` has no value"));
        }
        match (&mut current, key) {
            (None, "project") => l.project = value.to_string(),
            (None, "own") => l.own.push(value.to_string()),
            (None, "exempt") => l.exempt.push(value.to_string()),
            (None, "lock") => l.locks.push(value.to_string()),
            (None, "toolchain") => l.toolchain.push(value.to_string()),
            (None, "stale") => l.stale_days = value.parse().map_err(|_| format!("line {n}: stale is a number of days"))?,
            (None, "reviewer") => {
                let words: Vec<&str> = value.split_whitespace().collect();
                if words.len() < 3 {
                    return Err(format!("line {n}: a reviewer is a name, a public key file and their git email(s)"));
                }
                if !words[0].chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
                    return Err(format!("line {n}: a reviewer's name is letters, digits, - _ and ."));
                }
                if l.reviewers.iter().any(|r| r.name == words[0]) {
                    return Err(format!("line {n}: reviewer {} twice", words[0]));
                }
                l.reviewers.push(Reviewer {
                    name: words[0].to_string(),
                    public: words[1].to_string(),
                    emails: words[2..].iter().map(|e| e.to_ascii_lowercase()).collect(),
                });
            }
            (None, "sign") if l.sign.is_none() => l.sign = Some(value.to_string()),
            (None, "check-signature") if l.check_signature.is_none() => l.check_signature = Some(value.to_string()),
            (None, "sign" | "check-signature") => return Err(format!("line {n}: `{key}` twice")),
            (Some(c), "files") => c.files.push(value.to_string()),
            (Some(c), "standard") => c.standards.push(value.to_string()),
            (Some(c), "verify") => c.verify.push(value.to_string()),
            (Some(c), "recheck") => {
                if c.recheck.is_some() {
                    return Err(format!("line {n}: recheck twice"));
                }
                c.recheck = Some(Date::parse(value).ok_or(format!("line {n}: recheck is YYYY-MM-DD"))?);
            }
            (None, _) => return Err(format!("line {n}: `{key}` is not a project setting (or belongs in a [component])")),
            (Some(c), _) => return Err(format!("line {n}: `{key}` is not a setting of [{}]", c.name)),
        }
    }
    if let Some(c) = current.take() {
        l.components.push(finish(c)?);
    }
    if l.project.is_empty() {
        return Err("the ledger names no project".into());
    }
    if l.signed() != l.sign.is_some() || l.signed() != l.check_signature.is_some() {
        return Err("signed reviews need reviewers, `sign` and `check-signature`, all three".into());
    }
    Ok(l)
}

fn finish(c: Component) -> Result<Component, String> {
    if c.files.is_empty() {
        return Err(format!("line {}: [{}] has no files", c.line, c.name));
    }
    if c.verify.is_empty() {
        return Err(format!("line {}: [{}] has no verify command: a part kept by no check is not kept", c.line, c.name));
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "# a project\nproject = P\nown = src/**/*.rs\nexempt = src/main.rs\nlock = Cargo.lock\ntoolchain = rustc --version\nstale = 30\n\n[hash]\nfiles = src/hash.rs\nstandard = FIPS 202 # August 2015\nverify = cargo test hash\nverify = python3 check.py\nrecheck = 2027-01-31\n";

    #[test]
    fn a_ledger_reads() {
        let l = parse(GOOD).unwrap();
        assert_eq!(l.project, "P");
        assert_eq!(l.stale_days, 30);
        assert_eq!(l.components.len(), 1);
        let c = &l.components[0];
        assert_eq!(c.standards, vec!["FIPS 202 # August 2015"]);
        assert_eq!(c.verify.len(), 2);
        assert_eq!(c.recheck.unwrap().to_string(), "2027-01-31");
    }

    #[test]
    fn reviewers_read_with_their_emails() {
        let l = parse(
            "project = P\nreviewer = alice keys/a.pub Alice@Example.org a2@x\nsign = s\ncheck-signature = c\n",
        )
        .unwrap();
        assert!(l.signed());
        assert_eq!(l.reviewers[0].emails, vec!["alice@example.org", "a2@x"]);
        assert!(!parse(GOOD).unwrap().signed());
    }

    #[test]
    fn faults_are_named_with_their_line() {
        for (bad, says) in [
            ("project = P\n[a]\nfiles = x\n", "no verify"),
            ("project = P\n[a]\nverify = true\n", "no files"),
            ("project = P\nfiles = x\n", "not a project setting"),
            ("project = P\n[a]\nfiles = x\nverify = true\n[a]\nfiles = y\nverify = true\n", "twice"),
            ("project = P\n[a]\nfiles = x\nverify = true\nrecheck = 2027-13-01\n", "YYYY-MM-DD"),
            ("project = P\n[a b]\n", "letters"),
            ("own = x\n", "no project"),
            ("project = P\nwhat\n", "key = value"),
            ("project = P\n[a]\nfiles = x\nverify = true\ncolour = red\n", "not a setting of [a]"),
            ("project = P\nreviewer = a k.pub\n", "git email"),
            ("project = P\nreviewer = a k.pub a@x\n", "all three"),
            ("project = P\nsign = s\ncheck-signature = c\n", "all three"),
            ("project = P\nreviewer = a k a@x\nreviewer = a k b@x\n", "twice"),
            ("project = P\nsign = s\nsign = t\n", "twice"),
        ] {
            let e = parse(bad).unwrap_err();
            assert!(e.contains(says), "{bad:?}: {e}");
        }
    }
}
