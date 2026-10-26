# Steward

A maintenance ledger for code you write yourself.

A project that depends on nothing from outside has no upstream to find its
faults and fix them: every part it wrote, it keeps. Steward makes that
upkeep explicit. A ledger in the project says which files the project owns,
which part each belongs to, which standard each part follows, how each is
verified and when its standard should be read again; Steward checks the
project against it, runs the checks, and records who reviewed what, with
which tools, in plain files under version control.

Steward itself depends on nothing but Rust's standard library: its SHA-256,
ledger format, glob matching and calendar are written in `src/`.

## What it answers

- **Has anything changed that nobody reviewed?** Each part's files are
  hashed; a part whose hash differs from its last review is `CHANGED`, and
  `status` fails.
- **Is everything we wrote accounted for?** Files matched by `own` must
  belong to a part or be `exempt`; any other is `UNCLAIMED`.
- **Has anything from outside crept in?** The ledger's Cargo.lock files may
  hold path packages only; a package from a registry or a repository is
  `OUTSIDE`.
- **Is a review backed by a passing check?** `review` refuses a part unless
  its last verification passed, on exactly its current files and with the
  current toolchain.
- **Was it reviewed by someone who did not write it?** Where the ledger
  names reviewers, a review is signed with the reviewer's key and refused
  to anyone who wrote, or co-wrote, a commit it covers; `status` checks
  every signature, and the separation again from git, each time.
- **What needs attention?** `report` lists, per part, its standard, its last
  review and verification, verifications older than `stale` days, parts
  whose toolchain changed since they were verified, and standards due to be
  re-read (`recheck`).

## The ledger

`steward.ledger`, at the project's root:

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

Settings before the first `[part]` are the project's; `own`, `exempt`,
`lock`, `toolchain`, `files`, `standard` and `verify` repeat. Comments are
whole lines starting with `#`. `verify` commands run with `sh -c` in the
project's root. Every part needs files and at least one check.

## Signed reviews

A project without an independent audit can at least keep its reviews apart
from its changes. Name the reviewers, their public keys, and the email
addresses git records them by as authors, and the commands that sign and
check:

```text
reviewer = alice keys/alice.pub alice@example.org
reviewer = bob keys/bob.pub bob@example.org
sign = basis-token sign "$STEWARD_REVIEWER" "$STEWARD_MESSAGE"
check-signature = basis-token verify "$STEWARD_PUBLIC" "$STEWARD_MESSAGE" "$STEWARD_SIGNATURE"
```

Then a review is made on a commit, as one of them:

```bash
steward review sha3 --as bob --note "read against FIPS 202 and its errata"
```

It is refused while the part has changes not committed, and refused to a
reviewer who is the author or a co-author (`Co-authored-by:`) of a commit
that changed the part since its last signed review. The review's fields,
the project and the commit are signed by `sign`, run with the terminal's
input so that it can ask for a PIN; with Basis's `basis-token`, that is an
ML-DSA-65 signature by a key kept on its token. `status` then fails a
part whose last review is unsigned (`UNSIGNED`), by no listed reviewer or
not carrying its reviewer's signature (`FORGED`), or by someone who wrote
a change it covers (`SELF-REVIEW`), so that a record added or edited by
hand is not taken on trust.

The separation is as strong as the email addresses in the history: git
records what its committers say. The signatures bind each review to its
reviewer's key, the part's content and the commit.

A project with one author and no second reviewer can still sign its
reviews, without the separation: name that reviewer with `self-reviewer`
instead of `reviewer`.

```text
self-reviewer = carol keys/carol.pub carol@example.org
```

Their reviews of changes they wrote then stand, and say so: `review` and
`status` call each one a self-review, and `status` names every
self-reviewer, so that the ledger and its output do not claim more than
was done. Naming them `reviewer` again restores the rule: their
self-reviews become `SELF-REVIEW` faults, and the changes those covered
are left for the next review by someone else.

## The records

Beside the ledger, appended and never rewritten, tab-separated:

- `steward.history`: every verification (date, part, content hash, pass or
  fail, seconds, toolchain fingerprint);
- `steward.reviews`: every review (date, part, content hash, toolchain
  fingerprint, and where reviews are signed the commit, the reviewer and
  the signature, then the note).

Commit them with the code: the project's upkeep is then part of its history.

## Use

```bash
cargo build --release
```

```bash
steward status                      # the default; exit 1 on any fault
steward verify [PART...]            # run the checks and record them
steward review PART [--as REVIEWER] --note "what was looked at"
steward report [--strict]           # --strict: exit 1 if anything is due
steward deps                        # the lock files alone
steward toolchain                   # what the toolchain commands print
steward hash PART
```

`--dir DIR` points at a project other than the current directory.

A part's content hash is SHA-256 over its files in sorted order: each path,
a NUL byte, the length as 8 bytes little-endian, the contents.

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
