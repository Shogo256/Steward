// SPDX-License-Identifier: Apache-2.0
//! Paths matched against patterns, relative to the project with `/`
//! between parts: `*` any run within a part, `?` one character, `**` as a
//! whole part any number of parts (none included). Files are found by
//! walking only the directory a pattern names before its first wildcard.

use std::path::Path;

fn part(p: &[char], s: &[char]) -> bool {
    match (p.first(), s.first()) {
        (None, None) => true,
        (Some('*'), _) => part(&p[1..], s) || (!s.is_empty() && part(p, &s[1..])),
        (Some('?'), Some(_)) => part(&p[1..], &s[1..]),
        (Some(a), Some(b)) if a == b => part(&p[1..], &s[1..]),
        _ => false,
    }
}

fn parts(p: &[&str], s: &[&str]) -> bool {
    match (p.first(), s.first()) {
        (None, None) => true,
        (Some(&"**"), _) => parts(&p[1..], s) || (!s.is_empty() && parts(p, &s[1..])),
        (Some(a), Some(b)) => {
            let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
            part(&a, &b) && parts(&p[1..], &s[1..])
        }
        _ => false,
    }
}

/// Whether `path` (relative, `/`-separated) matches `pattern`.
pub fn matches(pattern: &str, path: &str) -> bool {
    let p: Vec<&str> = pattern.split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    parts(&p, &s)
}

/// The files under `root` that `pattern` matches, relative and sorted.
pub fn files(root: &Path, pattern: &str) -> Vec<String> {
    let literal: Vec<&str> = pattern.split('/').take_while(|p| !p.contains(['*', '?'])).collect();
    let base = literal.join("/");
    let mut out = Vec::new();
    walk(root, &base, &mut out);
    out.retain(|f| matches(pattern, f));
    out.sort();
    out
}

fn walk(root: &Path, rel: &str, out: &mut Vec<String>) {
    let full = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
    if full.is_file() {
        out.push(rel.to_string());
        return;
    }
    let Ok(entries) = std::fs::read_dir(&full) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        // Version control and build output are never anyone's source.
        if name == ".git" || name == "target" {
            continue;
        }
        let child = if rel.is_empty() { name } else { format!("{rel}/{name}") };
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            walk(root, &child, out);
        } else if t.is_file() {
            out.push(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn patterns() {
        assert!(matches("src/*.rs", "src/main.rs"));
        assert!(!matches("src/*.rs", "src/bin/x.rs"));
        assert!(matches("src/**/*.rs", "src/main.rs"));
        assert!(matches("src/**/*.rs", "src/bin/a/x.rs"));
        assert!(matches("**", "a/b/c"));
        assert!(matches("a/?.c", "a/b.c"));
        assert!(!matches("a/?.c", "a/bb.c"));
        assert!(matches("kernels/mlkem.rho", "kernels/mlkem.rho"));
        assert!(!matches("kernels/mlkem.rho", "kernels/mldsa.rho"));
        assert!(matches("*", "x"));
        assert!(!matches("*", "x/y"));
    }
}
