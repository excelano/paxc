//! Expressions, checked where they actually live.
//!
//! Most of a flow's logic is expression text sitting in string positions all
//! over the JSON — an email body, a filter query, a condition, a loop's
//! collection. Counting the corpus, 183 strings carry expressions and they
//! hold 248 references to variables, actions and loop items. A checker that
//! ignores them is looking at the quarter of the artifact that happens to be
//! structural.
//!
//! Two things this deliberately does not do.
//!
//! It does not report a parse failure as a malformed expression. `paexpr`
//! has a full recursive-descent parser, but it declines some valid PA on
//! purpose — computed subscripts, for one — because it exists to answer "can
//! this be rendered as pax?", and a No there is not evidence of a defect.
//! Delimiter balance is checked lexically instead, which is what catches the
//! unclosed paren without inheriting the grammar's deliberate gaps.
//!
//! It does not check function names against `pa::functions`. That registry
//! holds 53 entries and is the set paxc can lower, not the set PA defines.
//! The corpus alone calls `union` and `decodeUriComponent`, both real and
//! both absent, so the check would fire on working flows. It becomes
//! possible once the registry covers PA's published function library.
//!
//! Scanning is lexical and quote-aware rather than a regex over the whole
//! string, and that distinction is load-bearing. One corpus flow builds a
//! SharePoint URI containing `getbytitle('ContosoRecognition')` as literal
//! text outside any `@{...}`; another has the word-plus-paren shape inside
//! HTML prose in an email body. Both look like calls to a regex and are not
//! calls at all.

use std::collections::HashSet;

use serde_json::{Map, Value};

use super::Finding;

pub const UNTERMINATED_INTERPOLATION: &str = "expr-unterminated-interpolation";
pub const UNBALANCED_PARENS: &str = "expr-unbalanced-parens";
pub const UNTERMINATED_STRING: &str = "expr-unterminated-string";
pub const UNKNOWN_VARIABLE: &str = "expr-unknown-variable";
pub const UNKNOWN_ACTION: &str = "expr-unknown-action";
pub const UNKNOWN_PARAMETER: &str = "expr-unknown-parameter";
pub const ITEMS_OUTSIDE_LOOP: &str = "expr-items-outside-loop";

/// Keys under an action that hold nested actions. Walked separately, with
/// their own path and their own enclosing-loop context, so they must not be
/// swept up as expression text belonging to the parent.
const CONTAINER_KEYS: &[&str] = &["actions", "else", "cases", "default"];

/// Action types that name the variable they mutate in `inputs.name` rather
/// than through an expression. Renaming a variable at its declaration
/// breaks these exactly as silently as it breaks a `variables('...')`
/// reference, so they resolve against the same set.
const VARIABLE_MUTATORS: &[&str] = &[
    "SetVariable",
    "IncrementVariable",
    "DecrementVariable",
    "AppendToArrayVariable",
    "AppendToStringVariable",
];

/// Accessors whose first argument names something declared elsewhere in the
/// flow, and which therefore can be checked.
const VARIABLE_ACCESSOR: &str = "variables";
const ITEM_ACCESSOR: &str = "items";
const PARAMETER_ACCESSOR: &str = "parameters";
const ACTION_ACCESSORS: &[&str] = &["outputs", "body", "actions", "result"];

/// What the flow declares, gathered before anything is checked against it.
struct Declared {
    variables: HashSet<String>,
    actions: HashSet<String>,
    parameters: HashSet<String>,
    /// Foreach names, so a misplaced `items('...')` can be told from one
    /// naming a loop that does not exist at all.
    loops: HashSet<String>,
}

impl Declared {
    /// Case-insensitive membership. PA's expression language does not
    /// distinguish case — one corpus expression calls both `toLower` and
    /// `tolower` — and rather than take a position on whether names are
    /// folded too, a difference in case is treated as a match so the check
    /// cannot invent a defect out of one.
    fn holds(set: &HashSet<String>, name: &str) -> bool {
        set.contains(name) || set.iter().any(|d| d.eq_ignore_ascii_case(name))
    }
}

pub fn check(definition: &Map<String, Value>) -> Vec<Finding> {
    let actions = match definition.get("actions").and_then(Value::as_object) {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut declared = Declared {
        variables: HashSet::new(),
        actions: HashSet::new(),
        parameters: HashSet::new(),
        loops: HashSet::new(),
    };
    collect_declared(actions, &mut declared);
    if let Some(params) = definition.get("parameters").and_then(Value::as_object) {
        declared.parameters.extend(params.keys().cloned());
    }
    // A trigger is a legitimate target for `outputs('<trigger name>')`, so
    // it belongs in the same set as the actions.
    if let Some(triggers) = definition.get("triggers").and_then(Value::as_object) {
        declared.actions.extend(triggers.keys().cloned());
    }

    let mut out = Vec::new();
    walk_actions(actions, "actions", &[], &declared, &mut out);
    if let Some(triggers) = definition.get("triggers").and_then(Value::as_object) {
        for (name, trigger) in triggers {
            walk_json(
                trigger,
                &format!("triggers/{name}"),
                &[],
                &declared,
                &mut out,
            );
        }
    }
    out
}

/// Every variable, action and loop name in the flow, at any depth.
fn collect_declared(actions: &Map<String, Value>, out: &mut Declared) {
    for (name, action) in actions {
        out.actions.insert(name.clone());
        let Some(action) = action.as_object() else {
            continue;
        };
        if action.get("type").and_then(Value::as_str) == Some("InitializeVariable") {
            let declared = action
                .get("inputs")
                .and_then(|i| i.get("variables"))
                .and_then(Value::as_array);
            for var in declared.into_iter().flatten() {
                if let Some(var_name) = var.get("name").and_then(Value::as_str) {
                    out.variables.insert(var_name.to_string());
                }
            }
        }
        if action.get("type").and_then(Value::as_str) == Some("Foreach") {
            out.loops.insert(name.clone());
        }
        for nested in nested_action_maps(action) {
            collect_declared(nested, out);
        }
    }
}

/// The nested `actions` maps hanging off one action, in whatever container
/// shape holds them.
fn nested_action_maps(action: &Map<String, Value>) -> Vec<&Map<String, Value>> {
    let mut out = Vec::new();
    if let Some(nested) = action.get("actions").and_then(Value::as_object) {
        out.push(nested);
    }
    for branch in ["else", "default"] {
        let nested = action
            .get(branch)
            .and_then(Value::as_object)
            .and_then(|b| b.get("actions"))
            .and_then(Value::as_object);
        if let Some(nested) = nested {
            out.push(nested);
        }
    }
    if let Some(cases) = action.get("cases").and_then(Value::as_object) {
        for case in cases.values() {
            let nested = case
                .as_object()
                .and_then(|c| c.get("actions"))
                .and_then(Value::as_object);
            if let Some(nested) = nested {
                out.push(nested);
            }
        }
    }
    out
}

/// Walk actions, carrying the list of Foreach names that lexically enclose
/// the current point so `items('...')` can be checked against it.
fn walk_actions(
    actions: &Map<String, Value>,
    path: &str,
    enclosing: &[String],
    declared: &Declared,
    out: &mut Vec<Finding>,
) {
    for (name, action) in actions {
        let action_path = format!("{path}/{name}");
        let Some(action) = action.as_object() else {
            continue;
        };

        let action_type = action.get("type").and_then(Value::as_str).unwrap_or("");
        if VARIABLE_MUTATORS.contains(&action_type) {
            let target = action
                .get("inputs")
                .and_then(|i| i.get("name"))
                .and_then(Value::as_str);
            if let Some(target) = target
                && !Declared::holds(&declared.variables, target)
            {
                out.push(
                    Finding::error(
                        UNKNOWN_VARIABLE,
                        format!("{action_path}/inputs/name"),
                        format!("`{action_type}` targets `{target}`, which is never initialized"),
                    )
                    .with_note(
                        nearest_hint(&declared.variables, target).unwrap_or_else(|| {
                            "every variable must be introduced by an InitializeVariable action"
                                .to_string()
                        }),
                    ),
                );
            }
        }

        // The action's own fields, minus the containers and minus runAfter,
        // whose keys are action names rather than expression text.
        for (key, value) in action {
            if CONTAINER_KEYS.contains(&key.as_str()) || key == "runAfter" {
                continue;
            }
            walk_json(
                value,
                &format!("{action_path}/{key}"),
                enclosing,
                declared,
                out,
            );
        }

        let inner: Vec<String> = if action.get("type").and_then(Value::as_str) == Some("Foreach") {
            let mut v = enclosing.to_vec();
            v.push(name.clone());
            v
        } else {
            enclosing.to_vec()
        };

        if let Some(nested) = action.get("actions").and_then(Value::as_object) {
            walk_actions(
                nested,
                &format!("{action_path}/actions"),
                &inner,
                declared,
                out,
            );
        }
        for branch in ["else", "default"] {
            let nested = action
                .get(branch)
                .and_then(Value::as_object)
                .and_then(|b| b.get("actions"))
                .and_then(Value::as_object);
            if let Some(nested) = nested {
                walk_actions(
                    nested,
                    &format!("{action_path}/{branch}/actions"),
                    &inner,
                    declared,
                    out,
                );
            }
        }
        if let Some(cases) = action.get("cases").and_then(Value::as_object) {
            for (case_name, case) in cases {
                let nested = case
                    .as_object()
                    .and_then(|c| c.get("actions"))
                    .and_then(Value::as_object);
                if let Some(nested) = nested {
                    walk_actions(
                        nested,
                        &format!("{action_path}/cases/{case_name}/actions"),
                        &inner,
                        declared,
                        out,
                    );
                }
            }
        }
    }
}

/// Every string anywhere in a JSON value, checked as potential expression
/// text.
fn walk_json(
    value: &Value,
    path: &str,
    enclosing: &[String],
    declared: &Declared,
    out: &mut Vec<Finding>,
) {
    match value {
        Value::String(s) => check_string(s, path, enclosing, declared, out),
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                walk_json(item, &format!("{path}/{i}"), enclosing, declared, out);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                walk_json(item, &format!("{path}/{key}"), enclosing, declared, out);
            }
        }
        _ => {}
    }
}

fn check_string(
    s: &str,
    path: &str,
    enclosing: &[String],
    declared: &Declared,
    out: &mut Vec<Finding>,
) {
    let scan = scan(s);

    if scan.unterminated_interpolation {
        out.push(
            Finding::error(
                UNTERMINATED_INTERPOLATION,
                path,
                "`@{` is never closed by a matching `}`",
            )
            .with_note(
                "PA reads the rest of the string as expression text when the brace is left open",
            ),
        );
    }
    if scan.unterminated_string {
        out.push(Finding::error(
            UNTERMINATED_STRING,
            path,
            "a `'` string literal in the expression is never closed",
        ));
    }
    match scan.paren_balance {
        b if b > 0 => out.push(Finding::error(
            UNBALANCED_PARENS,
            path,
            format!("expression is missing {b} closing parenthesis/es"),
        )),
        b if b < 0 => out.push(Finding::error(
            UNBALANCED_PARENS,
            path,
            format!("expression has {} unmatched closing parenthesis/es", -b),
        )),
        _ => {}
    }

    for reference in &scan.refs {
        let Some(arg) = &reference.arg else { continue };
        let func = reference.func.as_str();

        if func.eq_ignore_ascii_case(VARIABLE_ACCESSOR) {
            if !Declared::holds(&declared.variables, arg) {
                out.push(
                    Finding::error(
                        UNKNOWN_VARIABLE,
                        path,
                        format!("`variables('{arg}')` names no initialized variable"),
                    )
                    .with_note(
                        nearest_hint(&declared.variables, arg).unwrap_or_else(|| {
                            "every variable must be introduced by an InitializeVariable action"
                                .to_string()
                        }),
                    ),
                );
            }
        } else if func.eq_ignore_ascii_case(ITEM_ACCESSOR) {
            let enclosed = enclosing.iter().any(|l| l.eq_ignore_ascii_case(arg));
            if !enclosed {
                let exists = Declared::holds(&declared.loops, arg);
                out.push(
                    Finding::error(
                        ITEMS_OUTSIDE_LOOP,
                        path,
                        format!("`items('{arg}')` is not inside that loop"),
                    )
                    .with_note(if exists {
                        format!("`{arg}` is a loop elsewhere in the flow; `items` only reads the loop it sits inside")
                    } else {
                        format!("no Foreach named `{arg}` encloses this expression")
                    }),
                );
            }
        } else if func.eq_ignore_ascii_case(PARAMETER_ACCESSOR) {
            if !Declared::holds(&declared.parameters, arg) {
                out.push(Finding::error(
                    UNKNOWN_PARAMETER,
                    path,
                    format!("`parameters('{arg}')` names no declared parameter"),
                ));
            }
        } else if ACTION_ACCESSORS
            .iter()
            .any(|a| func.eq_ignore_ascii_case(a))
            && !Declared::holds(&declared.actions, arg)
        {
            out.push(
                Finding::error(
                    UNKNOWN_ACTION,
                    path,
                    format!("`{func}('{arg}')` names no action in this flow"),
                )
                .with_note(nearest_hint(&declared.actions, arg).unwrap_or_else(
                    || "the reference resolves to null at run time rather than failing".to_string(),
                )),
            );
        }
    }
}

/// "did you mean" for a name that differs from a declared one only by case
/// or by a character or two. Kept cheap: exact-length single-edit distance
/// is enough to catch the typo without pretending to be a spell checker.
fn nearest_hint(declared: &HashSet<String>, name: &str) -> Option<String> {
    let near = declared.iter().find(|d| close(d, name))?;
    Some(format!("did you mean `{near}`?"))
}

fn close(a: &str, b: &str) -> bool {
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (a_lower, b_lower) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
    if a.len() == b.len() {
        return a_lower
            .chars()
            .zip(b_lower.chars())
            .filter(|(x, y)| x != y)
            .count()
            <= 1;
    }
    let (long, short) = if a.len() > b.len() {
        (&a_lower, &b_lower)
    } else {
        (&b_lower, &a_lower)
    };
    let mut long_chars = long.chars().peekable();
    let mut skipped = false;
    for c in short.chars() {
        match long_chars.next() {
            Some(l) if l == c => {}
            Some(_) if !skipped => {
                skipped = true;
                if long_chars.peek() != Some(&c) {
                    return false;
                }
                long_chars.next();
            }
            _ => return false,
        }
    }
    true
}

/// One accessor call found in expression text.
struct Reference {
    func: String,
    /// The first argument when it is a plain string literal. `None` when the
    /// call takes no arguments or computes its first one, in which case
    /// there is nothing to resolve and nothing is claimed about it.
    arg: Option<String>,
}

#[derive(Default)]
struct Scan {
    refs: Vec<Reference>,
    paren_balance: i32,
    unterminated_string: bool,
    unterminated_interpolation: bool,
}

/// Split a string into its expression regions and scan each.
///
/// A leading `@` means the whole remainder is one expression; `@@` is an
/// escaped literal `@` and starts nothing. Otherwise expressions appear as
/// `@{...}` interpolations with literal text between them, and only the
/// insides are expression syntax.
fn scan(s: &str) -> Scan {
    let mut out = Scan::default();

    if let Some(rest) = s.strip_prefix('@')
        && !rest.starts_with('@')
        && !rest.starts_with('{')
    {
        scan_region(rest, &mut out);
        return out;
    }

    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            match bytes.get(i + 1) {
                Some(b'@') => {
                    i += 2;
                    continue;
                }
                Some(b'{') => {
                    let body_start = i + 2;
                    match matching_brace(&s[body_start..]) {
                        Some(end) => {
                            scan_region(&s[body_start..body_start + end], &mut out);
                            i = body_start + end + 1;
                            continue;
                        }
                        None => {
                            out.unterminated_interpolation = true;
                            return out;
                        }
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    out
}

/// Byte index of the `}` closing an interpolation, given the slice starting
/// just after `@{`. Quote-aware, so a `}` inside a string literal does not
/// close it, and brace-counting, so nested object syntax does not either.
fn matching_brace(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        if bytes.get(i + 1) == Some(&b'\'') {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// Scan one expression region: balance delimiters and pull out the accessor
/// calls whose first argument is a literal.
fn scan_region(region: &str, out: &mut Scan) {
    let bytes = region.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => match string_literal_end(region, i) {
                Some(end) => i = end,
                None => {
                    out.unterminated_string = true;
                    return;
                }
            },
            b'(' => {
                out.paren_balance += 1;
                i += 1;
            }
            b')' => {
                out.paren_balance -= 1;
                i += 1;
            }
            b if b.is_ascii_alphabetic() || b == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &region[start..i];
                let mut j = i;
                while j < bytes.len() && bytes[j] == b' ' {
                    j += 1;
                }
                if bytes.get(j) == Some(&b'(') && is_checkable_accessor(word) {
                    out.refs.push(Reference {
                        func: word.to_string(),
                        arg: literal_first_arg(region, j + 1),
                    });
                }
            }
            _ => i += 1,
        }
    }
}

fn is_checkable_accessor(word: &str) -> bool {
    [VARIABLE_ACCESSOR, ITEM_ACCESSOR, PARAMETER_ACCESSOR]
        .iter()
        .chain(ACTION_ACCESSORS.iter())
        .any(|a| word.eq_ignore_ascii_case(a))
}

/// The first argument at `from` when it is a plain `'...'` literal, with
/// `''` read as an escaped quote. None for a computed argument or no
/// argument at all — one corpus call in 249 computes its argument, and
/// guessing at those would be inventing findings.
fn literal_first_arg(region: &str, from: usize) -> Option<String> {
    let bytes = region.as_bytes();
    let mut i = from;
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    if bytes.get(i) != Some(&b'\'') {
        return None;
    }
    let mut arg = String::new();
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if bytes.get(i + 1) == Some(&b'\'') {
                arg.push('\'');
                i += 2;
                continue;
            }
            return Some(arg);
        }
        let end = next_char_boundary(region, i);
        arg.push_str(&region[i..end]);
        i = end;
    }
    None
}

/// Index just past the `'` that closes the literal starting at `start`.
fn string_literal_end(region: &str, start: usize) -> Option<usize> {
    let bytes = region.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if bytes.get(i + 1) == Some(&b'\'') {
                i += 2;
                continue;
            }
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn next_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i + 1;
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(definition: Value) -> Vec<Finding> {
        check(definition.as_object().unwrap())
    }

    fn codes(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|f| f.code).collect()
    }

    fn with_var(name: &str, body: Value) -> Value {
        json!({"actions": {
            "Init": {"type": "InitializeVariable", "runAfter": {},
                     "inputs": {"variables": [{"name": name, "type": "String"}]}},
            "Use": body,
        }})
    }

    #[test]
    fn a_declared_variable_resolves() {
        let f = run(with_var(
            "total",
            json!({"type": "Compose", "inputs": "@variables('total')"}),
        ));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn an_undeclared_variable_is_reported() {
        let f = run(with_var(
            "total",
            json!({"type": "Compose", "inputs": "@variables('totl')"}),
        ));
        assert_eq!(codes(&f), vec![UNKNOWN_VARIABLE]);
        assert!(f[0].note.as_ref().unwrap().contains("did you mean `total`"));
    }

    #[test]
    fn a_variable_differing_only_in_case_is_not_reported() {
        let f = run(with_var(
            "Total",
            json!({"type": "Compose", "inputs": "@variables('total')"}),
        ));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn an_unknown_action_reference_is_reported() {
        let f = run(json!({"actions": {
            "A": {"type": "Compose", "runAfter": {}, "inputs": "x"},
            "B": {"type": "Compose", "inputs": "@outputs('Ay')"},
        }}));
        assert_eq!(codes(&f), vec![UNKNOWN_ACTION]);
    }

    #[test]
    fn a_trigger_is_a_valid_outputs_target() {
        let f = run(json!({
            "triggers": {"When_a_row_changes": {"type": "Request"}},
            "actions": {"A": {"type": "Compose", "inputs": "@outputs('When_a_row_changes')"}},
        }));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn items_inside_its_own_loop_resolves() {
        let f = run(json!({"actions": {
            "Loop": {"type": "Foreach", "runAfter": {}, "foreach": "@outputs('Loop')",
                "actions": {"Inner": {"type": "Compose", "inputs": "@items('Loop')"}}},
        }}));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn items_naming_a_loop_it_is_not_inside_is_reported() {
        let f = run(json!({"actions": {
            "Loop": {"type": "Foreach", "runAfter": {}, "actions": {
                "Inner": {"type": "Compose", "inputs": "x"}}},
            "After": {"type": "Compose", "inputs": "@items('Loop')"},
        }}));
        assert_eq!(codes(&f), vec![ITEMS_OUTSIDE_LOOP]);
        assert!(
            f[0].note
                .as_ref()
                .unwrap()
                .contains("elsewhere in the flow")
        );
    }

    #[test]
    fn nested_loops_both_count_as_enclosing() {
        let f = run(json!({"actions": {
            "Outer": {"type": "Foreach", "runAfter": {}, "actions": {
                "Inner": {"type": "Foreach", "actions": {
                    "Deep": {"type": "Compose", "inputs": "@concat(items('Outer'), items('Inner'))"}}}}},
        }}));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn an_unclosed_paren_is_reported() {
        let f = run(json!({"actions": {
            "A": {"type": "Compose", "inputs": "@concat('a', 'b'"},
        }}));
        assert_eq!(codes(&f), vec![UNBALANCED_PARENS]);
        assert!(f[0].message.contains("missing 1"));
    }

    #[test]
    fn a_surplus_paren_is_reported() {
        let f = run(json!({"actions": {
            "A": {"type": "Compose", "inputs": "@concat('a'))"},
        }}));
        assert_eq!(codes(&f), vec![UNBALANCED_PARENS]);
        assert!(f[0].message.contains("1 unmatched"));
    }

    #[test]
    fn an_unclosed_interpolation_is_reported() {
        let f = run(json!({"actions": {
            "A": {"type": "Compose", "inputs": "value is @{concat('a','b')"},
        }}));
        assert_eq!(codes(&f), vec![UNTERMINATED_INTERPOLATION]);
    }

    #[test]
    fn an_unclosed_string_literal_is_reported() {
        let f = run(json!({"actions": {
            "A": {"type": "Compose", "inputs": "@concat('a, 'b')"},
        }}));
        assert!(codes(&f).contains(&UNTERMINATED_STRING));
    }

    #[test]
    fn call_shaped_text_outside_an_interpolation_is_not_a_call() {
        // The SharePoint URI case from the corpus: `getbytitle('X')` is
        // literal text, and so is a `variables('ghost')` that never sits
        // inside `@{...}`.
        let f = run(json!({"actions": {
            "A": {"type": "Compose",
                  "inputs": "_api/web/lists/variables('ghost')/items(@{outputs('A')})"},
        }}));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn a_reference_inside_a_string_literal_is_not_a_call() {
        let f = run(json!({"actions": {
            "A": {"type": "Compose", "inputs": "@concat('variables(''ghost'')', 'x')"},
        }}));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn an_email_address_is_not_an_expression() {
        let f = run(json!({"actions": {
            "A": {"type": "Compose", "inputs": {"To": "someone@example.com"}},
        }}));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn a_doubled_at_is_an_escaped_literal() {
        let f = run(json!({"actions": {
            "A": {"type": "Compose", "inputs": "@@variables('ghost')"},
        }}));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn a_computed_argument_is_not_guessed_at() {
        let f = run(json!({"actions": {
            "A": {"type": "Compose", "inputs": "@variables(concat('a','b'))"},
        }}));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn expressions_are_found_at_any_depth_in_inputs() {
        let f = run(json!({"actions": {
            "A": {"type": "Compose", "inputs": {
                "parameters": {"emailMessage": {"To": ["@variables('ghost')"]}}}},
        }}));
        assert_eq!(codes(&f), vec![UNKNOWN_VARIABLE]);
        assert_eq!(f[0].path, "actions/A/inputs/parameters/emailMessage/To/0");
    }

    #[test]
    fn a_variable_declared_in_a_nested_scope_still_counts() {
        let f = run(json!({"actions": {
            "Scope": {"type": "Scope", "runAfter": {}, "actions": {
                "Init": {"type": "InitializeVariable",
                         "inputs": {"variables": [{"name": "inner", "type": "String"}]}}}},
            "Use": {"type": "Compose", "inputs": "@variables('inner')"},
        }}));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn a_declared_parameter_resolves() {
        let f = run(json!({
            "parameters": {"$connections": {"type": "Object"}},
            "actions": {"A": {"type": "Compose", "inputs": "@parameters('$connections')"}},
        }));
        assert!(f.is_empty(), "{f:?}");
    }
}
