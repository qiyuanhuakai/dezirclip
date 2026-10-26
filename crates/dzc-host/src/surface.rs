//! Read-only, explicitly delegated Surface queries with one authoritative state.
use crate::{Host, Session};
use dzc_core::api::{self, PrivacyAccessIntent, SensitiveAccess};
use dzc_protocol::wire::*;
use serde_json::Value;

pub(crate) fn check_state(request: &SurfacePageParams, state: &RuntimeState) -> Result<(), WireErrorCode> {
    if request.service_instance_id != state.service_instance_id { return Err(WireErrorCode::PrivacyBlocked); }
    if request.expected_epoch != state.privacy_epoch { return Err(WireErrorCode::PrivacyEpochChanged); }
    if request.expected_history_revision != state.history_revision { return Err(WireErrorCode::HistoryRevisionChanged); }
    Ok(())
}
impl Host {
    pub(crate) fn surface_capability(&self) -> Value {
        let info = crate::build_info(dzc_protocol::build_info::Component::Desktop, api::DATABASE_SCHEMA_VERSION);
        serde_json::json!({"version":1,"max_page_size":100,"build":{
            "component_version":info.component_version,"source_revision":info.source_revision,
            "build_id":info.build_id,"target":info.target}})
    }
    pub(crate) fn surface_page(&self, session: &Session, request: SurfacePageParams) -> Result<Value, WireErrorCode> {
        if !session.surface_reader() || !self.grants.still_valid(session) { return Err(WireErrorCode::PermissionDenied); }
        // Capture publishes to RAM before its event. Holding this same queue
        // lock spans both state observations and the actual metadata query.
        let rows = self.session.lock().map_err(|_| WireErrorCode::Unavailable)?;
        let before = self.snapshot_with_session(&rows)?;
        check_state(&request, &before)?;
        let now = crate::router::now_ms()?;
        let permit = self.history.privacy().authorize_read(PrivacyAccessIntent::RestrictedSummary, SensitiveAccess::Denied,
            Some(request.expected_epoch), now).map_err(crate::router::privacy_error)?;
        let page = api::SummaryQuery { limit: request.limit.saturating_add(1), offset: request.offset,
            kind: request.kind.clone(), tag: request.tag.clone(), pinned: request.pinned.unwrap_or(false) };
        let search = request.query.as_ref().filter(|value| !value.is_empty()).map(|query| api::SearchRequest {
            query: query.clone(), page: page.clone(), mode: match request.mode.unwrap_or(SearchMode::Contains) {
                SearchMode::Contains => api::SummarySearchMode::Contains, SearchMode::Fuzzy => api::SummarySearchMode::Fuzzy,
                SearchMode::Regex => api::SummarySearchMode::Regex, SearchMode::Fts5 => api::SummarySearchMode::Fts5,
            }});
        let entries = self.history_page_with_session(page, search, &permit, now, &rows)?;
        self.history.privacy().check_read(&permit, crate::router::now_ms()?).map_err(crate::router::privacy_error)?;
        let after = self.snapshot_with_session(&rows)?;
        check_state(&request, &after)?;
        if !self.grants.still_valid(session) { return Err(WireErrorCode::PermissionDenied); }
        let has_more = entries.len() > usize::from(request.limit);
        let entries = entries.into_iter().take(usize::from(request.limit)).map(crate::router::summary).collect();
        let mut page = SurfacePage { entries, state: after, has_more, next_offset: None };
        loop {
            page.next_offset = if page.has_more { Some(request.offset.saturating_add(page.entries.len() as u32)) } else { None };
            let value = serde_json::to_value(&page).map_err(|_| WireErrorCode::Internal)?;
            dzc_protocol::wire::validate("SurfacePage", &value).map_err(|_| WireErrorCode::Internal)?;
            if serde_json::to_vec(&value).map_err(|_| WireErrorCode::Internal)?.len() <= MAX_FRAME_BYTES - 4096 { return Ok(value); }
            if page.entries.pop().is_none() { return Err(WireErrorCode::FrameTooLarge); }
            page.has_more = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn grant_session(host: &Host) -> (crate::Grant, Session) {
        let grant = host.grants().issue_surface_read(Duration::from_secs(30)).unwrap();
        let session = host.hello(HelloParams { protocol_version: VERSION, client_version: "synthetic".into(), client_role: ClientRole::Surface,
            capabilities: vec![], required_capabilities: vec!["surface.history.page".into()], grant: Some(grant.token().into()) }).unwrap().0;
        (grant, session)
    }
    fn request(host: &Host) -> SurfacePageParams {
        let state = host.snapshot().unwrap();
        SurfacePageParams { service_instance_id: state.service_instance_id, expected_epoch: state.privacy_epoch,
            expected_history_revision: state.history_revision, limit: 1, offset: 0, query: None, mode: None, kind: None, tag: None, pinned: None }
    }
    fn call(host: &Host, session: &Session, parameters: &SurfacePageParams) -> Response {
        host.dispatch(session, Request { request_id: "surface-test".into(), method: Method::SurfaceHistoryPage, params: serde_json::to_value(parameters).unwrap() })
    }
    fn insert(host: &Host, text: &str, protected: bool) {
        let entry = serde_json::from_value(serde_json::json!({"id":0,"content_type":"text","content":text,"preview":text,
            "source_app":"synthetic","timestamp":1,"is_pinned":false,"tags":if protected {vec!["sensitive"]} else {vec![]}})).unwrap();
        host.history.save(&entry, None).unwrap();
    }
    #[test]
    fn surface_delegation_is_finite_read_only_and_revocable() {
        let host = crate::router::tests::synthetic_host();
        assert!(host.grants().issue_surface_read(Duration::ZERO).is_err());
        assert!(host.grants().issue_surface_read(Duration::from_secs(601)).is_err());
        let (grant, session) = grant_session(&host);
        assert_eq!(session.permissions(), &[Permission::HistoryRead]);
        let page = request(&host);
        assert!(call(&host, &session, &page).result.is_some());
        for method in [Method::SurfacePaste, Method::SurfaceCopy, Method::SurfaceActivate, Method::HistoryGet, Method::PrivacySet, Method::ServiceStop] {
            let parameters = match method { Method::PrivacySet => serde_json::json!({"active":true}), Method::ServiceStop => serde_json::json!({}),
                Method::SurfaceActivate => serde_json::json!({}), _ => serde_json::json!({"id":1}) };
            assert_eq!(host.dispatch(&session, Request { request_id:"read-only".into(), method, params:parameters }).error.unwrap().code, WireErrorCode::PermissionDenied);
        }
        host.grants().revoke(&grant);
        assert_eq!(call(&host, &session, &page).error.unwrap().code, WireErrorCode::PermissionDenied);
    }
    #[test]
    fn page_is_bounded_searchable_metadata_without_protected_body() {
        let host = crate::router::tests::synthetic_host();
        insert(&host, "ordinary first", false); insert(&host, "ordinary second", false); insert(&host, "private forbidden", true);
        let (_, session) = grant_session(&host);
        let mut parameters = request(&host);
        let first: SurfacePage = serde_json::from_value(call(&host, &session, &parameters).result.unwrap()).unwrap();
        assert_eq!(first.entries.len(), 1); assert!(first.has_more); assert_eq!(first.next_offset, Some(1));
        assert_eq!(first.state, host.snapshot().unwrap());
        parameters.offset = first.next_offset.unwrap();
        let second: SurfacePage = serde_json::from_value(call(&host, &session, &parameters).result.unwrap()).unwrap();
        assert_eq!(second.entries.len(), 1); assert!(!second.has_more); assert_ne!(first.entries[0].id, second.entries[0].id);
        parameters.offset = 0; parameters.query = Some("forbidden".into());
        let hidden = call(&host, &session, &parameters).result.unwrap();
        assert_eq!(hidden["entries"], serde_json::json!([])); assert!(!hidden.to_string().contains("private forbidden"));
        parameters.query = Some("second".into());
        let found: SurfacePage = serde_json::from_value(call(&host, &session, &parameters).result.unwrap()).unwrap();
        assert_eq!(found.entries.len(), 1); assert_eq!(found.entries[0].preview.as_deref(), Some("ordinary second"));
        parameters.limit = 101;
        assert_eq!(call(&host, &session, &parameters).error.unwrap().code, WireErrorCode::InvalidRequest);
    }
    #[test]
    fn claimed_surface_or_changed_instance_epoch_revision_cannot_query_old_state() {
        let host = crate::router::tests::synthetic_host();
        let ungranted = host.hello(HelloParams { protocol_version: VERSION, client_version:"synthetic".into(), client_role:ClientRole::Surface,
            capabilities:vec![], required_capabilities:vec!["surface.history.page".into()], grant:None });
        assert!(matches!(ungranted, Err(WireErrorCode::UnsupportedCapability)));
        let (_, session) = grant_session(&host);
        let original = request(&host);
        let mut changed = original.clone(); changed.service_instance_id = "another-host".into();
        assert_eq!(call(&host, &session, &changed).error.unwrap().code, WireErrorCode::PrivacyBlocked);
        changed = original.clone(); changed.expected_epoch += 1;
        assert_eq!(call(&host, &session, &changed).error.unwrap().code, WireErrorCode::PrivacyEpochChanged);
        insert(&host, "new current history", false);
        assert_eq!(call(&host, &session, &original).error.unwrap().code, WireErrorCode::HistoryRevisionChanged);
        assert!(call(&host, &session, &request(&host)).result.is_some());
    }
    #[test]
    fn unpublished_ram_insert_edit_reorder_evict_delete_and_rollback_expire_previous_pages() {
        let host=crate::router::tests::synthetic_host();let (_,reader)=grant_session(&host);
        let row=|id:i64,time:i64|serde_json::from_value(serde_json::json!({"id":id,"content_type":"text","content":"original body","preview":"same preview","source_app":"synthetic","timestamp":time,"is_pinned":false})).unwrap();
        {let mut rows=host.session.lock().unwrap();rows.push_back(row(-1,1));rows.push_back(row(-2,2));}
        for change in 0..6 {
            let original=request(&host);let event_sequence=host.snapshot().unwrap().event_sequence;
            let first:SurfacePage=serde_json::from_value(call(&host,&reader,&original).result.unwrap()).unwrap();
            assert!(first.has_more);
            assert_eq!(host.snapshot().unwrap().history_revision,original.expected_history_revision,"a metadata read cannot mutate its own revision");
            {let mut rows=host.session.lock().unwrap();match change {
                0=>rows.push_back(row(-3,3)),
                1=>rows.iter_mut().find(|entry|entry.id== -1).unwrap().content="new full body, unchanged preview".into(),
                2=>{let entry=rows.pop_front().unwrap();rows.push_back(entry);},
                3=>{rows.pop_front();rows.push_back(row(-4,4));},
                4=>{rows.retain(|entry|entry.id!= -3);},
                _=>{let previous=rows.clone();rows.clear();*rows=previous;},
            }}
            let mut continuation=original.clone();continuation.offset=first.next_offset.unwrap();
            assert_eq!(call(&host,&reader,&continuation).error.unwrap().code,WireErrorCode::HistoryRevisionChanged);
            let current=host.snapshot().unwrap();assert_eq!(current.privacy_epoch,original.expected_epoch);
            assert_eq!(current.event_sequence,event_sequence,"this rejects RAM writes before any capture notification");
        }
    }
    #[test]
    fn hello_health_events_desktop_snapshot_and_surface_share_one_host_revision() {
        let host=crate::router::tests::synthetic_host();let (_,reader)=grant_session(&host);let before=host.snapshot().unwrap();
        let row=serde_json::from_value(serde_json::json!({"id":-9,"content_type":"text","content":"ordinary RAM","preview":"ordinary","source_app":"synthetic","timestamp":1,"is_pinned":false})).unwrap();
        host.session.lock().unwrap().push_back(row);
        let actual=host.snapshot().unwrap();assert!(actual.history_revision>before.history_revision);
        let (_,hello)=host.hello(HelloParams{protocol_version:VERSION,client_version:"synthetic".into(),client_role:ClientRole::Cli,capabilities:vec![],required_capabilities:vec![],grant:None}).unwrap();
        assert_eq!(hello.state,actual);
        let dispatch=|session:&Session,method,params|host.dispatch(session,Request{request_id:"state-contract".into(),method,params});
        let health:RuntimeState=serde_json::from_value(dispatch(&reader,Method::Health,serde_json::json!({})).result.unwrap()).unwrap();assert_eq!(health,actual);
        let events:EventsResult=serde_json::from_value(dispatch(&reader,Method::EventsSubscribe,serde_json::json!({"after_sequence":0,"wait_ms":0})).result.unwrap()).unwrap();assert_eq!(events.state,actual);
        let grant=host.grants().issue_control(ClientRole::Desktop).unwrap();
        let (desktop,_)=host.hello(HelloParams{protocol_version:VERSION,client_version:"synthetic".into(),client_role:ClientRole::Desktop,capabilities:vec![],required_capabilities:vec![],grant:Some(grant.token().into())}).unwrap();
        let snapshot:BodySnapshot=serde_json::from_value(dispatch(&desktop,Method::DesktopHistorySummary,serde_json::json!({"id":-9,"service_instance_id":actual.service_instance_id,"expected_epoch":actual.privacy_epoch,"reveal":false})).result.unwrap()).unwrap();assert_eq!(snapshot.state,actual);
        let page:SurfacePage=serde_json::from_value(call(&host,&reader,&request(&host)).result.unwrap()).unwrap();assert_eq!(page.state,actual);
        assert_eq!(host.history.runtime_snapshot(crate::router::now_ms().unwrap()).unwrap().history_revision,before.history_revision,"Core's DB-only type was not redefined");
    }
}
