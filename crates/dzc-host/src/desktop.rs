//! Explicitly granted Desktop hydration. Surface and MCP never enter this path.
use crate::{Host, Session};
use dzc_core::api::{self, PrivacyReadPermit};
use dzc_protocol::wire::*;
use serde::Serialize;
use serde_json::Value;
use std::{sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}}, time::{Duration, Instant}};
use base64::Engine;

const MAX_BYTES: usize = 32 * 1024 * 1024;
const CHUNK_BYTES: usize = 192 * 1024;
struct Snapshot { id: String, owner: String, epoch: u64, expires: Instant, bytes: Vec<u8> }
#[derive(Default)]
pub(crate) struct Snapshots(Arc<Mutex<Vec<Snapshot>>>,Arc<AtomicBool>);
impl Snapshots {
    pub(crate) fn clear(&self) -> Result<(), WireErrorCode> {
        self.0.lock().map_err(|_| WireErrorCode::Unavailable)?.clear();
        Ok(())
    }
    fn store(&self, owner: &Session, state: RuntimeState, value: &impl Serialize) -> Result<BodySnapshot, WireErrorCode> {
        let bytes = serde_json::to_vec(value).map_err(|_| WireErrorCode::Internal)?;
        if bytes.len() > MAX_BYTES { return Err(WireErrorCode::FrameTooLarge); }
        let mut snapshots = self.0.lock().map_err(|_| WireErrorCode::Unavailable)?;
        snapshots.retain(|snapshot| snapshot.expires > Instant::now() && snapshot.epoch == state.privacy_epoch);
        if snapshots.len() >= 4 || snapshots.iter().map(|snapshot| snapshot.bytes.len()).sum::<usize>().saturating_add(bytes.len()) > MAX_BYTES {
            return Err(WireErrorCode::Busy);
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let total_bytes = bytes.len() as u32;
        snapshots.push(Snapshot { id: id.clone(), owner: owner.id().into(), epoch: state.privacy_epoch, expires: Instant::now() + Duration::from_secs(5), bytes });
        if !self.1.swap(true,Ordering::AcqRel) {
            let storage=self.0.clone();
            let running=self.1.clone();
            if std::thread::Builder::new().name("dzc-snapshot-expiry".into()).spawn(move||loop {
                let delay={
                    let Ok(mut snapshots)=storage.lock() else {running.store(false,Ordering::Release);return;};
                    let now=Instant::now();
                    snapshots.retain(|snapshot|snapshot.expires>now);
                    let Some(expires)=snapshots.iter().map(|snapshot|snapshot.expires).min() else {
                        running.store(false,Ordering::Release);return;
                    };
                    expires.saturating_duration_since(now)
                };
                std::thread::sleep(delay);
            }).is_err() {
                self.1.store(false,Ordering::Release);
                snapshots.retain(|snapshot|snapshot.id!=id);
                return Err(WireErrorCode::Unavailable);
            }
        }
        Ok(BodySnapshot { snapshot_id: id, total_bytes, state })
    }
    fn read(&self, owner: &Session, state: RuntimeState, request: SnapshotReadParams) -> Result<SnapshotChunk, WireErrorCode> {
        if request.service_instance_id != state.service_instance_id || request.expected_epoch != state.privacy_epoch { return Err(WireErrorCode::PrivacyBlocked); }
        let mut snapshots = self.0.lock().map_err(|_| WireErrorCode::Unavailable)?;
        snapshots.retain(|snapshot| snapshot.expires > Instant::now() && snapshot.epoch == state.privacy_epoch);
        let index = snapshots.iter().position(|snapshot| snapshot.id == request.snapshot_id && snapshot.owner == owner.id()).ok_or(WireErrorCode::NotFound)?;
        let snapshot = &snapshots[index];
        let start = request.offset as usize;
        if start > snapshot.bytes.len() { return Err(WireErrorCode::InvalidRequest); }
        let end = start.saturating_add(CHUNK_BYTES).min(snapshot.bytes.len());
        let chunk = SnapshotChunk { data_base64: base64::engine::general_purpose::STANDARD.encode(&snapshot.bytes[start..end]),
            offset: request.offset, next_offset: end as u32, finished: end == snapshot.bytes.len(), state };
        if chunk.finished { snapshots.remove(index); }
        Ok(chunk)
    }
}

fn blocked<T>(_: T) -> WireErrorCode { WireErrorCode::PrivacyBlocked }
fn value<T: Serialize>(schema: &str, result: T) -> Result<Value, WireErrorCode> {
    let result = serde_json::to_value(result).map_err(|_| WireErrorCode::Internal)?;
    dzc_protocol::wire::validate(schema, &result).map_err(|_| WireErrorCode::Internal)?;
    Ok(result)
}
pub(crate) fn entry(mut entry: api::ClipboardEntry, compact: bool) -> DesktopEntry {
    if compact {
        if matches!(entry.content_type.as_str(), "text" | "code" | "url" | "rich_text") {
            entry.content = entry.content.chars().take(2000).collect();
            entry.preview = entry.preview.chars().take(500).collect();
        }
        // Full resource representations have their own explicit getter. List
        // metadata must not accumulate HTML or image bodies across 150 rows.
        if matches!(entry.content_type.as_str(), "image" | "html") || entry.html_content.is_some() {
            entry.content.clear();
            entry.html_content = None;
        }
        entry.ocr_text = None;
    }
    DesktopEntry { id: entry.id, content_type: entry.content_type, content: entry.content, html_content: entry.html_content,
        source_app: entry.source_app.chars().take(256).collect(), timestamp: entry.timestamp, preview: entry.preview,
        is_pinned: entry.is_pinned, tags: entry.tags, use_count: i64::from(entry.use_count), is_external: false,
        pinned_order: entry.pinned_order, file_preview_exists: entry.file_preview_exists, content_kinds: entry.content_kinds,
        ocr_text: entry.ocr_text, ocr_status: entry.ocr_status }
}

impl Host {
    pub(crate) fn desktop(&self, session: &Session, method: Method, parameters: Value) -> Result<Value, WireErrorCode> {
        session.require(Permission::DesktopHistory)?;
        if session.role() != ClientRole::Desktop { return Err(WireErrorCode::PermissionDenied); }
        let now = crate::router::now_ms()?;
        if method == Method::DesktopSnapshotRead {
            let request: SnapshotReadParams = serde_json::from_value(parameters).map_err(|_| WireErrorCode::InvalidRequest)?;
            let permit = self.history.privacy().authorize_read(api::PrivacyAccessIntent::InteractiveRead, api::SensitiveAccess::Denied, Some(request.expected_epoch), now).map_err(blocked)?;
            let chunk = self.snapshots.read(session, self.snapshot()?, request)?;
            self.history.privacy().check_read(&permit, crate::router::now_ms()?).map_err(blocked)?;
            return value("SnapshotChunk", chunk);
        }
        if method == Method::DesktopHistoryPage {
            let page: DesktopPageParams = serde_json::from_value(parameters).map_err(|_| WireErrorCode::InvalidRequest)?;
            let permit = self.history.privacy().authorize_read(api::PrivacyAccessIntent::InteractiveRead, api::SensitiveAccess::Denied, None, now).map_err(blocked)?;
            let entries = self.desktop_page(page, &permit, now)?;
            let entries = DesktopEntries { entries: entries.into_iter().map(|row| entry(row, true)).collect() };
            value("DesktopEntries", &entries)?;
            self.history.privacy().check_read(&permit, crate::router::now_ms()?).map_err(blocked)?;
            return self.authorized_snapshot(session, &permit, &entries);
        }
        let request: DesktopEntryParams = serde_json::from_value(parameters).map_err(|_| WireErrorCode::InvalidRequest)?;
        if request.service_instance_id != self.snapshot()?.service_instance_id { return Err(WireErrorCode::PrivacyBlocked); }
        let permit = self.history.privacy().authorize_read(api::PrivacyAccessIntent::InteractiveRead,
            if request.reveal { api::SensitiveAccess::ExplicitUserAction } else { api::SensitiveAccess::Denied }, Some(request.expected_epoch), now).map_err(blocked)?;
        let row = if request.id < 0 {
            let session = self.session.lock().map_err(|_| WireErrorCode::Unavailable)?;
            let row = session.iter().find(|row| row.id == request.id).ok_or(WireErrorCode::NotFound)?;
            match request.thumbnail_dimension {
                Some(maximum) => self.history.session_entry_thumbnail_authorized(row, &permit, now, maximum),
                None => self.history.session_entry_for_preview_authorized(row, &permit, now),
            }.map_err(blocked)?
        } else {
            match request.thumbnail_dimension {
                Some(maximum) => self.history.get_entry_thumbnail_authorized(request.id, &permit, now, maximum),
                None => self.history.get_entry_for_preview_authorized(request.id, &permit, now),
            }.map_err(blocked)?.ok_or(WireErrorCode::NotFound)?
        };
        let row = entry(row, false);
        value("DesktopEntry", &row)?;
        self.history.privacy().check_read(&permit, crate::router::now_ms()?).map_err(blocked)?;
        self.authorized_snapshot(session, &permit, &row)
    }

    pub(crate) fn authorized_snapshot(&self,session:&Session,permit:&PrivacyReadPermit,result:&impl Serialize)->Result<Value,WireErrorCode> {
        let state=self.snapshot()?;
        if state.privacy_epoch!=permit.epoch() {return Err(WireErrorCode::PrivacyBlocked);}
        value("BodySnapshot",self.snapshots.store(session,state,result)?)
    }

    fn desktop_page(&self, page: DesktopPageParams, permit: &PrivacyReadPermit, now: i64) -> Result<Vec<api::ClipboardEntry>, WireErrorCode> {
        let mut owned_session=self.session.lock().map_err(|_|WireErrorCode::Unavailable)?;
        let original=owned_session.make_contiguous();
        let mut session=self.history.entries_metadata_for_gui(original,true,now).map_err(blocked)?;
        session.retain(|entry|page.kind.as_ref().is_none_or(|kind|kind==&entry.content_type));
        if let Some(query) = page.query.as_deref().filter(|query| !query.is_empty()) {
            let restricted = self.history.privacy().authorize_read(api::PrivacyAccessIntent::RestrictedSummary, api::SensitiveAccess::Denied, Some(permit.epoch()), now).map_err(blocked)?;
            let found = self.history.search_summaries_for_session_authorized(original, &api::SearchRequest { query: query.into(), mode: api::SummarySearchMode::Contains,
                page: api::SummaryQuery {limit:1000, offset:0, kind:page.kind.clone(), tag:None, pinned:false} }, &restricted, now).map_err(blocked)?;
            session.retain(|entry| found.entries.iter().any(|summary| summary.id == entry.id));
        }
        drop(owned_session);
        let start = page.offset.saturating_sub(session.len() as u32);
        let rows = self.history.query(&api::HistoryQuery { limit:i32::from(page.limit) + session.len() as i32, offset:start as i32,metadata:true,
            kind:page.kind.as_deref(), search:page.query.as_deref().filter(|query| !query.is_empty()).map(|query| (query,api::HistorySearchMode::Contains)),
            exclude_sensitive:page.query.is_some(), ..Default::default() }).map_err(|_| WireErrorCode::Unavailable)?;
        let mut rows=self.history.entries_metadata_for_gui(&rows,true,now).map_err(blocked)?;
        rows.append(&mut session);
        rows.sort_by(|a,b| b.is_pinned.cmp(&a.is_pinned).then_with(||b.pinned_order.cmp(&a.pinned_order)).then_with(||b.timestamp.cmp(&a.timestamp)).then_with(||b.id.cmp(&a.id)));
        Ok(rows.into_iter().skip((page.offset-start) as usize).take(page.limit as usize).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session(host:&Host,role:ClientRole,granted:bool)->Session {
        let grant=granted.then(||host.grants().issue_control(role).unwrap());
        host.hello(HelloParams{protocol_version:dzc_protocol::wire::VERSION,client_version:"synthetic".into(),client_role:role,
            capabilities:vec![],required_capabilities:vec![],grant:grant.map(|grant|grant.token().into())}).unwrap().0
    }
    fn call(host:&Host,session:&Session,method:Method,parameters:Value)->Response {
        host.dispatch(session,Request{request_id:"synthetic-desktop".into(),method,params:parameters})
    }
    fn insert(host:&Host,text:&str,protected:bool)->i64 {
        let entry=serde_json::from_value(serde_json::json!({"id":0,"content_type":"text","content":text,"preview":text,"source_app":"Synthetic","source_app_path":"/synthetic/private/process.exe","timestamp":1,"is_pinned":false,"tags":if protected {vec!["sensitive"]} else {vec![]}})).unwrap();
        host.history.save(&entry,None).unwrap()
    }
    fn get(host:&Host,session:&Session,id:i64,reveal:bool)->Response {
        let state=host.snapshot().unwrap();
        call(host,session,Method::DesktopHistoryGet,serde_json::json!({"id":id,"reveal":reveal,"expected_epoch":state.privacy_epoch,"service_instance_id":state.service_instance_id}))
    }
    #[test]
    fn desktop_grants_do_not_follow_claimed_roles_or_cli_control_and_sensitive_reveal_is_explicit() {
        let host=crate::router::tests::synthetic_host();
        let id=insert(&host,"synthetic protected body",true);
        for role in [ClientRole::Desktop,ClientRole::Surface,ClientRole::Mcp,ClientRole::Cli] {
            assert_eq!(get(&host,&session(&host,role,false),id,true).error.unwrap().code,WireErrorCode::PermissionDenied);
        }
        assert_eq!(get(&host,&session(&host,ClientRole::Cli,true),id,true).error.unwrap().code,WireErrorCode::PermissionDenied);
        let desktop=session(&host,ClientRole::Desktop,true);
        assert_eq!(get(&host,&desktop,id,false).error.unwrap().code,WireErrorCode::NotFound);
        let snapshot:BodySnapshot=serde_json::from_value(get(&host,&desktop,id,true).result.unwrap()).unwrap();
        let chunk=host.snapshots.read(&desktop,host.snapshot().unwrap(),SnapshotReadParams{snapshot_id:snapshot.snapshot_id,offset:0,
            expected_epoch:snapshot.state.privacy_epoch,service_instance_id:snapshot.state.service_instance_id}).unwrap();
        let bytes=base64::engine::general_purpose::STANDARD.decode(chunk.data_base64).unwrap();
        let entry:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(entry["content"],"synthetic protected body");
        assert!(entry.get("source_app_path").is_none());
        assert!(!String::from_utf8(bytes).unwrap().contains("process.exe"));
    }
    #[test]
    fn completed_snapshots_release_quota_while_incomplete_snapshots_remain_bounded() {
        let host=crate::router::tests::synthetic_host();
        let desktop=session(&host,ClientRole::Desktop,true);
        let id=insert(&host,"ordinary completed snapshot",false);
        for _ in 0..5 {
            let descriptor:BodySnapshot=serde_json::from_value(get(&host,&desktop,id,false).result.unwrap()).unwrap();
            let request=SnapshotReadParams{snapshot_id:descriptor.snapshot_id,offset:0,
                expected_epoch:descriptor.state.privacy_epoch,service_instance_id:descriptor.state.service_instance_id};
            let response=call(&host,&desktop,Method::DesktopSnapshotRead,serde_json::to_value(&request).unwrap());
            let chunk:SnapshotChunk=serde_json::from_value(response.result.unwrap()).unwrap();
            assert!(chunk.finished);
            assert!(host.snapshots.0.lock().unwrap().is_empty());
            assert_eq!(call(&host,&desktop,Method::DesktopSnapshotRead,serde_json::to_value(request).unwrap()).error.unwrap().code,WireErrorCode::NotFound);
        }
        let large="x".repeat(CHUNK_BYTES+1);
        for _ in 0..4 {
            let descriptor=host.snapshots.store(&desktop,host.snapshot().unwrap(),&large).unwrap();
            let chunk=host.snapshots.read(&desktop,host.snapshot().unwrap(),SnapshotReadParams{snapshot_id:descriptor.snapshot_id,offset:0,
                expected_epoch:descriptor.state.privacy_epoch,service_instance_id:descriptor.state.service_instance_id}).unwrap();
            assert!(!chunk.finished);
        }
        assert_eq!(host.snapshots.0.lock().unwrap().len(),4);
        assert_eq!(host.snapshots.store(&desktop,host.snapshot().unwrap(),&"still bounded").unwrap_err(),WireErrorCode::Busy);
    }
    #[test]
    fn snapshots_are_bound_to_connection_instance_epoch_expiry_and_total_budget() {
        let host=crate::router::tests::synthetic_host();
        let desktop=session(&host,ClientRole::Desktop,true);
        let other=session(&host,ClientRole::Desktop,true);
        let id=insert(&host,"ordinary synthetic",false);
        let descriptor:BodySnapshot=serde_json::from_value(get(&host,&desktop,id,false).result.unwrap()).unwrap();
        let request=SnapshotReadParams{snapshot_id:descriptor.snapshot_id.clone(),offset:0,expected_epoch:descriptor.state.privacy_epoch,service_instance_id:descriptor.state.service_instance_id.clone()};
        assert_eq!(host.snapshots.read(&other,host.snapshot().unwrap(),request.clone()).unwrap_err(),WireErrorCode::NotFound);
        let mut changed=request.clone();changed.service_instance_id="another-instance".into();
        assert_eq!(host.snapshots.read(&desktop,host.snapshot().unwrap(),changed).unwrap_err(),WireErrorCode::PrivacyBlocked);
        host.history.privacy().toggle(crate::router::now_ms().unwrap()).unwrap();
        assert_eq!(call(&host,&desktop,Method::DesktopSnapshotRead,serde_json::to_value(request).unwrap()).error.unwrap().code,WireErrorCode::PrivacyBlocked);
        assert!(host.snapshots.0.lock().unwrap().is_empty());
        let descriptor=host.snapshots.store(&desktop,host.snapshot().unwrap(),&"expired").unwrap();
        host.snapshots.0.lock().unwrap()[0].expires=Instant::now()-Duration::from_secs(1);
        assert_eq!(host.snapshots.read(&desktop,host.snapshot().unwrap(),SnapshotReadParams{snapshot_id:descriptor.snapshot_id,offset:0,expected_epoch:descriptor.state.privacy_epoch,service_instance_id:descriptor.state.service_instance_id}).unwrap_err(),WireErrorCode::NotFound);
        let bytes="x".repeat(9*1024*1024);
        for _ in 0..3 {host.snapshots.store(&desktop,host.snapshot().unwrap(),&bytes).unwrap();}
        assert_eq!(host.snapshots.store(&desktop,host.snapshot().unwrap(),&bytes).unwrap_err(),WireErrorCode::Busy);
        assert_eq!(host.snapshots.store(&desktop,host.snapshot().unwrap(),&"x".repeat(MAX_BYTES+1)).unwrap_err(),WireErrorCode::FrameTooLarge);
    }
}
