# How to work in this repo

- **Every change gets its own pull request.** Branch off the current `main`,
  commit the change there, push, and open a pull request to `main` — without
  asking first. Do not add an unrelated change to a branch whose pull request
  is already open, and do not reuse a branch whose pull request has been
  merged: start a new one from `main` instead.

## The one rule that matters

`wave-core` must never call a Windows API. It reads the system only through
`SystemProvider`. Anything that decides *what* to remove belongs there, behind
that trait, with a fixture test next to it.

`wave-win32` is the opposite: it may only fetch, never decide. If a function
there contains a judgement about whether something is a leftover, it is in the
wrong crate.

This is what keeps the deletion logic testable without a virtual machine. It is
easy to erode one convenient call at a time, so treat it as load-bearing.

## Safety rules that outrank convenience

- Nothing is deleted, only moved to a backup, and every operation is journalled
  before it runs.
- The protection list is checked before scoring, not after.
- A read that fails becomes a visible warning. An inventory with a silent gap is
  worse than no inventory: the protection list works by knowing every installed
  program, so a missing entry can turn another program's folder into a
  "leftover".
- Registry reads always cover both the 64-bit and 32-bit view.
- Never follow a reparse point while scanning or deleting.

## Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --exclude wave-app --all-targets -- -D warnings
cargo test -p wave-core
cargo clippy -p wave-win32 --target x86_64-pc-windows-msvc -- -D warnings
npm run lint && npm run build
```
