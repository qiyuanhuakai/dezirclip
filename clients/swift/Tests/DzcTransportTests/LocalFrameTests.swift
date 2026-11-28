import Darwin
import DzcProtocol
import DzcSession
import DzcTransport
import Foundation
import Testing

private func socketPair() throws -> (Int32, Int32) {
    var descriptors: [Int32] = [0, 0]
    let status = descriptors.withUnsafeMutableBufferPointer {
        Darwin.socketpair(AF_UNIX, SOCK_STREAM, 0, $0.baseAddress)
    }
    guard status == 0 else { throw LocalFrameError.socket(errno) }
    var timeout = timeval(tv_sec: 1, tv_usec: 0)
    guard setsockopt(descriptors[1], SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size)) == 0 else {
        let error = LocalFrameError.socket(errno)
        Darwin.close(descriptors[0])
        Darwin.close(descriptors[1])
        throw error
    }
    return (descriptors[0], descriptors[1])
}

private func sockets() throws -> (LocalFrameConnection, Int32) {
    let (client, peer) = try socketPair()
    return (LocalFrameConnection(verifiedSocket: client), peer)
}

private func write(_ fd: Int32, _ data: Data) throws {
    try FileHandle(fileDescriptor: fd, closeOnDealloc: false).write(contentsOf: data)
}

@Test func framedExchangeUsesNetworkByteOrderAndUtf8() async throws {
    let (stream, peer) = try sockets()
    defer { Darwin.close(peer) }
    // Split the header and payload across writes against the real socket.
    try write(peer, Data([0, 0]))
    try write(peer, Data([0, 3]))
    try write(peer, Data("中".utf8))
    let result = try await stream.exchange(Data("{}".utf8))
    #expect(result == Data("中".utf8))
    let request = try FileHandle(fileDescriptor: peer, closeOnDealloc: false).read(upToCount: 6)
    #expect(request == Data([0, 0, 0, 2, 123, 125]))
}

@Test func malformedResponseRetiresTheConnection() async throws {
    for (bytes, expected) in [
        ([UInt8(0), 16, 0, 1], LocalFrameError.invalidLength),
        ([UInt8(0), 0, 0, 0], LocalFrameError.invalidLength),
        ([UInt8(0), 0, 0, 1, 255], LocalFrameError.invalidUTF8),
    ] {
        let (stream, peer) = try sockets()
        defer { Darwin.close(peer) }
        try write(peer, Data(bytes))
        await #expect(throws: expected) { try await stream.exchange(Data("{}".utf8)) }
        await #expect(throws: LocalFrameError.disconnected) { try await stream.exchange(Data("{}".utf8)) }
    }
}

@Test func partialFrameUsesOneDeadline() async throws {
    let (stream, peer) = try sockets()
    defer { Darwin.close(peer) }
    try write(peer, Data([0, 0, 0, 4, 123]))
    await #expect(throws: LocalFrameError.timedOut) {
        try await stream.exchange(Data("{}".utf8), timeoutMilliseconds: 55)
    }
}

@Test func cancellingAnOutstandingExchangeDoesNotWaitForTheDeadline() async throws {
    let (stream, peer) = try sockets()
    defer { Darwin.close(peer) }
    let worker = Task.detached { try await stream.exchange(Data("{}".utf8), timeoutMilliseconds: 5_000) }
    let request = try FileHandle(fileDescriptor: peer, closeOnDealloc: false).read(upToCount: 6)
    #expect(request == Data([0, 0, 0, 2, 123, 125]))
    let clock = ContinuousClock()
    let start = clock.now
    worker.cancel()
    await #expect(throws: CancellationError.self) { try await worker.value }
    #expect(clock.now - start < .seconds(1))
    await #expect(throws: LocalFrameError.disconnected) { try await stream.exchange(Data("{}".utf8)) }
}

@Test func framingRejectsDeepJSONAndIgnoresBracketsInsideStrings() async throws {
    let (stream, peer) = try sockets()
    defer { Darwin.close(peer) }

    let depth = WireLimits.maximumJSONDepth + 1
    let nested = Data((String(repeating: "[", count: depth) + "0" + String(repeating: "]", count: depth)).utf8)
    await #expect(throws: LocalFrameError.nestingLimit) { try await stream.exchange(nested) }

    let quoted = Data(("\"" + String(repeating: #"\"[{"#, count: depth) + "\"").utf8)
    var length = UInt32(quoted.count).bigEndian
    try write(peer, Data(bytes: &length, count: 4) + quoted)
    let result = try await stream.exchange(Data("{}".utf8))
    #expect(result == quoted)

    length = UInt32(nested.count).bigEndian
    try write(peer, Data(bytes: &length, count: 4) + nested)
    await #expect(throws: LocalFrameError.nestingLimit) { try await stream.exchange(Data("{}".utf8)) }
    await #expect(throws: LocalFrameError.disconnected) { try await stream.exchange(Data("{}".utf8)) }
}

private func hostState() -> [String: Any] {
    ["service_instance_id": "fixture", "history_revision": 4, "privacy_epoch": 2, "event_sequence": 7]
}

private func respond(_ peer: Int32, id: String, result: [String: Any]) throws {
    let body = try JSONSerialization.data(withJSONObject: ["request_id": id, "result": result])
    var length = UInt32(body.count).bigEndian
    try write(peer, Data(bytes: &length, count: 4) + body)
}

private func hello(_ peer: Int32, permissions: [String] = ["history_read", "desktop_history", "clipboard_copy"]) throws {
    try respond(peer, id: "1", result: [
        "session_id": "test-session", "protocol_version": ["major": 1, "minor": 0], "core_version": "0.2.0",
        "permissions": permissions, "state": hostState(), "max_frame_bytes": WireLimits.maximumFrameBytes,
    ])
}

private func openDesktop(_ socket: Int32, expectedInstance: String = "fixture") async throws -> DesktopSession {
    try await DesktopSession.open(verifiedSocket: socket, expectedInstance: expectedInstance,
                                  grant: String(repeating: "a", count: 64), clientVersion: "test", requireCopy: true)
}

@Test func desktopSessionRejectsWrongInstanceAndMissingPermissions() async throws {
    for (instance, permissions, error) in [
        ("replacement", ["history_read", "desktop_history", "clipboard_copy"], DesktopSessionError.wrongInstance),
        ("fixture", ["history_read"], DesktopSessionError.missingPermission(.desktopHistory)),
    ] {
        let (socket, peer) = try socketPair()
        defer { Darwin.close(peer) }
        try hello(peer, permissions: permissions)
        await #expect(throws: error) { try await openDesktop(socket, expectedInstance: instance) }
    }
}

@Test func desktopSessionRejectsAReplyWithWrongCorrelation() async throws {
    let (socket, peer) = try socketPair()
    defer { Darwin.close(peer) }
    try hello(peer)
    try respond(peer, id: "previous", result: ["events": [], "state": hostState(), "resync_required": true])
    let session = try await openDesktop(socket)
    let params = try SubscribeParams(afterSequence: 0)
    await #expect(throws: DesktopSessionError.invalidResponse) { try await session.subscribe(params) }
    await #expect(throws: DesktopSessionError.disconnected) { try await session.subscribe(params) }
}

@Test func privacyMutationRequiresPermissionAndLostReplyIsAmbiguous() async throws {
    let (readSocket, readPeer) = try socketPair()
    defer { Darwin.close(readPeer) }
    try hello(readPeer)
    let read = try await openDesktop(readSocket)
    let enable = try PrivacySetParams(active: true)
    await #expect(throws: DesktopSessionError.missingPermission(.privacyWrite)) { try await read.setPrivacy(enable) }
    try respond(readPeer, id: "2", result: ["active": false, "privacy_epoch": 2, "expires_at_ms": NSNull()])
    let unchanged = try await read.privacyState()
    #expect(!unchanged.active && unchanged.privacyEpoch == 2 && unchanged.expiresAtMs == nil)
    let encoded = try JSONEncoder().encode(unchanged)
    let fields = try #require(try JSONSerialization.jsonObject(with: encoded) as? [String: Any])
    #expect(fields["expires_at_ms"] is NSNull)
    #expect(throws: DecodingError.self) {
        try JSONDecoder().decode(PrivacyState.self, from: Data(#"{"active":false,"privacy_epoch":2}"#.utf8))
    }
    await read.close()

    let (writeSocket, writePeer) = try socketPair()
    defer { Darwin.close(writePeer) }
    try hello(writePeer, permissions: ["history_read", "desktop_history", "clipboard_copy", "privacy_write"])
    let write = try await openDesktop(writeSocket)
    #expect(Darwin.shutdown(writePeer, SHUT_WR) == 0)
    await #expect(throws: DesktopSessionError.privacyOutcomeUnknown) { try await write.setPrivacy(enable) }
    await #expect(throws: DesktopSessionError.disconnected) { try await write.privacyState() }
}

@Test func desktopSessionReturnsResyncAndRejectsInvalidSnapshotChunks() async throws {
    let (socket, peer) = try socketPair()
    defer { Darwin.close(peer) }
    try hello(peer)
    try respond(peer, id: "2", result: ["events": [], "state": hostState(), "resync_required": true])
    try respond(peer, id: "3", result: [
        "snapshot_id": String(repeating: "1", count: 32), "total_bytes": 2, "state": hostState(),
    ])
    try respond(peer, id: "4", result: [
        "data_base64": "e30=", "offset": 0, "next_offset": 1, "finished": true, "state": hostState(),
    ])
    let session = try await openDesktop(socket)
    let events = try await session.subscribe(SubscribeParams(afterSequence: 0))
    #expect(events.resyncRequired)
    let page = try DesktopPageParams(limit: 20, offset: 0)
    await #expect(throws: DesktopSessionError.invalidSnapshot) { try await session.historyPage(page) }
    await #expect(throws: DesktopSessionError.disconnected) { try await session.historyPage(page) }
}

@Test func desktopSessionReportsAnAmbiguousCopyAndDoesNotReuseTheStream() async throws {
    let (socket, peer) = try socketPair()
    defer { Darwin.close(peer) }
    try hello(peer)
    #expect(Darwin.shutdown(peer, SHUT_WR) == 0)
    let session = try await openDesktop(socket)
    let state = session.hello.state
    await #expect(throws: DesktopSessionError.copyOutcomeUnknown) { try await session.copy(id: 1, state: state) }
    await #expect(throws: DesktopSessionError.disconnected) { try await session.copy(id: 1, state: state) }
}
