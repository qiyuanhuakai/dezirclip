//! Explicit MCP Host mode never reads private control credentials or starts a
//! daemon. Ordinary bodies require a separately delegated, read-only MCP grant.
use crate::api::{ClipboardEntry,HistoryQuery,HistorySearchMode};
use dzc_host::{Client,wire::*};
fn row(entry:EntrySummary,text:Option<String>)->ClipboardEntry {
    ClipboardEntry{id:entry.id,content_type:entry.kind,content:text.unwrap_or_default(),html_content:None,source_app:entry.source_app,source_app_path:None,
        timestamp:entry.timestamp,preview:entry.preview.unwrap_or_default(),is_pinned:entry.pinned,tags:entry.tags,use_count:0,is_external:false,pinned_order:0,
        file_preview_exists:false,content_kinds:Vec::new(),ocr_text:None,ocr_status:None}
}
pub(crate) fn query(query:&HistoryQuery<'_>)->Result<Vec<ClipboardEntry>,String> {
    let database=dzc_host::data::database_path()?;
    let endpoint=dzc_host::controller::endpoint(&database).map_err(|_|"LocalHostUnavailable")?;
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|"LocalHostUnavailable")?.block_on(async {
        let grant=std::env::var("DEZIRCLIP_HOST_GRANT").ok();
        let mut client=Client::connect(&endpoint,ClientRole::Mcp,grant).await.map_err(|_|"LocalHostUnavailable")?;
        if let Some(id)=query.id {
            let result:EntryResult=client.call(Method::HistoryGet,serde_json::json!({"id":id,"service_instance_id":client.hello.state.service_instance_id}),"EntryResult").await.map_err(|_|"HostEntryUnavailable")?;
            let text=result.text.ok_or("HostBodyRepresentationUnavailable")?;
            return Ok(vec![row(result.entry,Some(text))]);
        }
        let page=HistoryPageParams{limit:query.limit.clamp(1,1000) as u16,offset:query.offset.max(0) as u32,kind:query.kind.map(str::to_owned),tag:query.tag.map(str::to_owned),pinned:Some(query.pinned)};
        let result:HistoryResult=if let Some((text,mode))=query.search {
            let mode=match mode {HistorySearchMode::Contains=>SearchMode::Contains,HistorySearchMode::Fuzzy=>SearchMode::Fuzzy,HistorySearchMode::Regex=>SearchMode::Regex,HistorySearchMode::Fts5=>SearchMode::Fts5};
            client.call(Method::HistorySearch,HistorySearchParams{query:text.into(),mode,page},"HistoryResult").await
        }else{client.call(Method::HistoryList,page,"HistoryResult").await}.map_err(|_|"HostHistoryUnavailable")?;
        Ok(result.entries.into_iter().map(|entry|row(entry,None)).collect())
    })
}
