pub mod clipboard_repo;
pub mod cli_query;
pub mod migrations;
pub mod settings_repo;
pub mod sensitive_tags;
pub mod tag_repo;
pub mod tag_page;
pub mod tag_batch;
pub mod custom_themes;

pub mod protection;

mod desktop_search;
mod capture_settings;

pub(crate) mod privacy_policy;
#[cfg(test)]
mod privacy_policy_tests;
