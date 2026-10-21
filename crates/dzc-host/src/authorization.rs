use dzc_protocol::wire::{ClientRole, HelloParams, Permission, WireErrorCode};
use rand::RngCore;
use std::{sync::Mutex, time::{Duration, Instant}};

/// Created by trusted Host code, not a deserializable role or capability claim.
#[derive(Clone)]
pub struct Grant { token: String, role: ClientRole, permissions: Vec<Permission>, expires: Option<Instant> }
impl Grant { pub fn token(&self) -> &str { &self.token } }

#[derive(Default)]
pub struct GrantAuthority { grants: Mutex<Vec<Grant>> }
impl GrantAuthority {
    pub fn issue(&self, role: ClientRole, permissions: Vec<Permission>, lifetime: Duration) -> Result<Grant, WireErrorCode> {
        if lifetime.is_zero() || lifetime > Duration::from_secs(30 * 86400) { return Err(WireErrorCode::InvalidRequest); }
        self.issue_inner(role, permissions, Some(Instant::now() + lifetime))
    }
    /// A local private control credential lives only for this Host instance.
    pub fn issue_control(&self, role: ClientRole) -> Result<Grant, WireErrorCode> {
        if !matches!(role, ClientRole::Desktop | ClientRole::Cli) { return Err(WireErrorCode::PermissionDenied); }
        self.issue_inner(role, vec![Permission::HistoryRead, Permission::HistoryBody, Permission::ClipboardCopy,
            Permission::ClipboardPaste, Permission::PrivacyWrite, Permission::ServiceManage, Permission::SurfaceControl], None)
    }
    fn issue_inner(&self, role: ClientRole, mut permissions: Vec<Permission>, expires: Option<Instant>) -> Result<Grant, WireErrorCode> {
        if role == ClientRole::Mcp && permissions.iter().any(|permission| !matches!(permission, Permission::HistoryRead | Permission::HistoryBody)) { return Err(WireErrorCode::PermissionDenied); }
        if role == ClientRole::Surface && permissions.iter().any(|permission| !matches!(permission, Permission::HistoryRead | Permission::ClipboardCopy | Permission::ClipboardPaste | Permission::SurfaceControl)) { return Err(WireErrorCode::PermissionDenied); }
        permissions.push(Permission::HistoryRead);
        permissions.sort_by_key(|value| format!("{value:?}"));
        permissions.dedup();
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.try_fill_bytes(&mut bytes).map_err(|_| WireErrorCode::Unavailable)?;
        let grant = Grant { token: bytes.iter().map(|byte| format!("{byte:02x}")).collect(), role, permissions, expires };
        let mut grants = self.grants.lock().map_err(|_| WireErrorCode::Unavailable)?;
        grants.retain(|grant| grant.expires.is_none_or(|expiry| expiry > Instant::now()));
        if grants.len() >= 32 { return Err(WireErrorCode::Busy); }
        grants.push(grant.clone());
        Ok(grant)
    }
    pub fn revoke(&self, grant: &Grant) {
        if let Ok(mut grants) = self.grants.lock() { grants.retain(|candidate| candidate.token != grant.token); }
    }
    pub(crate) fn revoke_delegated(&self, token: &str) -> Result<(), WireErrorCode> {
        let mut grants = self.grants.lock().map_err(|_| WireErrorCode::Unavailable)?;
        grants.retain(|grant| grant.expires.is_none() || grant.token != token);
        Ok(())
    }
    pub(crate) fn accept(&self, hello: &HelloParams) -> Result<Session, WireErrorCode> {
        let Some(token) = &hello.grant else { return Ok(Session { role: hello.client_role, permissions: vec![Permission::HistoryRead], grant: None }); };
        let grants = self.grants.lock().map_err(|_| WireErrorCode::Unavailable)?;
        let grant = grants.iter().find(|grant| grant.token == *token && grant.role == hello.client_role
            && grant.expires.is_none_or(|expiry| expiry > Instant::now())).ok_or(WireErrorCode::PermissionDenied)?;
        Ok(Session { role: hello.client_role, permissions: grant.permissions.clone(), grant: Some(grant.clone()) })
    }
    pub(crate) fn still_valid(&self, session: &Session) -> bool {
        let Some(grant) = &session.grant else { return true; };
        self.grants.lock().is_ok_and(|grants| grants.iter().any(|current| current.token == grant.token
            && current.expires.is_none_or(|expiry| expiry > Instant::now())))
    }
}

pub struct Session { role: ClientRole, permissions: Vec<Permission>, grant: Option<Grant> }
impl Session {
    pub fn role(&self) -> ClientRole { self.role }
    pub fn permissions(&self) -> &[Permission] { &self.permissions }
    pub(crate) fn require(&self, permission: Permission) -> Result<(), WireErrorCode> {
        if self.permissions.contains(&permission) { Ok(()) } else { Err(WireErrorCode::PermissionDenied) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hello(role: ClientRole) -> HelloParams {
        HelloParams { protocol_version: dzc_protocol::wire::VERSION, client_version: "synthetic".into(), client_role: role,
            capabilities: vec!["service_manage".into()], required_capabilities: vec![], grant: None }
    }
    #[test]
    fn claimed_roles_cannot_grant_authority_and_tokens_are_role_bound_and_revocable() {
        let authority = GrantAuthority::default();
        for role in [ClientRole::Desktop, ClientRole::Surface, ClientRole::Cli, ClientRole::Mcp] {
            let session = authority.accept(&hello(role)).unwrap();
            assert_eq!(session.require(Permission::ServiceManage), Err(WireErrorCode::PermissionDenied));
            assert_eq!(session.require(Permission::HistoryBody), Err(WireErrorCode::PermissionDenied));
        }
        let grant = authority.issue_control(ClientRole::Cli).unwrap();
        let mut request = hello(ClientRole::Mcp); request.grant = Some(grant.token().into());
        assert!(authority.accept(&request).is_err());
        request.client_role = ClientRole::Cli;
        let session = authority.accept(&request).unwrap();
        assert!(session.require(Permission::ServiceManage).is_ok());
        authority.revoke(&grant);
        assert!(!authority.still_valid(&session));
        assert!(authority.issue_control(ClientRole::Mcp).is_err());
        assert!(authority.issue(ClientRole::Surface, vec![Permission::HistoryBody], Duration::from_secs(1)).is_err());
    }
}
