use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Clone, Default, Debug, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub zephyr_extensions: bool,
}

/// `Settings` plus any keys it does not know, so a stray key warns instead
/// of rejecting the whole object.
#[derive(Deserialize)]
struct WithUnknown {
    #[serde(flatten)]
    settings: Settings,
    #[serde(flatten)]
    unknown: Map<String, Value>,
}

impl Settings {
    /// Parse `initializationOptions`, returning warnings to show the user.
    pub fn from_json(options: Value) -> (Self, Vec<String>) {
        match &options {
            Value::Null => return (Self::default(), Vec::new()),
            Value::Object(_) => {}
            other => {
                let warning = format!(
                    "initializationOptions should be an object, got {}; using defaults",
                    json_type_name(other)
                );
                return (Self::default(), vec![warning]);
            }
        }

        match serde_json::from_value::<WithUnknown>(options) {
            Ok(parsed) => {
                let warnings = parsed
                    .unknown
                    .keys()
                    .map(|key| format!("unknown option `{key}`"))
                    .collect();
                (parsed.settings, warnings)
            }
            Err(e) => {
                let warning = format!("could not read initializationOptions: {e}; using defaults");
                (Self::default(), vec![warning])
            }
        }
    }
}

fn json_type_name(v: &Value) -> &'static str {
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

    #[test]
    fn valid_options_parse_without_warnings() {
        let (settings, warnings) = Settings::from_json(json!({"zephyr_extensions": true}));
        assert!(settings.zephyr_extensions);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn empty_and_null_options_use_defaults_silently() {
        for options in [json!({}), Value::Null] {
            let (settings, warnings) = Settings::from_json(options.clone());
            assert!(!settings.zephyr_extensions, "{options}");
            assert!(warnings.is_empty(), "{options}: {warnings:?}");
        }
    }

    #[test]
    fn unknown_option_warns() {
        for (key, value) in [
            ("zephyr-extensions", json!(true)),
            ("zephyrExtensions", json!(true)),
            ("kconfig", json!({"zephyr_extensions": true})),
        ] {
            let (settings, warnings) = Settings::from_json(json!({ key: value }));
            assert!(!settings.zephyr_extensions, "{key}");
            assert_eq!(warnings, [format!("unknown option `{key}`")]);
        }
    }

    #[test]
    fn unknown_option_does_not_discard_known_ones() {
        let (settings, warnings) =
            Settings::from_json(json!({"zephyr_extensions": true, "bogus": 1}));
        assert!(settings.zephyr_extensions);
        assert_eq!(warnings, ["unknown option `bogus`"]);
    }

    #[test]
    fn wrong_value_type_warns_and_uses_defaults() {
        let (settings, warnings) = Settings::from_json(json!({"zephyr_extensions": "true"}));
        assert!(!settings.zephyr_extensions);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("could not read initializationOptions: invalid type"),
            "{warnings:?}"
        );
    }

    #[test]
    fn non_object_options_warn_and_use_defaults() {
        for (options, kind) in [
            (json!(true), "a boolean"),
            (json!("zephyr_extensions"), "a string"),
            (json!([]), "an array"),
        ] {
            let (settings, warnings) = Settings::from_json(options);
            assert!(!settings.zephyr_extensions);
            assert_eq!(
                warnings,
                [format!(
                    "initializationOptions should be an object, got {kind}; using defaults"
                )]
            );
        }
    }
}
