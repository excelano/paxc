//! PA expression-function library: the registry of function names paxc
//! recognizes and the paxr-side evaluators for the ones paxr can simulate.
//!
//! This is the single source of truth for paxc's function-library surface.
//! Each entry pairs a PA function name with its arity contract and an
//! optional paxr evaluator. `paxr_eval: None` means the function is
//! recognized by paxc (so resolver hints, `is_known_function` checks, and
//! diagnostics work) but paxr cannot simulate it locally; the call falls
//! through to the same "unknown" treatment a fully-unrecognized name
//! would get, surfacing the standard skip notice in non-quiet runs.
//!
//! When `paxr_eval` is `Some`, the dispatcher (see `eval_call` in
//! `interpreter.rs`) validates the args against `arity` first and only
//! then invokes the evaluator. Arity-mismatched calls fall back to the
//! unknown branch -- preserving the original match-arm behavior where a
//! guard like `"toUpper" if args.len() == 1` failed silently to (Null,
//! true).
//!
//! To add a new PA function paxr should evaluate: write the `fn
//! eval_<name>(args: &[Value]) -> Value` helper, then add a `FunctionDef`
//! entry referencing it. Stub additions (paxr_eval: None) only require
//! the registry entry plus a name reservation in this module.

use crate::interpreter::Value;
use uuid::Uuid;

/// Argument-count contract enforced by the dispatcher before invoking
/// `paxr_eval`. Values inside the function body can assume the contract
/// was satisfied; deeper validation (type checks, value ranges) lives in
/// each evaluator.
pub enum Arity {
    Exact(usize),
    Range(usize, usize),
    AtLeast(usize),
}

impl Arity {
    pub fn check(&self, n: usize) -> bool {
        match self {
            Arity::Exact(k) => n == *k,
            Arity::Range(lo, hi) => n >= *lo && n <= *hi,
            Arity::AtLeast(k) => n >= *k,
        }
    }
}

pub type PaxrEvalFn = fn(&[Value]) -> Value;

pub struct FunctionDef {
    pub name: &'static str,
    pub arity: Arity,
    pub paxr_eval: Option<PaxrEvalFn>,
}

/// PA expression-prefix names users sometimes call bare without parens.
/// These are not callable functions; the list exists only so the
/// resolver's "did you mean to call it?" hint covers them.
pub static ACCESSORS: &[&str] = &[
    "body",
    "items",
    "outputs",
    "variables",
    "parameters",
    "triggerBody",
    "triggerOutputs",
];

/// The full registry: every function in PA's published expression-function
/// reference except the handful noted at the stub block below. Order is
/// grouped by category for readability; the dispatcher does a linear scan,
/// which stays cheap at a couple of hundred entries because lookups happen
/// once per call site, not once per evaluation step.
pub static FUNCTIONS: &[FunctionDef] = &[
    // arithmetic / numeric
    FunctionDef {
        name: "add",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_add),
    },
    FunctionDef {
        name: "sub",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_sub),
    },
    FunctionDef {
        name: "mul",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_mul),
    },
    FunctionDef {
        name: "div",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_div),
    },
    FunctionDef {
        name: "mod",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_mod),
    },
    FunctionDef {
        name: "min",
        arity: Arity::AtLeast(0),
        paxr_eval: Some(eval_min),
    },
    FunctionDef {
        name: "max",
        arity: Arity::AtLeast(0),
        paxr_eval: Some(eval_max),
    },
    FunctionDef {
        name: "range",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_range),
    },
    // comparison / logic
    FunctionDef {
        name: "coalesce",
        arity: Arity::AtLeast(0),
        paxr_eval: Some(eval_coalesce),
    },
    FunctionDef {
        name: "equals",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_equals),
    },
    FunctionDef {
        name: "less",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_less),
    },
    FunctionDef {
        name: "lessOrEquals",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_less_or_equals),
    },
    FunctionDef {
        name: "greater",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_greater),
    },
    FunctionDef {
        name: "greaterOrEquals",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_greater_or_equals),
    },
    FunctionDef {
        name: "not",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_not),
    },
    FunctionDef {
        name: "and",
        arity: Arity::AtLeast(0),
        paxr_eval: Some(eval_and),
    },
    FunctionDef {
        name: "or",
        arity: Arity::AtLeast(0),
        paxr_eval: Some(eval_or),
    },
    // string
    FunctionDef {
        name: "concat",
        arity: Arity::AtLeast(0),
        paxr_eval: Some(eval_concat),
    },
    FunctionDef {
        name: "toUpper",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_to_upper),
    },
    FunctionDef {
        name: "toLower",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_to_lower),
    },
    FunctionDef {
        name: "trim",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_trim),
    },
    FunctionDef {
        name: "substring",
        arity: Arity::Range(2, 3),
        paxr_eval: Some(eval_substring),
    },
    FunctionDef {
        name: "indexOf",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_index_of),
    },
    FunctionDef {
        name: "lastIndexOf",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_last_index_of),
    },
    FunctionDef {
        name: "startsWith",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_starts_with),
    },
    FunctionDef {
        name: "endsWith",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_ends_with),
    },
    FunctionDef {
        name: "replace",
        arity: Arity::Exact(3),
        paxr_eval: Some(eval_replace),
    },
    FunctionDef {
        name: "split",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_split),
    },
    FunctionDef {
        name: "join",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_join),
    },
    // URI encoding
    FunctionDef {
        name: "uriComponent",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_uri_component),
    },
    FunctionDef {
        name: "uriComponentToString",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_uri_component_to_string),
    },
    // conversion / identity
    FunctionDef {
        name: "string",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_string),
    },
    FunctionDef {
        name: "int",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_int),
    },
    FunctionDef {
        name: "bool",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_bool),
    },
    FunctionDef {
        name: "guid",
        arity: Arity::Exact(0),
        paxr_eval: Some(eval_guid),
    },
    FunctionDef {
        name: "createArray",
        arity: Arity::AtLeast(0),
        paxr_eval: Some(eval_create_array),
    },
    // polymorphic
    FunctionDef {
        name: "length",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_length),
    },
    FunctionDef {
        name: "empty",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_empty),
    },
    FunctionDef {
        name: "contains",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_contains),
    },
    // array
    FunctionDef {
        name: "first",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_first),
    },
    FunctionDef {
        name: "last",
        arity: Arity::Exact(1),
        paxr_eval: Some(eval_last),
    },
    FunctionDef {
        name: "skip",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_skip),
    },
    FunctionDef {
        name: "take",
        arity: Arity::Exact(2),
        paxr_eval: Some(eval_take),
    },
    // Recognized but not paxr-implemented. These are real PA functions taken
    // from the published expression-function reference, so the decoder can
    // lower calls to them into pax source and the resolver's hints work, but
    // paxr can't simulate them locally -- users get the standard "skipping
    // unknown" notice. Arities come from each function's documented signature.
    //
    // `if` is deliberately absent: it is a pax statement keyword, so
    // `let x = if(a, b, c)` does not parse, and registering it would make the
    // decoder emit pax source that cannot compile. `items`, `outputs`, and
    // `variables` are absent because they are ACCESSORS, intercepted in
    // `render_call` before the generic-call path ever sees them.
    FunctionDef {
        name: "utcNow",
        arity: Arity::AtLeast(0),
        paxr_eval: None,
    },
    FunctionDef {
        name: "formatDateTime",
        arity: Arity::AtLeast(0),
        paxr_eval: None,
    },
    // string
    FunctionDef {
        name: "chunk",
        arity: Arity::Exact(2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "formatNumber",
        arity: Arity::Range(2, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "isFloat",
        arity: Arity::Range(1, 2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "isInt",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "nthIndexOf",
        arity: Arity::Exact(3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "slice",
        arity: Arity::Range(2, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "trimByteOrderMark",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    // collection
    FunctionDef {
        name: "intersection",
        arity: Arity::AtLeast(2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "reverse",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "sort",
        arity: Arity::Range(1, 2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "union",
        arity: Arity::AtLeast(2),
        paxr_eval: None,
    },
    // logical comparison
    FunctionDef {
        name: "strongEquals",
        arity: Arity::AtLeast(2),
        paxr_eval: None,
    },
    // conversion
    FunctionDef {
        name: "array",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "base64",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "base64ToBinary",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "base64ToJson",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "base64ToString",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "binary",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "dataUri",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "dataUriToBinary",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "dataUriToString",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "decimal",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "decodeBase64", // deprecated
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "decodeDataUri",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "decodeUriComponent",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "encodeBase64",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "encodeUriComponent",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "float",
        arity: Arity::Range(1, 2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "json",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "uriComponentToBinary",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "xml",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    // math
    FunctionDef {
        name: "pow",
        arity: Arity::Exact(2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "rand",
        arity: Arity::Exact(2),
        paxr_eval: None,
    },
    // date and time
    FunctionDef {
        name: "addDays",
        arity: Arity::Range(2, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "addHours",
        arity: Arity::Range(2, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "addMinutes",
        arity: Arity::Range(2, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "addSeconds",
        arity: Arity::Range(2, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "addToTime",
        arity: Arity::Range(3, 4),
        paxr_eval: None,
    },
    FunctionDef {
        name: "convertFromUtc",
        arity: Arity::Range(2, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "convertTimeZone",
        arity: Arity::Range(3, 4),
        paxr_eval: None,
    },
    FunctionDef {
        name: "convertToUtc",
        arity: Arity::Range(2, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "dateDifference",
        arity: Arity::Exact(2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "dayOfMonth",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "dayOfWeek",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "dayOfYear",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "formatTimeSpan",
        arity: Arity::Range(2, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "getFutureTime",
        arity: Arity::Range(2, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "getPastTime",
        arity: Arity::Range(2, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "parseDateTime",
        arity: Arity::Range(1, 3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "startOfDay",
        arity: Arity::Range(1, 2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "startOfHour",
        arity: Arity::Range(1, 2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "startOfMonth",
        arity: Arity::Range(1, 2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "subtractFromTime",
        arity: Arity::Range(3, 4),
        paxr_eval: None,
    },
    FunctionDef {
        name: "ticks",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    // workflow
    FunctionDef {
        name: "action",
        arity: Arity::Exact(0),
        paxr_eval: None,
    },
    FunctionDef {
        name: "formDataMultiValues",
        arity: Arity::Exact(2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "formDataValue",
        arity: Arity::Exact(2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "listCallbackUrl",
        arity: Arity::Exact(0),
        paxr_eval: None,
    },
    FunctionDef {
        name: "multipartBody",
        arity: Arity::Exact(2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "result",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "triggerFormDataMultiValues",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "triggerFormDataValue",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "triggerMultipartBody",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "workflow",
        arity: Arity::Exact(0),
        paxr_eval: None,
    },
    // uri parsing
    FunctionDef {
        name: "uriHost",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "uriPath",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "uriPathAndQuery",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "uriPort",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "uriQuery",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "uriScheme",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    // manipulation: json & xml
    FunctionDef {
        name: "addProperty",
        arity: Arity::Exact(3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "decodeXmlName",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "decodeXmlValue",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "encodeXmlName",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "encodeXmlValue",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "mergeObjects",
        arity: Arity::Exact(2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "removeProperty",
        arity: Arity::Exact(2),
        paxr_eval: None,
    },
    FunctionDef {
        name: "setProperty",
        arity: Arity::Exact(3),
        paxr_eval: None,
    },
    FunctionDef {
        name: "xpath",
        arity: Arity::Exact(2),
        paxr_eval: None,
    },
    // PA accessors -- recognized so the decoder can emit them as call forms
    // and the resolver's hints work, but paxr can't simulate them locally
    // (no trigger / connector context). Standard skip-notice on evaluation.
    // `iterationIndexes` is dispatched specially in the interpreter and
    // never reaches the registry's `paxr_eval` slot, so `None` is correct.
    FunctionDef {
        name: "triggerBody",
        arity: Arity::Exact(0),
        paxr_eval: None,
    },
    FunctionDef {
        name: "triggerOutputs",
        arity: Arity::Exact(0),
        paxr_eval: None,
    },
    FunctionDef {
        name: "trigger",
        arity: Arity::Exact(0),
        paxr_eval: None,
    },
    FunctionDef {
        name: "parameters",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "body",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "actions",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    FunctionDef {
        name: "iterationIndexes",
        arity: Arity::Exact(1),
        paxr_eval: None,
    },
    // Singular `item()` is PA's no-arg form for "current foreach element".
    // Decoder emits it as a generic call; future slices may collapse it to
    // the surrounding iterator's pax name.
    FunctionDef {
        name: "item",
        arity: Arity::Exact(0),
        paxr_eval: None,
    },
];

/// Linear lookup. Fine at this scale.
pub fn lookup(name: &str) -> Option<&'static FunctionDef> {
    FUNCTIONS.iter().find(|f| f.name == name)
}

// ========================================================================
// Helpers shared by multiple evaluators.
// ========================================================================

/// Two-int binop. Both args must coerce to int; otherwise Null.
fn binary_int_op<F: Fn(i64, i64) -> i64>(args: &[Value], f: F) -> Value {
    if let (Some(a), Some(b)) = (args[0].as_int(), args[1].as_int()) {
        Value::Int(f(a, b))
    } else {
        Value::Null
    }
}

/// Two-int comparison. Both args must coerce to int; otherwise Null.
fn binary_cmp_op<F: Fn(i64, i64) -> bool>(args: &[Value], f: F) -> Value {
    if let (Some(a), Some(b)) = (args[0].as_int(), args[1].as_int()) {
        Value::Bool(f(a, b))
    } else {
        Value::Null
    }
}

/// Variadic boolean fold for `and` / `or`. Zero args → null, one arg →
/// the arg as-is, two-or-more → fold left with `f`. Any non-boolean
/// argument short-circuits to null.
fn fold_bool<F: Fn(bool, bool) -> bool>(args: &[Value], f: F) -> Value {
    if args.is_empty() {
        return Value::Null;
    }
    let mut acc = match args[0].as_bool() {
        Some(b) => b,
        None => return Value::Null,
    };
    for v in &args[1..] {
        match v.as_bool() {
            Some(b) => acc = f(acc, b),
            None => return Value::Null,
        }
    }
    Value::Bool(acc)
}

/// Case-insensitive substring search. Character-based indexing (UTF-8
/// chars count as 1 each). `from_end = true` selects the last match.
/// Empty needle returns 0 to match PA's behavior.
fn index_of_ci(haystack: &Value, needle: &Value, from_end: bool) -> Value {
    let (Value::Str(h), Value::Str(n)) = (haystack, needle) else {
        return Value::Null;
    };
    if n.is_empty() {
        return Value::Int(0);
    }
    let h_lower = h.to_lowercase();
    let n_lower = n.to_lowercase();
    let byte_idx = if from_end {
        h_lower.rfind(&n_lower)
    } else {
        h_lower.find(&n_lower)
    };
    match byte_idx {
        Some(b) => {
            let char_idx = h_lower[..b].chars().count() as i64;
            Value::Int(char_idx)
        }
        None => Value::Int(-1),
    }
}

/// Case-insensitive prefix / suffix check. `is_start` selects starts-with.
fn string_boundary_ci(haystack: &Value, needle: &Value, is_start: bool) -> Value {
    let (Value::Str(h), Value::Str(n)) = (haystack, needle) else {
        return Value::Null;
    };
    let h = h.to_lowercase();
    let n = n.to_lowercase();
    Value::Bool(if is_start {
        h.starts_with(&n)
    } else {
        h.ends_with(&n)
    })
}

/// Polymorphic length: chars for strings, items for arrays, entries for
/// objects. Returns Null for other kinds.
fn length_of(v: &Value) -> Value {
    match v {
        Value::Str(s) => Value::Int(s.chars().count() as i64),
        Value::Array(items) => Value::Int(items.len() as i64),
        Value::Object(entries) => Value::Int(entries.len() as i64),
        _ => Value::Null,
    }
}

fn is_empty(v: &Value) -> Value {
    match v {
        Value::Str(s) => Value::Bool(s.is_empty()),
        Value::Array(items) => Value::Bool(items.is_empty()),
        Value::Object(entries) => Value::Bool(entries.is_empty()),
        _ => Value::Null,
    }
}

/// Membership across strings (case-sensitive substring), arrays
/// (structural equality of elements), and objects (key match by
/// stringified needle).
fn contains_of(haystack: &Value, needle: &Value) -> Value {
    match haystack {
        Value::Str(s) => {
            let n = needle.coerce_str();
            Value::Bool(s.contains(&n))
        }
        Value::Array(items) => Value::Bool(items.iter().any(|i| i.equals(needle))),
        Value::Object(entries) => {
            let n = needle.coerce_str();
            Value::Bool(entries.iter().any(|(k, _)| k == &n))
        }
        _ => Value::Null,
    }
}

/// `min(a, b, c, ...)` or `min([a, b, c])`. PA supports both forms.
fn min_or_max_int(args: &[Value], smallest: bool) -> Value {
    let nums: Option<Vec<i64>> = if args.len() == 1 {
        match &args[0] {
            Value::Array(items) => items.iter().map(Value::as_int).collect(),
            _ => args.iter().map(Value::as_int).collect(),
        }
    } else {
        args.iter().map(Value::as_int).collect()
    };
    match nums {
        Some(ns) if !ns.is_empty() => {
            let picked = if smallest {
                *ns.iter().min().unwrap()
            } else {
                *ns.iter().max().unwrap()
            };
            Value::Int(picked)
        }
        _ => Value::Null,
    }
}

/// RFC 3986 percent-encoding for PA's `uriComponent`. Unreserved chars
/// (ALPHA / DIGIT / `-` / `_` / `.` / `~`) pass through; everything else
/// including multi-byte UTF-8 gets `%XX` per byte. Matches the JavaScript
/// `encodeURIComponent` behavior PA uses under the hood.
fn uri_component_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Inverse of `uri_component_encode`. Returns None if the input contains
/// a malformed escape or decodes to invalid UTF-8.
fn uri_component_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hi = (bytes[i + 1] as char).to_digit(16)?;
            let lo = (bytes[i + 2] as char).to_digit(16)?;
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

// ========================================================================
// Per-function evaluators.
// ========================================================================

fn eval_add(args: &[Value]) -> Value {
    binary_int_op(args, i64::wrapping_add)
}
fn eval_sub(args: &[Value]) -> Value {
    binary_int_op(args, i64::wrapping_sub)
}
fn eval_mul(args: &[Value]) -> Value {
    binary_int_op(args, i64::wrapping_mul)
}

fn eval_div(args: &[Value]) -> Value {
    if let (Some(a), Some(b)) = (args[0].as_int(), args[1].as_int())
        && b != 0
    {
        return Value::Int(a / b);
    }
    Value::Null
}

fn eval_mod(args: &[Value]) -> Value {
    if let (Some(a), Some(b)) = (args[0].as_int(), args[1].as_int())
        && b != 0
    {
        return Value::Int(a.rem_euclid(b));
    }
    Value::Null
}

fn eval_min(args: &[Value]) -> Value {
    min_or_max_int(args, true)
}
fn eval_max(args: &[Value]) -> Value {
    min_or_max_int(args, false)
}

fn eval_range(args: &[Value]) -> Value {
    if let (Some(start), Some(count)) = (args[0].as_int(), args[1].as_int())
        && count >= 0
    {
        return Value::Array((0..count).map(|i| Value::Int(start + i)).collect());
    }
    Value::Null
}

fn eval_coalesce(args: &[Value]) -> Value {
    args.iter()
        .find(|v| !matches!(v, Value::Null))
        .cloned()
        .unwrap_or(Value::Null)
}

fn eval_equals(args: &[Value]) -> Value {
    Value::Bool(args[0].equals(&args[1]))
}

fn eval_less(args: &[Value]) -> Value {
    binary_cmp_op(args, |a, b| a < b)
}
fn eval_less_or_equals(args: &[Value]) -> Value {
    binary_cmp_op(args, |a, b| a <= b)
}
fn eval_greater(args: &[Value]) -> Value {
    binary_cmp_op(args, |a, b| a > b)
}
fn eval_greater_or_equals(args: &[Value]) -> Value {
    binary_cmp_op(args, |a, b| a >= b)
}

fn eval_not(args: &[Value]) -> Value {
    match args[0].as_bool() {
        Some(b) => Value::Bool(!b),
        None => Value::Null,
    }
}

fn eval_and(args: &[Value]) -> Value {
    fold_bool(args, |a, b| a && b)
}
fn eval_or(args: &[Value]) -> Value {
    fold_bool(args, |a, b| a || b)
}

fn eval_concat(args: &[Value]) -> Value {
    let mut s = String::new();
    for a in args {
        s.push_str(&a.coerce_str());
    }
    Value::Str(s)
}

fn eval_to_upper(args: &[Value]) -> Value {
    match &args[0] {
        Value::Str(s) => Value::Str(s.to_uppercase()),
        _ => Value::Null,
    }
}

fn eval_to_lower(args: &[Value]) -> Value {
    match &args[0] {
        Value::Str(s) => Value::Str(s.to_lowercase()),
        _ => Value::Null,
    }
}

fn eval_trim(args: &[Value]) -> Value {
    match &args[0] {
        Value::Str(s) => Value::Str(s.trim().to_string()),
        _ => Value::Null,
    }
}

fn eval_substring(args: &[Value]) -> Value {
    let Value::Str(s) = &args[0] else {
        return Value::Null;
    };
    let Some(start) = args[1].as_int() else {
        return Value::Null;
    };
    let chars: Vec<char> = s.chars().collect();
    let start = start.max(0) as usize;
    if start >= chars.len() {
        return Value::Str(String::new());
    }
    let end = if args.len() == 3 {
        match args[2].as_int() {
            Some(n) => (start + n.max(0) as usize).min(chars.len()),
            None => return Value::Null,
        }
    } else {
        chars.len()
    };
    Value::Str(chars[start..end].iter().collect())
}

fn eval_index_of(args: &[Value]) -> Value {
    index_of_ci(&args[0], &args[1], false)
}
fn eval_last_index_of(args: &[Value]) -> Value {
    index_of_ci(&args[0], &args[1], true)
}
fn eval_starts_with(args: &[Value]) -> Value {
    string_boundary_ci(&args[0], &args[1], true)
}
fn eval_ends_with(args: &[Value]) -> Value {
    string_boundary_ci(&args[0], &args[1], false)
}

fn eval_replace(args: &[Value]) -> Value {
    match (&args[0], &args[1], &args[2]) {
        (Value::Str(s), Value::Str(old), Value::Str(new)) if !old.is_empty() => {
            Value::Str(s.replace(old.as_str(), new))
        }
        _ => Value::Null,
    }
}

fn eval_split(args: &[Value]) -> Value {
    match (&args[0], &args[1]) {
        (Value::Str(s), Value::Str(delim)) if !delim.is_empty() => Value::Array(
            s.split(delim.as_str())
                .map(|p| Value::Str(p.to_string()))
                .collect(),
        ),
        _ => Value::Null,
    }
}

fn eval_join(args: &[Value]) -> Value {
    match (&args[0], &args[1]) {
        (Value::Array(items), Value::Str(delim)) => {
            let parts: Vec<String> = items.iter().map(Value::coerce_str).collect();
            Value::Str(parts.join(delim))
        }
        _ => Value::Null,
    }
}

fn eval_uri_component(args: &[Value]) -> Value {
    match &args[0] {
        Value::Str(s) => Value::Str(uri_component_encode(s)),
        _ => Value::Null,
    }
}

fn eval_uri_component_to_string(args: &[Value]) -> Value {
    match &args[0] {
        Value::Str(s) => uri_component_decode(s)
            .map(Value::Str)
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn eval_string(args: &[Value]) -> Value {
    Value::Str(args[0].coerce_str())
}

fn eval_int(args: &[Value]) -> Value {
    match &args[0] {
        Value::Int(n) => Value::Int(*n),
        Value::Str(s) => s
            .trim()
            .parse::<i64>()
            .map(Value::Int)
            .unwrap_or(Value::Null),
        Value::Bool(b) => Value::Int(if *b { 1 } else { 0 }),
        _ => Value::Null,
    }
}

fn eval_bool(args: &[Value]) -> Value {
    match &args[0] {
        Value::Bool(b) => Value::Bool(*b),
        Value::Int(n) => Value::Bool(*n != 0),
        Value::Str(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Value::Bool(true),
            "false" | "0" => Value::Bool(false),
            _ => Value::Null,
        },
        _ => Value::Null,
    }
}

fn eval_guid(_args: &[Value]) -> Value {
    Value::Str(Uuid::new_v4().to_string())
}
fn eval_create_array(args: &[Value]) -> Value {
    Value::Array(args.to_vec())
}

fn eval_length(args: &[Value]) -> Value {
    length_of(&args[0])
}
fn eval_empty(args: &[Value]) -> Value {
    is_empty(&args[0])
}
fn eval_contains(args: &[Value]) -> Value {
    contains_of(&args[0], &args[1])
}

fn eval_first(args: &[Value]) -> Value {
    match &args[0] {
        Value::Array(items) => items.first().cloned().unwrap_or(Value::Null),
        Value::Str(s) => s
            .chars()
            .next()
            .map(|c| Value::Str(c.to_string()))
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn eval_last(args: &[Value]) -> Value {
    match &args[0] {
        Value::Array(items) => items.last().cloned().unwrap_or(Value::Null),
        Value::Str(s) => s
            .chars()
            .next_back()
            .map(|c| Value::Str(c.to_string()))
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn eval_skip(args: &[Value]) -> Value {
    match (&args[0], args[1].as_int()) {
        (Value::Array(items), Some(n)) => {
            let n = n.max(0) as usize;
            Value::Array(items.iter().skip(n).cloned().collect())
        }
        _ => Value::Null,
    }
}

fn eval_take(args: &[Value]) -> Value {
    match (&args[0], args[1].as_int()) {
        (Value::Array(items), Some(n)) => {
            let n = n.max(0) as usize;
            Value::Array(items.iter().take(n).cloned().collect())
        }
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::parser;
    use chumsky::Parser as _;
    use chumsky::input::Input;

    /// Every registered name must be legal in pax call position. The decoder
    /// gates generic-call rendering on `is_known_function`, so a name that the
    /// registry admits but the parser rejects turns decode output into pax
    /// source that cannot compile. `if` is the live example: it is a pax
    /// statement keyword, so it stays out of the registry, and this test is
    /// what keeps the next such addition from landing unnoticed. Type-name
    /// keywords (`int`, `string`, `bool`, `float`, `array`) are fine here --
    /// they are only reserved in type position.
    #[test]
    fn every_registered_name_parses_in_call_position() {
        let mut rejected: Vec<&str> = Vec::new();
        for def in FUNCTIONS {
            let src = format!("let _probe = {}()", def.name);
            let Ok(tokens) = lexer::lexer().parse(src.as_str()).into_result() else {
                rejected.push(def.name);
                continue;
            };
            let parsed = parser::parser()
                .parse(
                    tokens
                        .as_slice()
                        .map((src.len()..src.len()).into(), |(t, s)| (t, s)),
                )
                .into_result();
            if parsed.is_err() {
                rejected.push(def.name);
            }
        }
        assert!(
            rejected.is_empty(),
            "registered function names that pax cannot parse as calls, so the \
             decoder would emit uncompilable source for them: {rejected:?}"
        );
    }

    /// `lookup` returns the first match, so a duplicate silently shadows
    /// whatever arity or evaluator came later in the table.
    #[test]
    fn registry_has_no_duplicate_names() {
        let mut seen = std::collections::BTreeSet::new();
        let dupes: Vec<&str> = FUNCTIONS
            .iter()
            .filter(|f| !seen.insert(f.name))
            .map(|f| f.name)
            .collect();
        assert!(dupes.is_empty(), "duplicate registry entries: {dupes:?}");
    }

    /// Accessors are intercepted in `render_call` before the generic-call
    /// path, so listing them in `FUNCTIONS` as well would create an entry
    /// that can never be reached through rendering.
    #[test]
    fn intercepted_accessors_are_not_also_registered() {
        for name in ["variables", "items", "outputs"] {
            assert!(
                lookup(name).is_none(),
                "`{name}` is intercepted in render_call; a FunctionDef entry \
                 for it is unreachable"
            );
        }
    }
}
