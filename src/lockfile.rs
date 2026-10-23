// SPDX-License-Identifier: Apache-2.0
//! Cargo.lock read for one thing: which packages come from outside (a
//! registry or a repository, written `source = "..."`), as opposed to paths
//! in the project's own tree.

/// (name, source) of every package; source None for a path package.
pub fn packages(text: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for block in text.split("[[package]]").skip(1) {
        let mut name = None;
        let mut source = None;
        for line in block.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("name = ") {
                name = Some(v.trim_matches('"').to_string());
            } else if let Some(v) = line.strip_prefix("source = ") {
                source = Some(v.trim_matches('"').to_string());
            } else if line.starts_with('[') {
                break;
            }
        }
        if let Some(n) = name {
            out.push((n, source));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn outside_packages_are_told_apart() {
        let lock = "version = 4\n\n[[package]]\nname = \"basis\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"sha3\"\nversion = \"0.10.8\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"x\"\n";
        let p = super::packages(lock);
        assert_eq!(p, vec![("basis".into(), None), ("sha3".into(), Some("registry+https://github.com/rust-lang/crates.io-index".into()))]);
    }
}
