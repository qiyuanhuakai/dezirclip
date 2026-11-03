use crate::{Host, Session};
use dzc_protocol::wire::*;
use serde_json::{json, Value};

impl Host {
    pub(crate) fn check_library_state(&self, session: &Session, instance: &str, epoch: u64) -> Result<(), WireErrorCode> {
        self.desktop_permission(session, instance)?;
        if self.snapshot()?.privacy_epoch != epoch {
            return Err(WireErrorCode::PrivacyEpochChanged);
        }
        Ok(())
    }

    pub(crate) fn library_summary(&self, session: &Session, method: Method, params: Value) -> Result<Value, WireErrorCode> {
        use dzc_core::api::{PrivacyAccessIntent, SensitiveAccess};
        let p: LibraryStateParams = serde_json::from_value(params).map_err(|_| WireErrorCode::InvalidRequest)?;
        self.check_library_state(session, &p.service_instance_id, p.expected_epoch)?;
        let now = crate::router::now_ms()?;
        let permit = self.history.privacy().authorize_read(PrivacyAccessIntent::InteractiveRead, SensitiveAccess::Denied,
            Some(p.expected_epoch), now).map_err(crate::router::privacy_error)?;
        let value = match method {
            Method::DesktopSettingsPreferences => json!({ "values": self.history.desktop_preferences().map_err(|_| WireErrorCode::Unavailable)? }),
            Method::DesktopTagsCounts => json!({ "values": self.history.tag_counts().map_err(|_| WireErrorCode::Unavailable)? }),
            Method::DesktopTagsColors => json!({ "values": self.history.tag_colors().map_err(|_| WireErrorCode::Unavailable)? }),
            Method::DesktopHistoryCount => json!({ "count": self.history.get_count().map_err(|_| WireErrorCode::Unavailable)? }),
            _ => return Err(WireErrorCode::InvalidRequest),
        };
        self.history.privacy().check_read(&permit, crate::router::now_ms()?).map_err(crate::router::privacy_error)?;
        if method == Method::DesktopHistoryCount { return Ok(value); }
        self.authorized_snapshot(session, &permit, &value)
    }

    pub(crate) fn delete_library_entries(&self, session: &Session, method: Method, params: Value) -> Result<Value, WireErrorCode> {
        use dzc_core::api::TagEntrySelection;
        let invalid = |_| WireErrorCode::InvalidRequest;
        let (tag, selection) = match method {
            Method::DesktopTagsDeleteContents => {
                let p: LibraryTagParams = serde_json::from_value(params).map_err(invalid)?;
                self.check_library_state(session, &p.service_instance_id, p.expected_epoch)?;
                (p.name, None)
            }
            Method::DesktopTagsDeleteEntries => {
                let p: LibraryTagBatchParams = serde_json::from_value(params).map_err(invalid)?;
                self.check_library_state(session, &p.service_instance_id, p.expected_epoch)?;
                let selected = match p.scope.as_str() {
                    "selected" if p.query.is_none() => TagEntrySelection::Selected { ids: p.ids },
                    "all" => TagEntrySelection::All { query: p.query.unwrap_or_default(), excluded_ids: p.ids },
                    _ => return Err(WireErrorCode::InvalidRequest),
                };
                (p.name, Some(selected))
            }
            _ => return Err(WireErrorCode::InvalidRequest),
        };
        let whole_tag = selection.is_none();
        let mut rows = self.session.lock().map_err(|_| WireErrorCode::Unavailable)?;
        let report = match selection {
            Some(selection) => self.history.delete_tag_entries(&tag, selection, Some(&self.directory)),
            None => self.history.delete_tag_with_entries(&tag, Some(&self.directory)),
        }.map_err(|_| WireErrorCode::Unavailable)?;
        rows.retain(|row| !report.deleted_ids.contains(&row.id) && !(whole_tag && row.tags.contains(&tag)));
        drop(rows);
        self.publish(EventKind::HistoryReset, EventData { entry_id: None, status: None })?;
        Ok(json!({ "deleted_count": report.deleted_ids.len(), "cleanup_failed": !report.cleanup_errors.is_empty() }))
    }

    pub(crate) fn edit_library(&self, session: &Session, method: Method, params: Value) -> Result<Value, WireErrorCode> {
        let invalid = |_| WireErrorCode::InvalidRequest;
        let storage = |_: dzc_core::api::CoreError| WireErrorCode::Unavailable;
        match method {
            Method::DesktopHistoryClear => {
                let p: LibraryStateParams = serde_json::from_value(params).map_err(invalid)?;
                self.check_library_state(session, &p.service_instance_id, p.expected_epoch)?;
                let mut rows = self.session.lock().map_err(|_| WireErrorCode::Unavailable)?;
                self.history.clear(Some(&self.directory)).map_err(storage)?;
                rows.retain(|row| row.is_pinned || !row.tags.is_empty());
            }
            Method::DesktopHistoryReorder => {
                let p: LibraryOrderParams = serde_json::from_value(params).map_err(invalid)?;
                self.check_library_state(session, &p.service_instance_id, p.expected_epoch)?;
                self.history.update_pinned_order(p.orders.into_iter().map(|pair| (pair.id, pair.order)).collect()).map_err(storage)?;
            }
            Method::DesktopTagsCreate | Method::DesktopTagsRemove => {
                let p: LibraryTagParams = serde_json::from_value(params).map_err(invalid)?;
                self.check_library_state(session, &p.service_instance_id, p.expected_epoch)?;
                if method == Method::DesktopTagsCreate {
                    self.history.create_tag(&p.name).map_err(storage)?;
                } else {
                    let mut rows = self.session.lock().map_err(|_| WireErrorCode::Unavailable)?;
                    self.history.remove_tag_preserving_entries(&p.name).map_err(storage)?;
                    for row in rows.iter_mut() {
                        row.tags.retain(|tag| tag != &p.name);
                    }
                }
            }
            Method::DesktopTagsColor => {
                let p: LibraryColorParams = serde_json::from_value(params).map_err(invalid)?;
                self.check_library_state(session, &p.service_instance_id, p.expected_epoch)?;
                self.history.set_tag_color(&p.name, p.color).map_err(storage)?;
            }
            Method::DesktopTagsRename => {
                let p: LibraryRenameParams = serde_json::from_value(params).map_err(invalid)?;
                self.check_library_state(session, &p.service_instance_id, p.expected_epoch)?;
                let name = p.new_name.trim();
                let mut rows = self.session.lock().map_err(|_| WireErrorCode::Unavailable)?;
                self.history.rename_tag(&p.name, name).map_err(storage)?;
                for row in rows.iter_mut() {
                    for tag in &mut row.tags {
                        if *tag == p.name {
                            name.clone_into(tag);
                        }
                    }
                    row.tags.sort();
                    row.tags.dedup();
                }
            }
            _ => return Err(WireErrorCode::InvalidRequest),
        }
        self.publish(EventKind::HistoryReset, EventData { entry_id: None, status: None })?;
        Ok(json!({ "performed": true }))
    }
}
