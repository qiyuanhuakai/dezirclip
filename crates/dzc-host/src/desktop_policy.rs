//! All policy mutations execute in the capture owner's Host, including RAM
//! session entries. Successful responses follow confirmed native revocation.
use crate::{Host,Session};
use dzc_core::api::{PrivacyAccessIntent,SensitiveAccess};
use dzc_protocol::wire::*;
use serde_json::Value;
fn blocked(error:impl ToString)->WireErrorCode {crate::router::privacy_error(error)}
fn value<T:serde::Serialize>(schema:&str,value:T)->Result<Value,WireErrorCode> {
    let value=serde_json::to_value(value).map_err(|_|WireErrorCode::Internal)?;
    dzc_protocol::wire::validate(schema,&value).map_err(|_|WireErrorCode::Internal)?;Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc,Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}};
    #[derive(Default)]
    struct Actions {fail:AtomicBool,revocations:AtomicUsize}
    impl crate::HostActions for Actions {
        fn platform_capabilities(&self)->Value {serde_json::json!({})}
        fn can_copy(&self)->bool {false}
        fn copy(&self,_:dzc_core::ports::ClipboardPayload)->Result<(),WireErrorCode> {Err(WireErrorCode::Unavailable)}
        fn can_paste(&self)->bool {false}
        fn paste(&self)->Result<(),WireErrorCode> {Err(WireErrorCode::Unavailable)}
        fn stop_helpers(&self)->Result<(),WireErrorCode> {Ok(())}
        fn revoke_private_exports(&self)->Result<(),WireErrorCode> {
            self.revocations.fetch_add(1,Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {Err(WireErrorCode::Busy)}else{Ok(())}
        }
    }
    fn host()->(Arc<Host>,Arc<Actions>) {
        let original=crate::router::tests::synthetic_host();let actions=Arc::new(Actions::default());
        let host=Host::with_session(crate::HostOptions{history:original.history.clone(),directory:original.directory.clone(),mode:HostMode::Service,actions:actions.clone()},Arc::new(Mutex::new(Default::default()))).unwrap();
        (host,actions)
    }
    fn session(host:&Host,role:ClientRole,control:bool)->Session {
        let grant=control.then(||host.grants().issue_control(role).unwrap());
        host.hello(HelloParams{protocol_version:VERSION,client_version:"fixture".into(),client_role:role,capabilities:vec![],required_capabilities:vec![],grant:grant.map(|grant|grant.token().into())}).unwrap().0
    }
    fn entry(id:i64,text:&str)->dzc_core::api::ClipboardEntry {
        serde_json::from_value(serde_json::json!({"id":id,"content_type":"text","content":text,"preview":text,"source_app":"Synthetic Editor","timestamp":1,"is_pinned":false,"tags":[]})).unwrap()
    }
    fn call(host:&Host,session:&Session,method:Method,id:i64,action:Option<&str>,rule:Option<&str>)->Response {
        let state=host.snapshot().unwrap();
        host.dispatch(session,Request{request_id:"policy-fixture".into(),method,params:serde_json::to_value(DesktopPolicyParams{service_instance_id:state.service_instance_id,expected_epoch:state.privacy_epoch,id:Some(id),action:action.map(str::to_owned),rule_id:rule.map(str::to_owned)}).unwrap()})
    }
    #[test]
    fn policy_uses_the_owners_real_session_and_revokes_before_ack() {
        let (host,actions)=host();let desktop=session(&host,ClientRole::Desktop,true);
        host.session.lock().unwrap().push_back(entry(-1,"synthetic session payload"));
        let before=host.snapshot().unwrap();actions.fail.store(true,Ordering::SeqCst);
        let denied=call(&host,&desktop,Method::DesktopPrivacyFeedback,-1,Some("protect_source"),None);
        assert_eq!(denied.error.unwrap().code,WireErrorCode::Busy);
        let after=host.snapshot().unwrap();assert!(after.privacy_epoch>before.privacy_epoch);
        assert_eq!(after.event_sequence,before.event_sequence);
        assert!(host.session.lock().unwrap()[0].tags.iter().any(|tag|tag=="sensitive"));
        actions.fail.store(false,Ordering::SeqCst);
        let report=call(&host,&desktop,Method::DesktopPrivacyDecision,-1,None,None);
        let serialized=serde_json::to_string(report.result.as_ref().unwrap()).unwrap();
        assert!(!serialized.contains("synthetic session payload"));assert!(!serialized.contains("content"));
        assert!(host.snapshot().unwrap().event_sequence>before.event_sequence);
        assert!(actions.revocations.load(Ordering::SeqCst)>=2);
        // Exact exceptions bind the real session item, and removal re-protects
        // that same item rather than an unrelated GUI process's RAM history.
        assert!(call(&host,&desktop,Method::DesktopPrivacyFeedback,-1,Some("exact_exception"),None).error.is_none());
        assert!(!host.session.lock().unwrap()[0].tags.iter().any(|tag|tag=="sensitive"));
        let rule=host.history.privacy_policy().rules().unwrap().into_iter().find(|rule|rule.kind==dzc_core::api::PrivacyRuleKind::ExactException).unwrap();
        assert!(call(&host,&desktop,Method::DesktopPrivacyRemoveRule,-1,None,Some(&rule.id)).error.is_none());
        assert!(host.session.lock().unwrap()[0].tags.iter().any(|tag|tag=="sensitive"));
    }
    #[test]
    fn policy_and_session_mutation_require_grant_original_instance_and_epoch() {
        let (host,_)=host();host.session.lock().unwrap().push_back(entry(-7,"ordinary RAM item"));
        for role in [ClientRole::Desktop,ClientRole::Cli,ClientRole::Mcp,ClientRole::Surface] {
            let client=session(&host,role,false);
            assert_eq!(call(&host,&client,Method::DesktopPrivacyFeedback,-7,Some("not_sensitive"),None).error.unwrap().code,WireErrorCode::PermissionDenied);
        }
        let cli=session(&host,ClientRole::Cli,true);
        assert_eq!(call(&host,&cli,Method::DesktopPrivacyFeedback,-7,Some("not_sensitive"),None).error.unwrap().code,WireErrorCode::PermissionDenied);
        let desktop=session(&host,ClientRole::Desktop,true);let state=host.snapshot().unwrap();
        let mut request=serde_json::json!({"service_instance_id":"previous-instance","expected_epoch":state.privacy_epoch,"id":-7,"action":"pin","pinned":true,"reveal":false});
        let dispatch=|params:Value|host.dispatch(&desktop,Request{request_id:"session-fixture".into(),method:Method::DesktopHistoryMutate,params});
        assert_eq!(dispatch(request.clone()).error.unwrap().code,WireErrorCode::PrivacyBlocked);
        assert_eq!(host.session.lock().unwrap()[0].id,-7);
        request["service_instance_id"]=state.service_instance_id.into();
        host.history.privacy().set_session(dzc_core::api::PrivacySessionCommand::On{duration_ms:None},crate::router::now_ms().unwrap()).unwrap();
        assert_eq!(dispatch(request.clone()).error.unwrap().code,WireErrorCode::PrivacyEpochChanged);
        request["expected_epoch"]=host.snapshot().unwrap().privacy_epoch.into();
        assert_eq!(dispatch(request.clone()).error.unwrap().code,WireErrorCode::ManualPersistenceConfirmationRequired);
        assert_eq!(host.session.lock().unwrap()[0].id,-7);
        request["confirm_persistence"]=true.into();
        let result=dispatch(request).result.unwrap();assert!(result["id"].as_i64().unwrap()>0);
        assert!(host.session.lock().unwrap().is_empty());
        assert!(host.history.runtime_snapshot(crate::router::now_ms().unwrap()).unwrap().session.active);
        let mut protected=entry(-8,"synthetic protected RAM item");protected.tags.push("sensitive".into());
        host.session.lock().unwrap().push_back(protected);
        let state=host.snapshot().unwrap();
        let denied=dispatch(serde_json::json!({"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch,
            "id":-8,"action":"pin","pinned":true,"reveal":false,"confirm_persistence":true}));
        assert_eq!(denied.error.unwrap().code,WireErrorCode::SensitiveAccessDenied);
        assert_eq!(host.session.lock().unwrap()[0].id,-8,"write confirmation is not a sensitive read grant");
    }
}
impl Host {
    fn policy_permit(&self,epoch:u64)->Result<dzc_core::api::PrivacyReadPermit,WireErrorCode> {
        if self.is_stopping() {return Err(WireErrorCode::Busy);}
        self.history.privacy().authorize_read(PrivacyAccessIntent::InteractiveRead,SensitiveAccess::ExplicitUserAction,Some(epoch),crate::router::now_ms()?).map_err(blocked)
    }
    fn finish_policy(&self,result:dzc_core::api::CoreResult<dzc_core::api::RuntimeSnapshot>)->Result<dzc_core::api::RuntimeSnapshot,WireErrorCode> {
        self.synchronize_privacy()?;
        let snapshot=result.map_err(blocked)?;
        if self.is_stopping() || self.snapshot()?.privacy_epoch!=snapshot.privacy_epoch {return Err(WireErrorCode::PrivacyEpochChanged);}
        Ok(snapshot)
    }
    /// Trusted in-process Desktop facade; no body is serialized through IPC.
    pub fn policy_feedback_owned(&self,id:i64,action:dzc_core::api::PrivacyFeedbackAction,epoch:u64)->Result<dzc_core::api::RuntimeSnapshot,WireErrorCode> {
        let _request=self.requests.lock().map_err(|_|WireErrorCode::Unavailable)?;
        let permit=self.policy_permit(epoch)?;let now=crate::router::now_ms()?;
        let result=if id<0 {let mut entries=self.session.lock().map_err(|_|WireErrorCode::Unavailable)?;let row=entries.iter_mut().find(|row|row.id==id).ok_or(WireErrorCode::NotFound)?;
            self.history.privacy_policy().feedback_for_session(row,action,&permit,now)}else{self.history.privacy_policy().feedback(id,action,&permit,now)};
        self.finish_policy(result)
    }
    pub fn policy_remove_owned(&self,id:&str,epoch:u64)->Result<dzc_core::api::RuntimeSnapshot,WireErrorCode> {
        let _request=self.requests.lock().map_err(|_|WireErrorCode::Unavailable)?;
        let permit=self.policy_permit(epoch)?;
        let result={let mut entries=self.session.lock().map_err(|_|WireErrorCode::Unavailable)?;
            self.history.privacy_policy().remove_rule_with_session(id,entries.make_contiguous(),&permit,crate::router::now_ms()?)};
        self.finish_policy(result)
    }
    pub fn policy_rules_owned(&self,epoch:u64)->Result<Vec<dzc_core::api::PrivacyRuleSummary>,WireErrorCode> {
        let _request=self.requests.lock().map_err(|_|WireErrorCode::Unavailable)?;
        let permit=self.policy_permit(epoch)?;
        let result=self.history.privacy_policy().rules().map_err(blocked)?;
        self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(blocked)?;Ok(result)
    }
    pub fn policy_decision_owned(&self,id:i64,epoch:u64)->Result<dzc_core::api::PrivacyDecisionReport,WireErrorCode> {
        let _request=self.requests.lock().map_err(|_|WireErrorCode::Unavailable)?;
        let permit=self.policy_permit(epoch)?;let now=crate::router::now_ms()?;
        if id<0 {let entries=self.session.lock().map_err(|_|WireErrorCode::Unavailable)?;let row=entries.iter().find(|row|row.id==id).ok_or(WireErrorCode::NotFound)?;
            self.history.privacy_policy().decision_for_session_entry(row,&permit,now).map_err(blocked)}
        else {self.history.privacy_policy().decision(id,&permit,now).map_err(blocked)}
    }
    pub(crate) fn desktop_policy(&self,session:&Session,method:Method,parameters:Value)->Result<Value,WireErrorCode> {
        let request:DesktopPolicyParams=serde_json::from_value(parameters).map_err(|_|WireErrorCode::InvalidRequest)?;
        self.desktop_permission(session,&request.service_instance_id)?;
        session.require(Permission::PrivacyWrite)?;
        let now=crate::router::now_ms()?;
        let permit=self.history.privacy().authorize_read(PrivacyAccessIntent::InteractiveRead,SensitiveAccess::ExplicitUserAction,Some(request.expected_epoch),now).map_err(blocked)?;
        let policy=self.history.privacy_policy();
        if method==Method::DesktopPrivacyRules {
            let rules=policy.rules().map_err(blocked)?;
            let result=value("DesktopPolicyRules",serde_json::json!({"rules":rules}))?;
            self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(blocked)?;
            return self.authorized_snapshot(session,&permit,&result);
        }
        if method==Method::DesktopPrivacyDecision {
            let id=request.id.ok_or(WireErrorCode::InvalidRequest)?;
            let report=if id<0 {
                let entries=self.session.lock().map_err(|_|WireErrorCode::Unavailable)?;
                let entry=entries.iter().find(|entry|entry.id==id).ok_or(WireErrorCode::NotFound)?;
                policy.decision_for_session_entry(entry,&permit,now)
            }else{policy.decision(id,&permit,now)}.map_err(blocked)?;
            self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(blocked)?;
            return value("DesktopPolicyDecision",report);
        }
        let result=if method==Method::DesktopPrivacyFeedback {
            let id=request.id.ok_or(WireErrorCode::InvalidRequest)?;
            let action=serde_json::from_value(serde_json::Value::String(request.action.ok_or(WireErrorCode::InvalidRequest)?)).map_err(|_|WireErrorCode::InvalidRequest)?;
            if id<0 {let mut entries=self.session.lock().map_err(|_|WireErrorCode::Unavailable)?;let entry=entries.iter_mut().find(|entry|entry.id==id).ok_or(WireErrorCode::NotFound)?;
                policy.feedback_for_session(entry,action,&permit,now)}
            else {policy.feedback(id,action,&permit,now)}
        }else {
            let id=request.rule_id.ok_or(WireErrorCode::InvalidRequest)?;
            let mut entries=self.session.lock().map_err(|_|WireErrorCode::Unavailable)?;
            policy.remove_rule_with_session(&id,entries.make_contiguous(),&permit,now)
        };
        // A cleanup error may occur after SQL committed. Revoke the actual
        // owner's exports even when the Core mutation reports an error.
        let result=self.finish_policy(result)?;
        let state=self.snapshot()?;
        if state.privacy_epoch!=result.privacy_epoch {return Err(WireErrorCode::PrivacyBlocked);}
        value("DesktopRuntimeValue",DesktopRuntimeValue{state,privacy:PrivacyState{active:result.session.active,expires_at_ms:result.session.expires_at_ms,privacy_epoch:result.privacy_epoch}})
    }
}
