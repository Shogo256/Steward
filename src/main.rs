// SPDX-License-Identifier: Apache-2.0
//! steward: the upkeep of code a project writes itself.
//!
//! A project that depends on nothing from outside has no upstream to find
//! and fix its faults; it has to keep its own parts. Steward reads the
//! project's ledger (`steward.ledger`, see ledger.rs) and answers what that
//! takes: which files are the project's own and which part each belongs
//! to, which standard each part follows, how each is verified, whether a
//! part has changed since someone last reviewed it, when it was last
//! verified and with which tools, and when its standard is due to be read
//! again.
//!
//!     steward [--dir DIR] status            the default; exit 1 on a fault
//!     steward [--dir DIR] verify [PART...]  run the parts' checks, record them
//!     steward [--dir DIR] review PART [--as REVIEWER] --note TEXT
//!     steward [--dir DIR] report [--strict]
//!     steward [--dir DIR] deps
//!     steward [--dir DIR] toolchain
//!     steward [--dir DIR] hash PART
//!
//! Where the ledger names reviewers, a review is signed and is refused to
//! anyone who wrote a change it would cover (see signing.rs).

mod date;
mod glob;
mod ledger;
mod lockfile;
mod records;
mod sha256;
mod signing;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{exit, Command};
use std::time::Instant;

use date::Date;
use ledger::{Component, Ledger};
use records::{Review, Signed, Verification};

const LEDGER: &str = "steward.ledger";

struct Project {
    dir: PathBuf,
    ledger: Ledger,
}

impl Project {
    fn open(dir: &Path) -> Result<Project, String> {
        let path = dir.join(LEDGER);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let ledger = ledger::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Project { dir: dir.to_path_buf(), ledger })
    }

    /// The files of a component; the patterns that match nothing.
    fn files(&self, c: &Component) -> (Vec<String>, Vec<String>) {
        let mut files = BTreeSet::new();
        let mut missing = Vec::new();
        for pattern in &c.files {
            let found = glob::files(&self.dir, pattern);
            if found.is_empty() {
                missing.push(pattern.clone());
            }
            files.extend(found);
        }
        (files.into_iter().collect(), missing)
    }

    /// SHA-256 over the component's files, sorted: each path, a NUL, its
    /// length as 8 bytes little-endian, its contents.
    fn hash(&self, c: &Component) -> Result<String, String> {
        let (files, _) = self.files(c);
        let mut h = sha256::Sha256::new();
        for f in &files {
            let data = std::fs::read(self.dir.join(f)).map_err(|e| format!("{f}: {e}"))?;
            h.update(f.as_bytes());
            h.update(&[0]);
            h.update(&(data.len() as u64).to_le_bytes());
            h.update(&data);
        }
        Ok(sha256::hex(&h.finish()))
    }

    /// The toolchain's outputs, and their fingerprint (16 hex digits).
    fn toolchain(&self) -> (String, String) {
        let mut text = String::new();
        for cmd in &self.ledger.toolchain {
            let out = Command::new("sh").arg("-c").arg(cmd).current_dir(&self.dir).output();
            let shown = match out {
                Ok(o) => format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
                Err(e) => format!("(could not run: {e})\n"),
            };
            text.push_str(&format!("$ {cmd}\n{}", shown.trim_end()));
            text.push('\n');
        }
        let fp = sha256::digest_hex(text.as_bytes())[..16].to_string();
        (text, fp)
    }

    fn component(&self, name: &str) -> Result<&Component, String> {
        self.ledger
            .components
            .iter()
            .find(|c| c.name == name)
            .ok_or(format!("no part [{name}] in {}; it has {}", LEDGER, self.names().join(", ")))
    }

    fn names(&self) -> Vec<String> {
        self.ledger.components.iter().map(|c| c.name.clone()).collect()
    }

    /// Packages from outside in the ledger's lock files: (lock, package, source).
    fn outside_packages(&self) -> Result<Vec<(String, String, String)>, String> {
        let mut out = Vec::new();
        for lock in &self.ledger.locks {
            let text = std::fs::read_to_string(self.dir.join(lock)).map_err(|e| format!("{lock}: {e}"))?;
            for (name, source) in lockfile::packages(&text) {
                if let Some(s) = source {
                    out.push((lock.clone(), name, s));
                }
            }
        }
        Ok(out)
    }
}

fn last<T>(items: &[T], keep: impl Fn(&T) -> bool) -> Option<&T> {
    items.iter().rev().find(|x| keep(x))
}

// ------------------------------------------------------------------ status

fn status(p: &Project) -> Result<bool, String> {
    let reviews = records::reviews(&p.dir)?;
    let mut ok = true;
    let mut claimed: BTreeMap<String, String> = BTreeMap::new();
    println!("{}: {} parts", p.ledger.project, p.ledger.components.len());
    let width = p.ledger.components.iter().map(|c| c.name.len()).max().unwrap_or(0);
    for c in &p.ledger.components {
        let (files, missing) = p.files(c);
        for f in &files {
            if let Some(other) = claimed.insert(f.clone(), c.name.clone()) {
                println!("  TWICE      {f} is in [{other}] and [{}]", c.name);
                ok = false;
            }
        }
        let hash = p.hash(c)?;
        let state = match last(&reviews, |r| r.component == c.name) {
            None => {
                ok = false;
                "UNREVIEWED  never reviewed".to_string()
            }
            Some(r) if r.hash == hash => match signed_fault(p, c, &reviews, r, &files) {
                None => format!("ok          reviewed {}{}", r.date, by(r)),
                Some(fault) => {
                    ok = false;
                    fault
                }
            },
            Some(r) => {
                ok = false;
                format!("CHANGED     since its review of {}", r.date)
            }
        };
        let count = if files.len() == 1 { "1 file".to_string() } else { format!("{} files", files.len()) };
        println!("  {:<width$}  {state}  ({count})", c.name);
        for m in missing {
            println!("  MISSING    [{}] {m} matches no file", c.name);
            ok = false;
        }
    }
    let exempt: BTreeSet<String> = p.ledger.exempt.iter().flat_map(|e| glob::files(&p.dir, e)).collect();
    let mut owned = BTreeSet::new();
    for pattern in &p.ledger.own {
        owned.extend(glob::files(&p.dir, pattern));
    }
    let unclaimed: Vec<&String> = owned.iter().filter(|f| !claimed.contains_key(*f) && !exempt.contains(*f)).collect();
    for f in &unclaimed {
        println!("  UNCLAIMED  {f}: owned, but in no part (add it to one, or to `exempt`)");
    }
    ok &= unclaimed.is_empty();
    ok &= deps(p)?;
    println!("{}", if ok { "every part is as reviewed" } else { "faults above" });
    Ok(ok)
}

/// " by NAME" for a signed review.
fn by(r: &Review) -> String {
    r.signed.as_ref().map(|s| format!(" by {}", s.reviewer)).unwrap_or_default()
}

/// Where reviews are signed, what is wrong with the part's last review `r`:
/// unsigned, by no listed reviewer, a signature that does not check, or a
/// reviewer who wrote a change it covers. None where reviews are not signed.
fn signed_fault(p: &Project, c: &Component, reviews: &[Review], r: &Review, files: &[String]) -> Option<String> {
    if !p.ledger.signed() {
        return None;
    }
    let Some(s) = &r.signed else { return Some(format!("UNSIGNED    the review of {} is not signed", r.date)) };
    let Some(reviewer) = p.ledger.reviewers.iter().find(|x| x.name == s.reviewer) else {
        return Some(format!("FORGED      {} is no reviewer of this ledger", s.reviewer));
    };
    match signing::check(&p.dir, &p.ledger, reviewer, &records::message(&p.ledger.project, r, s), &s.signature) {
        Ok(true) => {}
        Ok(false) => return Some(format!("FORGED      the review of {} does not carry {}'s signature", r.date, s.reviewer)),
        Err(e) => return Some(format!("UNCHECKED   {e}")),
    }
    let earlier = reviews.iter().take_while(|x| !std::ptr::eq(*x, r)).filter(|x| x.component == c.name && x.signed.is_some());
    let since = earlier.last().and_then(|x| x.signed.as_ref()).map(|x| x.commit.as_str());
    match signing::authors(&p.dir, since, &s.commit, files) {
        Ok(authors) => {
            let own: Vec<&String> = reviewer.emails.iter().filter(|e| authors.contains(*e)).collect();
            if let Some(e) = own.first() {
                return Some(format!("SELF-REVIEW {} reviewed changes written as {e}", s.reviewer));
            }
        }
        Err(e) => return Some(format!("UNCHECKED   {e}")),
    }
    None
}

fn deps(p: &Project) -> Result<bool, String> {
    let outside = p.outside_packages()?;
    for lock in &p.ledger.locks {
        let n = outside.iter().filter(|(l, _, _)| l == lock).count();
        if n == 0 {
            println!("  lock       {lock}: our own packages only");
        }
    }
    for (lock, name, source) in &outside {
        println!("  OUTSIDE    {lock}: {name} from {source}");
    }
    Ok(outside.is_empty())
}

// ------------------------------------------------------------------ verify

fn verify(p: &Project, names: &[String]) -> Result<bool, String> {
    let chosen: Vec<&Component> = if names.is_empty() {
        p.ledger.components.iter().collect()
    } else {
        names.iter().map(|n| p.component(n)).collect::<Result<_, _>>()?
    };
    let (_, toolchain) = p.toolchain();
    let today = Date::today();
    let mut all = true;
    let width = chosen.iter().map(|c| c.name.len()).max().unwrap_or(0);
    for c in chosen {
        let hash = p.hash(c)?;
        let start = Instant::now();
        let mut passed = true;
        for cmd in &c.verify {
            let out = Command::new("sh").arg("-c").arg(cmd).current_dir(&p.dir).output().map_err(|e| format!("sh: {e}"))?;
            if !out.status.success() {
                passed = false;
                println!("  FAIL  [{}] {cmd}", c.name);
                let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                let tail: Vec<&str> = text.lines().rev().take(30).collect();
                for l in tail.into_iter().rev() {
                    println!("        {l}");
                }
                break;
            }
        }
        let seconds = start.elapsed().as_secs_f64();
        records::record_verification(
            &p.dir,
            &Verification { date: today, component: c.name.clone(), hash, passed, seconds, toolchain: toolchain.clone() },
        )?;
        println!("  {}  {:<width$}  {:.1} s", if passed { "pass" } else { "FAIL" }, c.name, seconds);
        all &= passed;
    }
    Ok(all)
}

// ------------------------------------------------------------------ review

fn review(p: &Project, name: &str, note: &str, reviewer: Option<&str>) -> Result<(), String> {
    if note.trim().is_empty() {
        return Err("a review says what was looked at: --note TEXT".into());
    }
    if !p.ledger.signed() && reviewer.is_some() {
        return Err("this ledger names no reviewers: review without --as".into());
    }
    let c = p.component(name)?;
    let hash = p.hash(c)?;
    let (_, toolchain) = p.toolchain();
    let history = records::history(&p.dir)?;
    match last(&history, |v| v.component == c.name) {
        None => return Err(format!("[{name}] has never been verified: run `steward verify {name}` first")),
        Some(v) if v.hash != hash => {
            return Err(format!("[{name}] changed since its last verification ({}): run `steward verify {name}` again", v.date))
        }
        Some(v) if !v.passed => return Err(format!("[{name}]'s last verification failed ({}); a failing part is not reviewed", v.date)),
        Some(v) if v.toolchain != toolchain => {
            return Err(format!("the toolchain changed since [{name}] was last verified ({}): verify it with these tools first", v.date))
        }
        Some(_) => {}
    }
    let mut r = Review { date: Date::today(), component: c.name.clone(), hash, toolchain, note: note.to_string(), signed: None };
    if p.ledger.signed() {
        r.signed = Some(signed_review(p, c, &r, reviewer.ok_or("reviews here are signed: review PART --as REVIEWER --note TEXT")?)?);
    }
    records::record_review(&p.dir, &r)?;
    println!("[{name}] reviewed{}", by(&r));
    Ok(())
}

/// The signature of `reviewer` on the review `r` of `c`, once the part is
/// committed as it stands and none of its changes since its last signed
/// review are theirs.
fn signed_review(p: &Project, c: &Component, r: &Review, reviewer: &str) -> Result<Signed, String> {
    let who = p.ledger.reviewers.iter().find(|x| x.name == reviewer).ok_or_else(|| {
        format!("{reviewer} is not a reviewer of this ledger; it has {}", p.ledger.reviewers.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", "))
    })?;
    let (files, _) = p.files(c);
    let dirty = signing::uncommitted(&p.dir, &files)?;
    if !dirty.is_empty() {
        return Err(format!("[{}] has changes not committed ({}): a review is of a commit", c.name, dirty.join(", ")));
    }
    let commit = signing::head(&p.dir)?;
    let reviews = records::reviews(&p.dir)?;
    let since = last(&reviews, |x| x.component == c.name && x.signed.is_some()).and_then(|x| x.signed.as_ref()).map(|x| x.commit.clone());
    let authors = signing::authors(&p.dir, since.as_deref(), &commit, &files)?;
    if let Some(e) = who.emails.iter().find(|e| authors.contains(*e)) {
        return Err(format!(
            "{reviewer} wrote changes to [{}] since its last signed review (as {e}): another reviewer reviews them",
            c.name
        ));
    }
    let mut s = Signed { commit, reviewer: reviewer.to_string(), signature: String::new() };
    let message = records::message(&p.ledger.project, r, &s);
    s.signature = signing::sign(&p.dir, &p.ledger, who, &message)?;
    if !signing::check(&p.dir, &p.ledger, who, &message, &s.signature)? {
        return Err(format!("the signature does not check against {}'s public key ({})", who.name, who.public));
    }
    Ok(s)
}

// ------------------------------------------------------------------ report

fn report(p: &Project, strict: bool) -> Result<bool, String> {
    let history = records::history(&p.dir)?;
    let reviews = records::reviews(&p.dir)?;
    let (_, toolchain) = p.toolchain();
    let today = Date::today();
    let mut due = 0;
    println!("{} on {today}, toolchain {toolchain}", p.ledger.project);
    for c in &p.ledger.components {
        let hash = p.hash(c)?;
        println!();
        println!("[{}]", c.name);
        for s in &c.standards {
            println!("  standard   {s}");
        }
        match last(&reviews, |r| r.component == c.name) {
            Some(r) if r.hash == hash => println!("  review     {}{} ({})", r.date, by(r), r.note),
            Some(r) => {
                due += 1;
                println!("  review     CHANGED since {} ({})", r.date, r.note)
            }
            None => {
                due += 1;
                println!("  review     NONE")
            }
        }
        match last(&history, |v| v.component == c.name) {
            Some(v) => {
                let age = today.days - v.date.days;
                let old = age > p.ledger.stale_days;
                let moved = v.toolchain != toolchain;
                let changed = v.hash != hash;
                due += usize::from(old || moved || changed || !v.passed);
                println!(
                    "  verified   {} {} days ago, {} in {:.1} s{}{}{}",
                    v.date,
                    age,
                    if v.passed { "passed" } else { "FAILED" },
                    v.seconds,
                    if old { format!("; STALE (over {} days)", p.ledger.stale_days) } else { String::new() },
                    if moved { "; TOOLCHAIN CHANGED since" } else { "" },
                    if changed { "; the files CHANGED since" } else { "" },
                );
            }
            None => {
                due += 1;
                println!("  verified   NEVER")
            }
        }
        match c.recheck {
            Some(d) if d <= today => {
                due += 1;
                println!("  recheck    DUE since {d}: read the standard and its errata again, then move the date")
            }
            Some(d) => println!("  recheck    {d}, in {} days", d.days - today.days),
            None => println!("  recheck    no date set"),
        }
    }
    println!();
    println!("{due} thing(s) to attend to");
    Ok(!strict || due == 0)
}

// -------------------------------------------------------------------- main

fn usage() -> ! {
    eprintln!("steward [--dir DIR] [status | verify [PART...] | review PART [--as REVIEWER] --note TEXT | report [--strict] | deps | toolchain | hash PART]");
    exit(2)
}

fn run(args: &[String]) -> Result<bool, String> {
    let mut dir = PathBuf::from(".");
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dir" => dir = PathBuf::from(it.next().ok_or("--dir needs a directory")?),
            "-h" | "--help" => usage(),
            _ => rest.push(a.clone()),
        }
    }
    let p = Project::open(&dir)?;
    match rest.first().map(String::as_str) {
        None | Some("status") => status(&p),
        Some("verify") => verify(&p, &rest[1..]),
        Some("review") => {
            let name = rest.get(1).ok_or("review needs a part's name")?;
            let (mut note, mut reviewer) = (None, None);
            let mut it = rest[2..].iter();
            while let Some(a) = it.next() {
                match (a.as_str(), it.next()) {
                    ("--note", Some(n)) if note.is_none() => note = Some(n.clone()),
                    ("--as", Some(r)) if reviewer.is_none() => reviewer = Some(r.clone()),
                    _ => return Err("review PART [--as REVIEWER] --note TEXT".into()),
                }
            }
            review(&p, name, &note.ok_or("review PART [--as REVIEWER] --note TEXT")?, reviewer.as_deref()).map(|_| true)
        }
        Some("report") => report(&p, rest.get(1).map(String::as_str) == Some("--strict")),
        Some("deps") => deps(&p),
        Some("toolchain") => {
            let (text, fp) = p.toolchain();
            print!("{text}");
            println!("fingerprint {fp}");
            Ok(true)
        }
        Some("hash") => {
            let c = p.component(rest.get(1).ok_or("hash needs a part's name")?)?;
            println!("{}", p.hash(c)?);
            Ok(true)
        }
        Some(other) => Err(format!("unknown command `{other}`")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => {}
        Ok(false) => exit(1),
        Err(e) => {
            eprintln!("steward: {e}");
            exit(2);
        }
    }
}
