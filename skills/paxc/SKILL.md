---
name: paxc
description: >-
  Author and maintain Power Automate cloud flows as `pax` source with `paxc` and
  `paxr` — a compiler and interpreter for the pax DSL. Reach for it on the complaint
  as much as the request: "this flow is a wall of escaped JSON", "I can't review or
  diff a flow in the designer", "reordering these actions means rewiring every
  runAfter by hand", "I need to add a step to this exported flow", "put error
  handling around this scope", "can this flow go in version control". Use whenever a
  task means writing, editing, or refactoring a Power Automate flow
  (`definition.json` inside a legacy import package) that has, or should have, a
  `.pax` source file beside it: adding an action, changing a condition, wrapping
  steps in an `on failed` handler, extracting a scope, promoting a hand-wired
  `runAfter` to source order, or turning an exported flow into source with `paxc
  --decode`. Use it just as readily when there is no flow yet and someone asks for
  one in plain language -- "build me a flow that emails the owner when an item is
  added to the list", "automate this in Power Automate", "when someone submits the
  form, post it to Teams" -- because PA has no authoring API an agent can drive and
  pax is the only route from a request to a working flow; `connectors.md` carries
  verified connector bodies so a greenfield flow does not have to start from an
  export. Prefer it over hand-editing `definition.json`: pax owns the programmable
  parts (variables, control flow, expressions) and infers the `runAfter` graph from
  source order, replacing the click-heavy escaped-string JSON. Not for other workflow
  engines (Zapier, n8n, Logic Apps authored against its own API), and do not model
  connector actions in pax syntax — connector calls (SharePoint, Outlook, Teams,
  HTTP, ParseJson) stay opaque as `pa <Name>` blocks with their bodies in the `pa/`
  folder beside the source. That is the design endpoint, not a gap.
---

# paxc — the pax compiler for Power Automate

`paxc` compiles the **pax** DSL into the `definition.json` that Power Automate
cloud flows expect. Companion interpreter `paxr` runs the same source locally so
you can see what a flow does without deploying it. The source you write looks
like imperative code — pax infers PA's `runAfter` dependency graph from source
order, so you never hand-wire it.

pax owns the programmable parts of a flow (variables, control flow, expressions).
PA-specific parts (connector calls, ParseJson, non-default triggers, connection
references) live in JSON files under a `pa/` folder next to the source. paxc
drops those files verbatim into the emitted flow. The shape of those files is
PA's, not pax's, which is why a connector body is written rather than modelled
in pax syntax — and why `connectors.md` in this directory carries verified
bodies for the connectors that come up most.

**There is no other way in.** Power Automate has no authoring API an agent can
drive; the designer is a browser surface that has to be clicked. For a human,
pax replaces something tedious. For an agent, it replaces nothing — it is the
only route from a request to a working flow, and the loop it makes possible is
worked end to end below.

The authoritative sources for pax and paxc are the binary itself (`paxc --help`),
[README](https://github.com/excelano/paxc/blob/main/README.md), and
[REFERENCE.md](https://github.com/excelano/paxc/blob/main/REFERENCE.md); if
anything here conflicts with them, they win. These recipes assume a paxc that ships
the round-trip decoder (`paxc --decode`), the file-based `pa/` folder convention,
the `on <status>` error-path handlers, and the `switch` / `until` / `scope` /
`terminate` statements. Check for `--decode` in `paxc --help`, and expect an older
copy to reject the statement keywords as syntax errors; if any are missing, upgrade with
`sudo apt install --only-upgrade paxc` (Debian/Ubuntu) or by re-running the
install one-liner from the README.

## The one rule that decides what belongs where

**pax owns the programmable layer; the `pa/` folder owns the PA-specific
shapes.** Variables, `let` bindings, `if`/`switch`/`foreach`/`until`/`scope`,
`on <status>` handlers, `terminate`, arithmetic and boolean expressions, string
concat, PA accessor calls, member and subscript paths — those are pax source.
Every connector call (SharePoint, Outlook, Teams, HTTP), every `ParseJson`
schema, every non-default trigger (Recurrence, HTTP request, connector webhook),
and the top-level `connectionReferences` map — those live as opaque JSON files
under `pa/` and are referenced from source by name (`pa <Name>` for actions;
implicit for triggers and connection references).

Writing a connector body is expected; guessing its shape is not. What a body
needs is the operation's `operationId` and its exact parameter keys, and there
are three places to get them, in order: `connectors.md` in this directory, which
carries verified bodies for SharePoint, Outlook, Teams, Forms and Approvals;
Microsoft's connector reference at `learn.microsoft.com/connectors/<apiname>/`,
whose parameter **Key** column is exactly what goes in `inputs.parameters`; or
PA's own "Peek code" on an existing action, pasted verbatim, which `paxc
--decode` will also lift out of an export wholesale.

## Running it

```sh
paxc flow.pax > flow.json                                  # emit PA flow definition JSON
paxc --target pa-legacy --name myflow --out myflow.zip flow.pax   # legacy import package (.zip)
paxc --decode flow.zip                                     # decode a PA export → .pax + pa/
paxc --decode definition.json --out-dir my_flow/           # decode raw inner definition
paxr flow.pax                                              # run locally: debug + end-of-run state dump
paxr -v flow.pax                                           # verbose trace of every action
paxr -q flow.pax                                           # exit-code-only
paxr -d flow.pax                                           # only debug() output
```

`--target pa-legacy` produces the ZIP you import through PA's **My flows →
Import → Import Package (Legacy)** path. Without `--target`, `paxc` writes flow
JSON to stdout — useful for `diff`ing before and after a source edit.

Both binaries take `--version` (or `-V`) and `--help` (or `-h`).

## Language essentials

```
var counter: int = 0                       // → InitializeVariable action
let region = config.region                 // → Compose action (immutable)
counter += 1                               // → IncrementVariable
message &= ", world"                       // → AppendToStringVariable
tags += "urgent"                           // → AppendToArrayVariable
```

Six v1 types: `int`, `float`, `string`, `bool`, `array`, `object`. Assignment
forms map one-to-one to PA's variable actions: `=` → `SetVariable`, `+=` on
int/float → `IncrementVariable`, `-=` on int/float → `DecrementVariable`, `&=`
on string → `AppendToStringVariable`, `+=` on array → `AppendToArrayVariable`.
`let` bindings are immutable Composes; `var` bindings are mutable.

Control flow — `if` / `else if` / `else`, `switch` / `case` / `default`,
`foreach`, `until`, `scope`, `terminate` — mirrors what PA models. Each block
maps to its named PA action (`Condition`, `Switch`, `Apply_to_each`, `Until`,
`Scope`, `Terminate`) with source order becoming `runAfter`.

Expressions cover arithmetic (`+ - * /`, integer `/` matches PA's int-divide),
comparison, boolean (`&& || !`), string concat (`&`), and function calls.
Function names pass through to PA's expression language unchanged — `concat`,
`length`, `toUpper`, `substring`, and anything else PA supports all work. See
`reference.md` for the operator precedence table.

## PA accessors and path expressions

**These are the part an agent will guess wrong** without the skill — they look
like function calls, but they are PA's own runtime accessors that paxc emits
unchanged and paxr partially simulates:

```
triggerBody()          triggerOutputs()     trigger()
parameters("name")     body("Compose_x")    outputs("Compose_x")
actions("Scope_foo")   iterationIndexes("For_each")
item()
```

**Double quotes here.** pax source has one string literal and it is
double-quoted; a `'` is a parse error. The single-quoted form you have seen in
PA documentation (`outputs('Compose_x')`) is PA's own expression syntax, and it
is correct in exactly one place: inside `pa/*.json`, where you write PA
expressions and paxc emits them verbatim. Source is pax, files are PA.

**Path expressions** use PA's safe-navigation form. Identifier keys take dot
notation (paxc rewrites them to PA's `?['field']` on emit); non-identifier keys
(slashes, spaces, digits, numeric indexes) take the subscript form directly:

```
let email  = triggerBody()?["body/email"]      // subscript for slashed keys
let first  = items?[0]                         // subscript for numeric index
let region = config.endpoints.primary          // dot chain: sugar for ?['endpoints']?['primary']
let mix    = triggerBody()?["body/value"].name // freely mixed
```

Subscript keys must be a string or non-negative integer literal.

## The `pa` primitive and the `pa/` folder

```
pa Post_to_webhook
```

with `pa/Post_to_webhook.json`:

```json
{
  "type": "Http",
  "inputs": {
    "method": "POST",
    "uri": "https://example.com/hooks/daily",
    "body": { "summary": "@{variables('summary')}" }
  }
}
```

`pa <Name>` references an opaque action whose body lives in `pa/<Name>.json`
next to the source. `<Name>` must be a valid pax identifier
(`[A-Za-z_][A-Za-z0-9_]*`) so it doubles as a filesystem-safe filename. paxc
drops the file's contents verbatim into `definition.actions[<Name>]` and slots
it into the `runAfter` graph in source order — the file's own `runAfter` (if
any) is informational only.

**Inside the JSON file you write PA expression syntax directly**:
`@{variables('total')}` for variables, `@{outputs('Compose_x')}` for Compose
outputs, `@{triggerBody()}` for trigger data. The convention is for the file to
match PA's "Peek code" output byte-for-byte — that guarantees round-trip
fidelity.

**Do not write a `pa` statement for a trigger.** The statement form declares an
opaque *action*; a trigger is declared by its file alone. `pa/<Name>.trigger.json`
picks the trigger (filename stem = trigger key, contents dropped verbatim), and
paxc finds it with nothing in the source referring to it. Without one, paxc emits
a default manual "Button" trigger. Connection references go in
`pa/connectionReferences.json` and end up at the top level of the emitted flow.

## Round-tripping from PA exports (`--decode`)

```sh
paxc --decode MyFlow_2026.zip                   # → MyFlow_2026/definition.pax + pa/
paxc --decode definition.json --out-dir src/    # raw inner-definition input
```

`--decode` accepts either a legacy PA export `.zip` (from PA's "Export → Package
(Legacy)") or the inner `Microsoft.Flow/flows/<guid>/definition.json`. It writes
a `.pax` source file plus a `pa/` folder of opaque action bodies. Variables,
Composes, `if`/`foreach`/`until`/`switch`/`scope`, `on` handlers, `terminate`,
and PA accessor expressions lower natively; connectors, ParseJson, and anything
else paxc can't yet render fall back as `pa <Name>` blocks with their bodies
byte-for-byte in `pa/`. paxc prints a stderr warning per fallback so nothing is
silently lost.

PA action keys with non-identifier characters (`Send_an_email_(V2)`) are
normalized on decode and the mapping is stored in `pa/flow.json.actionNameMap`;
re-encoding via `paxc --target pa-legacy` restores the original key byte-for-byte.

This makes `--decode` → refactor pax source → recompile a legitimate workflow
for real flows already in Power Automate.

## Safety: never commit real PA exports or decoded sources

Real PA exports (and everything decoded from them) contain **tenant IDs, user
OIDs, SharePoint site URLs, and connection reference IDs**. Never commit them
to a repo. Test corpora that pull real exports (like this project's own
`tests/corpus/`) are gitignored on purpose; a decoded working copy of a client
flow belongs in a sibling `{repo}-testing` directory or outside the tree
entirely, not inside the repo with a hopeful `.gitignore` entry.

If a user asks the agent to "decode this flow" or "add pax source for our
production flow," the output belongs outside the repo unless the user has
sanitized it first.

A flow authored from scratch is a milder case of the same thing. No export went
into it, but the site URL, list ids and recipient addresses a user hands over
still identify their tenant, so a greenfield `pa/` folder is no more committable
than a decoded one unless those values are placeholders.

## Worked recipes

### Author a connector flow end to end

This is the loop the rest of the skill exists to serve. A user asks for
something in plain language: *when someone adds an item to our Requests list,
email the owner if it's urgent and post it to the team channel.*

Three questions have to be answered by the user, because no amount of
compiler is going to know them: which site and list, which channel, and who
counts as the owner. Ask for them together rather than one at a time. Everything
else the agent can do alone.

Lay out the source tree. The trigger is picked up from its filename, so nothing
in the pax source refers to it:

```
requests/
├── requests.pax
└── pa/
    ├── When_an_item_is_created.trigger.json
    ├── Send_email.json
    ├── Post_to_channel.json
    └── connectionReferences.json
```

The four JSON files come from `connectors.md` with the placeholders filled in.
`connectionReferences.json` needs one entry per connector, here SharePoint,
Outlook and Teams. The pax source carries everything else:

```
let priority = triggerBody()?["Priority"]?["Value"]

var subject: string = "New request: " & triggerBody()?["Title"]
var summary: string = subject & " — owner " & triggerBody()?["OwnerEmail"]

if priority == "Urgent" {
  pa Send_email
}

pa Post_to_channel
```

The two sides are joined by name and nothing else. `connectors.md`'s send-mail
body reads `@variables('subject')`, and that is a live reference to the `var
subject` above it; the same goes for `@variables('summary')` in the Teams body.
paxc drops the file verbatim and never checks the name, so a mismatch is not a
compile error — it is an empty subject line at run time. Any field the trigger
already carries can be read straight from the JSON side instead, which is what
`emailMessage/To` should be here: `"@triggerBody()?['OwnerEmail']"`.

Run it locally before packaging anything. `paxr requests.pax` executes the
source with the connector calls skipped and the trigger absent, so every
accessor comes back null and the `if` does not fire — it proves the source
runs and the actions are ordered as intended, not that the branch is right. To
exercise the branch, stub the trigger fields as `var` literals first, which is
the stub-and-fix pattern below.

```sh
paxr requests.pax                     # control flow only
paxc requests.pax | head -40          # eyeball the emitted definition
paxc --target pa-legacy --name requests --out requests.zip requests.pax
```

Hand the user `requests.zip` and tell them where it goes: **My flows → Import →
Import Package (Legacy)**, then pick a connection for each of the three
prompts, then **Import**.

That last step is a human's, and deliberately so. Importing binds the flow to
real connections — mail will go out as whoever owns the Outlook connection —
and consent for that belongs to the person whose mailbox it is, not to the agent
that wrote the flow. Everything up to it is the agent's, and none of it needs a
click.

If the import is rejected, the message names the action and the property, and
that is usually enough to place the fault: a missing parameter key means the
body was short a required field, and a complaint about a connection reference
means `host.connectionName` and the key in `connectionReferences.json` have
drifted apart. Fix the file, recompile, hand over a fresh zip.

### Author a flow from scratch — the stub-and-fix pattern

For a flow that needs connector data, don't reach for a connector call
immediately. Stub the connector's *output* with a `var` holding literal sample
data, get the pax control flow working locally with `paxr`, then swap the stub
for a `pa <Name>` connector call once the shape is right. Massive time saver
over trying to hand-author connector JSON up front.

```
var pending: array = [
  { "title": "Renew SSL", "owner": "alice", "due": "2026-08-01" },
  { "title": "Rotate keys", "owner": "bob",   "due": "2026-08-15" },
]

var summary: string = ""
foreach task in pending {
  summary &= task.title & " (" & task.owner & ")\n"
}
```

Run with `paxr flow.pax` to confirm the control flow. Then replace the `var
pending` block with `pa Get_pending_items` and drop the real SharePoint "Get
items" body (from PA "Peek code") into `pa/Get_pending_items.json`.

### Decode an existing PA export and start editing

```sh
paxc --decode MyFlow_2026.zip           # → MyFlow_2026/definition.pax + pa/
$EDITOR MyFlow_2026/definition.pax      # refactor: extract a scope, add an on-failed handler, etc.
paxc --target pa-legacy \
  --name MyFlow_2026 \
  --out MyFlow_2026_edited.zip \
  MyFlow_2026/definition.pax            # re-emit → import through PA
```

Fallback `pa <Name>` blocks in the decoded output are fine — they re-emit
byte-for-byte, so a partial-native decode is still a lossless round-trip.

### Add an on-failed handler around an existing action

Wrap the risky step in a named scope, then attach an `on failed` (or
`on failed or timedout`) handler pointing at it:

```
scope fetch_data {
  pa HTTP_Get_Data
}

on failed or timedout fetch_data {
  debug("recoverable call failure")
  terminate failed "fetch failed" code "FetchFailed"
}
```

The handler compiles to a `Scope` action whose `runAfter` points at
`Scope_fetch_data` with `["Failed", "TimedOut"]`. `pa <Name>` blocks are also
valid handler targets without a wrapping scope.

### Compile and package for portal import

```sh
paxc --target pa-legacy --name daily_digest --out daily_digest.zip flow.pax
```

Then import through **My flows → Import → Import Package (Legacy)**. If
`--name` is omitted, paxc uses `pa/flow.json`'s `displayName` if present, else
the source filename.

## When to stop and switch

- **Direct PA designer work** for connector authentication, connection
  provisioning, environment-scoped policies, and anything under the PA portal
  that isn't in the flow's `definition.json`. paxc is the compiler for the flow
  definition, not the whole environment.
- **Non-PA workflow engines** — Zapier, n8n, Temporal, and Azure Logic Apps
  authored directly against the Logic Apps API. pax targets Power Automate's
  cloud flow shape specifically.
- **An unverified `operationId` or parameter key** — this is the one thing an
  agent should not settle on its own. A guessed shape produces a flow that
  imports and then fails at run time, which is the worst place to find out.
  Take the body from `connectors.md`, from Microsoft's connector reference, or
  from a peek at a real action; if none of the three answers it, say so and ask
  rather than approximate. Nothing about this is a secrecy rule — connector
  shapes are public API surface, and no tenant value is needed to write one.

Two more pages sit in this directory. `reference.md` has the complete language
grammar, per-statement semantics, expression function catalog, `on`-handler
naming rules, and the round-trip decoder's native-vs-fallback coverage matrix.
`connectors.md` has verified `pa/` bodies for the Standard connectors that come
up most, and the rules for reading a shape off Microsoft's connector reference
when the one you need is not there.
