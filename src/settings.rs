use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub zephyr_extensions: bool,
    /// Patterns of the names of the Kconfig files to read from the
    /// workspace. `*` matches any characters and `?` matches one character.
    pub kconfig_files: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zephyr_extensions: false,
            kconfig_files: ["Kconfig", "Kconfig.*", "Kconfig_*"]
                .map(String::from)
                .to_vec(),
        }
    }
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
                let unknown = parsed
                    .unknown
                    .keys()
                    .map(|key| format!("unknown option `{key}`"));
                let paths = parsed
                    .settings
                    .kconfig_files
                    .iter()
                    .filter(|p| p.contains('/'))
                    .map(|p| {
                        format!(
                            "`{p}` in kconfig_files has a `/`, but patterns match only file names"
                        )
                    });
                let warnings = unknown.chain(paths).collect();
                (parsed.settings, warnings)
            }
            Err(e) => {
                let warning = format!("could not read initializationOptions: {e}; using defaults");
                (Self::default(), vec![warning])
            }
        }
    }

    /// Whether a file with this name is a Kconfig file to read from the
    /// workspace.
    pub fn is_kconfig_file(&self, name: &str) -> bool {
        self.kconfig_files.iter().any(|p| glob_match(p, name))
    }
}

/// Whether `name` matches `pattern`, in which `*` matches any characters and
/// `?` matches one character.
pub(crate) fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0, 0);
    // After a `*`: the pattern position after it, and the name position
    // from which to try again.
    let mut star = None;
    while ni < n.len() {
        // Check `*` first, because the name can have a `*` too.
        if pi < p.len() && p[pi] == '*' {
            star = Some((pi + 1, ni));
            pi += 1;
        } else if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if let Some((sp, sn)) = star {
            // Let the `*` match one more character.
            star = Some((sp, sn + 1));
            pi = sp;
            ni = sn + 1;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
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

    #[test]
    fn default_kconfig_files() {
        let settings = Settings::default();
        for name in ["Kconfig", "Kconfig.arm", "Kconfig.", "Kconfig_foo"] {
            assert!(settings.is_kconfig_file(name), "{name}");
        }
        for name in ["Config.in", "MyKconfig", "kconfig", "Kconfig-foo"] {
            assert!(!settings.is_kconfig_file(name), "{name}");
        }
    }

    #[test]
    fn kconfig_files_replace_the_default() {
        // Buildroot and OpenWrt.
        let (settings, warnings) = Settings::from_json(
            json!({"kconfig_files": ["Config.in*", "Config-*.in", "Config.?"]}),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        for name in [
            "Config.in",
            "Config.in.host",
            "Config-kernel.in",
            "Config-.in",
            "Config.x",
        ] {
            assert!(settings.is_kconfig_file(name), "{name}");
        }
        for name in ["Kconfig", "Config-kernel.in.bak", "Config.", "Config.xy"] {
            assert!(!settings.is_kconfig_file(name), "{name}");
        }
    }

    #[test]
    fn star_can_match_after_a_failed_try() {
        let settings = Settings {
            kconfig_files: vec!["a*b*c".to_string()],
            ..Default::default()
        };
        for name in ["abc", "aXbYbZc", "abbc", "a*b*c"] {
            assert!(settings.is_kconfig_file(name), "{name}");
        }
        for name in ["ab", "abd", "aXbYcZ", "bc"] {
            assert!(!settings.is_kconfig_file(name), "{name}");
        }
    }

    #[test]
    fn star_matches_a_star_in_the_name() {
        let settings = Settings {
            kconfig_files: vec!["*".to_string(), "Kconfig.*".to_string()],
            ..Default::default()
        };
        for name in ["*x", "Kconfig.*foo"] {
            assert!(settings.is_kconfig_file(name), "{name}");
        }
    }

    #[test]
    fn kconfig_file_pattern_with_a_slash_warns() {
        let (settings, warnings) =
            Settings::from_json(json!({"kconfig_files": ["Kconfig", "arch/*/Kconfig"]}));
        assert_eq!(settings.kconfig_files, ["Kconfig", "arch/*/Kconfig"]);
        assert_eq!(
            warnings,
            ["`arch/*/Kconfig` in kconfig_files has a `/`, but patterns match only file names"]
        );
    }

    #[test]
    fn kconfig_files_of_wrong_type_warn_and_use_defaults() {
        for value in [json!("Config.in"), json!(null), json!(["Config.in", 1])] {
            let (settings, warnings) = Settings::from_json(json!({ "kconfig_files": value }));
            assert!(settings.is_kconfig_file("Kconfig"), "{value}");
            assert_eq!(warnings.len(), 1, "{value}: {warnings:?}");
        }
    }

    #[test]
    fn empty_kconfig_files_match_no_file() {
        let (settings, warnings) = Settings::from_json(json!({"kconfig_files": []}));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!settings.is_kconfig_file("Kconfig"));
    }
}
