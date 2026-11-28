import AppKit
import Darwin
import DzcProtocol
import DzcSession
import Foundation

private struct Bootstrap: Decodable {
    let instance: String
    let grant: String
    let entryId: Int64
    let marker: String
    let socket: String

    private enum CodingKeys: String, CodingKey {
        case instance, grant, marker, socket
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

/// Runs in a same-user process other than the helper's actual launcher.
func verifyForeignProcessRejected(socket: String) async throws {
    let fd = try await DarwinHostPeer.connect(to: socket, timeout: .seconds(2))
    defer { Darwin.close(fd) }
    // The listener checks the actual parent PID before reading a frame. An
    // accepted peer would stay open here until the Host's read deadline.
    var interest = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
    try require(Darwin.poll(&interest, 1, 1_500) > 0, "Host kept an unrelated socket open")
    var byte: UInt8 = 0
    let count = Darwin.recv(fd, &byte, 1, MSG_DONTWAIT)
    try require(count == 0 || (count < 0 && errno == ECONNRESET), "Host did not close the unrelated socket")
}

@MainActor
func verifySharedHost(directory: URL, version: String) async throws {
    let (host, receipt) = try await BundledDesktopHost.launchWithReceipt(in: directory, clientVersion: version)
    do {
        let credentials = try JSONDecoder().decode(Bootstrap.self, from: receipt)
        try await verifyStartedHost(host, credentials: credentials, version: version)
    } catch {
        try? await host.stop()
        throw error
    }
}

@MainActor
private func verifyStartedHost(_ host: BundledDesktopHost, credentials: Bootstrap, version: String) async throws {
    let socket = credentials.socket
    let control = URL(fileURLWithPath: socket).deletingLastPathComponent().appendingPathComponent("host.control.json")
    try require(!FileManager.default.fileExists(atPath: control.path),
                "inherited grant was written into discovery")
    let foreign = Process()
    foreign.executableURL = URL(fileURLWithPath: CommandLine.arguments[0])
    foreign.arguments = ["--foreign-host", socket]
    try foreign.run()
    foreign.waitUntilExit()
    try require(foreign.terminationStatus == 0, "Host accepted an unrelated process")
    do {
        let fd = try await host.verifiedSocket()
        _ = try await DesktopSession.open(verifiedSocket: fd, expectedInstance: credentials.instance,
                                          grant: String(repeating: "a", count: 64), clientVersion: version)
        throw ProbeFailure("Host accepted an invalid Desktop grant")
    } catch DesktopSessionError.remote(.permissionDenied) {}
    do {
        let fd = try await host.verifiedSocket()
        _ = try await DesktopSession.open(verifiedSocket: fd, expectedInstance: credentials.instance,
                                          grant: credentials.grant, clientVersion: version + "-mismatch")
        throw ProbeFailure("Desktop accepted a signed Host from a different version")
    } catch DesktopSessionError.incompatibleHost {}
    do {
        _ = try await host.openSession(requirePrivacyWrite: true)
        throw ProbeFailure("a grant without privacy permission opened a privacy mutation session")
    } catch DesktopSessionError.remote(.unsupportedCapability) {}

    // An identity probe can close before the server reads the peer credentials.
    for _ in 0..<8 {
        let idle = try await host.verifiedSocket()
        Darwin.close(idle)
    }
    let query = try await host.openSession()
    let events = try await host.openSession()
    let actions = try await host.openSession(requireCopy: true)
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
    let joined = events.hello.state
    let replay = try RuntimeState(serviceInstanceId: joined.serviceInstanceId, historyRevision: joined.historyRevision,
                                  privacyEpoch: joined.privacyEpoch, eventSequence: 0)
    let published = try await events.subscribe(after: replay)
    try require(published.events.contains { $0.event == .entryAdded && $0.data.entryId == credentials.entryId },
                "real Host did not publish its history event")

    let polling = Task {
        try await events.subscribe(after: published.state, waitMs: 5_000)
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
    let resumed = try await host.openSession()
    try require(resumed.hello.state.serviceInstanceId == events.hello.state.serviceInstanceId &&
                resumed.hello.sessionId != events.hello.sessionId, "reconnect reused the retired session")
    let resumedEvents = try await resumed.subscribe(after: published.state)
    try require(!resumedEvents.resyncRequired, "reconnecting to the same Host lost a retained event cursor")

    try await host.sendCommand(UInt8(ascii: "p"))
    let privacy = try await JSONDecoder().decode(PrivacyReceipt.self, from: host.readReceipt())
    try require(privacy.privacyEpoch > selected.state.privacyEpoch, "privacy epoch did not advance")
    let previousClipboard = clipboard.changeCount
    do {
        _ = try await actions.copy(id: credentials.entryId, state: selected.state)
        throw ProbeFailure("stale clipboard selection was copied")
    } catch DesktopSessionError.remote(.privacyEpochChanged) {}
    try require(clipboard.changeCount == previousClipboard, "rejected copy changed the system clipboard")

    try await host.sendCommand(UInt8(ascii: "r"))
    let revoked = try await JSONDecoder().decode(RevokeReceipt.self, from: host.readReceipt())
    try require(revoked.revoked, "helper did not revoke the grant")
    do {
        _ = try await query.historyPage(DesktopPageParams(limit: 20, offset: 0))
        throw ProbeFailure("revoked session read history")
    } catch DesktopSessionError.remote(.permissionDenied) {}
    await query.close()
    await actions.close()
    await resumed.close()
    let partial = try await host.verifiedSocket()
    defer { Darwin.close(partial) }
    var firstByte: UInt8 = 0
    try require(Darwin.write(partial, &firstByte, 1) == 1, "could not start a partial frame before shutdown")
    try await host.stop()
    try await host.stop()
    do {
        _ = try await host.openSession()
        throw ProbeFailure("a stopped launcher opened a new session")
    } catch DesktopSessionError.disconnected {}
    print("Verified signed Swift sessions against the shared Host: parent restriction, grant, matching component version, inline page, chunked history, events, actual pasteboard, stale epoch, revocation and shutdown")
}
