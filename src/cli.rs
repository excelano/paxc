//! CLI surface shared by the `paxc` and `paxr` binaries.
//!
//! The two have different work to do and so take different flags, but they
//! end their usage blocks the same way and exit the same way. Keeping the
//! common tail here means the four-flag standard (`--version`, `-V`,
//! `--help`, `-h`) and the skill flags are stated once instead of in two
//! places that drift apart.

use std::process;

/// The trailing section every pax binary's usage block carries verbatim.
pub const COMMON_FLAGS: &str = "\
Other flags:
  --help, -h     print this help and exit
  --version, -V  print the version and exit

Claude Code:
  --install-skill    install the pax skill into ~/.claude/skills/paxc
  --uninstall-skill  remove it again";

/// Print usage to stderr and exit 2 — the error path for a malformed invocation.
pub fn usage(text: &str) -> ! {
    eprintln!("{text}");
    process::exit(2);
}

/// Print usage to stdout and exit 0 — the success path for an explicit `--help`.
pub fn help(text: &str) -> ! {
    println!("{text}");
    process::exit(0);
}
