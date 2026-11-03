# Steward

A maintenance ledger for code you write yourself.

A project that depends on nothing from outside has no upstream to find its
faults and fix them: every part it wrote, it keeps. Steward makes that
upkeep explicit. A ledger in the project says which files the project owns,
which part each belongs to, which standard each part follows, how each is
verified and when its standard should be read again. Steward checks the
project against it, runs the checks, and records who reviewed what, with
which tools, in plain files committed with the code.

Steward is a command-line tool. It works on a project in any language: the
checks it runs are the project's own commands (`cargo test`, `pytest`,
`npm test`, ...). It depends on nothing but Rust's standard library.

## Installing

Requirements: Rust (to build), `sh` (to run the checks; on Windows, Git
Bash or similar), and git for signed reviews.

```bash
git clone <this repository's URL> steward
```

```bash
cargo install --path steward
```

This puts `steward` on the `PATH` (in `~/.cargo/bin`).

## Getting started

1. Write `steward.ledger` at the root of your project. The smallest one
   names the project, the files it owns, and one part with its check:

   ```text
   project = myproject
   own = src/**/*.rs

   [parser]
   files = src/parser.rs
   standard = RFC 8259
   verify = cargo test parser
   ```

2. Run the checks and record them:

   ```bash
   steward verify
   ```

3. Look at each part and record that you did:

   ```bash
   steward review parser --note "read against RFC 8259"
   ```

4. Commit the ledger and the records (`steward.history`,
   `steward.reviews`) with your code.

5. From then on, `steward status` passes while every part is as it was
   reviewed. Run it in CI.

## Day to day

| When | Run |
|---|---|
| Any time: is everything as reviewed? | `steward status` |
| After changing a part | `steward verify PART`, then `steward review PART --note "..."`, then commit the records |
| Now and then: what needs attention? | `steward report` |
| In CI | `steward status`, and `steward report --strict` to fail on anything due |

A review is accepted only right after a passing verification of exactly
the part's current files, with the current toolchain: change a file or a
tool, and the part must be verified again before it can be reviewed.

## Commands

```text
steward status                      the default; exit 1 on any fault
steward verify [PART...]            run the parts' checks (all by default) and record them
steward review PART [--as REVIEWER] --note TEXT
steward report [--strict]           per part: standard, review, verification, recheck date
steward deps                        the lock files alone
steward toolchain                   what the toolchain commands print, and its fingerprint
steward hash PART                   a part's content hash
```

`--dir DIR` works on a project other than the current directory. Exit
status: 0 when all is well, 1 on a fault, 2 on an error (a bad ledger, an
unknown part, a refused review).

## What status reports

| Label | Meaning | What to do |
|---|---|---|
| `ok` | The part is as it was last reviewed | Nothing |
| `UNREVIEWED` | The part has never been reviewed | `verify`, then `review` |
| `CHANGED` | Its files changed since its last review | `verify`, then `review` |
| `MISSING` | A `files` pattern matches no file | Fix the pattern, or restore the file |
| `TWICE` | A file belongs to two parts | Keep it in one |
| `UNCLAIMED` | A file matched by `own` belongs to no part | Add it to a part, or to `exempt` |
| `OUTSIDE` | A lock file holds a package from a registry or a repository | Remove the dependency |
| `UNSIGNED` | Where reviews are signed: the last review is not | Review it again, as a reviewer |
| `FORGED` | Its signature does not check, or its reviewer is not in the ledger | Review it again, as a reviewer |
| `SELF-REVIEW` | Its reviewer wrote a change it covers | Have another reviewer review it |
| `UNCHECKED` | The signature or the history could not be checked | Fix the command or the repository named |

## The ledger

`steward.ledger`, at the project's root. Settings before the first
`[part]` are the project's; those after belong to that part.

```text
project = Basis
own = host/src/**/*.rs
exempt = host/src/bin/*.rs
lock = host/Cargo.lock
toolchain = rustc --version
toolchain = cc --version
stale = 90

[sha3]
files = host/src/hash.rs
standard = FIPS 202 (August 2015)
standard = FIPS 180-4 (August 2015)
verify = cd host && cargo test --release --offline --lib hash
verify = python3 -m unittest tests.test_hash
recheck = 2027-04-01
```

| Setting | Where | Repeats | Meaning |
|---|---|---|---|
| `project` | project | no | The project's name |
| `own` | project | yes | Glob of the files the project writes itself |
| `exempt` | project | yes | Glob of owned files that belong to no part |
| `lock` | project | yes | A Cargo.lock that may hold path packages only |
| `toolchain` | project | yes | A command whose output identifies the tools (`rustc --version`) |
| `stale` | project | no | Days after which a verification is old (default 90) |
| `files` | part | yes | Glob of the part's files |
| `standard` | part | yes | The standard the part follows |
| `verify` | part | yes | A check, run with `sh -c` in the project's root; every part needs one |
| `recheck` | part | no | YYYY-MM-DD by which to read the standard and its errata again |
| `reviewer` | project | yes | See signed reviews |
| `self-reviewer` | project | yes | See signed reviews |
| `sign` | project | no | See signed reviews |
| `check-signature` | project | no | See signed reviews |

Globs take `*`, `?` and `**` (any number of directories). Comments are
whole lines starting with `#`; a `#` inside a value is part of it. A
mistake in the ledger is reported with its line.

## Signed reviews

Reviews can be signed, and kept apart from the changes they review. Name
the reviewers (a name, a public key file, and the email addresses git
records them by as authors) and the commands that sign and check:

```text
reviewer = alice keys/alice.pub alice@example.org
reviewer = bob keys/bob.pub bob@example.org
sign = basis-token sign "$STEWARD_REVIEWER" "$STEWARD_MESSAGE"
check-signature = basis-token verify "$STEWARD_PUBLIC" "$STEWARD_MESSAGE" "$STEWARD_SIGNATURE"
```

A review is then made on a commit, as one of them:

```bash
steward review sha3 --as bob --note "read against FIPS 202 and its errata"
```

- It is refused while the part has changes not committed.
- It is refused to a reviewer who is the author or a co-author
  (`Co-authored-by:`) of a commit that changed the part since its last
  signed review kept apart from its changes.
- The review's fields, the project and the commit are signed by `sign`,
  which runs with the terminal's input so that it can ask for a PIN, and
  prints the signature. `check-signature` exits 0 for a good one. Steward
  passes them `STEWARD_REVIEWER`, `STEWARD_MESSAGE` (a file holding what to
  sign), `STEWARD_PUBLIC` (the reviewer's public key file) and
  `STEWARD_SIGNATURE`. With Basis's `basis-token`, the signature is
  ML-DSA-65, by a key kept on its token under a PIN; any pair of commands
  that sign and check can be used instead.
- Every `status` checks each signature, and the separation again from git,
  so a record added or edited by hand fails as `UNSIGNED`, `FORGED` or
  `SELF-REVIEW`.

A project with one author can sign its reviews without the separation by
naming that person with `self-reviewer` instead of `reviewer`:

```text
self-reviewer = carol keys/carol.pub carol@example.org
```

Their reviews of their own changes then stand, and say so: `review` and
`status` mark each one as a self-review, and `status` names every
self-reviewer. Naming them `reviewer` again restores the rule.

## The records

Beside the ledger, appended and never rewritten, one line a record,
tab-separated:

- `steward.history`: every verification (date, part, content hash, pass or
  fail, seconds, toolchain fingerprint);
- `steward.reviews`: every review (date, part, content hash, toolchain
  fingerprint; where reviews are signed, the commit, the reviewer and the
  signature; then the note).

Commit them with the code: the project's upkeep is then part of its
history. A part's content hash is SHA-256 over its files in sorted order:
each path, a NUL byte, the length as 8 bytes little-endian, the contents.
The toolchain fingerprint is the first 16 hex digits of the SHA-256 of
what the `toolchain` commands print.

## Testing

```bash
cargo test --release
```

FIPS 180-4's SHA-256 examples, the calendar over 1.6 million days, the
ledger's format and its faults, glob patterns, and the whole flow through
the binary on a scratch project, signed reviews included (in a scratch git
repository, with a stand-in signature).

## License

Apache License 2.0; see [LICENSE](LICENSE).
