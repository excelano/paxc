//! Wraps paxc's compiled output in deployment-format artifacts.
//!
//! Currently supports the Power Automate "Import Package (Legacy)" format:
//! a flat zip with `manifest.json` at the root and the flow assets under
//! `Microsoft.Flow/flows/<package-guid>/`. The paxc emitter's JSON is the
//! raw Logic Apps workflow definition; the package wraps it in PA-specific
//! envelopes (flow resource, display name, connection references) and adds
//! the minor quirks PA expects (`$authentication` / `$connections`
//! parameters, schema wrapper on the manual trigger's `inputs`, lowercase
//! variable type names).
//!
//! The envelope shape was determined by round-tripping a real minimal flow
//! export from a tenant, not guessed -- see `examples/tour.pax` and the
//! packager tests for the artifact-matching invariants.

use crate::pa::emitter;
use crate::pa::{JsonError, ZipError};
use crate::resolver::ResolvedProgram;
use serde_json::{Map, Value, json};
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use uuid::Uuid;
use zip::{CompressionMethod, System, ZipWriter, write::SimpleFileOptions};

/// Output targets paxc can produce beyond the raw JSON.
#[derive(Debug, Clone, Copy)]
pub enum Target {
    /// Power Automate "Import Package (Legacy)" zip.
    PaLegacy,
}

#[derive(Debug)]
pub enum PackageError {
    Io(io::Error),
    Zip(ZipError),
    Json(JsonError),
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageError::Io(e) => write!(f, "io: {e}"),
            PackageError::Zip(e) => write!(f, "zip: {e}"),
            PackageError::Json(e) => write!(f, "json: {e}"),
        }
    }
}

impl std::error::Error for PackageError {}

impl From<io::Error> for PackageError {
    fn from(e: io::Error) -> Self {
        PackageError::Io(e)
    }
}

pub fn package(
    program: &ResolvedProgram,
    target: Target,
    name: &str,
    out_path: &Path,
) -> Result<(), PackageError> {
    match target {
        Target::PaLegacy => package_pa_legacy(program, name, out_path),
    }
}

fn package_pa_legacy(
    program: &ResolvedProgram,
    name: &str,
    out_path: &Path,
) -> Result<(), PackageError> {
    // Compile, then transform to PA-inner shape.
    let compiled = emitter::emit(program);
    let inner_def = transform_for_pa(&compiled);
    // Connection references come from `pa/connectionReferences.json` via
    // the resolver and end up at the top level of the compiled object.
    // Without forwarding them into the envelope, PA's importer rejects
    // every connector action ("API connection reference '<x>' could not
    // be found"). An absent file -> the existing empty default.
    let connection_references = compiled
        .get("connectionReferences")
        .cloned()
        .unwrap_or_else(|| json!({}));
    // Build per-connection-reference resource descriptors that the legacy
    // package wires together via apisMap, connectionsMap, and the root
    // manifest's `resources` block. PA's importer needs the full graph;
    // an empty connectionsMap fails with PackageFlowMissingConnectionMap.
    let conn_resources = build_connection_resources(&connection_references);

    let package_guid = Uuid::new_v4().to_string();
    let flow_guid = Uuid::new_v4().to_string();
    let telemetry_guid = Uuid::new_v4().to_string();
    // Timestamp is cosmetic; PA regenerates creator/time fields on import.
    // A static value keeps packages reproducible from paxc's perspective.
    let created_time = "2026-04-21T00:00:00.0000000Z";

    let root_manifest = build_root_manifest(
        name,
        &package_guid,
        &telemetry_guid,
        created_time,
        &conn_resources,
    );
    let inner_manifest = build_inner_manifest(&package_guid);
    let flow_def = build_flow_envelope(inner_def, name, &flow_guid, connection_references);

    let apis_map = build_apis_map(&conn_resources);
    let connections_map = build_connections_map(&conn_resources);

    let files: Vec<(String, Vec<u8>)> = vec![
        (
            "manifest.json".to_string(),
            serde_json::to_vec(&root_manifest).map_err(json_err)?,
        ),
        (
            "Microsoft.Flow/flows/manifest.json".to_string(),
            serde_json::to_vec(&inner_manifest).map_err(json_err)?,
        ),
        (
            format!("Microsoft.Flow/flows/{package_guid}/apisMap.json"),
            serde_json::to_vec(&apis_map).map_err(json_err)?,
        ),
        (
            format!("Microsoft.Flow/flows/{package_guid}/connectionsMap.json"),
            serde_json::to_vec(&connections_map).map_err(json_err)?,
        ),
        (
            format!("Microsoft.Flow/flows/{package_guid}/definition.json"),
            serde_json::to_vec(&flow_def).map_err(json_err)?,
        ),
    ];

    write_zip(out_path, &files)?;
    Ok(())
}

/// Transforms paxc's `{"definition": {...}}` output into the shape PA's
/// importer expects inside `properties.definition`. Rebuilds the object
/// in canonical key order: `$schema`, `contentVersion`, `parameters`,
/// `triggers`, `actions`.
fn transform_for_pa(compiled: &Value) -> Value {
    let old = compiled.get("definition").and_then(|v| v.as_object());
    let mut out = Map::new();

    if let Some(old) = old {
        if let Some(v) = old.get("$schema") {
            out.insert("$schema".to_string(), v.clone());
        }
        if let Some(v) = old.get("contentVersion") {
            out.insert("contentVersion".to_string(), v.clone());
        }
        // Parameters block: PA expects these even when unused.
        out.insert(
            "parameters".to_string(),
            json!({
                "$authentication": {"defaultValue": {}, "type": "SecureObject"},
                "$connections": {"defaultValue": {}, "type": "Object"}
            }),
        );
        if let Some(v) = old.get("triggers") {
            let mut triggers = v.clone();
            fix_manual_trigger_inputs(&mut triggers);
            // A connector trigger (SharePoint "when an item is created",
            // Outlook "when a new email arrives", Forms, Teams) needs the same
            // import fixups as a connector action: the importer wants
            // `host.connectionReferenceName`, and a trigger decoded from a real
            // export carries an `inputs.authentication` the importer rejects.
            // Triggers outside the `OpenApiConnection*` family — Recurrence,
            // the manual Request/Button handled just above — fall through
            // untouched.
            fix_connector_inputs(&mut triggers);
            out.insert("triggers".to_string(), triggers);
        }
        if let Some(v) = old.get("actions") {
            let mut actions = v.clone();
            lowercase_var_types(&mut actions);
            fix_connector_inputs(&mut actions);
            out.insert("actions".to_string(), actions);
        }
    }
    Value::Object(out)
}

/// PA's manual trigger expects `inputs: {schema: {...}}` even when empty,
/// not `inputs: {}` as paxc currently emits. Rewrites the matching shape.
fn fix_manual_trigger_inputs(triggers: &mut Value) {
    let Some(obj) = triggers.as_object_mut() else {
        return;
    };
    for (_, trig) in obj.iter_mut() {
        let Some(t) = trig.as_object_mut() else {
            continue;
        };
        let is_manual = t.get("type").and_then(|v| v.as_str()) == Some("Request")
            && t.get("kind").and_then(|v| v.as_str()) == Some("Button");
        if is_manual {
            t.insert(
                "inputs".to_string(),
                json!({"schema": {"type": "object", "properties": {}, "required": []}}),
            );
        }
    }
}

/// PA's designer exports variable types in lowercase (`integer`, `string`,
/// etc.). paxc's emitter uses the canonical Logic Apps capitalization. Both
/// forms probably work, but we match PA for safety. Recurses through the
/// same container set as `fix_connector_inputs` (Foreach, Scope, Until,
/// If/else, Switch cases + default) so a `var` declared inside any of them
/// gets the same normalization as one at the top level.
fn lowercase_var_types(actions: &mut Value) {
    let Some(obj) = actions.as_object_mut() else {
        return;
    };
    for (_, action) in obj.iter_mut() {
        let Some(a) = action.as_object_mut() else {
            continue;
        };
        let kind = a
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        match kind.as_str() {
            "InitializeVariable" => {
                if let Some(vars) = a
                    .get_mut("inputs")
                    .and_then(|i| i.get_mut("variables"))
                    .and_then(|v| v.as_array_mut())
                {
                    for var in vars {
                        if let Some(ty) = var.get_mut("type")
                            && let Some(s) = ty.as_str()
                        {
                            *ty = Value::String(s.to_lowercase());
                        }
                    }
                }
            }
            "Foreach" | "Scope" | "Until" => {
                if let Some(nested) = a.get_mut("actions") {
                    lowercase_var_types(nested);
                }
            }
            "If" => {
                if let Some(nested) = a.get_mut("actions") {
                    lowercase_var_types(nested);
                }
                if let Some(else_obj) = a.get_mut("else").and_then(|e| e.get_mut("actions")) {
                    lowercase_var_types(else_obj);
                }
            }
            "Switch" => {
                if let Some(cases) = a.get_mut("cases").and_then(|v| v.as_object_mut()) {
                    for (_, case) in cases.iter_mut() {
                        if let Some(nested) = case.get_mut("actions") {
                            lowercase_var_types(nested);
                        }
                    }
                }
                if let Some(default) = a.get_mut("default").and_then(|d| d.get_mut("actions")) {
                    lowercase_var_types(default);
                }
            }
            _ => {}
        }
    }
}

/// Per-connector import fixups. PA's exporter and importer disagree
/// on connector input shapes in two ways that consistently appear together:
///
/// 1. `inputs.authentication: "@parameters('$authentication')"` is exported
///    but rejected on import (`WorkflowRunActionInputsInvalidProperty`).
///    Auto-injected from `connectionReferences` at runtime, so redundant.
/// 2. The connection-reference key is exported under `inputs.host.connectionName`
///    (legacy "Peek code") or `inputs.host.connection` (current "Code view"),
///    but the importer wants `inputs.host.connectionReferenceName` (same VALUE,
///    different field name). Without it the importer fails with
///    `WorkflowRunActionInputsMissingProperty`.
///
/// Applies to the whole `OpenApiConnection*` family, matched by prefix rather
/// than enumerated: PA has at least three members — `OpenApiConnection` for a
/// plain connector call, `OpenApiConnectionWebhook` for one that registers a
/// callback (Approvals, Forms), and `OpenApiConnectionNotification` for a
/// push trigger (Outlook's "when a new email arrives") — and every one of them
/// carries the same `inputs.host` connection model these two fixups act on.
/// Enumerating the members is what let the third slip through unfixed
/// (#19). Recurses through container bodies to catch nested connectors, and
/// runs over the action map and the trigger map alike: a connector trigger
/// needs both fixups, and every other trigger type is ignored by the same
/// type test that ignores non-connector actions.
fn fix_connector_inputs(actions: &mut Value) {
    let Some(obj) = actions.as_object_mut() else {
        return;
    };
    for (_, action) in obj.iter_mut() {
        let Some(a) = action.as_object_mut() else {
            continue;
        };
        let kind = a
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        match kind.as_str() {
            k if k.starts_with("OpenApiConnection") => {
                if let Some(inputs) = a.get_mut("inputs").and_then(|v| v.as_object_mut()) {
                    inputs.remove("authentication");
                    if let Some(host) = inputs.get_mut("host").and_then(|v| v.as_object_mut()) {
                        // PA's import-time validator wants
                        // `host.connectionReferenceName`; PA's run-/save-time
                        // validator wants `host.connectionName`. The
                        // connection-reference key travels under a different
                        // field name depending on export vintage: the legacy
                        // "Peek code" carried `connectionName`, while the
                        // current designer's "Code view" emits `connection`.
                        // Read whichever is present, set both legacy names to
                        // that key so each validator is satisfied, and drop the
                        // modern `connection` field so only the legacy shape
                        // remains.
                        let conn = host
                            .get("connectionName")
                            .or_else(|| host.get("connectionReferenceName"))
                            .or_else(|| host.get("connection"))
                            .cloned();
                        if let Some(conn) = conn {
                            host.remove("connection");
                            host.insert("connectionName".to_string(), conn.clone());
                            host.insert("connectionReferenceName".to_string(), conn);
                        }
                    }
                }
            }
            "Foreach" | "Scope" | "Until" => {
                if let Some(nested) = a.get_mut("actions") {
                    fix_connector_inputs(nested);
                }
            }
            "If" => {
                if let Some(nested) = a.get_mut("actions") {
                    fix_connector_inputs(nested);
                }
                if let Some(else_obj) = a.get_mut("else").and_then(|e| e.get_mut("actions")) {
                    fix_connector_inputs(else_obj);
                }
            }
            "Switch" => {
                if let Some(cases) = a.get_mut("cases").and_then(|v| v.as_object_mut()) {
                    for (_, case) in cases.iter_mut() {
                        if let Some(nested) = case.get_mut("actions") {
                            fix_connector_inputs(nested);
                        }
                    }
                }
                if let Some(default) = a.get_mut("default").and_then(|d| d.get_mut("actions")) {
                    fix_connector_inputs(default);
                }
            }
            _ => {}
        }
    }
}

/// Per-connection-reference resource descriptor used to wire up the legacy
/// package's manifest, apisMap, and connectionsMap. Built from
/// `pa/connectionReferences.json` (forwarded through the compiled envelope).
struct ConnResource {
    /// Connection reference name (the key in connectionReferences and the
    /// label inside `host.connectionReferenceName`).
    ref_name: String,
    /// API path (e.g., `/providers/Microsoft.PowerApps/apis/shared_sharepointonline`).
    api_id: String,
    /// User-facing API display name (best-effort from `apiName`; PA fills
    /// in the canonical name during import). Cosmetic — not load-bearing.
    api_display_name: String,
    /// Resource GUID for the API entry in the root manifest.
    api_guid: String,
    /// Resource GUID for the connection entry in the root manifest.
    connection_guid: String,
}

/// Walk the connectionReferences map and synthesize a ConnResource for each
/// reference. Generates package-local GUIDs for the API and connection
/// resources; the import experience prompts the user to map each connection
/// to one in their tenant.
fn build_connection_resources(connection_references: &Value) -> Vec<ConnResource> {
    let Some(map) = connection_references.as_object() else {
        return Vec::new();
    };
    map.iter()
        .map(|(name, body)| {
            let api_id = body
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let api_name = body
                .get("apiName")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            // PA's display names for connectors are usually a friendly
            // capitalization of the apiName (`sharepointonline` →
            // `SharePoint`). We don't carry the friendly map; fall back to
            // the apiName itself, which the importer overrides with the
            // tenant's canonical name anyway.
            let api_display_name = if api_name.is_empty() {
                name.clone()
            } else {
                api_name
            };
            ConnResource {
                ref_name: name.clone(),
                api_id,
                api_display_name,
                api_guid: Uuid::new_v4().to_string(),
                connection_guid: Uuid::new_v4().to_string(),
            }
        })
        .collect()
}

fn build_apis_map(conn_resources: &[ConnResource]) -> Value {
    let mut m = Map::new();
    for r in conn_resources {
        m.insert(r.ref_name.clone(), Value::String(r.api_guid.clone()));
    }
    Value::Object(m)
}

fn build_connections_map(conn_resources: &[ConnResource]) -> Value {
    let mut m = Map::new();
    for r in conn_resources {
        m.insert(r.ref_name.clone(), Value::String(r.connection_guid.clone()));
    }
    Value::Object(m)
}

fn build_root_manifest(
    name: &str,
    package_guid: &str,
    telemetry_guid: &str,
    created_time: &str,
    conn_resources: &[ConnResource],
) -> Value {
    let mut resources = Map::new();

    // Flow resource depends on every API and connection it uses.
    let mut flow_depends_on: Vec<Value> = Vec::new();
    for r in conn_resources {
        flow_depends_on.push(Value::String(r.api_guid.clone()));
        flow_depends_on.push(Value::String(r.connection_guid.clone()));
    }
    resources.insert(
        package_guid.to_string(),
        json!({
            "type": "Microsoft.Flow/flows",
            // Default to "Update" because round-tripping an existing flow
            // (decode → edit → re-import to overwrite) is the primary
            // forward direction for paxc. Users importing a fresh flow can
            // pick "Create as new" in the dialog instead. `creationType`
            // lists the full set of options the user can choose from.
            "suggestedCreationType": "Update",
            "creationType": "Existing, New, Update",
            "details": {"displayName": name},
            "configurableBy": "User",
            "hierarchy": "Root",
            "dependsOn": flow_depends_on
        }),
    );

    // For each connection reference, declare two resources: the API and the
    // user-mapped connection. The connection depends on its API.
    for r in conn_resources {
        resources.insert(
            r.api_guid.clone(),
            json!({
                "id": r.api_id,
                "name": r.ref_name,
                "type": "Microsoft.PowerApps/apis",
                "suggestedCreationType": "Existing",
                "details": {"displayName": r.api_display_name},
                "configurableBy": "System",
                "hierarchy": "Child",
                "dependsOn": []
            }),
        );
        resources.insert(
            r.connection_guid.clone(),
            json!({
                "type": "Microsoft.PowerApps/apis/connections",
                "suggestedCreationType": "Existing",
                "creationType": "Existing",
                "details": {"displayName": r.ref_name},
                "configurableBy": "User",
                "hierarchy": "Child",
                "dependsOn": [r.api_guid.clone()]
            }),
        );
    }

    json!({
        "schema": "1.0",
        "details": {
            "displayName": name,
            "description": "",
            "createdTime": created_time,
            "packageTelemetryId": telemetry_guid,
            "creator": "N/A",
            "sourceEnvironment": ""
        },
        "resources": resources
    })
}

fn build_inner_manifest(package_guid: &str) -> Value {
    json!({
        "packageSchemaVersion": "1.0",
        "flowAssets": {"assetPaths": [package_guid]}
    })
}

fn build_flow_envelope(
    inner_def: Value,
    name: &str,
    flow_guid: &str,
    connection_references: Value,
) -> Value {
    json!({
        "name": flow_guid,
        "id": format!("/providers/Microsoft.Flow/flows/{flow_guid}"),
        "type": "Microsoft.Flow/flows",
        "properties": {
            "apiId": "/providers/Microsoft.PowerApps/apis/shared_logicflows",
            "displayName": name,
            "definition": inner_def,
            "connectionReferences": connection_references,
            "flowFailureAlertSubscribed": false,
            "isManaged": false
        }
    })
}

fn json_err(e: serde_json::Error) -> PackageError {
    PackageError::Json(JsonError::new(e))
}

fn zip_err(e: zip::result::ZipError) -> PackageError {
    PackageError::Zip(ZipError::new(e))
}

fn write_zip(out_path: &Path, files: &[(String, Vec<u8>)]) -> Result<(), PackageError> {
    let file = File::create(out_path)?;
    let mut zip = ZipWriter::new(file);
    // `.system(Unix)` is set explicitly rather than left to default. zip 2
    // hardcoded Unix for every entry it wrote; zip 8 derives it from the build
    // host, so a Windows-hosted paxc would stamp `System::Dos` and pick up the
    // DOS attribute bits. The package is a build artifact whose bytes are
    // matched against real tenant exports, so it has to come out the same
    // wherever paxc was compiled. CI runs on Linux only and cannot see this.
    let options: SimpleFileOptions = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .system(System::Unix)
        .unix_permissions(0o644);

    for (path, data) in files {
        zip.start_file(path, options).map_err(zip_err)?;
        zip.write_all(data)?;
    }
    zip.finish().map_err(zip_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercase_var_types_recurses_into_scope_until_switch() {
        // var declared inside a Scope, Until, or Switch case body must get
        // its type lowercased like one at the workflow root would.
        let mut actions = json!({
            "Scope_outer": {
                "type": "Scope",
                "actions": {
                    "Initialize_a": {
                        "type": "InitializeVariable",
                        "inputs": { "variables": [{"name": "a", "type": "Integer"}] }
                    }
                }
            },
            "Until_loop": {
                "type": "Until",
                "actions": {
                    "Initialize_b": {
                        "type": "InitializeVariable",
                        "inputs": { "variables": [{"name": "b", "type": "String"}] }
                    }
                }
            },
            "Switch_route": {
                "type": "Switch",
                "cases": {
                    "c1": {
                        "actions": {
                            "Initialize_c": {
                                "type": "InitializeVariable",
                                "inputs": { "variables": [{"name": "c", "type": "Boolean"}] }
                            }
                        }
                    }
                },
                "default": {
                    "actions": {
                        "Initialize_d": {
                            "type": "InitializeVariable",
                            "inputs": { "variables": [{"name": "d", "type": "Array"}] }
                        }
                    }
                }
            }
        });
        lowercase_var_types(&mut actions);
        assert_eq!(
            actions["Scope_outer"]["actions"]["Initialize_a"]["inputs"]["variables"][0]["type"],
            "integer"
        );
        assert_eq!(
            actions["Until_loop"]["actions"]["Initialize_b"]["inputs"]["variables"][0]["type"],
            "string"
        );
        assert_eq!(
            actions["Switch_route"]["cases"]["c1"]["actions"]["Initialize_c"]["inputs"]["variables"]
                [0]["type"],
            "boolean"
        );
        assert_eq!(
            actions["Switch_route"]["default"]["actions"]["Initialize_d"]["inputs"]["variables"][0]
                ["type"],
            "array"
        );
    }

    #[test]
    fn fix_connector_inputs_removes_authentication_field() {
        let mut actions = json!({
            "Get_items": {
                "type": "OpenApiConnection",
                "inputs": {
                    "parameters": { "$top": 5 },
                    "host": { "apiId": "x" },
                    "authentication": "@parameters('$authentication')"
                }
            }
        });
        fix_connector_inputs(&mut actions);
        let inputs = &actions["Get_items"]["inputs"];
        assert!(inputs.get("authentication").is_none());
        assert!(inputs.get("parameters").is_some());
        assert!(inputs.get("host").is_some());
    }

    #[test]
    fn fix_connector_inputs_emits_both_connection_name_fields() {
        // PA's import-time and save-time validators want different field
        // names for the same value. We set both.
        let mut actions = json!({
            "Get_items": {
                "type": "OpenApiConnection",
                "inputs": {
                    "host": {
                        "apiId": "x",
                        "connectionName": "shared_sharepointonline",
                        "operationId": "GetItems"
                    }
                }
            }
        });
        fix_connector_inputs(&mut actions);
        let host = &actions["Get_items"]["inputs"]["host"];
        assert_eq!(
            host.get("connectionName").and_then(|v| v.as_str()),
            Some("shared_sharepointonline")
        );
        assert_eq!(
            host.get("connectionReferenceName").and_then(|v| v.as_str()),
            Some("shared_sharepointonline")
        );
    }

    #[test]
    fn fix_connector_inputs_recurses_through_containers() {
        let mut actions = json!({
            "Apply_to_each": {
                "type": "Foreach",
                "actions": {
                    "If_check": {
                        "type": "If",
                        "actions": {
                            "Inner_call": {
                                "type": "OpenApiConnectionWebhook",
                                "inputs": {
                                    "host": {},
                                    "authentication": "@parameters('$authentication')"
                                }
                            }
                        },
                        "else": {
                            "actions": {
                                "Else_call": {
                                    "type": "OpenApiConnection",
                                    "inputs": {
                                        "host": {},
                                        "authentication": "@parameters('$authentication')"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        fix_connector_inputs(&mut actions);
        let inner =
            &actions["Apply_to_each"]["actions"]["If_check"]["actions"]["Inner_call"]["inputs"];
        let else_call = &actions["Apply_to_each"]["actions"]["If_check"]["else"]["actions"]["Else_call"]
            ["inputs"];
        assert!(inner.get("authentication").is_none());
        assert!(else_call.get("authentication").is_none());
    }

    #[test]
    fn fix_connector_inputs_recurses_through_switch_and_until() {
        // Switch cases, Switch default, and Until bodies are part of the
        // container set fix_connector_inputs handles. A connector buried in
        // any of them must still get its authentication stripped and host
        // fields normalized.
        let mut actions = json!({
            "Switch_route": {
                "type": "Switch",
                "cases": {
                    "case1": {
                        "actions": {
                            "Case_call": {
                                "type": "OpenApiConnection",
                                "inputs": {
                                    "host": { "connectionName": "case_conn" },
                                    "authentication": "@parameters('$authentication')"
                                }
                            }
                        }
                    }
                },
                "default": {
                    "actions": {
                        "Default_call": {
                            "type": "OpenApiConnection",
                            "inputs": {
                                "host": { "connectionName": "default_conn" },
                                "authentication": "@parameters('$authentication')"
                            }
                        }
                    }
                }
            },
            "Until_loop": {
                "type": "Until",
                "actions": {
                    "Until_call": {
                        "type": "OpenApiConnectionWebhook",
                        "inputs": {
                            "host": { "connectionName": "until_conn" },
                            "authentication": "@parameters('$authentication')"
                        }
                    }
                }
            }
        });
        fix_connector_inputs(&mut actions);
        let case_call =
            &actions["Switch_route"]["cases"]["case1"]["actions"]["Case_call"]["inputs"];
        let default_call = &actions["Switch_route"]["default"]["actions"]["Default_call"]["inputs"];
        let until_call = &actions["Until_loop"]["actions"]["Until_call"]["inputs"];
        for inp in [case_call, default_call, until_call] {
            assert!(inp.get("authentication").is_none());
            assert!(inp["host"].get("connectionName").is_some());
            assert!(inp["host"].get("connectionReferenceName").is_some());
        }
    }

    #[test]
    fn fix_connector_inputs_normalizes_modern_connection_field() {
        // The current designer's "Code view" emits the connection-reference key
        // as `host.connection` (not `connectionName`) and omits the
        // authentication line entirely. paxc must still recover the key and
        // produce the legacy pair the importer requires, dropping the modern
        // `connection` field so only `connectionName` / `connectionReferenceName`
        // remain.
        let mut actions = json!({
            "Send_an_email": {
                "type": "OpenApiConnection",
                "inputs": {
                    "parameters": { "emailMessage/To": "you@example.com" },
                    "host": {
                        "apiId": "/providers/Microsoft.PowerApps/apis/shared_office365",
                        "connection": "shared_office365",
                        "operationId": "SendEmailV2"
                    }
                }
            }
        });
        fix_connector_inputs(&mut actions);
        let host = &actions["Send_an_email"]["inputs"]["host"];
        assert!(host.get("connection").is_none());
        assert_eq!(host["connectionName"], "shared_office365");
        assert_eq!(host["connectionReferenceName"], "shared_office365");
    }

    #[test]
    fn connector_trigger_gains_connection_reference_name() {
        // Most real business flows are connector-triggered. The trigger map
        // goes through the same fixup pass as the action map, so the importer
        // finds the `connectionReferenceName` it requires.
        let mut triggers = json!({
            "When_an_item_is_created": {
                "type": "OpenApiConnectionWebhook",
                "inputs": {
                    "host": {
                        "connectionName": "shared_sharepointonline",
                        "operationId": "GetOnNewItems",
                        "apiId": "/providers/Microsoft.PowerApps/apis/shared_sharepointonline"
                    },
                    "parameters": { "table": "list-guid" }
                }
            }
        });
        fix_connector_inputs(&mut triggers);
        let host = &triggers["When_an_item_is_created"]["inputs"]["host"];
        assert_eq!(host["connectionName"], "shared_sharepointonline");
        assert_eq!(host["connectionReferenceName"], "shared_sharepointonline");
    }

    #[test]
    fn connector_trigger_loses_authentication() {
        // A trigger decoded from a real export carries the authentication line
        // PA's importer rejects with WorkflowRunActionInputsInvalidProperty.
        let mut triggers = json!({
            "When_a_new_email_arrives": {
                "type": "OpenApiConnectionWebhook",
                "inputs": {
                    "host": { "connectionName": "shared_office365" },
                    "authentication": "@parameters('$authentication')"
                }
            }
        });
        fix_connector_inputs(&mut triggers);
        let inputs = &triggers["When_a_new_email_arrives"]["inputs"];
        assert!(inputs.get("authentication").is_none());
    }

    #[test]
    fn notification_trigger_gets_the_connector_fixups() {
        // Outlook's "when a new email arrives (V3)" is neither a plain
        // OpenApiConnection nor a Webhook but a third type, and enumerating
        // the first two left it unfixed (#19). Shape taken from a
        // Microsoft-published flow definition.
        let mut triggers = json!({
            "When_a_new_email_arrives_V3": {
                "type": "OpenApiConnectionNotification",
                "inputs": {
                    "host": {
                        "connectionName": "shared_office365",
                        "operationId": "OnNewEmailV3",
                        "apiId": "/providers/Microsoft.PowerApps/apis/shared_office365"
                    },
                    "parameters": { "importance": "Any" },
                    "authentication": "@parameters('$authentication')"
                },
                "splitOn": "@triggerOutputs()?['body/value']"
            }
        });
        fix_connector_inputs(&mut triggers);
        let trigger = &triggers["When_a_new_email_arrives_V3"];
        assert!(trigger["inputs"].get("authentication").is_none());
        assert_eq!(
            trigger["inputs"]["host"]["connectionReferenceName"],
            "shared_office365"
        );
        // Everything outside `inputs` is the trigger's own business.
        assert_eq!(trigger["splitOn"], "@triggerOutputs()?['body/value']");
    }

    #[test]
    fn a_type_that_merely_contains_connection_is_not_matched() {
        // The family test is a prefix, not a substring: `ApiConnection` is
        // Logic Apps' own older shape, outside what paxc targets, and it must
        // not be rewritten just because its name overlaps.
        let mut actions = json!({
            "Legacy": {
                "type": "ApiConnection",
                "inputs": {
                    "host": { "connection": { "name": "@parameters('$connections')" } },
                    "authentication": "@parameters('$authentication')"
                }
            }
        });
        let snapshot = actions.clone();
        fix_connector_inputs(&mut actions);
        assert_eq!(actions, snapshot);
    }

    #[test]
    fn recurrence_trigger_is_untouched_by_connector_fixups() {
        // The connector pass now runs over triggers, so the non-connector
        // trigger types have to come through byte-identical.
        let mut triggers = json!({
            "Recurrence": {
                "type": "Recurrence",
                "recurrence": { "frequency": "Day", "interval": 1 }
            }
        });
        let snapshot = triggers.clone();
        fix_connector_inputs(&mut triggers);
        assert_eq!(triggers, snapshot);
    }

    #[test]
    fn manual_trigger_survives_both_trigger_passes() {
        // fix_manual_trigger_inputs rewrites the manual trigger's inputs, then
        // fix_connector_inputs runs over the same map. The Request/Button type
        // is not a connector, so the schema block must still be there after.
        let mut triggers = json!({
            "manual": { "type": "Request", "kind": "Button", "inputs": {} }
        });
        fix_manual_trigger_inputs(&mut triggers);
        fix_connector_inputs(&mut triggers);
        assert_eq!(triggers["manual"]["inputs"]["schema"]["type"], "object");
    }

    #[test]
    fn fix_connector_inputs_leaves_non_connectors_alone() {
        // Variable / Compose actions don't have inputs.authentication, but
        // the pass should be a no-op even on hypothetical other types.
        let mut actions = json!({
            "Initialize_x": {
                "type": "InitializeVariable",
                "inputs": { "variables": [{ "name": "x", "type": "Integer" }] }
            },
            "Compose_y": {
                "type": "Compose",
                "inputs": "hello"
            }
        });
        let snapshot = actions.clone();
        fix_connector_inputs(&mut actions);
        assert_eq!(actions, snapshot);
    }

    /// Read the `system` byte out of every central directory header.
    ///
    /// Walks the central directory properly rather than scanning for the
    /// signature, since compressed payloads can contain those four bytes.
    fn central_directory_host_bytes(zip_bytes: &[u8]) -> Vec<u8> {
        let u16_at = |i: usize| u16::from_le_bytes([zip_bytes[i], zip_bytes[i + 1]]) as usize;
        let u32_at = |i: usize| {
            u32::from_le_bytes([
                zip_bytes[i],
                zip_bytes[i + 1],
                zip_bytes[i + 2],
                zip_bytes[i + 3],
            ]) as usize
        };
        let eocd = (0..zip_bytes.len().saturating_sub(21))
            .rev()
            .find(|&i| zip_bytes[i..i + 4] == *b"PK\x05\x06")
            .expect("no end-of-central-directory record");
        let count = u16_at(eocd + 10);
        let mut at = u32_at(eocd + 16);
        let mut hosts = Vec::with_capacity(count);
        for _ in 0..count {
            assert_eq!(
                zip_bytes[at..at + 4],
                *b"PK\x01\x02",
                "central directory entry not where the record said"
            );
            // Offset 4 is `version made by`: low byte the spec version, high
            // byte the host system.
            hosts.push(zip_bytes[at + 5]);
            at += 46 + u16_at(at + 28) + u16_at(at + 30) + u16_at(at + 32);
        }
        hosts
    }

    #[test]
    fn every_entry_is_written_with_the_unix_host_byte() {
        // zip 2 hardcoded Unix for every entry it wrote. zip 8 takes it from
        // the build host unless told otherwise, so without an explicit
        // `.system(Unix)` a Windows-built paxc stamps `System::Dos` here and
        // the package stops being identical across build hosts.
        //
        // Note what this test can and cannot do. Run on Linux it cannot tell
        // an explicit `.system(Unix)` from the host default, which agrees --
        // so it would not have caught the regression it exists to prevent.
        // What it does is pin the invariant on whatever host runs it, which
        // makes it a real check on a Windows or macOS developer machine and
        // on any future runner, and it fails everywhere if the call is
        // changed rather than dropped.
        let dir = std::env::temp_dir().join(format!("paxc-hostbyte-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("hostbyte.zip");
        let files = vec![
            ("manifest.json".to_string(), b"{}".to_vec()),
            (
                "Microsoft.Flow/flows/manifest.json".to_string(),
                b"[]".to_vec(),
            ),
        ];
        write_zip(&out, &files).expect("write_zip failed");

        let hosts = central_directory_host_bytes(&std::fs::read(&out).unwrap());
        assert_eq!(hosts.len(), files.len(), "one entry per file");
        for host in hosts {
            assert_eq!(host, System::Unix as u8, "entry not written as Unix-hosted");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
