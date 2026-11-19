//! `SpecTest`, `OfficialSpecTest` and `CustomSpecTest`: the Mustache spec files, loaded from YAML as SnakeYAML
//! loads them, compiled with `emptyStringIsFalse(true).defaultValue("")` and a loader serving each test's
//! partials. Invalid delimiter specifications throw in JMustache, which the upstream test accepts.

use std::sync::Arc;

use java_compat::lang::{JError, JList, JMap, JValue, MapKind};
use serde_yaml::Value;

/// SnakeYAML's Java values: `LinkedHashMap`, `ArrayList`, `Integer`/`Long`, `Double`.
fn java(v: &Value) -> JValue {
    match v {
        Value::Null => JValue::Null,
        Value::Bool(b) => JValue::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i32::try_from(i).map(JValue::Int).unwrap_or(JValue::Long(i)),
            None => JValue::Double(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => JValue::str(s.as_str()),
        Value::Sequence(items) => JValue::List(JList::array_list(items.iter().map(java).collect())),
        Value::Mapping(m) => JValue::Map(JMap::from_entries(
            MapKind::Linked,
            m.iter().map(|(k, v)| (JValue::str(k.as_str().map(str::to_string).unwrap_or_else(|| serde_yaml::to_string(k).unwrap_or_default().trim().to_string())), java(v))),
        )),
        Value::Tagged(t) => java(&t.value),
    }
}

fn run(dir: &str, group: &str) -> (usize, Vec<String>) {
    let path = format!("{}/upstream/src/test/resources/{dir}/{group}.yml", env!("CARGO_MANIFEST_DIR"));
    let spec: Value = serde_yaml::from_str(&std::fs::read_to_string(&path).expect("spec file (git submodule update --init --recursive)")).unwrap();
    let mut failures = Vec::new();
    let tests = spec["tests"].as_sequence().unwrap();
    for test in tests {
        let name = test["name"].as_str().unwrap();
        let partials: Vec<(String, String)> = test
            .get("partials")
            .and_then(Value::as_mapping)
            .map(|m| m.iter().map(|(k, v)| (k.as_str().unwrap().to_string(), v.as_str().unwrap_or_default().to_string())).collect())
            .unwrap_or_default();
        // `SpecAwareTemplateLoader`: a missing partial is empty.
        let loader = move |n: &str| -> Result<String, JError> { Ok(partials.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()).unwrap_or_default()) };
        let compiler = jmustache::Compiler::new().empty_string_is_false(true).default_value("").with_loader(Arc::new(loader));
        let template = test["template"].as_str().unwrap();
        let expected = test["expected"].as_str().unwrap();
        let result = compiler.compile(template).and_then(|t| t.execute(java(&test["data"])));
        match result {
            Ok(out) if out == expected => {}
            Err(e) if e.message().is_some_and(|m| m.contains("Invalid delimiter")) => {}
            other => failures.push(format!("{dir}/{group}/{name}: expected {expected:?}, got {other:?}")),
        }
    }
    (tests.len(), failures)
}

fn suite(dir: &str, groups: &[&str]) {
    let mut total = 0;
    let mut failures = Vec::new();
    for g in groups {
        let (n, f) = run(dir, g);
        total += n;
        failures.extend(f);
    }
    assert!(failures.is_empty(), "{} of {total} failed:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn official_spec() {
    suite("specs/specs", &["comments", "delimiters", "interpolation", "inverted", "sections", "partials", "~inheritance"]);
}

#[test]
fn custom_spec() {
    suite("custom/specs", &["sections", "partials", "~inheritance"]);
}
