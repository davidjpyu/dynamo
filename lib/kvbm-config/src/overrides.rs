// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Helpers for applying sparse JSON overrides to `kv_connector_extra_config`.

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::KvbmConfig;

/// Recursively merge `overlay` into `base`.
///
/// Object values merge key-by-key. Non-object values replace the existing
/// value, including arrays and `null`.
pub fn deep_merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base_obj), Value::Object(overlay_obj)) => {
            for (key, overlay_value) in overlay_obj {
                match base_obj.get_mut(&key) {
                    Some(base_value) => deep_merge(base_value, overlay_value),
                    None => {
                        base_obj.insert(key, overlay_value);
                    }
                }
            }
        }
        (base_value, overlay_value) => {
            *base_value = overlay_value;
        }
    }
}

/// Apply an optional JSON object and repeated dotted-key overrides to `target`.
///
/// `config_json` is merged first. Individual `KEY.PATH=VALUE` overrides are
/// applied after it, so they have higher precedence. Override values are parsed
/// as JSON with a string fallback, which keeps `foo=2` typed as an integer while
/// allowing plain strings such as URLs.
pub fn apply_overrides(
    target: &mut Value,
    config_json: Option<&str>,
    overrides: &[String],
) -> Result<()> {
    if let Some(config_json) = config_json {
        let parsed: Value = serde_json::from_str(config_json).context("--kvbm-config JSON")?;
        ensure_object(&parsed, "--kvbm-config")?;
        deep_merge(target, parsed);
    }

    for override_arg in overrides {
        let (path, raw_value) = override_arg
            .split_once('=')
            .with_context(|| format!("--kvbm override must be KEY.PATH=VALUE: {override_arg}"))?;
        let value =
            serde_json::from_str(raw_value).unwrap_or_else(|_| Value::String(raw_value.into()));
        insert_path(target, path, value)
            .with_context(|| format!("applying --kvbm override {override_arg:?}"))?;
    }

    Ok(())
}

/// Validate that a `kv_connector_extra_config` JSON object parses for both
/// leader and worker profiles.
pub fn validate_extra_config(config: &Value) -> Result<()> {
    ensure_object(config, "kv_connector_extra_config")?;
    let json = serde_json::to_string(config)?;
    KvbmConfig::from_figment_with_json_for_leader(&json).context("leader profile validation")?;
    KvbmConfig::from_figment_with_json_for_worker(&json).context("worker profile validation")?;
    Ok(())
}

fn ensure_object(value: &Value, label: &str) -> Result<()> {
    if value.is_object() {
        Ok(())
    } else {
        bail!("{label} must be a JSON object")
    }
}

fn insert_path(target: &mut Value, path: &str, value: Value) -> Result<()> {
    let segments: Vec<&str> = path.split('.').collect();
    if segments.is_empty() || segments.iter().any(|s| s.is_empty()) {
        bail!("override path must contain non-empty dotted segments");
    }
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }

    let mut cursor = target;
    for segment in &segments[..segments.len() - 1] {
        let obj = cursor
            .as_object_mut()
            .context("override path traversed a non-object value")?;
        cursor = obj
            .entry((*segment).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !cursor.is_object() {
            *cursor = Value::Object(Map::new());
        }
    }

    let leaf = segments.last().expect("segments is non-empty");
    let obj = cursor
        .as_object_mut()
        .context("override path traversed a non-object value")?;
    obj.insert((*leaf).to_string(), value);
    Ok(())
}
