use crate::domain::theme::{parse_theme_library, ThemePackage, MAX_CUSTOM_THEMES};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

const LIBRARY_KEY: &str = "app.custom_themes";

pub fn load(conn: &Connection) -> Result<Vec<ThemePackage>, String> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key=?1",
            [LIBRARY_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    parse_theme_library(raw.as_deref())
}

fn save(conn: &Connection, themes: &[ThemePackage]) -> Result<(), String> {
    let raw = serde_json::to_string(themes).map_err(|error| error.to_string())?;
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![LIBRARY_KEY, raw]).map_err(|error| error.to_string())?;
    Ok(())
}

pub fn upsert(
    conn: &mut Connection,
    mut theme: ThemePackage,
    replace: bool,
) -> Result<Vec<ThemePackage>, String> {
    theme.validate()?;
    let transaction = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let mut themes = load(&transaction)?;
    if let Some(index) = themes.iter().position(|item| item.id == theme.id) {
        if !replace {
            return Err("a theme with this id already exists; edit its id before importing".into());
        }
        themes[index] = theme;
    } else {
        if themes.len() >= MAX_CUSTOM_THEMES {
            return Err("at most 32 custom themes can be saved".into());
        }
        themes.push(theme);
    }
    save(&transaction, &themes)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(themes)
}

pub fn remove(conn: &mut Connection, id: &str) -> Result<(Vec<ThemePackage>, bool), String> {
    let transaction = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let mut themes = load(&transaction)?;
    let before = themes.len();
    themes.retain(|theme| theme.id != id);
    if themes.len() == before {
        return Err("custom theme not found".into());
    }
    let selected: Option<String> = transaction
        .query_row(
            "SELECT value FROM settings WHERE key='app.theme'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let active = selected.as_deref() == Some(format!("custom:{id}").as_str());
    if active {
        transaction
            .execute("UPDATE settings SET value='mica' WHERE key='app.theme'", [])
            .map_err(|error| error.to_string())?;
    }
    save(&transaction, &themes)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok((themes, active))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> ThemePackage {
        ThemePackage {
            format: "dezirclip-theme".into(),
            version: 1,
            id: "sample".into(),
            name: "Sample".into(),
            author: None,
            description: None,
            base: "mica".into(),
            css:
                ":root.light-mode{--dc-accent:#123456}.dark-mode .dc-tag-entry{border-radius:12px}"
                    .into(),
        }
    }
    #[test]
    fn limits_and_failed_save_preserve_the_library_and_active_selection() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);")
            .unwrap();
        for index in 0..MAX_CUSTOM_THEMES {
            let mut theme = sample();
            theme.id = format!("sample-{index}");
            upsert(&mut conn, theme, false).unwrap();
        }
        assert!(upsert(&mut conn, sample(), false).is_err());
        assert_eq!(load(&conn).unwrap().len(), MAX_CUSTOM_THEMES);
        conn.execute(
            "INSERT INTO settings VALUES('app.theme','custom:sample-0')",
            [],
        )
        .unwrap();
        conn.execute_batch("CREATE TRIGGER fail_catalog BEFORE UPDATE ON settings WHEN NEW.key='app.custom_themes' BEGIN SELECT RAISE(ABORT,'fixture failure');END;").unwrap();
        assert!(remove(&mut conn, "sample-0").is_err());
        assert_eq!(load(&conn).unwrap().len(), MAX_CUSTOM_THEMES);
        assert_eq!(
            conn.query_row(
                "SELECT value FROM settings WHERE key='app.theme'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "custom:sample-0"
        );
    }
    #[test]
    fn import_conflict_is_atomic_and_removing_active_theme_falls_back() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);")
            .unwrap();
        let original = upsert(&mut conn, sample(), false).unwrap();
        let mut changed = sample();
        changed.name = "Changed".into();
        assert!(upsert(&mut conn, changed.clone(), false).is_err());
        assert_eq!(load(&conn).unwrap(), original);
        let mut invalid = sample();
        invalid.id = "external".into();
        invalid.css = ".x{background:url(https://example.invalid/image.png)}".into();
        assert!(upsert(&mut conn, invalid, false).is_err());
        assert_eq!(load(&conn).unwrap(), original);
        assert_eq!(upsert(&mut conn, changed, true).unwrap()[0].name, "Changed");
        conn.execute(
            "INSERT INTO settings VALUES('app.theme','custom:sample')",
            [],
        )
        .unwrap();
        let (themes, active) = remove(&mut conn, "sample").unwrap();
        assert!(active && themes.is_empty());
        assert_eq!(
            conn.query_row(
                "SELECT value FROM settings WHERE key='app.theme'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "mica"
        );
    }
}
