import AppKit
import Darwin
import Dispatch
import DzcProtocol
import DzcSession
import DzcTransport
import Foundation

private struct Bootstrap: Decodable {
    let instance: String
    let grant: String
    let entryId: Int64
    let marker: String

    private enum CodingKeys: String, CodingKey {
        case instance, grant, marker
        case entryId = "entry_id"
    }
}

private struct PrivacyReceipt: Decodable {
    let privacyEpoch: UInt64
    private enum CodingKeys: String, CodingKey { case privacyEpoch = "privacy_epoch" }
}

private struct RevokeReceipt: Decodable {
    let revoked: Bool
}

private func receipt<T: Decodable>(_ pipe: Pipe) throws -> T {
    let fd = pipe.fileHandleForReading.fileDescriptor
    let deadline = DispatchTime.now().uptimeNanoseconds + 5_000_000_000
    var data = Data()
    while data.count < 4096 {
        let now = DispatchTime.now().uptimeNanoseconds
        guard now < deadline else { throw ProbeFailure("helper receipt timed out") }
        let remaining = Int32((deadline - now + 999_999) / 1_000_000)
        var interest = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
        let ready = Darwin.poll(&interest, 1, remaining)
        if ready == 0 || (ready == -1 && errno == EINTR) { continue }
        try require(ready > 0, "helper receipt pipe unavailable")
        var byte: UInt8 = 0
        try require(Darwin.read(fd, &byte, 1) == 1, "incomplete helper receipt")
        if byte == 10 { return try JSONDecoder().decode(T.self, from: data) }
        data.append(byte)
    }
    throw ProbeFailure("helper receipt exceeded its limit")
}

/// Runs in a same-user process other than the helper's actual launcher.
func verifyForeignProcessRejected(socket: String) async throws {
    let fd = try connect(to: socket, until: Date().addingTimeInterval(2))
    let connection = LocalFrameConnection(verifiedSocket: fd)
    let request = Data(#"{"request_id":"foreign","method":"hello","params":{"protocol_version":{"major":1,"minor":0},"client_version":"foreign","client_role":"desktop","capabilities":[],"required_capabilities":[]}}"#.utf8)
    do {
        _ = try await connection.exchange(request)
    } catch LocalFrameError.disconnected {
        return
    } catch LocalFrameError.socket(let code) where code == ECONNRESET || code == EPIPE {
        return
    }
    throw ProbeFailure("unrelated process received a Host response")
}

@MainActor
func verifySharedHost(helper: URL, directory: URL) async throws {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false,
                                            attributes: [.posixPermissions: 0o700])
    let socket = directory.appendingPathComponent("host.sock").path
    let child = Process()
    let lifetime = Pipe()
    let output = Pipe()
    child.executableURL = helper
    child.arguments = [directory.path]
    child.standardInput = lifetime
    child.standardOutput = output
    try child.run()
    defer {
        try? lifetime.fileHandleForWriting.close()
        if child.isRunning { child.terminate() }
        child.waitUntilExit()
    }
    let credentials: Bootstrap = try receipt(output)
    try require(!FileManager.default.fileExists(atPath: directory.appendingPathComponent("host.control.json").path),
                "inherited grant was written into discovery")
    func open(grant: String, requireCopy: Bool = false) async throws -> DesktopSession {
        let fd = try connect(to: socket, until: Date().addingTimeInterval(3))
        do {
            try requireChild(fd, pid: child.processIdentifier, executable: helper)
        } catch {
            Darwin.close(fd)
            throw error
        }
        return try await DesktopSession.open(verifiedSocket: fd, expectedInstance: credentials.instance,
                                             grant: grant, clientVersion: "darwin-host-fixture", requireCopy: requireCopy)
    }

    let foreign = Process()
    foreign.executableURL = URL(fileURLWithPath: CommandLine.arguments[0])
    foreign.arguments = ["--foreign-host", socket]
    try foreign.run()
    foreign.waitUntilExit()
    try require(foreign.terminationStatus == 0, "Host accepted an unrelated process")
    do {
        _ = try await open(grant: String(repeating: "a", count: 64))
        throw ProbeFailure("Host accepted an invalid Desktop grant")
    } catch DesktopSessionError.remote(.permissionDenied) {}

    let query = try await open(grant: credentials.grant)
    let events = try await open(grant: credentials.grant)
    let actions = try await open(grant: credentials.grant, requireCopy: true)
    try require(Set(actions.hello.permissions) == [.historyRead, .desktopHistory, .clipboardCopy],
                "fixture received unexpected permission")
    try require(Set([query.hello.sessionId, events.hello.sessionId, actions.hello.sessionId]).count == 3,
                "query, events and actions did not negotiate independent sessions")
    let page = try await query.historyPage(DesktopPageParams(limit: 20, offset: 0))
    try require(page.value.entries.map(\.id) == [credentials.entryId], "real history page differs")
    let params = try DesktopEntryParams(id: credentials.entryId, serviceInstanceId: credentials.instance,
                                       expectedEpoch: page.state.privacyEpoch, reveal: false)
    let selected = try await query.historyEntry(params)
    let expectedText = credentials.marker + "\n" + String(repeating: "x", count: 200_000)
    try require(selected.value.content == expectedText, "chunked history body differs")
    let published = try await events.subscribe(SubscribeParams(afterSequence: 0))
    try require(published.events.contains { $0.event == .entryAdded && $0.data.entryId == credentials.entryId },
                "real Host did not publish its history event")

    let polling = Task {
        try await events.subscribe(SubscribeParams(afterSequence: published.state.eventSequence, waitMs: 5_000))
    }
    defer { polling.cancel() }
    let clipboard = NSPasteboard.general
    let before = clipboard.changeCount
    let copied = try await actions.copy(id: credentials.entryId, state: selected.state)
    try require(copied.performed && clipboard.changeCount != before && clipboard.string(forType: .string) == expectedText,
                "authorized copy during an independent subscription did not reach the real pasteboard")
    polling.cancel()
    do {
        _ = try await polling.value
        throw ProbeFailure("cancelled subscription returned success")
    } catch is CancellationError {}
    await events.close()
    let resumed = try await open(grant: credentials.grant)
    try require(resumed.hello.state.serviceInstanceId == events.hello.state.serviceInstanceId &&
                resumed.hello.sessionId != events.hello.sessionId, "reconnect reused the retired session")

    try lifetime.fileHandleForWriting.write(contentsOf: Data([UInt8(ascii: "p")]))
    let privacy: PrivacyReceipt = try receipt(output)
    try require(privacy.privacyEpoch > selected.state.privacyEpoch, "privacy epoch did not advance")
    let previousClipboard = clipboard.changeCount
    do {
        _ = try await actions.copy(id: credentials.entryId, state: selected.state)
        throw ProbeFailure("stale clipboard selection was copied")
    } catch DesktopSessionError.remote(.privacyEpochChanged) {}
    try require(clipboard.changeCount == previousClipboard, "rejected copy changed the system clipboard")

    try lifetime.fileHandleForWriting.write(contentsOf: Data([UInt8(ascii: "r")]))
    let revoked: RevokeReceipt = try receipt(output)
    try require(revoked.revoked, "helper did not revoke the grant")
    do {
        _ = try await query.historyPage(DesktopPageParams(limit: 20, offset: 0))
        throw ProbeFailure("revoked session read history")
    } catch DesktopSessionError.remote(.permissionDenied) {}
    await query.close()
    await actions.close()
    await resumed.close()
    try lifetime.fileHandleForWriting.close()
    let deadline = Date().addingTimeInterval(3)
    while child.isRunning && Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
    try require(!child.isRunning && child.terminationStatus == 0, "shared Host did not drain on launcher pipe closure")
    print("Verified signed Swift sessions against the shared Host: parent restriction, grant, inline page, chunked history, events, actual pasteboard, stale epoch, revocation and shutdown")
}
