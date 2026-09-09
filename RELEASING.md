# Releasing paxc

The release loop lives in `~/notes/releasing.md` — the ordered steps, the apt
step, crates.io, the winget submission and its rate rule, the spent-tag rule,
and the standing facts about tokens and secrets. Failure recipes are in
`~/notes/build_release_gotchas.md`. This file carries what is true of paxc and
not of its siblings.

| | |
|---|---|
| Loop | cargo-dist |
| Version lives in | `version` in `Cargo.toml` |
| `apt-ship` argument | `paxc` |
| crate | `paxc` |
| winget package | `Excelano.paxc` |
| Windows asset | `paxc-x86_64-pc-windows-msvc.zip` |
| Commands | `paxc`, `paxr` |

**The crate ships two binaries and they travel together.** `paxc` is the
compiler and `paxr` the interpreter; they version together, in one tarball, one
Debian package, one Homebrew formula, and one winget package with an alias for
each command. There is no way to release one without the other and no reason to
want to, since the interpreter exists to run the same source the compiler
consumes. This is why paxc is one winget PR and not two, unlike xfiles.

**The `NestedInstallerFiles` block carries both executables.** komac generates
from the previous version's manifest, so it comes along on its own. If a
manifest is ever written by hand, that block is the thing to get right: dropping
an entry ships a package that installs one command and looks fine doing it.

**Import a compiled flow before tagging, when a release touches action
emission.** The compiler's output is the contract — paxc emits flow definitions
Power Automate has to accept, and the test suite checks the shapes paxc believes
in rather than what the service currently tolerates. A green suite is not the
same claim. Test corpora built from real exports stay out of the repo; they
carry tenant identifiers.

**The tutorial is a separate deliverable.** `excelano.com/paxc/tutorial/` lives
outside this repo and no release rebuilds it, so a language change that
invalidates a tutorial step needs a deliberate follow-up there.

The README, `REFERENCE.md`, the landing page and `SECURITY.md` all refer to the
version as "latest" and need no per-release edit.
