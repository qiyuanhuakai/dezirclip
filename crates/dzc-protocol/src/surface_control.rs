//! Private inherited-pipe control messages, separate from authenticated Host IPC.
use crate::wire::{self, SurfaceBuildIdentity, SurfaceControlMessage, SurfaceControlType};
use std::io::{self, Read, Write};

pub const MAX_FRAME_BYTES: usize = 64 * 1024;
#[derive(Clone, Copy)]
pub enum Direction { ParentToChild, ChildToParent }
fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidData, "InvalidSurfaceControl") }
fn check(message: &SurfaceControlMessage, direction: Direction) -> io::Result<()> {
    if message.protocol_version != wire::VERSION { return Err(invalid()); }
    let parent = matches!(message.r#type, SurfaceControlType::Initialize | SurfaceControlType::Show | SurfaceControlType::Hide
        | SurfaceControlType::PasteResult | SurfaceControlType::Shutdown);
    if parent != matches!(direction, Direction::ParentToChild) { return Err(invalid()); }
    Ok(())
}
pub fn encode(message: &SurfaceControlMessage, direction: Direction) -> io::Result<Vec<u8>> {
    check(message, direction)?;
    let raw = wire::encode("SurfaceControlMessage", message).map_err(|_| invalid())?;
    if raw.is_empty() || raw.len() > MAX_FRAME_BYTES { return Err(invalid()); }
    Ok(raw)
}
pub fn decode(raw: &[u8], direction: Direction) -> io::Result<SurfaceControlMessage> {
    if raw.is_empty() || raw.len() > MAX_FRAME_BYTES { return Err(invalid()); }
    let message = wire::decode("SurfaceControlMessage", raw).map_err(|_| invalid())?;
    check(&message, direction)?;
    Ok(message)
}
pub fn read_frame(reader: &mut impl Read, direction: Direction) -> io::Result<SurfaceControlMessage> {
    let mut header = [0u8; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES { return Err(invalid()); }
    let mut raw = vec![0; length];
    reader.read_exact(&mut raw)?;
    decode(&raw, direction)
}
pub fn write_frame(writer: &mut impl Write, message: &SurfaceControlMessage, direction: Direction) -> io::Result<()> {
    let raw = encode(message, direction)?;
    writer.write_all(&(raw.len() as u32).to_be_bytes())?;
    writer.write_all(&raw)?;
    writer.flush()
}
pub fn same_build(actual: &SurfaceBuildIdentity, expected: &SurfaceBuildIdentity) -> bool {
    actual == expected && actual.source_revision.len() == 40 && actual.source_revision.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && actual.build_id == format!("{}@{}", actual.component_version, actual.source_revision)
        && !actual.component_version.is_empty() && !actual.target.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn stopped() -> SurfaceControlMessage {
        SurfaceControlMessage { protocol_version: wire::VERSION, request_id: "stop-1".into(), r#type: SurfaceControlType::Stopped,
            payload: serde_json::json!({"surface_session_id":"child-1"}) }
    }
    #[test]
    fn private_control_is_big_endian_bounded_and_directional() {
        let message = stopped(); let mut bytes = Vec::new();
        write_frame(&mut bytes, &message, Direction::ChildToParent).unwrap();
        assert_eq!(u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize, bytes.len() - 4);
        assert_eq!(read_frame(&mut Cursor::new(&bytes), Direction::ChildToParent).unwrap(), message);
        assert!(read_frame(&mut Cursor::new(bytes), Direction::ParentToChild).is_err());
        for bad in [vec![0,0,0,0], vec![0,1,0,1], vec![0,0,0,2,b'{']] {
            assert!(read_frame(&mut Cursor::new(bad), Direction::ChildToParent).is_err());
        }
    }
    #[test]
    fn invalid_payload_version_and_unrequested_body_are_rejected() {
        let mut message = stopped(); message.protocol_version.minor = 1;
        assert!(encode(&message, Direction::ChildToParent).is_err());
        message.protocol_version = wire::VERSION; message.payload["body"] = "not a control field".into();
        assert!(encode(&message, Direction::ChildToParent).is_err());
        message.payload = serde_json::json!({"surface_session_id":"child-1"}); message.request_id = "bad id".into();
        assert!(encode(&message, Direction::ChildToParent).is_err());
    }
    #[test]
    fn source_version_target_and_build_identifier_must_all_match() {
        let build = SurfaceBuildIdentity { component_version: "0.2.0-alpha.5".into(), source_revision: "a".repeat(40),
            build_id: format!("0.2.0-alpha.5@{}", "a".repeat(40)), target: "x86_64-unknown-linux-gnu".into() };
        assert!(same_build(&build, &build));
        let mut changed = build.clone(); changed.source_revision = "b".repeat(40); assert!(!same_build(&changed, &build));
        changed = build.clone(); changed.build_id = "forged".into(); assert!(!same_build(&changed, &changed));
        changed = build.clone(); changed.target = "aarch64-unknown-linux-gnu".into(); assert!(!same_build(&changed, &build));
    }
}
