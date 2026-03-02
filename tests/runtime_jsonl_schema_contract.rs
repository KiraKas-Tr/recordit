use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn schema_path() -> PathBuf {
    project_root().join("contracts/runtime-jsonl.schema.v1.json")
}

fn frozen_matrix_path() -> PathBuf {
    project_root().join("artifacts/validation/bd-1qfx.golden-artifact-matrix.json")
}

fn expected_event_types() -> BTreeSet<&'static str> {
    [
        "partial",
        "final",
        "llm_final",
        "reconciled_final",
        "vad_boundary",
        "mode_degradation",
        "trust_notice",
        "lifecycle_phase",
        "reconciliation_matrix",
        "asr_worker_pool",
        "chunk_queue",
        "cleanup_queue",
    ]
    .into_iter()
    .collect()
}

fn read_json(path: &PathBuf) -> Value {
    let raw = fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", path.display());
    });
    serde_json::from_str(&raw).unwrap_or_else(|err| {
        panic!("failed to parse {} as JSON: {err}", path.display());
    })
}

fn as_object<'a>(value: &'a Value, ctx: &str) -> &'a Map<String, Value> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("expected object for {ctx}"))
}

fn schema_event_shapes(schema: &Value) -> &Map<String, Value> {
    as_object(
        schema
            .get("$defs")
            .and_then(|v| v.get("eventShapes"))
            .expect("missing $defs.eventShapes in runtime JSONL schema"),
        "$defs.eventShapes",
    )
}

fn type_matches(value: &Value, type_name: &str) -> bool {
    match type_name {
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.as_f64().is_some(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        _ => false,
    }
}

fn validate_shape(event: &Map<String, Value>, shape: &Map<String, Value>, ctx: &str) {
    let required = shape
        .get("required")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("missing required[] for {ctx}"));
    for field in required {
        let field_name = field
            .as_str()
            .unwrap_or_else(|| panic!("non-string required field in {ctx}"));
        assert!(
            event.contains_key(field_name),
            "missing required field `{field_name}` for {ctx}"
        );
    }

    let properties = as_object(
        shape
            .get("properties")
            .unwrap_or_else(|| panic!("missing properties for {ctx}")),
        &format!("{ctx}.properties"),
    );

    for (field_name, field_schema) in properties {
        let Some(value) = event.get(field_name) else {
            continue;
        };

        if let Some(expected_const) = field_schema.get("const") {
            assert_eq!(
                value, expected_const,
                "const mismatch for field `{field_name}` in {ctx}"
            );
        }

        if let Some(enum_values) = field_schema.get("enum").and_then(|v| v.as_array()) {
            assert!(
                enum_values.iter().any(|entry| entry == value),
                "enum mismatch for field `{field_name}` in {ctx}: value={value}"
            );
        }

        if let Some(type_schema) = field_schema.get("type") {
            let matches_type = if let Some(type_name) = type_schema.as_str() {
                type_matches(value, type_name)
            } else if let Some(type_names) = type_schema.as_array() {
                type_names
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|type_name| type_matches(value, type_name))
            } else {
                false
            };
            assert!(
                matches_type,
                "type mismatch for field `{field_name}` in {ctx}: value={value}"
            );
        }

        if let Some(minimum) = field_schema.get("minimum").and_then(Value::as_f64) {
            let actual = value
                .as_f64()
                .unwrap_or_else(|| panic!("non-number field `{field_name}` in {ctx}"));
            assert!(
                actual >= minimum,
                "minimum violation for field `{field_name}` in {ctx}: {actual} < {minimum}"
            );
        }

        if let Some(maximum) = field_schema.get("maximum").and_then(Value::as_f64) {
            let actual = value
                .as_f64()
                .unwrap_or_else(|| panic!("non-number field `{field_name}` in {ctx}"));
            assert!(
                actual <= maximum,
                "maximum violation for field `{field_name}` in {ctx}: {actual} > {maximum}"
            );
        }
    }
}

fn runtime_jsonl_paths_from_matrix() -> Vec<PathBuf> {
    let matrix = read_json(&frozen_matrix_path());
    let mut paths = BTreeSet::new();

    if let Some(rows) = matrix.get("matrix").and_then(Value::as_array) {
        for row in rows {
            if let Some(path) = row.get("jsonl_path").and_then(Value::as_str) {
                paths.insert(project_root().join(path));
            }
        }
    }

    if let Some(rows) = matrix
        .get("trust_degradation_code_examples")
        .and_then(Value::as_array)
    {
        for row in rows {
            if let Some(path) = row.get("jsonl_path").and_then(Value::as_str) {
                let absolute = project_root().join(path);
                if absolute.is_file() {
                    paths.insert(absolute);
                }
            }
        }
    }

    paths.into_iter().collect()
}

#[test]
fn runtime_jsonl_schema_declares_expected_event_shapes() {
    let schema = read_json(&schema_path());
    let shapes = schema_event_shapes(&schema);

    let declared: BTreeSet<&str> = shapes.keys().map(String::as_str).collect();
    let expected = expected_event_types();
    assert_eq!(declared, expected, "schema event shape set drifted");

    let refs: BTreeSet<&str> = schema
        .get("oneOf")
        .and_then(Value::as_array)
        .expect("schema oneOf missing")
        .iter()
        .filter_map(|entry| entry.get("$ref").and_then(Value::as_str))
        .collect();
    let expected_refs: BTreeSet<String> = expected
        .into_iter()
        .map(|event_type| format!("#/$defs/eventShapes/{event_type}"))
        .collect();
    let expected_refs: BTreeSet<&str> = expected_refs.iter().map(String::as_str).collect();
    assert_eq!(refs, expected_refs, "schema oneOf refs drifted");
}

#[test]
fn frozen_runtime_jsonl_lines_conform_to_schema_shapes() {
    let schema = read_json(&schema_path());
    let shapes = schema_event_shapes(&schema);
    let paths = runtime_jsonl_paths_from_matrix();

    assert!(!paths.is_empty(), "no runtime JSONL fixture paths found");

    for path in paths {
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for (line_index, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let parsed: Value = serde_json::from_str(line).unwrap_or_else(|err| {
                panic!(
                    "failed to parse JSONL line {} in {}: {err}",
                    line_index + 1,
                    path.display()
                )
            });
            let event_obj = as_object(
                &parsed,
                &format!("{} line {}", path.display(), line_index + 1),
            );
            let event_type = event_obj
                .get("event_type")
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    panic!(
                        "missing string event_type at {} line {}",
                        path.display(),
                        line_index + 1
                    )
                });
            let shape = as_object(
                shapes.get(event_type).unwrap_or_else(|| {
                    panic!(
                        "schema missing event shape `{event_type}` for {} line {}",
                        path.display(),
                        line_index + 1
                    )
                }),
                &format!("shape `{event_type}`"),
            );
            validate_shape(
                event_obj,
                shape,
                &format!("{} line {} ({event_type})", path.display(), line_index + 1),
            );
        }
    }
}
