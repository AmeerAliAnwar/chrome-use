//! Lossless agent-configuration edits with semantic readback.
//!
//! Parsers and editors remain native. User configuration is never evaluated as
//! code, and parse errors never include source snippets (which may hold keys).
//! A patch is accepted only when the resulting full document equals the
//! requested semantic change, including all unrelated values.

use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use jsonc_parser::ParseOptions;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::str::FromStr;
use toml_edit::{DocumentMut, Item, Table, TableLike};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Format {
    Json,
    Jsonc,
    Toml,
    Yaml,
    Env,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub(super) enum Patch {
    Set { path: Vec<String>, value: Value },
    Remove { path: Vec<String> },
}

impl Patch {
    pub(super) fn path(&self) -> &[String] {
        match self {
            Self::Set { path, .. } | Self::Remove { path } => path,
        }
    }
    pub(super) fn value(&self) -> Option<&Value> {
        match self {
            Self::Set { value, .. } => Some(value),
            Self::Remove { .. } => None,
        }
    }
}

fn options(format: Format) -> ParseOptions {
    ParseOptions {
        allow_comments: matches!(format, Format::Jsonc),
        allow_loose_object_property_names: false,
        allow_trailing_commas: matches!(format, Format::Jsonc),
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
        allow_bare_decimal_point_numbers: false,
        allow_non_finite_numbers: false,
        allow_extended_string_escapes: false,
    }
}

fn text(raw: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(raw))
        .map_err(|_| "Configuration is not UTF-8".into())
}

pub(super) fn default_bytes(format: Format) -> &'static [u8] {
    match format {
        Format::Json | Format::Jsonc | Format::Yaml => b"{}\n",
        Format::Toml | Format::Env => b"",
    }
}

pub(super) fn parse(raw: &[u8], format: Format) -> Result<Value, String> {
    let source = text(raw)?;
    let value = match format {
        Format::Json | Format::Jsonc => {
            let opts = options(format);
            let root =
                CstRootNode::parse(source, &opts).map_err(|_| "Invalid JSON configuration")?;
            check_json_duplicates(
                &root
                    .object_value()
                    .ok_or("Configuration root must be an object")?,
            )?;
            jsonc_parser::parse_to_serde_value::<Value>(source, &opts)
                .map_err(|_| "Invalid JSON configuration")?
        }
        Format::Toml => {
            let doc = source
                .parse::<DocumentMut>()
                .map_err(|_| "Invalid TOML configuration")?;
            toml_table(doc.as_table())?
        }
        Format::Yaml => {
            // This parser rejects duplicate keys and more than one document.
            let value: Value = serde_yaml::from_str(source)
                .map_err(|_| "Invalid or unsupported YAML configuration")?;
            let file =
                yaml_edit::YamlFile::from_str(source).map_err(|_| "Invalid YAML configuration")?;
            if file.documents().count() != 1 {
                return Err("Configuration must contain exactly one YAML document".into());
            }
            value
        }
        Format::Env => Value::Object(
            env_entries(source)?
                .into_iter()
                .map(|e| (e.key, Value::String(e.value)))
                .collect(),
        ),
    };
    if !value.is_object() {
        return Err("Configuration root must be an object".into());
    }
    Ok(value)
}

fn check_json_duplicates(object: &CstObject) -> Result<(), String> {
    let mut names = HashSet::new();
    for prop in object.properties() {
        let name = prop.decoded_name().ok_or("Invalid JSON property name")?;
        if !names.insert(name) {
            return Err("Duplicate JSON configuration keys".into());
        }
        if let Some(value) = prop.value() {
            check_json_node(&value)?;
        }
    }
    Ok(())
}

fn check_json_node(node: &jsonc_parser::cst::CstNode) -> Result<(), String> {
    if let Some(object) = node.as_object() {
        check_json_duplicates(&object)?;
    }
    if let Some(array) = node.as_array() {
        for node in array.elements() {
            check_json_node(&node)?;
        }
    }
    Ok(())
}

fn toml_table(table: &dyn TableLike) -> Result<Value, String> {
    table
        .iter()
        .filter(|(_, item)| !item.is_none())
        .map(|(k, v)| Ok((k.into(), toml_item(v)?)))
        .collect::<Result<Map<String, Value>, String>>()
        .map(Value::Object)
}

fn toml_item(item: &Item) -> Result<Value, String> {
    if let Some(table) = item.as_table_like() {
        return toml_table(table);
    }
    if let Some(tables) = item.as_array_of_tables() {
        return tables
            .iter()
            .map(|t| toml_table(t))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array);
    }
    let value = item.as_value().ok_or("Unsupported TOML value")?;
    Ok(match value {
        toml_edit::Value::String(v) => Value::String(v.value().clone()),
        toml_edit::Value::Integer(v) => (*v.value()).into(),
        toml_edit::Value::Float(v) => serde_json::Number::from_f64(*v.value())
            .ok_or("Non-finite TOML values are unsupported")?
            .into(),
        toml_edit::Value::Boolean(v) => (*v.value()).into(),
        toml_edit::Value::Datetime(v) => serde_json::json!({"$tomlDatetime":v.value().to_string()}),
        toml_edit::Value::Array(v) => Value::Array(
            v.iter()
                .map(|v| toml_item(&Item::Value(v.clone())))
                .collect::<Result<_, _>>()?,
        ),
        toml_edit::Value::InlineTable(v) => toml_table(v)?,
    })
}

pub(super) fn get<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |v, key| v.as_object()?.get(key))
}

fn semantic_patch(value: &mut Value, patch: &Patch) -> Result<(), String> {
    let mut parent = value;
    for key in &patch.path()[..patch.path().len() - 1] {
        let object = parent
            .as_object_mut()
            .ok_or("A configuration parent is not an object")?;
        if !object.contains_key(key) {
            if patch.value().is_none() {
                return Ok(());
            }
            object.insert(key.clone(), Value::Object(Map::new()));
        }
        parent = object.get_mut(key).unwrap();
    }
    let object = parent
        .as_object_mut()
        .ok_or("A configuration parent is not an object")?;
    let key = patch.path().last().unwrap();
    match patch.value() {
        Some(value) => {
            object.insert(key.clone(), value.clone());
        }
        None => {
            object.remove(key);
        }
    }
    Ok(())
}

pub(super) fn patch(raw: &[u8], format: Format, patches: &[Patch]) -> Result<Vec<u8>, String> {
    if patches.len() > 32 {
        return Err("Too many configuration changes".into());
    }
    for (i, change) in patches.iter().enumerate() {
        let path = change.path();
        if path.is_empty()
            || path.len() > 16
            || path
                .iter()
                .any(|s| s.is_empty() || s.len() > 256 || s.contains(['\r', '\n', '\0']))
        {
            return Err("Invalid configuration field path".into());
        }
        if patches[..i]
            .iter()
            .any(|p| p.path().starts_with(path) || path.starts_with(p.path()))
        {
            return Err("Overlapping configuration changes are ambiguous".into());
        }
        if matches!(format, Format::Env) {
            if path.len() != 1 {
                return Err("Environment configuration keys cannot be nested".into());
            }
            if let Some(value) = change.value() {
                let value = value.as_str().ok_or("Environment values must be strings")?;
                if value.contains(['$', '\r', '\n']) {
                    return Err("Environment interpolation and multiline values require a native session override".into());
                }
            }
        }
    }
    let before = parse(raw, format)?;
    let mut expected = before.clone();
    for p in patches {
        semantic_patch(&mut expected, p)?;
    }
    if expected == before {
        return Ok(raw.to_vec());
    }
    let source = text(raw)?;
    let output = match format {
        Format::Json | Format::Jsonc => {
            let root = CstRootNode::parse(source, &options(format))
                .map_err(|_| "Invalid JSON configuration")?;
            for patch in patches {
                let mut parent = root
                    .object_value()
                    .ok_or("Configuration root must be an object")?;
                let mut absent = false;
                for key in &patch.path()[..patch.path().len() - 1] {
                    if patch.value().is_none() && parent.get(key).is_none() {
                        absent = true;
                        break;
                    }
                    parent = parent
                        .object_value_or_create(key)
                        .ok_or("A configuration parent is not an object")?;
                }
                if absent {
                    continue;
                }
                let key = patch.path().last().unwrap();
                match (parent.get(key), patch.value()) {
                    (Some(prop), Some(value)) => prop.set_value(json_input(value)),
                    (None, Some(value)) => {
                        parent.append(key, json_input(value));
                    }
                    (Some(prop), None) => prop.remove(),
                    (None, None) => {}
                }
            }
            root.to_string()
        }
        Format::Toml => {
            let mut doc = source
                .parse::<DocumentMut>()
                .map_err(|_| "Invalid TOML configuration")?;
            for patch in patches {
                edit_toml(doc.as_table_mut(), patch.path(), patch.value())?;
            }
            doc.to_string()
        }
        Format::Yaml => {
            let file =
                yaml_edit::YamlFile::from_str(source).map_err(|_| "Invalid YAML configuration")?;
            let doc = file.document().ok_or("Empty YAML configuration")?;
            for patch in patches {
                let mut parent = doc
                    .as_mapping()
                    .ok_or("Configuration root must be an object")?;
                let mut absent = false;
                for key in &patch.path()[..patch.path().len() - 1] {
                    if !parent.contains_key(key.as_str()) {
                        if patch.value().is_none() {
                            absent = true;
                            break;
                        }
                        let seed = yaml_edit::Document::from_str("{}\n")
                            .map_err(|_| "Cannot create YAML mapping")?;
                        parent.set(key.as_str(), seed.as_mapping().unwrap());
                    }
                    parent = parent
                        .get_mapping(key.as_str())
                        .ok_or("Cannot edit through a YAML scalar or alias")?;
                }
                if absent {
                    continue;
                }
                let key = patch.path().last().unwrap();
                match patch.value() {
                    Some(value) => {
                        let seed = yaml_edit::Document::from_str(&format!("value: {}\n", value))
                            .map_err(|_| "Unsupported YAML replacement")?;
                        parent.set(
                            key.as_str(),
                            seed.get("value").ok_or("Unsupported YAML replacement")?,
                        );
                    }
                    None => {
                        parent.remove(key.as_str());
                    }
                }
            }
            file.to_string()
        }
        Format::Env => edit_env(source, patches)?,
    };
    let output = keep_encoding(raw, output);
    if parse(&output, format)? != expected {
        return Err("Configuration edit did not preserve the requested document semantics".into());
    }
    Ok(output)
}

fn keep_encoding(original: &[u8], mut output: String) -> Vec<u8> {
    let source = String::from_utf8_lossy(original);
    let lf = source.bytes().filter(|&b| b == b'\n').count();
    if lf > 0 && source.matches("\r\n").count() == lf {
        output = output.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    let mut result = if original.starts_with(b"\xef\xbb\xbf") {
        b"\xef\xbb\xbf".to_vec()
    } else {
        Vec::new()
    };
    result.extend_from_slice(output.as_bytes());
    result
}

fn json_input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(v) => CstInputValue::Bool(*v),
        Value::Number(v) => CstInputValue::Number(v.to_string()),
        Value::String(v) => CstInputValue::String(v.clone()),
        Value::Array(v) => CstInputValue::Array(v.iter().map(json_input).collect()),
        Value::Object(v) => {
            CstInputValue::Object(v.iter().map(|(k, v)| (k.clone(), json_input(v))).collect())
        }
    }
}

fn toml_input(value: &Value) -> Result<toml_edit::Value, String> {
    Ok(match value {
        Value::Null => return Err("TOML has no null value; remove the field instead".into()),
        Value::Bool(v) => (*v).into(),
        Value::String(v) => v.as_str().into(),
        Value::Number(v) => {
            if let Some(v) = v.as_i64() {
                v.into()
            } else if v.is_f64() {
                v.as_f64().ok_or("Unsupported TOML number")?.into()
            } else {
                return Err("TOML integer is outside its supported range".into());
            }
        }
        Value::Array(v) => {
            let mut array = toml_edit::Array::new();
            for v in v {
                array.push(toml_input(v)?);
            }
            array.into()
        }
        Value::Object(v) => {
            let mut table = toml_edit::InlineTable::new();
            for (k, v) in v {
                table.insert(k, toml_input(v)?);
            }
            table.into()
        }
    })
}

fn edit_toml(
    parent: &mut dyn TableLike,
    path: &[String],
    value: Option<&Value>,
) -> Result<(), String> {
    let key = &path[0];
    if path.len() > 1 {
        if parent.get(key).is_none() {
            if value.is_none() {
                return Ok(());
            }
            let mut table = Table::new();
            table.set_implicit(true);
            parent.insert(key, Item::Table(table));
        }
        let table = parent
            .get_mut(key)
            .and_then(Item::as_table_like_mut)
            .ok_or("A TOML configuration parent is not a table")?;
        return edit_toml(table, &path[1..], value);
    }
    match value {
        None => {
            parent.remove(key);
        }
        Some(value) => {
            let mut new = toml_input(value)?;
            if let Some(old) = parent.get(key).and_then(Item::as_value) {
                *new.decor_mut() = old.decor().clone();
            }
            parent.insert(key, Item::Value(new));
        }
    }
    Ok(())
}

struct EnvEntry {
    key: String,
    value: String,
    start: usize,
    end: usize,
    line_start: usize,
    line_end: usize,
}

fn env_entries(source: &str) -> Result<Vec<EnvEntry>, String> {
    let mut entries = Vec::new();
    let mut names = HashSet::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        let mut body = bare.trim_start();
        if body.is_empty() || body.starts_with('#') {
            offset += line.len();
            continue;
        }
        if let Some(rest) = body.strip_prefix("export ") {
            body = rest.trim_start();
        }
        let (key, rhs) = body
            .split_once('=')
            .ok_or("Unsupported environment configuration syntax")?;
        let key = key.trim();
        if key.is_empty()
            || key.bytes().enumerate().any(|(i, c)| {
                !(c == b'_' || c.is_ascii_alphabetic() || i > 0 && c.is_ascii_digit())
            })
        {
            return Err("Invalid environment variable name".into());
        }
        if !names.insert(key.to_string()) {
            return Err("Duplicate environment configuration keys".into());
        }
        let rhs = rhs.trim_start();
        let start = offset + bare.len() - rhs.len();
        let (value, length) = if rhs.starts_with('"') {
            let mut escaped = false;
            let mut end = None;
            for (i, c) in rhs.char_indices().skip(1) {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    end = Some(i + 1);
                    break;
                }
            }
            let end = end.ok_or("Unclosed environment string")?;
            let value = serde_json::from_str::<String>(&rhs[..end])
                .map_err(|_| "Unsupported environment string escape")?;
            (value, end)
        } else if let Some(quoted) = rhs.strip_prefix('\'') {
            let end = quoted.find('\'').ok_or("Unclosed environment string")?;
            (quoted[..end].to_string(), end + 2)
        } else {
            let end = rhs
                .char_indices()
                .find(|(i, c)| {
                    *c == '#' && (*i == 0 || rhs.as_bytes()[i - 1].is_ascii_whitespace())
                })
                .map(|(i, _)| i)
                .unwrap_or(rhs.len());
            let value = rhs[..end].trim_end();
            (value.to_string(), value.len())
        };
        let trailing = rhs[length..].trim();
        if !trailing.is_empty() && !trailing.starts_with('#') {
            return Err("Unsupported environment string suffix".into());
        }
        entries.push(EnvEntry {
            key: key.into(),
            value,
            start,
            end: start + length,
            line_start: offset,
            line_end: offset + bare.len(),
        });
        offset += line.len();
    }
    Ok(entries)
}

fn edit_env(source: &str, patches: &[Patch]) -> Result<String, String> {
    let mut result = source.to_string();
    for patch in patches {
        if patch.path().len() != 1 {
            return Err("Environment configuration keys cannot be nested".into());
        }
        let key = &patch.path()[0];
        let value = patch
            .value()
            .map(|v| v.as_str().ok_or("Environment values must be strings"))
            .transpose()?;
        // dotenv implementations disagree about interpolation. Never claim a
        // literal saved value that a backend may expand into something else.
        if value.is_some_and(|v| v.contains(['$', '\r', '\n'])) {
            return Err(
                "Environment interpolation and multiline values require a native session override"
                    .into(),
            );
        }
        let entry = env_entries(&result)?.into_iter().find(|e| &e.key == key);
        match (entry, value) {
            (Some(entry), Some(value)) => {
                result.replace_range(
                    entry.start..entry.end,
                    &serde_json::to_string(value).map_err(|_| "Cannot encode environment value")?,
                );
            }
            (Some(entry), None) => {
                let comment = result[entry.end..entry.line_end].trim().to_string();
                result.replace_range(entry.line_start..entry.line_end, &comment);
            }
            (None, Some(value)) => {
                if !result.is_empty() && !result.ends_with('\n') {
                    result.push('\n');
                }
                result.push_str(&format!(
                    "{key}={}\n",
                    serde_json::to_string(value).map_err(|_| "Cannot encode environment value")?
                ));
            }
            (None, None) => {}
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn set(path: &[&str], value: Value) -> Patch {
        Patch::Set {
            path: path.iter().map(|s| s.to_string()).collect(),
            value,
        }
    }
    fn remove(path: &[&str]) -> Patch {
        Patch::Remove {
            path: path.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn jsonc_preserves_comments_spacing_and_literal_dotted_keys() {
        let source = b"{\n // keep\n \"model\": \"old\", // selected\n \"other\": {\"x\":1},\n \"chat.defaultModel\": \"a\",\n}\n";
        let out = patch(
            source,
            Format::Jsonc,
            &[
                set(&["model"], json!("new")),
                set(&["chat.defaultModel"], json!("b")),
            ],
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("// keep"));
        assert!(text.contains("// selected"));
        assert!(text.contains("\"other\": {\"x\":1}"));
        assert!(text.contains("\"chat.defaultModel\": \"b\""));
    }
    #[test]
    fn json_does_not_accept_json5_or_duplicate_keys() {
        for source in [
            r#"{model:'x'}"#,
            r#"{"a":1,"a":2}"#,
            r#"{"a":1,"\u0061":2}"#,
            r#"{"a":[{"x":1,"x":2}]}"#,
            r#"{"a":1,}"#,
        ] {
            assert!(parse(source.as_bytes(), Format::Json).is_err(), "{source}");
        }
    }
    #[test]
    fn all_formats_retain_bom_and_crlf() {
        for (format, source) in [
            (Format::Jsonc, "{\r\n \"model\": \"old\"\r\n}\r\n"),
            (Format::Toml, "# keep\r\nmodel = 'old' # selected\r\n"),
            (Format::Yaml, "# keep\r\nmodel: old # selected\r\n"),
            (Format::Env, "# keep\r\nexport MODEL = old # selected\r\n"),
        ] {
            let mut raw = b"\xef\xbb\xbf".to_vec();
            raw.extend_from_slice(source.as_bytes());
            let key = if matches!(format, Format::Env) {
                "MODEL"
            } else {
                "model"
            };
            let out = patch(&raw, format, &[set(&[key], json!("new"))]).unwrap();
            assert!(out.starts_with(b"\xef\xbb\xbf"));
            let text = String::from_utf8(out).unwrap();
            assert_eq!(text.matches('\n').count(), text.matches("\r\n").count());
        }
    }
    #[test]
    fn toml_preserves_trailing_comments_quoted_tables_and_inline_tables() {
        let source = b"# keep\nmodel = 'old' # selected\n[model_providers.\"a.b\"]\nname = 'private'\nhttp_headers = { a = 'unchanged', b = 'old' } # headers\n";
        let out = patch(
            source,
            Format::Toml,
            &[
                set(&["model"], json!("new")),
                set(
                    &["model_providers", "a.b", "http_headers", "b"],
                    json!("new"),
                ),
            ],
        )
        .unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("# selected"));
        assert!(out.contains("# headers"));
        assert!(out.contains("a = 'unchanged'"));
    }
    #[test]
    fn yaml_edits_nested_and_flow_mappings_without_losing_unrelated_values() {
        let source = b"# keep\nmodel:\n  default: old # selected\nother: {x: 1, y: two}\n";
        let out = patch(
            source,
            Format::Yaml,
            &[
                set(&["model", "default"], json!("new: model")),
                set(&["extra", "model"], json!("123")),
            ],
        )
        .unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("# keep"));
        assert!(out.contains("# selected"));
        assert!(out.contains("other: {x: 1, y: two}"));
    }
    #[test]
    fn removal_and_new_nested_fields_have_real_readback() {
        for (format, source) in [
            (Format::Jsonc, "{\"model\":\"old\",\"other\":1}"),
            (Format::Toml, "model = 'old'\nother = 1\n"),
            (Format::Yaml, "model: old\nother: 1\n"),
        ] {
            let out = patch(
                source.as_bytes(),
                format,
                &[
                    remove(&["model"]),
                    set(&["settings", "model"], json!("new")),
                ],
            )
            .unwrap();
            assert_eq!(
                parse(&out, format).unwrap(),
                json!({"other":1,"settings":{"model":"new"}})
            );
        }
    }
    #[test]
    fn no_change_keeps_original_bytes_including_quote_style() {
        let source = b"model = 'same' # keep\n";
        assert_eq!(
            patch(source, Format::Toml, &[set(&["model"], json!("same"))]).unwrap(),
            source
        );
    }

    #[test]
    fn a_toml_datetime_is_not_a_string_with_the_same_spelling() {
        let out = patch(
            b"model = 2026-10-11\n",
            Format::Toml,
            &[set(&["model"], json!("2026-10-11"))],
        )
        .unwrap();
        assert!(String::from_utf8(out.clone())
            .unwrap()
            .contains("\"2026-10-11\""));
        assert_eq!(
            parse(&out, Format::Toml).unwrap()["model"],
            json!("2026-10-11")
        );
    }
    #[test]
    fn scalar_parents_and_overlapping_changes_are_refused() {
        assert!(patch(
            b"{\"model\":1}",
            Format::Json,
            &[set(&["model", "name"], json!("x"))]
        )
        .is_err());
        assert!(patch(
            b"{}",
            Format::Json,
            &[
                set(&["model"], json!({})),
                set(&["model", "name"], json!("x"))
            ]
        )
        .is_err());
        assert!(patch(b"{}", Format::Json, &[set(&[], json!("x"))]).is_err());
    }
    #[test]
    fn malformed_source_errors_never_echo_inline_secrets() {
        for (format, raw) in [
            (Format::Json, b"{\"key\":\"synthetic-secret".as_slice()),
            (Format::Toml, b"key = 'synthetic-secret".as_slice()),
            (Format::Yaml, b"key: [synthetic-secret".as_slice()),
        ] {
            let error = parse(raw, format).unwrap_err();
            assert!(!error.contains("synthetic-secret"));
        }
    }
    #[test]
    fn yaml_alias_edits_cannot_mutate_other_consumers() {
        let source = b"shared: &shared\n  model: old\nagent: *shared\n";
        assert!(patch(
            source,
            Format::Yaml,
            &[set(&["agent", "model"], json!("new"))]
        )
        .is_err());
        assert!(patch(
            source,
            Format::Yaml,
            &[set(&["shared", "model"], json!("new"))]
        )
        .is_err());
    }
    #[test]
    fn env_preserves_inline_comments_and_never_evaluates_values() {
        let source = b"# keep\nexport MODEL = 'old' # selected\nOTHER=unchanged\n";
        let out = patch(source, Format::Env, &[set(&["MODEL"], json!("new model"))]).unwrap();
        assert!(String::from_utf8(out.clone())
            .unwrap()
            .contains("# selected"));
        assert_eq!(
            parse(&out, Format::Env).unwrap(),
            json!({"MODEL":"new model","OTHER":"unchanged"})
        );
        assert!(patch(source, Format::Env, &[set(&["MODEL"], json!("$TOKEN"))]).is_err());
        assert!(parse(b"X=1\nX=2\n", Format::Env).is_err());
        assert!(patch(
            b"MODEL='$TOKEN'\n",
            Format::Env,
            &[set(&["MODEL"], json!("$TOKEN"))]
        )
        .is_err());
    }
}
