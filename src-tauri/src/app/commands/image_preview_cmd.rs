#[tauri::command]
pub async fn get_image_preview(_path: String) -> Result<String, String> {
    Err("PrivacyResourceIdRequired".into())
}

#[cfg(test)]
mod tests;
