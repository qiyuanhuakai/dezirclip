use super::{CoreError, CoreResult, HistoryService};
use crate::domain::theme::{ThemePackage, MAX_THEME_BYTES};
use std::path::Path;

/// The saved theme and complete library produced by one committed import.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeImportOutcome {
    pub theme: ThemePackage,
    pub themes: Vec<ThemePackage>,
}

/// The complete library and selection change produced by one committed removal.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeRemovalOutcome {
    pub themes: Vec<ThemePackage>,
    pub active_reset: bool,
}

impl HistoryService {
    pub fn custom_themes(&self) -> CoreResult<Vec<ThemePackage>> {
        self.repository.custom_themes().map_err(CoreError::from)
    }

    pub fn custom_theme_ids(&self) -> CoreResult<Vec<String>> {
        Ok(self
            .custom_themes()?
            .into_iter()
            .map(|theme| theme.id)
            .collect())
    }

    pub fn custom_theme(&self, id: &str) -> CoreResult<ThemePackage> {
        self.custom_themes()?
            .into_iter()
            .find(|theme| theme.id == id)
            .ok_or_else(|| CoreError::validation("custom theme not found"))
    }

    pub fn effective_base_theme(&self, theme: &str) -> String {
        let Some(id) = theme.strip_prefix("custom:") else {
            return theme.to_string();
        };
        self.custom_theme(id)
            .map(|theme| theme.base)
            .unwrap_or_else(|_| "mica".into())
    }

    pub fn import_theme(&self, theme: ThemePackage) -> CoreResult<ThemeImportOutcome> {
        let id = theme.id.clone();
        let themes = self.repository.import_theme(theme)?;
        let theme = themes
            .iter()
            .find(|theme| theme.id == id)
            .cloned()
            .ok_or_else(|| CoreError::validation("saved theme not found"))?;
        Ok(ThemeImportOutcome { theme, themes })
    }

    pub fn import_theme_file(&self, path: &Path) -> CoreResult<ThemePackage> {
        let bytes = crate::filesystem::read_bounded_regular(path, MAX_THEME_BYTES)
            .map_err(|error| CoreError::validation(error.to_string()))?;
        let theme = ThemePackage::parse(&bytes).map_err(CoreError::validation)?;
        self.import_theme(theme).map(|outcome| outcome.theme)
    }

    pub fn remove_theme_with_outcome(&self, id: &str) -> CoreResult<ThemeRemovalOutcome> {
        let (themes, active_reset) = self.repository.remove_theme(id)?;
        Ok(ThemeRemovalOutcome {
            themes,
            active_reset,
        })
    }

    pub fn remove_theme(&self, id: &str) -> CoreResult<bool> {
        self.remove_theme_with_outcome(id)
            .map(|outcome| outcome.active_reset)
    }

    pub fn export_theme_file(&self, id: &str, path: &Path) -> CoreResult<()> {
        let bytes = self.custom_theme(id)?.export_bytes()?;
        self.ensure_export_destination(path)?;
        crate::filesystem::ensure_regular_destination(path)
            .map_err(|error| CoreError::validation(error.to_string()))?;
        crate::infrastructure::attachments::write_backup(path, &bytes).map_err(CoreError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> ThemePackage {
        ThemePackage {
            format: "dezirclip-theme".into(),
            version: 1,
            id: id.into(),
            name: " Sample ".into(),
            author: Some(" ".into()),
            description: None,
            base: "retro".into(),
            css: ":root.light-mode{--dc-accent:#123456}".into(),
        }
    }

    #[test]
    fn mutation_outcomes_retain_their_complete_committed_library() {
        let history = HistoryService::open(Path::new(":memory:")).unwrap();
        let first = history.import_theme(sample("first")).unwrap();
        assert_eq!(first.theme.name, "Sample");
        assert_eq!(first.theme.author, None);
        assert_eq!(first.themes, vec![first.theme.clone()]);
        assert_eq!(first.themes, history.custom_themes().unwrap());

        let second = history.import_theme(sample("second")).unwrap();
        assert_eq!(
            second.themes,
            vec![first.theme.clone(), second.theme.clone()]
        );
        assert_eq!(second.themes, history.custom_themes().unwrap());
        assert_eq!(history.custom_theme_ids().unwrap(), ["first", "second"]);
        assert_eq!(history.custom_theme("second").unwrap(), second.theme);

        let removed = history.remove_theme_with_outcome("first").unwrap();
        assert!(!removed.active_reset);
        assert_eq!(removed.themes, vec![second.theme.clone()]);
        assert_eq!(removed.themes, history.custom_themes().unwrap());
        assert_eq!(first.themes, vec![first.theme.clone()]);
        assert_eq!(second.themes, vec![first.theme, second.theme]);

        let path = std::env::temp_dir()
            .join(format!("dzc-theme-api-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&path, serde_json::to_vec(&sample("file")).unwrap()).unwrap();
        let imported = history.import_theme_file(&path);
        std::fs::remove_file(&path).unwrap();
        let imported = imported.unwrap();
        assert_eq!(imported.name, "Sample");
        assert_eq!(imported, history.custom_theme("file").unwrap());
    }

    #[test]
    fn failed_mutations_preserve_selection_and_active_removal_commits_fallback() {
        let history = HistoryService::open(Path::new(":memory:")).unwrap();
        let first = history.import_theme(sample("first")).unwrap().theme;
        let second = history.import_theme(sample("second")).unwrap().theme;
        let epoch = history.runtime_snapshot(0).unwrap().privacy_epoch;
        history
            .update_view_setting("app.theme", "custom:first", epoch)
            .unwrap();

        let mut duplicate = sample("first");
        duplicate.name = "Changed".into();
        assert!(history.import_theme(duplicate).is_err());
        let mut invalid = sample("invalid");
        invalid.css = ".x{background:url(https://example.invalid/image.png)}".into();
        assert!(history.import_theme(invalid).is_err());
        assert!(history.remove_theme_with_outcome("missing").is_err());
        assert_eq!(history.custom_themes().unwrap(), vec![first, second.clone()]);
        assert_eq!(
            history
                .desktop_preferences()
                .unwrap()
                .get("app.theme")
                .map(String::as_str),
            Some("custom:first")
        );

        let removed = history.remove_theme_with_outcome("first").unwrap();
        assert!(removed.active_reset);
        assert_eq!(removed.themes, vec![second]);
        assert_eq!(removed.themes, history.custom_themes().unwrap());
        assert_eq!(
            history
                .desktop_preferences()
                .unwrap()
                .get("app.theme")
                .map(String::as_str),
            Some("mica")
        );

        history
            .update_view_setting("app.theme", "custom:second", epoch)
            .unwrap();
        assert!(history.remove_theme("second").unwrap());
        assert!(history.custom_themes().unwrap().is_empty());
    }

    #[test]
    fn effective_base_theme_preserves_passthrough_and_damaged_library_fallback() {
        let history = HistoryService::open(Path::new(":memory:")).unwrap();
        assert_eq!(history.effective_base_theme("mica"), "mica");
        assert_eq!(history.effective_base_theme("unknown"), "unknown");
        assert_eq!(history.effective_base_theme("custom:missing"), "mica");
        history.import_theme(sample("sample")).unwrap();
        assert_eq!(history.effective_base_theme("custom:sample"), "retro");

        let epoch = history.runtime_snapshot(0).unwrap().privacy_epoch;
        history
            .repository
            .set_setting_at_epoch("app.custom_themes", "invalid JSON", epoch)
            .unwrap();
        assert!(history.custom_themes().is_err());
        assert_eq!(history.effective_base_theme("custom:sample"), "mica");
        assert_eq!(history.effective_base_theme("retro"), "retro");
    }
}
