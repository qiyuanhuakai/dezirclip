import Darwin
import DzcProtocol
import DzcTransport
import Foundation
import Testing

private func sockets() throws -> (LocalFrameConnection, Int32) {
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
    return (LocalFrameConnection(verifiedSocket: descriptors[0]), descriptors[1])
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
