//! One Host-local revision covers persistent history and the versioned RAM queue.
use dzc_protocol::wire::WireErrorCode;
#[derive(Default)]
pub(crate) struct HistoryVersion {observed:Option<(u64,u64)>,revision:u64}
impl HistoryVersion {
    pub fn observe(&mut self,database:u64,session:u64)->Result<u64,WireErrorCode> {
        let next=(database,session);
        if self.observed.is_none() {self.revision=database;}
        else if self.observed!=Some(next) {self.revision=self.revision.checked_add(1).ok_or(WireErrorCode::Unavailable)?.max(database);}
        if self.revision>9_007_199_254_740_991 {return Err(WireErrorCode::Unavailable);}
        self.observed=Some(next);Ok(self.revision)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn database_and_ram_each_advance_the_same_monotonic_host_revision() {
        let mut version=HistoryVersion::default();
        assert_eq!(version.observe(8,1),Ok(8));assert_eq!(version.observe(8,1),Ok(8));
        assert_eq!(version.observe(8,2),Ok(9));assert_eq!(version.observe(8,3),Ok(10));
        assert_eq!(version.observe(9,3),Ok(11));assert_eq!(version.observe(20,3),Ok(20));
        assert_eq!(version.observe(0,3),Ok(21));
        assert!(version.observe(9_007_199_254_740_992,3).is_err());
    }
}
