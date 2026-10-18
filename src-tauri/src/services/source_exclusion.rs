fn normalize(value: &str) -> String {
    value.trim().replace('\\', "/").to_lowercase()
}

pub fn parse_excluded_source_apps(value: &str) -> Vec<String> {
    let mut rules = Vec::new();
    for line in value.lines() {
        let rule = normalize(line);
        if !rule.is_empty() && !rules.contains(&rule) {
            rules.push(rule);
        }
    }
    rules
}

fn same_executable_name(left: &str, right: &str) -> bool {
    left == right
        || left.strip_suffix(".exe").unwrap_or(left)
            == right.strip_suffix(".exe").unwrap_or(right)
}

/// Source rules are whole names or paths, so a rule for a password manager
/// never also matches an unrelated process that merely contains its name.
pub fn source_app_is_excluded(
    rules: &[String],
    app_name: &str,
    process_path: Option<&str>,
) -> bool {
    if rules.is_empty() {
        return false;
    }

    let app_name = normalize(app_name);
    let process_path = process_path.map(normalize);
    let executable = process_path
        .as_deref()
        .and_then(|path| path.rsplit('/').next());

    rules.iter().any(|rule| {
        if rule.contains('/') {
            process_path.as_deref() == Some(rule.as_str())
        } else {
            same_executable_name(rule, &app_name)
                || executable.is_some_and(|name| same_executable_name(rule, name))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_excluded_source_apps, source_app_is_excluded};

    #[test]
    fn blank_rules_preserve_capture() {
        let rules = parse_excluded_source_apps(" \r\n\t\n");
        assert!(rules.is_empty());
        assert!(!source_app_is_excluded(&rules, "Unknown", None));
    }

    #[test]
    fn rules_are_trimmed_normalized_and_deduplicated() {
        assert_eq!(
            parse_excluded_source_apps("  Bitwarden.EXE\r\nbitwarden.exe\n\n C:\\Apps\\Keeper.exe "),
            vec!["bitwarden.exe", "c:/apps/keeper.exe"]
        );
    }

    #[test]
    fn names_match_exactly_ignoring_case_and_windows_exe_suffix() {
        let rules = parse_excluded_source_apps("Bitwarden\nKeePassXC.exe");
        assert!(source_app_is_excluded(&rules, "BITWARDEN.EXE", None));
        assert!(source_app_is_excluded(&rules, "keepassxc", None));
        assert!(!source_app_is_excluded(&rules, "bitwarden-helper.exe", None));
        assert!(!source_app_is_excluded(&rules, "my-bitwarden", None));
    }

    #[test]
    fn executable_path_supplies_the_full_name_when_x11_comm_is_truncated() {
        let rules = parse_excluded_source_apps("password-manager");
        assert!(source_app_is_excluded(
            &rules,
            "password-manage",
            Some("/opt/password-manager")
        ));
    }

    #[test]
    fn full_path_rules_do_not_match_a_different_installation() {
        let rules = parse_excluded_source_apps("C:\\Apps\\Bitwarden.exe\n/usr/bin/keepassxc");
        assert!(source_app_is_excluded(
            &rules,
            "Bitwarden.exe",
            Some("c:/APPS/bitwarden.exe")
        ));
        assert!(!source_app_is_excluded(
            &rules,
            "Bitwarden.exe",
            Some("C:\\Other\\Bitwarden.exe")
        ));
        assert!(source_app_is_excluded(
            &rules,
            "keepassxc",
            Some("/usr/bin/keepassxc")
        ));
        assert!(!source_app_is_excluded(&rules, "keepassxc", None));
    }
}
