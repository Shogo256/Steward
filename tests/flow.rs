// SPDX-License-Identifier: Apache-2.0
//! The upkeep of a small project from end to end, through the binary: a
//! part never reviewed fails status; verify then review passes it; a change
//! to its file fails it again and cannot be reviewed until verified; a
//! failing check cannot be reviewed; a file owned but in no part, and a
//! package from outside in a lock, fail status.

use std::path::{Path, PathBuf};
use std::process::Command;

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("steward-flow-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/hash.rs"), "fn h() {}\n").unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.join("ok.sh"), "exit 0\n").unwrap();
    std::fs::write(dir.join("Cargo.lock"), "version = 4\n\n[[package]]\nname = \"p\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(
        dir.join("steward.ledger"),
        "project = P\nown = src/**/*.rs\nexempt = src/main.rs\nlock = Cargo.lock\ntoolchain = echo tools-1\n\n\
         [hash]\nfiles = src/hash.rs\nstandard = FIPS 202\nverify = sh ok.sh\nrecheck = 2099-01-01\n",
    )
    .unwrap();
    dir
}

fn steward(dir: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_steward")).arg("--dir").arg(dir).args(args).output().unwrap();
    (out.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

#[test]
fn a_part_is_verified_then_reviewed_and_held_to_it() {
    let d = project("main");
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("UNREVIEWED"), "{out}");

    let (code, out) = steward(&d, &["review", "hash", "--note", "read it"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("never been verified"), "{out}");

    assert_eq!(steward(&d, &["verify"]).0, 0);
    let (code, out) = steward(&d, &["review", "hash", "--note", "read against FIPS 202"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("every part is as reviewed"), "{out}");

    // A change: status fails, review is refused until verified again.
    std::fs::write(d.join("src/hash.rs"), "fn h() { 1; }\n").unwrap();
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 1);
    assert!(out.contains("CHANGED"), "{out}");
    let (code, out) = steward(&d, &["review", "hash", "--note", "x"]);
    assert_eq!(code, 2);
    assert!(out.contains("changed since its last verification"), "{out}");
    assert_eq!(steward(&d, &["verify", "hash"]).0, 0);
    assert_eq!(steward(&d, &["review", "hash", "--note", "the change read"]).0, 0);
    assert_eq!(steward(&d, &["status"]).0, 0);

    // The toolchain moves: review refused until verified with it.
    let ledger = std::fs::read_to_string(d.join("steward.ledger")).unwrap().replace("tools-1", "tools-2");
    std::fs::write(d.join("steward.ledger"), ledger).unwrap();
    let (code, out) = steward(&d, &["review", "hash", "--note", "x"]);
    assert_eq!(code, 2);
    assert!(out.contains("toolchain changed"), "{out}");
    let (_, out) = steward(&d, &["report"]);
    assert!(out.contains("TOOLCHAIN CHANGED"), "{out}");
    assert_eq!(steward(&d, &["report", "--strict"]).0, 1);

    // The records are lines appended, readable by people.
    let history = std::fs::read_to_string(d.join("steward.history")).unwrap();
    assert_eq!(history.lines().count(), 2);
    assert_eq!(std::fs::read_to_string(d.join("steward.reviews")).unwrap().lines().count(), 2);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_failing_check_is_not_reviewed() {
    let d = project("failing");
    std::fs::write(d.join("ok.sh"), "echo broken >&2; exit 3\n").unwrap();
    let (code, out) = steward(&d, &["verify"]);
    assert_eq!(code, 1);
    assert!(out.contains("FAIL") && out.contains("broken"), "{out}");
    let (code, out) = steward(&d, &["review", "hash", "--note", "x"]);
    assert_eq!(code, 2);
    assert!(out.contains("failed"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn unclaimed_files_missing_files_and_outside_packages_are_faults() {
    let d = project("faults");
    steward(&d, &["verify"]);
    steward(&d, &["review", "hash", "--note", "ok"]);
    assert_eq!(steward(&d, &["status"]).0, 0);

    std::fs::write(d.join("src/random.rs"), "fn r() {}\n").unwrap();
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 1);
    assert!(out.contains("UNCLAIMED  src/random.rs"), "{out}");
    std::fs::remove_file(d.join("src/random.rs")).unwrap();

    let lock = std::fs::read_to_string(d.join("Cargo.lock")).unwrap()
        + "\n[[package]]\nname = \"sha3\"\nversion = \"0.10.8\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
    std::fs::write(d.join("Cargo.lock"), lock).unwrap();
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 1);
    assert!(out.contains("OUTSIDE") && out.contains("sha3"), "{out}");
    assert_eq!(steward(&d, &["deps"]).0, 1);

    std::fs::write(d.join("Cargo.lock"), "version = 4\n").unwrap();
    std::fs::remove_file(d.join("src/hash.rs")).unwrap();
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 1);
    assert!(out.contains("MISSING"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

fn git(dir: &Path, email: &str, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", &format!("user.email={email}"), "-c", "user.name=someone", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A project whose reviews are signed: two reviewers, and a stand-in
/// signature (the reviewer's name and the message's SHA-256) checked
/// against the name in their public key file.
fn signed_project(name: &str) -> PathBuf {
    let d = project(name);
    std::fs::create_dir_all(d.join("keys")).unwrap();
    std::fs::write(d.join("keys/alice.pub"), "alice\n").unwrap();
    std::fs::write(d.join("keys/bob.pub"), "bob\n").unwrap();
    let mut ledger = std::fs::read_to_string(d.join("steward.ledger")).unwrap();
    ledger = ledger.replace(
        "toolchain = echo tools-1\n",
        "toolchain = echo tools-1\n\
         reviewer = alice keys/alice.pub alice@example.org\n\
         reviewer = bob keys/bob.pub Bob@Example.org\n\
         sign = printf '%s-' \"$STEWARD_REVIEWER\"; sha256sum \"$STEWARD_MESSAGE\" | cut -d' ' -f1\n\
         check-signature = test \"$STEWARD_SIGNATURE\" = \"$(cat \"$STEWARD_PUBLIC\")-$(sha256sum \"$STEWARD_MESSAGE\" | cut -d' ' -f1)\"\n",
    );
    std::fs::write(d.join("steward.ledger"), ledger).unwrap();
    git(&d, "alice@example.org", &["init", "-q"]);
    git(&d, "alice@example.org", &["add", "."]);
    git(&d, "alice@example.org", &["commit", "-qm", "alice writes the hash"]);
    d
}

#[test]
fn a_signed_review_is_by_someone_else_and_checked_every_time() {
    let d = signed_project("signed");
    assert_eq!(steward(&d, &["verify"]).0, 0);

    // Unsigned, by no reviewer, or by the author: refused.
    let (code, out) = steward(&d, &["review", "hash", "--note", "x"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("--as REVIEWER"), "{out}");
    let (code, out) = steward(&d, &["review", "hash", "--as", "carol", "--note", "x"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("not a reviewer"), "{out}");
    let (code, out) = steward(&d, &["review", "hash", "--as", "alice", "--note", "my own"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("alice wrote changes to [hash]"), "{out}");

    // Another reviewer: signed, and status holds it.
    let (code, out) = steward(&d, &["review", "hash", "--as", "bob", "--note", "read against FIPS 202"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("reviewed by bob"), "{out}");
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("by bob"), "{out}");

    // A review edited by hand no longer carries its signature.
    let reviews = std::fs::read_to_string(d.join("steward.reviews")).unwrap();
    std::fs::write(d.join("steward.reviews"), reviews.replace("read against FIPS 202", "read twice")).unwrap();
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("FORGED"), "{out}");
    std::fs::write(d.join("steward.reviews"), &reviews).unwrap();

    // Bob changes the part: not committed, nobody reviews it; committed,
    // bob may not, alice (who wrote nothing since) may.
    std::fs::write(d.join("src/hash.rs"), "fn h() { 2; }\n").unwrap();
    assert_eq!(steward(&d, &["verify", "hash"]).0, 0);
    let (code, out) = steward(&d, &["review", "hash", "--as", "alice", "--note", "x"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("not committed"), "{out}");
    git(&d, "bob@example.org", &["commit", "-qam", "bob changes the hash"]);
    let (code, out) = steward(&d, &["review", "hash", "--as", "bob", "--note", "x"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("bob wrote changes"), "{out}");
    assert_eq!(steward(&d, &["review", "hash", "--as", "alice", "--note", "bob's change read"]).0, 0);
    assert_eq!(steward(&d, &["status"]).0, 0);

    // A co-author counts as an author.
    std::fs::write(d.join("src/hash.rs"), "fn h() { 3; }\n").unwrap();
    git(&d, "carol@example.org", &["commit", "-qam", "carol changes it\n\nCo-authored-by: Alice <ALICE@example.org>"]);
    assert_eq!(steward(&d, &["verify", "hash"]).0, 0);
    let (code, out) = steward(&d, &["review", "hash", "--as", "alice", "--note", "x"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("alice wrote changes"), "{out}");

    // A record of alice reviewing it anyway, signed and all, is caught by
    // status from git itself.
    let head = String::from_utf8(Command::new("git").arg("-C").arg(&d).args(["rev-parse", "HEAD"]).output().unwrap().stdout).unwrap();
    let hash = steward(&d, &["hash", "hash"]).1.trim().to_string();
    let tools = steward(&d, &["toolchain"]).1.lines().last().unwrap().trim_start_matches("fingerprint ").to_string();
    let today = reviews.split('\t').next().unwrap().to_string();
    let message = format!(
        "steward review 1\nproject P\npart hash\nhash {hash}\ntoolchain {tools}\ncommit {}\ndate {today}\nreviewer alice\nnote forged\n",
        head.trim()
    );
    let msg = d.join("forged.msg");
    std::fs::write(&msg, &message).unwrap();
    let sum = String::from_utf8(Command::new("sha256sum").arg(&msg).output().unwrap().stdout).unwrap();
    std::fs::remove_file(&msg).unwrap();
    let line = format!("{today}\thash\t{hash}\t{tools}\t{}\talice\talice-{}\tforged\n", head.trim(), &sum[..64]);
    let mut all = std::fs::read_to_string(d.join("steward.reviews")).unwrap();
    all.push_str(&line);
    std::fs::write(d.join("steward.reviews"), all).unwrap();
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("SELF-REVIEW alice"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_self_reviewers_review_of_their_own_changes_stands_and_says_so() {
    let d = signed_project("self");
    let ledger = std::fs::read_to_string(d.join("steward.ledger")).unwrap();
    let as_self = ledger.replace("reviewer = alice keys/alice.pub", "self-reviewer = alice keys/alice.pub");
    std::fs::write(d.join("steward.ledger"), &as_self).unwrap();
    assert_eq!(steward(&d, &["verify"]).0, 0);
    let (code, out) = steward(&d, &["review", "hash", "--as", "alice", "--note", "my own, read again"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("reviewed by alice, changes of their own among it (self-reviewer)"), "{out}");
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("by alice (self-review)"), "{out}");
    assert!(out.contains("alice's reviews of changes they wrote stand"), "{out}");

    // `reviewer` again: the rule is back. The review it let stand is a
    // fault, and alice may not review her change again on the strength of
    // her own review; bob, who wrote none of it, may.
    std::fs::write(d.join("steward.ledger"), &ledger).unwrap();
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("SELF-REVIEW alice"), "{out}");
    let (code, out) = steward(&d, &["review", "hash", "--as", "alice", "--note", "x"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("alice wrote changes"), "{out}");
    let (code, out) = steward(&d, &["review", "hash", "--as", "bob", "--note", "alice's code read"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("self-review"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_unsigned_review_does_not_stand_where_reviews_are_signed() {
    let d = signed_project("unsigned");
    assert_eq!(steward(&d, &["verify"]).0, 0);
    let hash = steward(&d, &["hash", "hash"]).1.trim().to_string();
    let tools = steward(&d, &["toolchain"]).1.lines().last().unwrap().trim_start_matches("fingerprint ").to_string();
    std::fs::write(d.join("steward.reviews"), format!("2026-01-01\thash\t{hash}\t{tools}\tbefore signing\n")).unwrap();
    let (code, out) = steward(&d, &["status"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("UNSIGNED"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}
