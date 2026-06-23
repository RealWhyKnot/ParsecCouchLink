use std::collections::BTreeMap;

use serde_json::Value;

pub(super) const HARVEST_PREFIX: &str = "# harvest ";

pub(super) fn fields(line: &str) -> BTreeMap<&str, &str> {
    line.split_whitespace()
        .filter_map(|part| part.split_once('='))
        .collect()
}

pub(super) fn parsed_u64(value: Option<&&str>) -> Option<u64> {
    value.and_then(|value| {
        value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .map_or_else(
                || value.parse::<u64>().ok(),
                |hex| u64::from_str_radix(hex, 16).ok(),
            )
    })
}

pub(super) fn cloned_field(value: Option<&&str>) -> Option<String> {
    value.map(|value| (*value).to_string())
}

pub(super) fn display_field<'a>(value: Option<&&'a str>) -> &'a str {
    value.copied().unwrap_or("-")
}

pub(super) fn harvest_value(line: &str) -> Option<Value> {
    let json = line.strip_prefix(HARVEST_PREFIX)?;
    serde_json::from_str(json).ok()
}

pub(super) fn json_string(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(|value| value.to_string())
}

pub(super) fn json_u64(value: &Value, key: &str) -> Option<u64> {
    let value = value.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse::<u64>().ok()))
}

pub(super) fn json_bool(value: &Value, key: &str) -> Option<bool> {
    let value = value.get(key)?;
    value.as_bool().or_else(|| {
        value.as_str().and_then(|value| match value {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        })
    })
}

pub(super) fn bump(map: &mut BTreeMap<String, u64>, key: &str) {
    *map.entry(key.to_string()).or_default() += 1;
}

pub(super) fn merge_counts(into: &mut BTreeMap<String, u64>, from: &BTreeMap<String, u64>) {
    for (key, value) in from {
        *into.entry(key.clone()).or_default() += value;
    }
}

pub(super) fn format_count_map(map: &BTreeMap<String, u64>) -> String {
    if map.is_empty() {
        "-".to_string()
    } else {
        map.iter()
            .map(|(key, value)| format!("{key}:{value}"))
            .collect::<Vec<_>>()
            .join(";")
    }
}

pub(super) fn min_assign(target: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        if target.map(|current| value < current).unwrap_or(true) {
            *target = Some(value);
        }
    }
}

pub(super) fn max_assign(target: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        if target.map(|current| value > current).unwrap_or(true) {
            *target = Some(value);
        }
    }
}
