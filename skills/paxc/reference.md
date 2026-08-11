# paxc reference

Complete reference for the pax language and the `paxc` / `paxr` CLIs. Load this
when `SKILL.md` isn't specific enough — full statement grammar, per-block
semantics, expression operators and precedence, PA accessor catalog, the
`on`-handler naming rules, and the round-trip decoder's coverage matrix.

## Invocation

```
paxc [--target pa-legacy] [--name NAME] [--out PATH] <file.pax>
paxc --decode <flow.json | flow.zip> [--out-dir DIR]
paxc --check <flow.json | flow.zip>
paxc --help | --version
paxr [--verbose | --quiet | --debug] <file.pax>
paxr --version
```

| Flag | Meaning |
|---|---|
| `--target pa-legacy` (paxc) | write a legacy PA import package `.zip` (portal: My flows → Import → Import Package (Legacy)) |
| `--name NAME` (paxc) | package/flow display name; defaults to `pa/flow.json`'s `displayName`, else the source filename |
| `--out PATH` (paxc) | output path for `--target` mode; defaults to `<name>.zip` in the current directory |
| `--decode` (paxc) | reverse mode: read an exported PA flow (zip or inner `definition.json`) and write `.pax` source + `pa/` folder |
| `--out-dir DIR` (paxc, decode) | for `.json` input, defaults to the input's parent directory; for `.zip`, defaults to a sister dir named after the zip's stem |
| `--check` (paxc) | read an exported PA flow (zip or inner `definition.json`) and report problems in it; writes nothing, exits 1 when any finding is an error |
| `--allow CODE` (paxc) | demote one finding code from error to warning; repeatable. Applies to a compile and to `--check`. Unknown codes are rejected |
| `--verbose` / `-v` (paxr) | trace every action the interpreter touches (`init`, `set`, `increment`, `compose`, `condition?`, `iter[N]`, …) |
| `--quiet` / `-q` (paxr) | suppress all output; exit code only |
| `--debug` / `-d` (paxr) | print only `debug()` output, no state dump |
| `--version` / `-V` (both) | print version and exit |

Default paxr output is the debug lines plus an end-of-run **state dump** listing
every binding and its final value, tagged `(var <type>)` or `(let)`.

## Types

Six v1 types: `int`, `float`, `string`, `bool`, `array`, `object`. No implicit
coercion between int and float except at variable initialization (an int literal
assigned to a float-typed `var` is coerced). Arithmetic promotes to float when
either operand is float; `int / int` stays int (matching PA's integer division).
Float literals require a digit after the decimal point (`1.5`, `0.25`) so
`obj.field` is never ambiguous.

paxr treats `5 == 5.0` as true for local simulation ergonomics. PA's expression
language uses strict JToken equality and would consider them unequal; do not
rely on cross-type `==` for business logic.

## Statements

```
var name: type [= expr]      // InitializeVariable action, named Initialize_<name>
let name = expr              // Compose action, named Compose_<name>, immutable
name = expr                  // SetVariable
name += expr                 // IncrementVariable (int/float) or AppendToArrayVariable (array)
name -= expr                 // DecrementVariable (int/float)
name &= expr                 // AppendToStringVariable (string)
pa Name                      // opaque action; body in pa/<Name>.json
debug(expr, …)               // paxr-only diagnostic; paxc strips at compile time
terminate <status> [message] [code expr]
```

A `var` with no initializer emits `InitializeVariable` with `value` omitted;
PA then supplies the type's zero value at runtime. This is the shape PA's own
designer produces, so decoded flows typically look this way.

String literals use double quotes with escape sequences `\n`, `\t`, `\"`, `\\`.

`//` starts a line comment and runs to the end of the line; there is no block
form. `#` is **not** a comment — it is a lex error, which is worth knowing
because so many configuration languages take it.

## Expressions

Operator precedence (tightest to loosest):

| Level | Operators |
|---|---|
| Unary | `!` (boolean not), `-` (numeric negation) |
| Multiplicative | `*`, `/` |
| Additive | `+`, `-` |
| Comparison | `==`, `!=`, `>`, `<`, `>=`, `<=` |
| String concat | `&` |
| Logical AND | `&&` |
| Logical OR | `\|\|` |

Parens override normally. In emitted JSON, expressions become PA expression
strings: `@{...}` wrapping for interpolated contexts (e.g. inside a `let`
value's `inputs`), bare function form for `if` conditions
(`@greater(variables('completed'), 0)`). paxc picks the wrapping.

### Member and subscript access

```
obj.field              // sugar for obj?["field"] — identifier keys only
obj?["non/ident/key"]  // subscript for non-identifier string keys
arr?[0]                // subscript for numeric index
```

Subscript keys must be a string literal or non-negative integer literal — no
expressions. Chains mix freely: `triggerBody()?["body/value"].name`.
paxr uses null-safe semantics: missing keys, out-of-range indexes, and
type-mismatched targets yield `null` instead of erroring (matching PA's
`?[...]`).

### PA accessor calls

paxc recognizes these as PA's runtime accessors and emits them unchanged; paxr
partially simulates them:

| Accessor | paxr behavior |
|---|---|
| `triggerBody()`, `triggerOutputs()`, `trigger()` | returns `null` with `<skipping unknown "…">` (no runtime data locally) |
| `parameters('name')` | returns `null` with `<skipping unknown "…">` |
| `body('actionKey')`, `outputs('actionKey')`, `actions('actionKey')` | returns `null` with `<skipping unknown "…">` |
| `iterationIndexes('loopKey')` | returns the active `foreach` iteration counter |
| `item()` | returns the current `foreach` item (via the iterator name) |

## Control flow

### if / else if / else

```
if cond { … } else if cond { … } else { … }
```

Compiles to `Condition`. Non-boolean condition expressions (a bare function
call) get auto-wrapped `equals(cond, true)`. `else if` chains nest inside the
`else` branch.

### switch

```
switch subject {
  case "active" { … }
  case 0 { … }
  default { … }
}
```

Compiles to `Switch`. Case values are **scalar literals only** — string, int, or
bool. Arbitrary expressions in a case clause are rejected. `default` is
optional; no matching case is a no-op.

### foreach

```
foreach task in tasks { … }
```

Compiles to `Apply_to_each`. The iterator name (`task`) is available by
dot-access inside the body. Mutations touch enclosing-scope variables; PA
runs iterations serially by default.

### until

```
until cond [max N] [timeout "PT10M"] { … }
```

PA do-while (body runs at least once; loop exits when `cond` becomes true).
Compiles to `Until`. Optional trailing clauses (must appear in the order
`max` then `timeout` when both present):

- `max N` — positive int literal (fits in 32 bits); default 60 in PA.
- `timeout "…"` — ISO 8601 duration string literal; default `PT1H` in PA.

paxr caps its local iteration at the user-set `max` (or 60); on cap-hit it
prints `<until "Until" hit iteration cap of N>` so you can tell a capped exit
from a natural one. paxr ignores `timeout` (no wall-clock simulation); PA still
enforces it.

### scope

```
scope [name] { … }
```

Wraps a block in a `Scope` action — a `runAfter` unit and the attachment point
for `on` handlers. Named: action key `Scope_<name>`. Anonymous: `Scope`,
auto-suffixed when repeated.

### on handlers

```
on <status> [or <status>]* <target> { … }
```

Statuses: `succeeded`, `failed`, `skipped`, `timedout` (mirrors PA's
`runAfter` set). Target: a **named scope** or **`pa <Name>`** declared earlier
in the source. Compiles to a `Scope` whose `runAfter` points at the target
with the listed status array.

Handler action name: `On_<status>_<target>` for single-status, or
`On_<status1>_<status2>_…_<target>` for multi-status; auto-suffixed on collision.
Handlers sit **off** the main sibling chain — a statement written after
handlers chains back to the last real action before the handlers, not to a
handler.

Multiple handlers on the same target are independent parallel actions in the
graph. Listing the same status twice in one handler is a resolve error.

**Namespace rule:** scope names and `pa <Name>` names share one global
namespace; two `scope work`, or `scope foo` + `pa foo`, or two `pa HTTP_Call`
are all resolve errors. This holds across mutually-exclusive branches (two
scopes with the same name in `if` and `else` are still rejected).

paxr walks the happy path — handlers containing `succeeded` fire locally;
handlers without `succeeded` print `<skipping on-<labels> handler "…">` and
move on. PA dispatches them correctly at runtime regardless.

### terminate

```
terminate succeeded
terminate cancelled
terminate failed
terminate failed "message"
terminate failed "message" code "CODE"
terminate failed code "CODE"
```

Compiles to `Terminate` with `runStatus` set. `message` and `code` are only
accepted after `failed` (PA ignores `runError` on the other statuses). Both
are expressions; `&` concat and variable references work. `code` is a
contextual keyword — `terminate failed code` (with `code` a string variable)
still parses as the message-only form.

paxr halts execution on reaching `terminate`; the state dump still prints
what was set up to that point.

## debug()

```
debug()                     // breadcrumb
debug(x)                    // auto-labeled with the source slice
debug(x, y, x - y)          // comma-separated, single line
```

paxr-only. paxc strips every `debug()` at compile time (they don't participate
in `runAfter`) and prints `note: dropped N debug() statement(s)` to stderr.

## The `pa` primitive and file convention

`pa <Name>` inserts an opaque action whose body is `pa/<Name>.json` next to
the source. `<Name>` must match `[A-Za-z_][A-Za-z0-9_]*` (so it's a safe
filename and a valid pax identifier).

- **Actions:** `pa/<Name>.json` — the file's contents drop verbatim into
  `definition.actions[<Name>]`. Any `runAfter` inside the file is informational
  — paxc's structural sequence wins on emit.
- **Triggers:** a single `pa/<Name>.trigger.json` selects the trigger. Filename
  stem = trigger key; contents drop into `definition.triggers[<Name>]`. Zero
  files → default manual "Button" trigger. Two or more → compile error.
- **Connection references:** `pa/connectionReferences.json` (contents drop at
  the emitted flow's top level).

Inside those JSON files you write PA expression syntax directly:
`variables('x')`, `outputs('Compose_x')`, `triggerBody()`. A value that is
nothing but an expression is written bare and keeps its type
(`"@variables('total')"`); a value mixing text and expressions interpolates
them one pair of braces at a time (`"<p>@{variables('total')}</p>"`) and always
produces a string. The convention is byte-for-byte with PA's "Peek code" output
— that guarantees round-trip fidelity. `connectors.md` has verified bodies for
the common connectors.

## Function calls

Function names pass through to PA's expression language unchanged; paxc does
not distinguish "built-in" from "unknown." paxr implements a subset for local
evaluation (anything outside the subset returns `null` with a
`<skipping unknown "name">` notice):

| Category | paxr-evaluable functions |
|---|---|
| Arithmetic | `add`, `sub`, `mul`, `div`, `mod`, `min`, `max`, `range` |
| Comparison and logic | `equals`, `less`, `lessOrEquals`, `greater`, `greaterOrEquals`, `and`, `or`, `not`, `coalesce` |
| Text | `concat`, `toUpper`, `toLower`, `trim`, `substring`, `indexOf`, `lastIndexOf`, `startsWith`, `endsWith`, `replace`, `split`, `uriComponent`, `uriComponentToString` |
| Polymorphic | `length`, `empty`, `contains` |
| Array | `first`, `last`, `skip`, `take`, `join`, `createArray` |
| Conversion and utility | `string`, `int`, `bool`, `guid` |

Semantics worth knowing:

- `length`, `empty`, and `contains` accept strings, arrays, and objects.
- `startsWith`, `endsWith`, `indexOf`, `lastIndexOf` are **case-insensitive**
  in PA and paxr; `string contains` is case-sensitive.
- `min` / `max` take variadic numeric args OR a single array.
- `coalesce` returns the first argument that is not `null`; `""` and `0` are
  not null.
- `int("  42  ")` trims whitespace; unparseable input returns `null` (does not
  error).
- `bool` accepts `"true"`/`"false"` (case-insensitive), `"1"`/`"0"`, integer
  `0`/`1`.
- `guid()` returns a fresh UUID per call; runs that use it aren't
  bit-for-bit reproducible (matches PA).

Date/time (`utcNow`, `formatDateTime`, `addMinutes`, …) are not yet in paxr;
they render as `null` locally but work in PA.

## The `runAfter` rule

Each statement's `runAfter` is the immediately preceding statement in source
order. The first statement runs after the trigger (empty `runAfter`). The rule
applies recursively inside control-flow bodies (each body chains fresh at its
first statement).

Two intentional exceptions:

- **`debug()`** is stripped at compile time — it doesn't participate in the
  graph.
- **`on` handlers** are side-attached: their `runAfter` points at their target
  with the chosen status(es), and a statement after handlers chains back to
  the last real action before the handlers.

`runAfter` inside a `pa/<Name>.json` body is informational only — paxc's
structural sequence wins on emit. This is why pasting PA "Peek code" into a
`pa/<Name>.json` Just Works: the file's internal `runAfter` is ignored, so
there's no need to rewrite it to match its new context.

## Checking a flow

The same checks run in two places: on demand against any flow definition with
`--check`, and automatically over the `pa/` bodies a compile is about to emit.

### During a compile

Compiling runs the checks over the flow it is about to produce and reports
anything that lands inside a `pa/` body, pointing at the file you edited rather
than at generated JSON:

```
Error: [expr-unknown-function] `noSuchFn(...)` is not a Power Automate expression function
   ╭─[ pa/Send_an_email.json:12:15 ]
12 │       "Subject": "@noSuchFn(variables('title'))"
   │                  ─────────────┬─────────────────
   │                               ╰─── here
```

This closes the gap that pax source has always been validated and `pa/` bodies
never were. An error fails the compile and **nothing is written** — no JSON on
stdout, no `.zip`.

`--allow <CODE>` demotes one code from error to warning; repeat it for more.
The finding is still reported in full, with a note saying it was allowed. It is
there for when paxc is wrong about your flow and you would rather ship than wait
for a fix. An unknown code is rejected rather than accepted silently, since a
mistyped `--allow` would otherwise fail the build for the reason it was passed
to prevent. `--allow` means the same thing in `--check`.

Findings that fall outside every `pa/` body are not the author's: that is JSON
paxc generated from pax the resolver already validated, so one of them is a paxc
bug. They report as such, name the issue tracker, and are fatal — shipping a
flow paxc knows is broken is worse than an alarming message. `--allow` does not
reach them.

### On demand (`--check`)

`paxc --check` reads a flow definition and reports problems in it. It writes
nothing, compiles nothing, and needs no `.pax` source: the input is the
artifact PA consumes, so a flow that was never written in pax checks the same
as one that was. It accepts an export envelope, the inner properties map, or a
bare definition object, from either a `.json` file or a legacy `.zip`.

Each finding prints as `<severity>: [<code>] <json-path>: <message>`, with an
optional `note:` line under it. The path follows the JSON nesting, so
`actions/Scope/actions/Get_items` and
`actions/Send_email/inputs/parameters/emailMessage/Body` both lead straight to
the spot. Codes are stable and greppable. Exit status is 1 if any finding is an
error, 0 otherwise, including when there are warnings.

**`runAfter` graph.** `runafter-unknown-target` (an edge naming an action that
does not exist), `runafter-cross-scope` (naming a real action in a different
`actions` map, which never fires — `runAfter` reaches siblings only),
`runafter-self`, `runafter-bad-status` (not one of `Succeeded`, `Failed`,
`Skipped`, `TimedOut`), `runafter-malformed`, `runafter-unreachable` (in or
behind a cycle), `scope-no-entry` (no action starts the scope),
`action-not-object`. Warning: `runafter-empty-status`, a status list that
matches no outcome.

**References and expressions.** `expr-unknown-variable` (a `variables('x')`, or
a `SetVariable`/`AppendTo*`/`Increment` target, naming nothing an
`InitializeVariable` declares), `expr-unknown-action` (an `outputs`, `body`,
`actions` or `result` call naming no action or trigger), `expr-items-outside-loop`,
`expr-unknown-parameter`, `expr-unbalanced-parens`, `expr-unterminated-string`,
`expr-unterminated-interpolation`, `expr-unknown-function` (a call to something
PA does not define, which fails the run rather than the import). Near-miss
names carry a "did you mean".

Name resolution folds case, because PA folds case when it resolves a function
name — `tolower` and `toLower` both run. An accessor whose first argument is
computed rather than a literal is left alone rather than guessed at.

A call is only a call inside an expression region. Word-plus-paren in literal
text is not one, which is what keeps a SharePoint URI containing
`getbytitle('X')`, or an email body containing the words "Direct reports (if
any)", from being reported.

`$authentication` and `$connections` need no declaration. PA supplies both, and
a bare definition or a hand-assembled fragment carries neither while still being
correct — only a complete export declares them.

**Not checked.** Connector `operationId`s and parameter keys, so a body naming
an operation the connector does not have still passes. Argument counts, so a
real function called with the wrong number of arguments passes. And a parse
failure is never reported as a malformed expression — delimiter balance is
checked lexically instead, so valid PA that pax cannot render is not mistaken
for a defect.

## Round-trip decoder coverage

`paxc --decode` lowers the following to native pax; anything else falls back
to `pa <Name>` with the action body byte-for-byte in `pa/`:

**Native (lowers to pax source):**

- Variable lifecycle: `InitializeVariable`, `SetVariable`, `IncrementVariable`,
  `DecrementVariable`, `AppendToStringVariable`, `AppendToArrayVariable`.
- `Compose` (as `let`), but only when the action key has shape
  `Compose_<identifier>` — a bare `Compose` (PA's default first-Compose name)
  falls back.
- Containers: `If`, `Foreach`, `Until`, `Switch`, `Scope`.
- `Terminate`.
- On-handlers — a `Scope` whose `runAfter` points at a **single**, addressable
  target with non-default statuses, where the target is a named scope or
  `pa <Name>`. Handlers attached to natively-lowered variables/lets or
  anonymous scopes fall back.
- Values: JSON literals and PA expression strings (`@variables('x')`,
  `@add(x, 1)`, `@triggerBody()?['body/email']`, `"hello @{variables('n')}!"`),
  when every node has a pax-renderable form.
- `If` conditions: both `@`-string form and PA designer's structured-object form
  (`{"and": [{"equals": [...]}]}`).
- `items('<For_each_action_key>')` inside a foreach body lowers to the iterator name.
- PA accessors listed above.
- Path segments: identifier keys → `.field`; non-identifier keys and numeric
  indexes → `?["…"]`.

**Fallback (opaque `pa <Name>` with the body in `pa/`):**

- All connectors (`OpenApiConnection`, `OpenApiConnectionWebhook`), `ParseJson`,
  and any action whose required structural piece (condition, collection, switch
  subject, case literal) doesn't render.
- Containers whose structural piece fell back — the whole container is opaque,
  with children nested inside the opaque body.

paxc prints a stderr warning per fallback so nothing decodes silently. Every
fallback re-encodes byte-for-byte on `paxc --target pa-legacy`, so a partial
native decode is still a lossless round-trip.

**`actionNameMap`:** action keys with characters outside pax's identifier rules
(`Send_an_email_(V2)`) are normalized on decode; the original→normalized map is
stored in `pa/flow.json.actionNameMap`. The encoder reads it and restores the
original key byte-for-byte. Missing or malformed `pa/flow.json` falls back to
the user-written name verbatim.

## paxr output modes

Default: `debug()` output plus end-of-run state dump. `--verbose`/`-v`: trace
event per action touched. `--quiet`/`-q`: no output, exit code only.
`--debug`/`-d`: only `debug()` lines, no state dump. The four modes are
mutually exclusive.

State dump format:

```
end state:
counter (var int) = 2
label (let) = "done"
```

`(var <type>)` for `var` bindings, `(let)` for Compose bindings.
