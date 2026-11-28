import Darwin
import Dispatch
import Foundation

public enum LocalFrameError: Error, Equatable {
    case invalidLength
    case invalidUTF8
    case timedOut
    case disconnected
    case exchangeInProgress
    case socket(Int32)
}

/// Framed transport after the launcher verifies the operating system peer.
/// The caller must use an independent connection for a concurrent long poll.
public actor LocalFrameConnection {
    public static let maximumFrameBytes = 1_048_576
    private let descriptor: Int32
    private var inFlight = false
    private var usable = true

    /// Takes ownership of the already authenticated socket descriptor.
    public init(verifiedSocket: Int32) {
        descriptor = verifiedSocket
    }

    deinit { Darwin.close(descriptor) }

    /// A single deadline covers the request and the entire response frame.
    /// A failed exchange retires the stream because its frame boundary is unknown.
    public func exchange(_ request: Data, timeoutMilliseconds: UInt32 = 2_000) async throws -> Data {
        guard usable else { throw LocalFrameError.disconnected }
        guard !inFlight else { throw LocalFrameError.exchangeInProgress }
        guard !request.isEmpty, request.count <= Self.maximumFrameBytes else { throw LocalFrameError.invalidLength }
        guard String(data: request, encoding: .utf8) != nil else { throw LocalFrameError.invalidUTF8 }
        try Task.checkCancellation()
        inFlight = true
        defer { inFlight = false }

        let socket = descriptor
        let deadline = DispatchTime.now().uptimeNanoseconds + UInt64(timeoutMilliseconds) * 1_000_000
        let worker = Task.detached {
            try Self.transact(socket, request, until: deadline)
        }
        do {
            return try await withTaskCancellationHandler {
                try await worker.value
            } onCancel: {
                worker.cancel()
            }
        } catch {
            usable = false
            _ = Darwin.shutdown(descriptor, SHUT_RDWR)
            throw error
        }
    }

    private static func transact(_ fd: Int32, _ payload: Data, until deadline: UInt64) throws -> Data {
        var noSignal: Int32 = 1
        guard setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout<Int32>.size)) == 0 else {
            throw LocalFrameError.socket(errno)
        }
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
        guard String(data: response, encoding: .utf8) != nil else { throw LocalFrameError.invalidUTF8 }
        return response
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
            if transferred < 0 { throw LocalFrameError.socket(errno) }
            if transferred == 0 { throw LocalFrameError.disconnected }
            offset += transferred
        }
    }
}
