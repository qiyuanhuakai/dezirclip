//! RAM history mutations and their revision share one lock.
use super::ClipboardEntry;
use std::{collections::VecDeque,ops::{Deref,DerefMut},sync::{LockResult,Mutex,MutexGuard,PoisonError,TryLockError,TryLockResult}};

struct Data {rows:VecDeque<ClipboardEntry>,revision:u64}
pub struct SessionHistoryStore(Mutex<Data>);
pub struct SessionHistoryGuard<'a> {data:MutexGuard<'a,Data>,mutated:bool}
impl Default for SessionHistoryStore {fn default()->Self {Self::new(VecDeque::new())}}
impl SessionHistoryStore {
    pub fn new(rows:VecDeque<ClipboardEntry>)->Self {Self(Mutex::new(Data{rows,revision:0}))}
    pub fn lock(&self)->LockResult<SessionHistoryGuard<'_>> {
        self.0.lock().map(|data|SessionHistoryGuard{data,mutated:false})
            .map_err(|error|PoisonError::new(SessionHistoryGuard{data:error.into_inner(),mutated:false}))
    }
    pub fn try_lock(&self)->TryLockResult<SessionHistoryGuard<'_>> {
        self.0.try_lock().map(|data|SessionHistoryGuard{data,mutated:false}).map_err(|error|match error {
            TryLockError::WouldBlock=>TryLockError::WouldBlock,
            TryLockError::Poisoned(error)=>TryLockError::Poisoned(PoisonError::new(SessionHistoryGuard{data:error.into_inner(),mutated:false})),
        })
    }
    pub fn revision(&self)->Result<u64,&'static str> {self.lock().map_err(|_|"SessionHistoryUnavailable")?.revision()}
}
impl SessionHistoryGuard<'_> {
    pub fn revision(&self)->Result<u64,&'static str> {
        if self.data.revision>9_007_199_254_740_991 {Err("SessionHistoryRevisionExhausted")}else{Ok(self.data.revision)}
    }
}
impl Deref for SessionHistoryGuard<'_> {type Target=VecDeque<ClipboardEntry>;fn deref(&self)->&Self::Target {&self.data.rows}}
impl DerefMut for SessionHistoryGuard<'_> {
    fn deref_mut(&mut self)->&mut Self::Target {
        if !self.mutated {self.data.revision=self.data.revision.saturating_add(1);self.mutated=true;}
        &mut self.data.rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(id:i64)->ClipboardEntry {serde_json::from_value(serde_json::json!({"id":id,"content_type":"text","content":"body","preview":"same preview","source_app":"synthetic","timestamp":1,"is_pinned":false})).unwrap()}
    #[test]
    fn reads_do_not_advance_and_every_mutable_guard_covers_edits_order_removal_and_rollback() {
        let store=SessionHistoryStore::new(VecDeque::from([entry(-1),entry(-2)]));
        assert_eq!(store.revision(),Ok(0));assert_eq!(store.lock().unwrap().iter().count(),2);assert_eq!(store.revision(),Ok(0));
        {let mut rows=store.lock().unwrap();rows.iter_mut().find(|row|row.id== -1).unwrap().content="changed body with same preview".into();}
        assert_eq!(store.revision(),Ok(1));
        {let mut rows=store.lock().unwrap();let first=rows.pop_front().unwrap();rows.push_back(first);}
        assert_eq!(store.revision(),Ok(2));
        {let mut rows=store.lock().unwrap();rows.retain(|row|row.id!= -2);rows.push_back(entry(-3));rows.pop_front();}
        assert_eq!(store.revision(),Ok(3));
        {let mut rows=store.lock().unwrap();let original=rows.clone();rows.clear();*rows=original;}
        assert_eq!(store.revision(),Ok(4));
    }
    #[test]
    fn a_reader_holds_one_snapshot_and_a_concurrent_mutator_commits_only_after_release() {
        let store=std::sync::Arc::new(SessionHistoryStore::new(VecDeque::from([entry(-1)])));
        let read=store.lock().unwrap();let original=read.revision().unwrap();
        let other=store.clone();let (observed,receive)=std::sync::mpsc::channel();
        let worker=std::thread::spawn(move|| {
            assert!(matches!(other.try_lock(),Err(TryLockError::WouldBlock)));observed.send(()).unwrap();
            other.lock().unwrap().push_back(entry(-2));
        });
        receive.recv().unwrap();assert_eq!(read.len(),1);assert_eq!(read.revision(),Ok(original));
        drop(read);worker.join().unwrap();assert_eq!(store.revision(),Ok(original+1));assert_eq!(store.lock().unwrap().len(),2);
    }
    #[test]
    fn revision_exhaustion_cannot_wrap_into_a_previously_accepted_revision() {
        let store=SessionHistoryStore::default();store.0.lock().unwrap().revision=9_007_199_254_740_991;
        store.lock().unwrap().push_back(entry(-1));assert_eq!(store.revision(),Err("SessionHistoryRevisionExhausted"));
        store.0.lock().unwrap().revision=u64::MAX;store.lock().unwrap().clear();assert!(store.revision().is_err());
    }
}
