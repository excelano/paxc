# paxc — the pax compiler

`paxc` compiles the **pax** DSL into [Power Automate](https://powerautomate.microsoft.com/) cloud flow definitions. Write terse, readable code; get the verbose `definition.json` that Power Automate expects. Companion interpreter `paxr` runs the same source locally for fast iteration.

For the full language reference, see [REFERENCE.md](REFERENCE.md).

Every construct listed in REFERENCE.md is implemented and tested, end-to-end deployment to Power Automate is validated, and a legacy-format package target lets you import compiled flows directly through the Power Automate portal.

Round-trip ingest from real PA exports is the primary forward direction: `paxc --decode <flow.json>` reads an exported PA flow JSON and writes a `.pax` source file plus a `pa/` folder. Variables, Compose, container actions (`if`/`foreach`/`until`/`switch`/`scope`), `on` error-path handlers, `terminate`, and PA expressions including the standard accessors (`triggerBody()`, `triggerOutputs()`, `parameters('X')`, `body('Foo')`, `iterationIndexes('Loop')`, etc.) and slash- or index-style path expressions (`triggerBody()?["body/email"]`, `arr?[0]`) all round-trip to pax source. PA action keys with characters outside pax's identifier rules (e.g. `Send_an_email_(V2)`) are normalized on decode and restored byte-for-byte on re-encode via `pa/flow.json.actionNameMap`.

The division of labour is a file convention. Connector bodies, ParseJson, and any non-default trigger live in JSON files next to the source under a `pa/` folder; connectors (`OpenApiConnection`, `OpenApiConnectionWebhook`, `ParseJson`, and the rest) stay opaque as `pa <Name>` blocks pointing at those files. pax owns the programmable parts — variables, control flow, expressions — and the files own the PA-specific parts. Opaque connectors are the design endpoint, not a residual gap.

## Why

The Power Automate browser designer is slow and click-heavy. The underlying flow definition is JSON that's technically hand-editable but structured in ways that fight you: actions are a map keyed by name, dependencies are encoded as a `runAfter` graph, and expressions live inside escaped strings. pax is a small DSL that turns all of that into source code you can actually read and maintain, and `paxc` is the compiler that emits the JSON.

Equivalent pax and JSON for initializing a counter:

```
var counter: int = 1
```

```json
{
  "Initialize_counter": {
    "type": "InitializeVariable",
    "inputs": {
      "variables": [
        { "name": "counter", "type": "Integer", "value": 1 }
      ]
    },
    "runAfter": {}
  }
}
```

The source is shorter, and more importantly, the `runAfter` dependency graph is inferred from source order so you never hand-wire it.

## What pax covers

The language supports typed variables and `let` Compose bindings; assignment and compound assignment; arithmetic, boolean, and string concatenation expressions; member access; `if`/`else if`/`else`, `foreach`, `until`, `switch`, `scope`, `terminate`, and `on <status>` error-path handlers; function calls that pass through to Power Automate's expression language; a `pa <Name>` primitive for any PA-shaped action whose body lives in `pa/<Name>.json` next to the source (connectors, ParseJson, anything PA-designer-shaped); and a `debug()` statement that paxr prints at runtime and paxc strips at compile time.

Triggers are file-based: drop a single `pa/<Name>.trigger.json` next to the source to pick the trigger; without one, paxc generates a default manual ("Button") trigger. Connection references go in `pa/connectionReferences.json` and end up at the flow's top level on emit.

## Install

### Debian and Ubuntu

Add the Excelano apt repository once (one-time setup):

```bash
curl -fsSL https://excelano.com/apt/setup.sh | sudo sh
```

Then install the `.deb`, so `apt upgrade` keeps it current:

```bash
sudo apt install paxc
```

Both `paxc` and `paxr` are installed into `/usr/bin/`, with reference docs at `/usr/share/doc/paxc/`. Supported architectures: `amd64`, `arm64`.

### Homebrew

On macOS or Linux, tap and trust the repository once — Homebrew gates third-party taps behind explicit trust (one-time setup):

```sh
brew tap excelano/tap
brew trust excelano/tap
```

Then install it, so `brew upgrade` keeps it current. Installs both `paxc` and `paxr`:

```sh
brew install paxc
```

### Prebuilt binary (Linux and macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/excelano/paxc/main/install.sh | sh
```

The installer downloads the right tarball for your platform from the GitHub release, verifies its checksum, and drops both `paxc` and `paxr` into `~/.cargo/bin` (or the equivalent on Windows). Releases also ship raw tarballs (`paxc-*.tar.xz` / `.zip`) for manual installation.

### Windows

With [WinGet](https://learn.microsoft.com/windows/package-manager/), so `winget upgrade` keeps it current (installs both `paxc` and `paxr`):

```powershell
winget install Excelano.paxc
```

Or run the standalone installer in PowerShell:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/excelano/paxc/releases/latest/download/paxc-installer.ps1 | iex"
```

### Cargo

If you have a Rust toolchain, install the latest release from [crates.io](https://crates.io/crates/paxc). This builds and installs both `paxc` and `paxr` into `~/.cargo/bin`:

```sh
cargo install paxc
```

### Build from source

Requires Rust (edition 2024, toolchain 1.85+). If you don't have Rust, install it first via [rustup](https://rustup.rs).

```sh
git clone https://github.com/excelano/paxc
cd paxc
cargo build --release
```

The binaries will be at `target/release/paxc` and `target/release/paxr`.

## Use it from Claude Code

paxc was built for AI coding agents as much as for people, so the repo ships an official [Claude Code](https://docs.claude.com/en/docs/claude-code) skill under [`skills/paxc/`](skills/paxc/). It teaches an agent the pax grammar, the `pa/` folder file convention (where connector bodies live), the PA accessor and path expressions, and the round-trip decoder — so it authors and maintains flows through paxc rather than routing around it to hand-edit `definition.json`. Drop it into your personal skills directory:

```sh
mkdir -p ~/.claude/skills/paxc
for f in SKILL.md reference.md; do
  curl -fsSL "https://raw.githubusercontent.com/excelano/paxc/main/skills/paxc/$f" \
    -o ~/.claude/skills/paxc/$f
done
```

Or, from a clone of this repo, `cp -r skills/paxc ~/.claude/skills/`.

## Uninstall

How you remove paxc depends on how you installed it. It installs two binaries, `paxc` and `paxr`, and stores no configuration or state of its own, so removing the binaries is the whole job.

If you installed via apt, `apt remove` also clears the reference docs under `/usr/share/doc/paxc/`:

```bash
sudo apt remove paxc
```

If you installed via the shell installer, the uninstaller removes both binaries from `~/.cargo/bin/`:

```sh
curl -fsSL https://raw.githubusercontent.com/excelano/paxc/main/uninstall.sh | sh
```

If you used `cargo install` (or prefer to do it by hand), the binaries are in `~/.cargo/bin/`:

```sh
rm ~/.cargo/bin/paxc ~/.cargo/bin/paxr
```

If you built from a clone, delete the checkout (or just `cargo clean` and `rm` the two `target/release/` binaries if you copied them onto your `PATH`).

## License

MIT. See [LICENSE](LICENSE).
