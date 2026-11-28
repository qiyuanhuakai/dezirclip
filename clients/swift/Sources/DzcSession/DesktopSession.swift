import Dispatch
import DzcProtocol
import DzcTransport
import Foundation

public enum DesktopSessionError: Error, Equatable {
    case remote(WireErrorCode)
    case incompatibleHost
    case missingPermission(Permission)
    case wrongInstance
    case stalePrivacyEpoch
    case invalidResponse
    case invalidSnapshot
    case exchangeInProgress
    case sessionLimitReached
    case disconnected
    case copyOutcomeUnknown
    case privacyOutcomeUnknown
}

public struct HistorySnapshot<Value: Sendable>: Sendable {
    public let state: RuntimeState
    public let value: Value
}

private struct Request<Params: Encodable>: Encodable {
    let id: String
    let method: DzcProtocol.Method
    let params: Params

    private enum CodingKeys: String, CodingKey {
        case id = "request_id"
        case method, params
    }
}

private struct Response<Result: Decodable>: Decodable {
    let id: String
    let result: Result?
    let error: WireError?

    private enum CodingKeys: String, CodingKey {
        case id = "request_id"
        case result, error
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let hasResult = container.contains(.result)
        let hasError = container.contains(.error)
        guard hasResult != hasError else { throw DesktopSessionError.invalidResponse }
        id = try container.decode(String.self, forKey: .id)
        result = hasResult ? try container.decode(Result.self, forKey: .result) : nil
        error = hasError ? try container.decode(WireError.self, forKey: .error) : nil
    }
}

/// Owns one verified local stream. Poll events on a separate session so a
/// waiting subscription or cancelled query cannot delay or retire a copy.
public actor DesktopSession {
    public nonisolated let hello: HelloResult
    private let transport: LocalFrameConnection
    private var nextID: UInt64 = 2
    private var busy = false
    private var closed = false
    private var onClose: (@Sendable () async -> Void)?
    private static let inlineID = "00000000000000000000000000000000"
    private static let inlineLimit = 32 * 1024

    private init(transport: LocalFrameConnection, hello: HelloResult, onClose: (@Sendable () async -> Void)?) {
        self.transport = transport
        self.hello = hello
        self.onClose = onClose
    }

    deinit {
        if let onClose { Task { await onClose() } }
    }

    /// Takes ownership of a socket after the launcher verifies the native
    /// peer. The instance and grant come from the trusted bootstrap.
    /// The Desktop and its bundled Host must have the same component version.
    public static func open(
        verifiedSocket: Int32, expectedInstance: String, grant: String, clientVersion: String,
        requireCopy: Bool = false, requirePrivacyWrite: Bool = false
    ) async throws -> DesktopSession {
        try await open(verifiedSocket: verifiedSocket, expectedInstance: expectedInstance, grant: grant,
                       clientVersion: clientVersion, requireCopy: requireCopy, requirePrivacyWrite: requirePrivacyWrite,
                       onClose: nil)
    }

    package static func open(
        verifiedSocket: Int32, expectedInstance: String, grant: String, clientVersion: String,
        requireCopy: Bool, requirePrivacyWrite: Bool, onClose: (@Sendable () async -> Void)?
    ) async throws -> DesktopSession {
        let transport = try LocalFrameConnection(verifiedSocket: verifiedSocket)
        let version = try ProtocolVersion(major: 1, minor: 0)
        var methods: [DzcProtocol.Method] = [.desktopHistoryPage, .desktopHistoryGet, .desktopSnapshotRead, .eventsSubscribe, .privacyGet]
        if requireCopy { methods.append(.desktopClipboardCopy) }
        if requirePrivacyWrite { methods.append(.privacySet) }
        let params = try HelloParams(
            protocolVersion: version, clientVersion: clientVersion, clientRole: .desktop,
            capabilities: ["desktop.inline_snapshot"], requiredCapabilities: methods.map(\.rawValue), grant: grant
        )
        let request = Request(id: "1", method: .hello, params: params)
        let response = try await transport.exchange(JSONEncoder().encode(request))
        let hello: HelloResult = try decode(response, expectedID: request.id)
        guard hello.protocolVersion.major == version.major, hello.coreVersion == clientVersion,
              !hello.sessionId.isEmpty else {
            await transport.invalidate()
            throw DesktopSessionError.incompatibleHost
        }
        guard !expectedInstance.isEmpty, hello.state.serviceInstanceId == expectedInstance else {
            throw DesktopSessionError.wrongInstance
        }
        var required: [Permission] = [.historyRead, .desktopHistory]
        if requireCopy { required.append(.clipboardCopy) }
        if requirePrivacyWrite { required.append(.privacyWrite) }
        for permission in required {
            guard hello.permissions.contains(permission) else { throw DesktopSessionError.missingPermission(permission) }
        }
        return DesktopSession(transport: transport, hello: hello, onClose: onClose)
    }

    public func close() async {
        closed = true
        let release = onClose
        onClose = nil
        await transport.invalidate()
        await release?()
    }

    public func historyPage(_ params: DesktopPageParams) async throws -> HistorySnapshot<DesktopEntries> {
        try await snapshot(.desktopHistoryPage, params, expectedEpoch: nil) { $0.entries.count <= Int(params.limit) }
    }

    public func historyEntry(_ params: DesktopEntryParams) async throws -> HistorySnapshot<DesktopEntry> {
        guard params.serviceInstanceId == hello.state.serviceInstanceId else { throw DesktopSessionError.wrongInstance }
        return try await snapshot(.desktopHistoryGet, params, expectedEpoch: params.expectedEpoch) { $0.id == params.id }
    }

    /// Resume from a state returned by this Host instance. After a helper
    /// restart, read a fresh page before subscribing on the new instance.
    public func subscribe(after state: RuntimeState, waitMs: UInt16 = 0, settingsOnly: Bool? = nil) async throws -> EventsResult {
        guard state.serviceInstanceId == hello.state.serviceInstanceId else { throw DesktopSessionError.wrongInstance }
        let params = try SubscribeParams(afterSequence: state.eventSequence, waitMs: waitMs, settingsOnly: settingsOnly)
        try begin()
        defer { busy = false }
        let timeout = UInt32(waitMs) + 2_000
        let response: EventsResult = try await call(.eventsSubscribe, params, timeoutMilliseconds: timeout)
        guard response.state.serviceInstanceId == hello.state.serviceInstanceId,
              response.events.allSatisfy({ $0.state.serviceInstanceId == response.state.serviceInstanceId }) else {
            await close()
            throw DesktopSessionError.wrongInstance
        }
        return response
    }

    public func privacyState() async throws -> PrivacyState {
        try begin()
        defer { busy = false }
        return try await call(.privacyGet, [String: String]())
    }

    /// A lost reply leaves the outcome unknown. Read the state on a new
    /// session before attempting another change.
    public func setPrivacy(_ params: PrivacySetParams) async throws -> PrivacyState {
        guard hello.permissions.contains(.privacyWrite) else { throw DesktopSessionError.missingPermission(.privacyWrite) }
        try begin()
        defer { busy = false }
        return try await call(.privacySet, params)
    }

    /// Copies through an explicitly authorized Desktop operation, with the
    /// instance and epoch from the user's selection. A lost reply is ambiguous.
    public func copy(id: Int64, state: RuntimeState, reveal: Bool = false) async throws -> OperationResult {
        guard hello.permissions.contains(.clipboardCopy) else { throw DesktopSessionError.missingPermission(.clipboardCopy) }
        guard state.serviceInstanceId == hello.state.serviceInstanceId else { throw DesktopSessionError.wrongInstance }
        let params = try DesktopCopyParams(
            id: id, serviceInstanceId: state.serviceInstanceId, expectedEpoch: state.privacyEpoch, reveal: reveal, paste: false
        )
        try begin()
        defer { busy = false }
        return try await call(.desktopClipboardCopy, params)
    }

    private func begin() throws {
        guard !closed else { throw DesktopSessionError.disconnected }
        guard !busy else { throw DesktopSessionError.exchangeInProgress }
        try Task.checkCancellation()
        busy = true
    }

    private func call<Params: Encodable, Result: Decodable & Sendable>(
        _ method: DzcProtocol.Method, _ params: Params, timeoutMilliseconds: UInt32 = 2_000
    ) async throws -> Result {
        guard !closed, nextID < UInt64.max else { throw DesktopSessionError.disconnected }
        let request = Request(id: String(nextID), method: method, params: params)
        nextID += 1
        let bytes = try JSONEncoder().encode(request)
        guard bytes.count <= Int(hello.maxFrameBytes) else { throw LocalFrameError.invalidLength }
        try Task.checkCancellation()
        do {
            let frame = try await transport.exchange(bytes, timeoutMilliseconds: timeoutMilliseconds)
            guard !closed else { throw DesktopSessionError.disconnected }
            return try Self.decode(frame, expectedID: request.id)
        } catch DesktopSessionError.remote(let code) {
            throw DesktopSessionError.remote(code)
        } catch {
            await close()
            if method == .desktopClipboardCopy { throw DesktopSessionError.copyOutcomeUnknown }
            if method == .privacySet { throw DesktopSessionError.privacyOutcomeUnknown }
            if let frameError = error as? LocalFrameError, frameError == .disconnected {
                throw DesktopSessionError.disconnected
            }
            throw error
        }
    }

    private static func decode<Result: Decodable>(_ frame: Data, expectedID: String) throws -> Result {
        let response = try JSONDecoder().decode(Response<Result>.self, from: frame)
        guard response.id == expectedID else { throw DesktopSessionError.invalidResponse }
        if let error = response.error { throw DesktopSessionError.remote(error.code) }
        guard let result = response.result else { throw DesktopSessionError.invalidResponse }
        return result
    }

    private func snapshot<Params: Encodable, Value: Decodable & Sendable>(
        _ method: DzcProtocol.Method, _ params: Params, expectedEpoch: UInt64?, validate: (Value) -> Bool
    ) async throws -> HistorySnapshot<Value> {
        try begin()
        defer { busy = false }
        let descriptor: BodySnapshot = try await call(method, params)
        guard descriptor.state.serviceInstanceId == hello.state.serviceInstanceId else {
            await close()
            throw DesktopSessionError.wrongInstance
        }
        if let expectedEpoch, descriptor.state.privacyEpoch != expectedEpoch {
            throw DesktopSessionError.stalePrivacyEpoch
        }
        do {
            let bytes = try await snapshotBytes(descriptor)
            try Task.checkCancellation()
            try LocalFrameConnection.validateJSONPayload(bytes)
            let value = try JSONDecoder().decode(Value.self, from: bytes)
            try Task.checkCancellation()
            guard validate(value) else { throw DesktopSessionError.invalidSnapshot }
            return HistorySnapshot(state: descriptor.state, value: value)
        } catch DesktopSessionError.remote(let code) {
            throw DesktopSessionError.remote(code)
        } catch {
            await close()
            throw error
        }
    }

    private func snapshotBytes(_ descriptor: BodySnapshot) async throws -> Data {
        let size = Int(descriptor.totalBytes)
        guard size >= 2 else { throw DesktopSessionError.invalidSnapshot }
        if let inline = descriptor.inlineJson {
            let bytes = Data(inline.utf8)
            guard descriptor.snapshotId == Self.inlineID, bytes.count == size, size <= Self.inlineLimit else {
                throw DesktopSessionError.invalidSnapshot
            }
            return bytes
        }
        guard descriptor.snapshotId != Self.inlineID else { throw DesktopSessionError.invalidSnapshot }
        var bytes = Data()
        bytes.reserveCapacity(size)
        let deadline = DispatchTime.now().uptimeNanoseconds + 5_000_000_000
        while bytes.count < size {
            let now = DispatchTime.now().uptimeNanoseconds
            guard now < deadline else { throw LocalFrameError.timedOut }
            let offset = UInt32(bytes.count)
            let request = try SnapshotReadParams(
                snapshotId: descriptor.snapshotId, offset: offset, expectedEpoch: descriptor.state.privacyEpoch,
                serviceInstanceId: descriptor.state.serviceInstanceId
            )
            let remaining = UInt32((deadline - now + 999_999) / 1_000_000)
            let chunk: SnapshotChunk = try await call(.desktopSnapshotRead, request, timeoutMilliseconds: remaining)
            guard chunk.offset == offset, chunk.state.serviceInstanceId == descriptor.state.serviceInstanceId,
                  chunk.state.privacyEpoch == descriptor.state.privacyEpoch,
                  let decoded = Data(base64Encoded: chunk.dataBase64), !decoded.isEmpty,
                  Int(chunk.nextOffset) == bytes.count + decoded.count, chunk.nextOffset <= descriptor.totalBytes,
                  chunk.finished == (chunk.nextOffset == descriptor.totalBytes) else {
                throw DesktopSessionError.invalidSnapshot
            }
            bytes.append(decoded)
        }
        return bytes
    }
}
