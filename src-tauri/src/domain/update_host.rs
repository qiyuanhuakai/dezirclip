//! Desktop Host coordination after verified download and before package installation.
pub trait UpdateHost: Send + Sync {
    /// Normally notify owned auxiliary clients and confirm their exit within
    /// the Host's bounded deadline. An unconfirmed client must return an error.
    fn prepare_install(&self) -> Result<(), ()>;
    /// Idempotently restore desired clients after partial preparation or failed
    /// installation. A Host implementation must not panic during coordination.
    fn install_aborted(&self) {}
}

pub struct NoAuxiliaryClients;
impl UpdateHost for NoAuxiliaryClients {
    fn prepare_install(&self) -> Result<(), ()> { Ok(()) }
}

pub fn install_after_host_ready<T>(host: &dyn UpdateHost, install: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    if host.prepare_install().is_err() {
        host.install_aborted();
        return Err("update_participants_not_ready".into());
    }
    let result = install();
    if result.is_err() { host.install_aborted(); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Host { ready: bool, events: Mutex<Vec<&'static str>> }
    impl UpdateHost for Host {
        fn prepare_install(&self) -> Result<(), ()> {
            self.events.lock().unwrap().push("notify_and_confirm");
            self.ready.then_some(()).ok_or(())
        }
        fn install_aborted(&self) { self.events.lock().unwrap().push("restore_desired_clients"); }
    }
    #[test]
    fn an_unconfirmed_exit_blocks_the_installer() {
        let host = Host { ready: false, events: Mutex::new(vec![]) };
        let result = install_after_host_ready(&host, || { panic!("installer must not run") });
        assert_eq!(result, Err::<(), _>("update_participants_not_ready".into()));
        assert_eq!(*host.events.lock().unwrap(), vec!["notify_and_confirm", "restore_desired_clients"]);
    }
    #[test]
    fn install_failure_notifies_the_host_after_confirmation() {
        let host = Host { ready: true, events: Mutex::new(vec![]) };
        let result = install_after_host_ready(&host, || Err::<(), _>("install_failed".into()));
        assert_eq!(result, Err("install_failed".into()));
        assert_eq!(*host.events.lock().unwrap(), vec!["notify_and_confirm", "restore_desired_clients"]);
        assert_eq!(install_after_host_ready(&NoAuxiliaryClients, || Ok(7)), Ok(7));
    }
    #[test]
    fn successful_install_does_not_restore_old_clients() {
        let host = Host { ready: true, events: Mutex::new(vec![]) };
        assert_eq!(install_after_host_ready(&host, || { host.events.lock().unwrap().push("install"); Ok(()) }), Ok(()));
        assert_eq!(*host.events.lock().unwrap(), vec!["notify_and_confirm", "install"]);
    }
}
