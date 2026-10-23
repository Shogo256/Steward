// SPDX-License-Identifier: Apache-2.0
//! What Steward writes beside the ledger, one line a record, tab-separated,
//! appended and never rewritten, so that the history of a project's upkeep
//! is itself under version control:
//!
//! - `steward.history`: every verification, `date component hash result
//!   seconds toolchain`;
//! - `steward.reviews`: every review, `date component hash toolchain note`.

use std::io::Write;
use std::path::Path;

use crate::date::Date;

#[derive(Debug, Clone, PartialEq)]
pub struct Verification {
    pub date: Date,
    pub component: String,
    pub hash: String,
    pub passed: bool,
    pub seconds: f64,
    pub toolchain: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    pub date: Date,
    pub component: String,
    pub hash: String,
    pub toolchain: String,
    pub note: String,
}

pub const HISTORY: &str = "steward.history";
pub const REVIEWS: &str = "steward.reviews";

fn lines(path: &Path) -> Result<Vec<(usize, Vec<String>)>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    Ok(text.lines().enumerate().filter(|(_, l)| !l.is_empty()).map(|(i, l)| (i + 1, l.split('\t').map(String::from).collect())).collect())
}

fn date(path: &Path, n: usize, s: &str) -> Result<Date, String> {
    Date::parse(s).ok_or(format!("{}:{n}: a date is YYYY-MM-DD", path.display()))
}

pub fn history(dir: &Path) -> Result<Vec<Verification>, String> {
    let path = dir.join(HISTORY);
    lines(&path)?
        .into_iter()
        .map(|(n, f)| {
            if f.len() != 6 || !(f[3] == "pass" || f[3] == "fail") {
                return Err(format!("{}:{n}: a verification is date, component, hash, pass|fail, seconds, toolchain", path.display()));
            }
            Ok(Verification {
                date: date(&path, n, &f[0])?,
                component: f[1].clone(),
                hash: f[2].clone(),
                passed: f[3] == "pass",
                seconds: f[4].parse().map_err(|_| format!("{}:{n}: seconds", path.display()))?,
                toolchain: f[5].clone(),
            })
        })
        .collect()
}

pub fn reviews(dir: &Path) -> Result<Vec<Review>, String> {
    let path = dir.join(REVIEWS);
    lines(&path)?
        .into_iter()
        .map(|(n, f)| {
            if f.len() != 5 {
                return Err(format!("{}:{n}: a review is date, component, hash, toolchain, note", path.display()));
            }
            Ok(Review { date: date(&path, n, &f[0])?, component: f[1].clone(), hash: f[2].clone(), toolchain: f[3].clone(), note: f[4].clone() })
        })
        .collect()
}

fn append(dir: &Path, file: &str, fields: &[String]) -> Result<(), String> {
    let path = dir.join(file);
    let line = fields.iter().map(|f| f.replace(['\t', '\n', '\r'], " ")).collect::<Vec<_>>().join("\t");
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(f, "{line}").map_err(|e| format!("{}: {e}", path.display()))
}

pub fn record_verification(dir: &Path, v: &Verification) -> Result<(), String> {
    append(
        dir,
        HISTORY,
        &[v.date.to_string(), v.component.clone(), v.hash.clone(), if v.passed { "pass" } else { "fail" }.into(), format!("{:.1}", v.seconds), v.toolchain.clone()],
    )
}

pub fn record_review(dir: &Path, r: &Review) -> Result<(), String> {
    append(dir, REVIEWS, &[r.date.to_string(), r.component.clone(), r.hash.clone(), r.toolchain.clone(), r.note.clone()])
}
