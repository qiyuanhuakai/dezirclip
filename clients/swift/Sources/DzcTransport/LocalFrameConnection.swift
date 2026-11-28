import Darwin
import Dispatch
import DzcProtocol
import Foundation

public enum LocalFrameError: Error, Equatable {
    case invalidLength
    case invalidUTF8
    case nestingLimit
    case timedOut
    case disconnected
    case exchangeInProgress
    case socket(Int32)
}

/// Framed transport after the launcher verifies the operating system peer.
/// The caller must use an independent connection for a concurrent long poll.
public actor LocalFrameConnection {
    public static let maximumFrameBytes = WireLimits.maximumFrameBytes
    private var descriptor: Int32?
    private var inFlight = false
    private var usable = true

    /// Takes ownership of the already authenticated socket descriptor, even on failure.
    public init(verifiedSocket: Int32) throws {
        var noSignal: Int32 = 1
        guard setsockopt(verifiedSocket, SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout<Int32>.size)) == 0 else {
            let failure = LocalFrameError.socket(errno)
            Darwin.close(verifiedSocket)
            throw failure
        }
        descriptor = verifiedSocket
    }

    deinit { if let descriptor { Darwin.close(descriptor) } }

    /// Retires a stream whose protocol exchange could not be trusted.
    public func invalidate() {
        usable = false
        guard let descriptor else { return }
        if inFlight {
            // The I/O worker still holds the raw descriptor. Unblock its
            // wait before closing so it cannot touch a recycled descriptor.
            _ = Darwin.shutdown(descriptor, SHUT_RDWR)
        } else {
            self.descriptor = nil
            Darwin.close(descriptor)
        }
    }

    /// A single deadline covers the request and the entire response frame.
    /// A failed exchange retires the stream because its frame boundary is unknown.
    public func exchange(_ request: Data, timeoutMilliseconds: UInt32 = 2_000) async throws -> Data {
        guard usable, let socket = descriptor else { throw LocalFrameError.disconnected }
        guard !inFlight else { throw LocalFrameError.exchangeInProgress }
        guard !request.isEmpty, request.count <= Self.maximumFrameBytes else { throw LocalFrameError.invalidLength }
        try Self.validateJSONPayload(request)
        try Task.checkCancellation()
        inFlight = true
        defer {
            inFlight = false
            if !usable { invalidate() }
        }
        let deadline = DispatchTime.now().uptimeNanoseconds + UInt64(timeoutMilliseconds) * 1_000_000
        let worker = Task.detached {
            try Self.transact(socket, request, until: deadline)
        }
        do {
            let response = try await withTaskCancellationHandler {
                try await worker.value
            } onCancel: {
                worker.cancel()
            }
            try Task.checkCancellation()
            guard usable else { throw LocalFrameError.disconnected }
            return response
        } catch {
            invalidate()
            throw error
        }
    }

    private static func transact(_ fd: Int32, _ payload: Data, until deadline: UInt64) throws -> Data {
        var length = UInt32(payload.count).bigEndian
        var request = Data(bytes: &length, count: 4)
        request.append(payload)
        try request.withUnsafeBytes { bytes in
            try transfer(fd, count: bytes.count, events: POLLOUT, until: deadline) { offset in
                Darwin.send(fd, bytes.baseAddress!.advanced(by: offset), bytes.count - offset, MSG_DONTWAIT)
            }
        }

        let header = try receive(fd, count: 4, until: deadline)
        let count = header.reduce(0) { ($0 << 8) | Int($1) }
        guard count > 0, count <= maximumFrameBytes else { throw LocalFrameError.invalidLength }
        let response = try receive(fd, count: count, until: deadline)
        try validateJSONPayload(response)
        return response
    }

    /// Checks UTF-8 and nesting even for JSON reassembled from opaque chunks.
    public static func validateJSONPayload(_ bytes: Data) throws {
        guard String(data: bytes, encoding: .utf8) != nil else { throw LocalFrameError.invalidUTF8 }
        var depth = 0
        var quoted = false
        var escaped = false
        for byte in bytes {
            if quoted {
                if escaped { escaped = false }
                else if byte == 92 { escaped = true }
                else if byte == 34 { quoted = false }
            } else {
                switch byte {
                case 34: quoted = true
                case 91, 123:
                    depth += 1
                    if depth > WireLimits.maximumJSONDepth { throw LocalFrameError.nestingLimit }
                case 93, 125: depth = max(0, depth - 1)
                default: break
                }
            }
        }
    }

    private static func receive(_ fd: Int32, count: Int, until deadline: UInt64) throws -> Data {
        var bytes = Data(count: count)
        try bytes.withUnsafeMutableBytes { storage in
            try transfer(fd, count: count, events: POLLIN, until: deadline) { offset in
                Darwin.recv(fd, storage.baseAddress!.advanced(by: offset), count - offset, MSG_DONTWAIT)
            }
        }
        return bytes
    }

    private static func transfer(
        _ fd: Int32, count: Int, events: Int32, until deadline: UInt64, operation: (Int) -> Int
    ) throws {
        var offset = 0
        while offset < count {
            try Task.checkCancellation()
            let now = DispatchTime.now().uptimeNanoseconds
            guard now < deadline else { throw LocalFrameError.timedOut }
            // Bound each wait so task cancellation does not wait for the I/O deadline.
            let remaining = (deadline - now + 999_999) / 1_000_000
            var interest = pollfd(fd: fd, events: Int16(events), revents: 0)
            let ready = Darwin.poll(&interest, 1, Int32(min(remaining, 50)))
            if ready == 0 || (ready < 0 && errno == EINTR) { continue }
            if ready < 0 { throw LocalFrameError.socket(errno) }
            try Task.checkCancellation()
            let transferred = operation(offset)
            if transferred < 0 && (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK) { continue }
            if transferred < 0 && (errno == EPIPE || errno == ECONNRESET || errno == ENOTCONN) {
                throw LocalFrameError.disconnected
            }
            if transferred < 0 { throw LocalFrameError.socket(errno) }
            if transferred == 0 { throw LocalFrameError.disconnected }
            offset += transferred
        }
    }
}
