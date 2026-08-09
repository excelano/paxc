//! The `runAfter` dependency graph, checked on its own terms.
//!
//! Every action in a flow except the ones that start a scope carries a
//! `runAfter` map naming the actions it waits on and which of their outcomes
//! it accepts. The graph is stored as back-references, one edge per waiting
//! action, so an edge that points at nothing is easy to produce and
//! invisible once produced: rename an action, reorder a scope, paste a block
//! in from another flow. PA imports such a flow without complaint, and the
//! waiting action then never runs. No error, no warning, no failed run to
//! look at. That failure mode is why this check exists and why it comes
//! first.
//!
//! `runAfter` is scoped to the containing `actions` map: an action inside a
//! Scope, a Condition branch, a Foreach or a Switch case may only wait on
//! its siblings there. All 68 edges in the round-trip corpus obey this, none
//! crossing a scope boundary. A reference naming a real action in some other
//! scope is therefore just as dead as one naming nothing at all, but the fix
//! is different, so the two are reported apart.
//!
//! Traversal here is directed by shape rather than by action type. The
//! decoder can afford to be type-directed because it only lowers the types
//! it knows; a checker is handed flows built from constructs pax has never
//! heard of and still has to walk them.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde_json::{Map, Value};

use super::Finding;

/// Outcomes PA accepts in a `runAfter` status list. The corpus only ever
/// exercises `Succeeded` and `Failed`; the other two are in the product and
/// a flow using them must not be flagged.
const VALID_STATUSES: &[&str] = &["Succeeded", "Failed", "Skipped", "TimedOut"];

pub const UNKNOWN_TARGET: &str = "runafter-unknown-target";
pub const CROSS_SCOPE: &str = "runafter-cross-scope";
pub const SELF_REFERENCE: &str = "runafter-self";
pub const BAD_STATUS: &str = "runafter-bad-status";
pub const EMPTY_STATUS: &str = "runafter-empty-status";
pub const MALFORMED: &str = "runafter-malformed";
pub const UNREACHABLE: &str = "runafter-unreachable";
pub const NO_ENTRY: &str = "scope-no-entry";
pub const NOT_OBJECT: &str = "action-not-object";

/// One `actions` map, with the JSON path that leads to it.
struct Scope<'a> {
    path: String,
    actions: &'a Map<String, Value>,
}

impl Scope<'_> {
    /// JSON path to one action within this scope, which is where a finding
    /// about that action points.
    fn at(&self, name: &str) -> String {
        format!("{}/{}", self.path, name)
    }
}

/// Check every `actions` map reachable from `root`.
pub fn check(root: &Map<String, Value>) -> Vec<Finding> {
    let mut scopes = Vec::new();
    collect_scopes(root, "actions".to_string(), &mut scopes);

    // Every action name in the flow, mapped to the scope holding it. Action
    // names are unique across a whole flow — verified across the corpus,
    // including nested scopes — so one flat index is enough to answer "does
    // this name exist somewhere else?".
    let mut index: HashMap<&str, &str> = HashMap::new();
    for scope in &scopes {
        for name in scope.actions.keys() {
            index.insert(name.as_str(), scope.path.as_str());
        }
    }

    let mut findings = Vec::new();
    for scope in &scopes {
        check_scope(scope, &index, &mut findings);
    }
    findings
}

/// Gather this `actions` map and every one nested beneath it.
///
/// The four nesting shapes: `actions` on a Scope, Condition, Foreach or
/// Until; `else.actions` on a Condition; `cases.<name>.actions` and
/// `default.actions` on a Switch. Anything else carrying an `actions` map is
/// picked up by the first arm regardless of its type, which is deliberate —
/// an unrecognized container should still be walked, not skipped.
fn collect_scopes<'a>(actions: &'a Map<String, Value>, path: String, out: &mut Vec<Scope<'a>>) {
    out.push(Scope {
        path: path.clone(),
        actions,
    });

    for (name, action) in actions {
        let Some(action) = action.as_object() else {
            continue;
        };
        if let Some(nested) = action.get("actions").and_then(Value::as_object) {
            collect_scopes(nested, format!("{path}/{name}/actions"), out);
        }
        for branch in ["else", "default"] {
            let nested = action
                .get(branch)
                .and_then(Value::as_object)
                .and_then(|b| b.get("actions"))
                .and_then(Value::as_object);
            if let Some(nested) = nested {
                collect_scopes(nested, format!("{path}/{name}/{branch}/actions"), out);
            }
        }
        if let Some(cases) = action.get("cases").and_then(Value::as_object) {
            for (case_name, case) in cases {
                let nested = case
                    .as_object()
                    .and_then(|c| c.get("actions"))
                    .and_then(Value::as_object);
                if let Some(nested) = nested {
                    collect_scopes(
                        nested,
                        format!("{path}/{name}/cases/{case_name}/actions"),
                        out,
                    );
                }
            }
        }
    }
}

fn check_scope(scope: &Scope<'_>, index: &HashMap<&str, &str>, out: &mut Vec<Finding>) {
    // Dependencies that survived validation, per action. Only these
    // participate in the reachability pass; an edge already reported as
    // broken should not also produce an "unreachable" finding for the same
    // action.
    let mut deps: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut entries = 0usize;
    let mut edge_errors = false;

    for (name, action) in scope.actions {
        let Some(action) = action.as_object() else {
            out.push(Finding::error(
                NOT_OBJECT,
                scope.at(name),
                "action is not a JSON object",
            ));
            continue;
        };

        // Absent and `{}` mean the same thing: this action starts the scope,
        // waiting on the trigger at top level or on the container entering
        // anywhere below it. Absent is the common form inside nested scopes
        // and must not be treated as missing data.
        let run_after = match action.get("runAfter") {
            None => {
                entries += 1;
                deps.insert(name, Vec::new());
                continue;
            }
            Some(Value::Object(m)) if m.is_empty() => {
                entries += 1;
                deps.insert(name, Vec::new());
                continue;
            }
            Some(Value::Object(m)) => m,
            Some(other) => {
                edge_errors = true;
                out.push(Finding::error(
                    MALFORMED,
                    scope.at(name),
                    format!(
                        "`runAfter` is {}, expected an object mapping action names to status lists",
                        type_name(other)
                    ),
                ));
                continue;
            }
        };

        let mut valid: Vec<&str> = Vec::new();
        for (target, statuses) in run_after {
            check_statuses(scope, name, target, statuses, out);

            if target == name {
                edge_errors = true;
                out.push(
                    Finding::error(
                        SELF_REFERENCE,
                        scope.at(name),
                        "waits on itself, so it can never run",
                    )
                    .with_note("remove the self-reference, or point it at the action that should precede this one"),
                );
                continue;
            }
            if scope.actions.contains_key(target) {
                valid.push(target.as_str());
                continue;
            }
            edge_errors = true;
            match index.get(target.as_str()) {
                Some(other_scope) => out.push(
                    Finding::error(
                        CROSS_SCOPE,
                        scope.at(name),
                        format!("waits on `{target}`, which is not a sibling"),
                    )
                    .with_note(format!(
                        "`{target}` exists at {other_scope}. `runAfter` only reaches siblings in \
                         the same actions map, so this edge never fires and the action never runs"
                    )),
                ),
                None => out.push(
                    Finding::error(
                        UNKNOWN_TARGET,
                        scope.at(name),
                        format!("waits on `{target}`, which does not exist in this flow"),
                    )
                    .with_note(
                        "PA imports a dangling runAfter edge without complaint and the action \
                         then never runs — check for a rename or a copied block",
                    ),
                ),
            }
        }
        deps.insert(name, valid);
    }

    if scope.actions.is_empty() {
        return;
    }

    // A scope with no starting action cannot begin. Suppressed when an edge
    // was already reported broken here, because that is the cause and
    // repeating it as a second finding buries the fix.
    if entries == 0 && !edge_errors {
        let first = scope
            .actions
            .keys()
            .next()
            .map(String::as_str)
            .unwrap_or("");
        out.push(
            Finding::error(
                NO_ENTRY,
                scope.at(first),
                "no action in this scope starts it — every action waits on another",
            )
            .with_note(
                "exactly one action should have an empty or absent `runAfter`; it runs when the \
                 scope is entered",
            ),
        );
        return;
    }

    report_unreachable(scope, &deps, out);
}

/// Settle every action whose dependencies are settled, repeatedly. What is
/// left over cannot be reached from the start of the scope: it either sits
/// in a `runAfter` cycle or waits on something that does.
///
/// Kahn's algorithm rather than a cycle search, because the useful statement
/// to a reader is "this action can never run", which covers the actions
/// downstream of a cycle as well as the ones inside it.
fn report_unreachable(scope: &Scope<'_>, deps: &BTreeMap<&str, Vec<&str>>, out: &mut Vec<Finding>) {
    let mut waiting: HashMap<&str, usize> = deps.iter().map(|(k, v)| (*k, v.len())).collect();
    let mut waiters: HashMap<&str, Vec<&str>> = HashMap::new();
    for (name, targets) in deps {
        for target in targets {
            waiters.entry(target).or_default().push(name);
        }
    }

    let mut queue: VecDeque<&str> = deps
        .iter()
        .filter(|(_, v)| v.is_empty())
        .map(|(k, _)| *k)
        .collect();
    let mut settled: HashSet<&str> = HashSet::new();

    while let Some(name) = queue.pop_front() {
        if !settled.insert(name) {
            continue;
        }
        for waiter in waiters.get(name).into_iter().flatten() {
            let remaining = waiting.entry(waiter).or_insert(0);
            *remaining = remaining.saturating_sub(1);
            if *remaining == 0 {
                queue.push_back(waiter);
            }
        }
    }

    for name in deps.keys() {
        if settled.contains(name) {
            continue;
        }
        let blocked: Vec<&str> = deps[name]
            .iter()
            .filter(|t| !settled.contains(*t))
            .copied()
            .collect();
        out.push(
            Finding::error(
                UNREACHABLE,
                scope.at(name),
                format!("can never run; it waits on {}", quoted_list(&blocked)),
            )
            .with_note(
                "the runAfter edges in this scope form a cycle, so no path leads here from the \
                 action that starts the scope",
            ),
        );
    }
}

/// Validate one edge's status list. A list that is not an array, holds a
/// non-string, or names an outcome PA does not define is an error; an empty
/// list is legal JSON that no outcome can satisfy, which is a warning
/// because it may be a deliberate way to park an action.
fn check_statuses(
    scope: &Scope<'_>,
    name: &str,
    target: &str,
    statuses: &Value,
    out: &mut Vec<Finding>,
) {
    let Some(list) = statuses.as_array() else {
        out.push(Finding::error(
            MALFORMED,
            scope.at(name),
            format!(
                "status list for `{target}` is {}, expected an array",
                type_name(statuses)
            ),
        ));
        return;
    };

    if list.is_empty() {
        out.push(
            Finding::warning(
                EMPTY_STATUS,
                scope.at(name),
                format!("accepts no outcome from `{target}`, so it never runs"),
            )
            .with_note(
                "an empty status list matches nothing; `[\"Succeeded\"]` is the usual intent",
            ),
        );
        return;
    }

    for status in list {
        match status.as_str() {
            Some(s) if VALID_STATUSES.contains(&s) => {}
            Some(s) => out.push(
                Finding::error(
                    BAD_STATUS,
                    scope.at(name),
                    format!("`{s}` is not a run status"),
                )
                .with_note(format!("expected one of {}", VALID_STATUSES.join(", "))),
            ),
            None => out.push(Finding::error(
                BAD_STATUS,
                scope.at(name),
                format!(
                    "status list for `{target}` holds {}, expected strings",
                    type_name(status)
                ),
            )),
        }
    }
}

fn quoted_list(names: &[&str]) -> String {
    match names {
        [] => "nothing that can be reached".to_string(),
        _ => names
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(v: Value) -> Vec<Finding> {
        check(v.as_object().unwrap())
    }

    fn codes(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|f| f.code).collect()
    }

    #[test]
    fn clean_chain_reports_nothing() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": {}},
            "B": {"type": "Compose", "runAfter": {"A": ["Succeeded"]}},
        }));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn dangling_target_is_an_error() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": {}},
            "B": {"type": "Compose", "runAfter": {"Typo": ["Succeeded"]}},
        }));
        assert_eq!(codes(&f), vec![UNKNOWN_TARGET]);
        assert_eq!(f[0].path, "actions/B");
    }

    #[test]
    fn target_in_another_scope_is_distinguished_from_a_typo() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": {}},
            "Scope": {"type": "Scope", "runAfter": {"A": ["Succeeded"]}, "actions": {
                "Inner": {"type": "Compose"}
            }},
            "B": {"type": "Compose", "runAfter": {"Inner": ["Succeeded"]}},
        }));
        assert_eq!(codes(&f), vec![CROSS_SCOPE]);
        assert!(
            f[0].note
                .as_ref()
                .unwrap()
                .contains("actions/Scope/actions")
        );
    }

    #[test]
    fn absent_run_after_starts_a_nested_scope() {
        let f = run(json!({
            "Loop": {"type": "Foreach", "runAfter": {}, "actions": {
                "First": {"type": "Compose"},
                "Second": {"type": "Compose", "runAfter": {"First": ["Succeeded"]}},
            }},
        }));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn condition_else_branch_is_walked() {
        let f = run(json!({
            "If": {"type": "If", "runAfter": {},
                "actions": {"T": {"type": "Compose"}},
                "else": {"actions": {"E": {"type": "Compose", "runAfter": {"Nope": ["Succeeded"]}}}}},
        }));
        assert_eq!(codes(&f), vec![UNKNOWN_TARGET]);
        assert_eq!(f[0].path, "actions/If/else/actions/E");
    }

    #[test]
    fn switch_cases_and_default_are_walked() {
        let f = run(json!({
            "Switch": {"type": "Switch", "runAfter": {},
                "cases": {"Case1": {"case": "x", "actions": {
                    "C": {"type": "Compose", "runAfter": {"Ghost": ["Succeeded"]}}}}},
                "default": {"actions": {
                    "D": {"type": "Compose", "runAfter": {"Ghost": ["Succeeded"]}}}}},
        }));
        assert_eq!(f.len(), 2);
        let scopes: Vec<&str> = f.iter().map(|f| f.path.as_str()).collect();
        assert!(scopes.contains(&"actions/Switch/cases/Case1/actions/C"));
        assert!(scopes.contains(&"actions/Switch/default/actions/D"));
    }

    #[test]
    fn self_reference_is_caught() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": {"A": ["Succeeded"]}},
        }));
        assert_eq!(codes(&f), vec![SELF_REFERENCE]);
    }

    #[test]
    fn cycle_makes_every_member_unreachable() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": {}},
            "B": {"type": "Compose", "runAfter": {"C": ["Succeeded"]}},
            "C": {"type": "Compose", "runAfter": {"B": ["Succeeded"]}},
        }));
        assert_eq!(codes(&f), vec![UNREACHABLE, UNREACHABLE]);
    }

    #[test]
    fn action_downstream_of_a_cycle_is_also_reported() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": {}},
            "B": {"type": "Compose", "runAfter": {"C": ["Succeeded"]}},
            "C": {"type": "Compose", "runAfter": {"B": ["Succeeded"]}},
            "D": {"type": "Compose", "runAfter": {"C": ["Succeeded"]}},
        }));
        assert_eq!(f.len(), 3);
    }

    #[test]
    fn scope_with_no_starting_action_is_reported_once() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": {"B": ["Succeeded"]}},
            "B": {"type": "Compose", "runAfter": {"A": ["Succeeded"]}},
        }));
        assert_eq!(codes(&f), vec![NO_ENTRY]);
    }

    #[test]
    fn no_entry_is_suppressed_when_a_broken_edge_explains_it() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": {"Gone": ["Succeeded"]}},
        }));
        assert_eq!(codes(&f), vec![UNKNOWN_TARGET]);
    }

    #[test]
    fn unknown_status_is_an_error() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": {}},
            "B": {"type": "Compose", "runAfter": {"A": ["Suceeded"]}},
        }));
        assert_eq!(codes(&f), vec![BAD_STATUS]);
    }

    #[test]
    fn all_four_statuses_are_accepted() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": {}},
            "B": {"type": "Compose",
                  "runAfter": {"A": ["Succeeded", "Failed", "Skipped", "TimedOut"]}},
        }));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn empty_status_list_is_a_warning() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": {}},
            "B": {"type": "Compose", "runAfter": {"A": []}},
        }));
        assert_eq!(codes(&f), vec![EMPTY_STATUS]);
        assert_eq!(f[0].severity, crate::check::Severity::Warning);
    }

    #[test]
    fn malformed_run_after_is_reported_not_ignored() {
        let f = run(json!({
            "A": {"type": "Compose", "runAfter": "Succeeded"},
        }));
        assert_eq!(codes(&f), vec![MALFORMED]);
    }

    #[test]
    fn empty_scope_is_not_reported() {
        let f = run(json!({
            "Scope": {"type": "Scope", "runAfter": {}, "actions": {}},
        }));
        assert!(f.is_empty(), "{f:?}");
    }
}
