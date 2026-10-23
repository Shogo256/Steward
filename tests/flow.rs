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
