use super::{CoreError, CoreResult, HistoryService};
use crate::domain::theme::{ThemePackage, MAX_THEME_BYTES};
use std::path::Path;

impl HistoryService {
    pub fn custom_theme_ids(&self) -> CoreResult<Vec<String>> {
        Ok(self
            .repository
            .custom_themes()?
            .into_iter()
            .map(|theme| theme.id)
            .collect())
    }

    pub fn custom_theme(&self, id: &str) -> CoreResult<ThemePackage> {
        self.repository
            .custom_themes()?
            .into_iter()
            .find(|theme| theme.id == id)
            .ok_or_else(|| CoreError::validation("custom theme not found"))
    }

    pub fn import_theme_file(&self, path: &Path) -> CoreResult<ThemePackage> {
        let bytes = crate::filesystem::read_bounded_regular(path, MAX_THEME_BYTES)
            .map_err(|error| CoreError::validation(error.to_string()))?;
        let theme = ThemePackage::parse(&bytes).map_err(CoreError::validation)?;
        self.repository.import_theme(theme.clone())?;
        Ok(theme)
    }

    pub fn remove_theme(&self, id: &str) -> CoreResult<bool> {
        self.repository.remove_theme(id).map_err(CoreError::from)
    }

    pub fn export_theme_file(&self, id: &str, path: &Path) -> CoreResult<()> {
        let bytes = self.custom_theme(id)?.export_bytes()?;
        self.ensure_export_destination(path)?;
        crate::filesystem::ensure_regular_destination(path)
            .map_err(|error| CoreError::validation(error.to_string()))?;
        crate::infrastructure::attachments::write_backup(path, &bytes).map_err(CoreError::from)
    }
}
