import AppKit
import Darwin
import DzcTransport
import Foundation
import Security

struct ProbeFailure: Error {
    let message: String
    init(_ message: String) { self.message = message }
}

func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw ProbeFailure(message) }
}

func connect(to path: String, until deadline: Date) throws -> Int32 {
    var address = sockaddr_un()
    let bytes = path.utf8CString.map(UInt8.init(bitPattern:))
    try require(bytes.count <= MemoryLayout.size(ofValue: address.sun_path), "socket path too long")
    let length = MemoryLayout.size(ofValue: address.sun_len) + MemoryLayout.size(ofValue: address.sun_family) + bytes.count
    address.sun_family = sa_family_t(AF_UNIX)
    address.sun_len = UInt8(length)
    withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: bytes) }
    repeat {
        let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        try require(fd >= 0, "socket creation failed")
        let status = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, socklen_t(length))
            }
        }
        if status == 0 {
            var timeout = timeval(tv_sec: 2, tv_usec: 0)
            var noSignal: Int32 = 1
            if setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size)) == 0,
               setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size)) == 0,
               setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout<Int32>.size)) == 0 {
                return fd
            }
            Darwin.close(fd)
            throw ProbeFailure("could not bound socket I/O")
        }
        let error = errno
        Darwin.close(fd)
        try require(error == ENOENT || error == ECONNREFUSED, "socket connection failed")
        Thread.sleep(forTimeInterval: 0.025)
    } while Date() < deadline
    throw ProbeFailure("helper did not open its socket")
}

func requireChild(_ fd: Int32, pid: Int32, executable: URL) throws {
    var uid: uid_t = 0
    var gid: gid_t = 0
    try require(getpeereid(fd, &uid, &gid) == 0 && uid == geteuid(), "incorrect socket user")
    var peerPID: pid_t = 0
    var length = socklen_t(MemoryLayout<pid_t>.size)
    try require(getsockopt(fd, SOL_LOCAL, LOCAL_PEERPID, &peerPID, &length) == 0
                && length == MemoryLayout<pid_t>.size && peerPID == pid, "incorrect socket process")

    // Ask Security.framework about the live peer identified by the kernel's
    // audit token. The PID by itself may have been reused.
    var token = [UInt32](repeating: 0, count: 8)
    let audit: Data = try token.withUnsafeMutableBytes { storage in
        var count = socklen_t(storage.count)
        try require(getsockopt(fd, SOL_LOCAL, LOCAL_PEERTOKEN, storage.baseAddress, &count) == 0
                    && count == storage.count, "socket audit token unavailable")
        return Data(storage)
    }
    var process: SecCode?
    let attributes = [kSecGuestAttributeAudit as String: audit] as CFDictionary
    try require(SecCodeCopyGuestWithAttributes(nil, attributes, [], &process) == errSecSuccess, "unrecognized signed process")
    guard let process else { throw ProbeFailure("missing signed process") }
    try require(SecCodeCheckValidity(process, [], nil) == errSecSuccess, "invalid live helper signature")

    var live: SecStaticCode?
    var installed: SecStaticCode?
    try require(SecCodeCopyStaticCode(process, [], &live) == errSecSuccess, "missing live helper code")
    try require(SecStaticCodeCreateWithPath(executable as CFURL, [], &installed) == errSecSuccess, "missing expected helper code")
    guard let live, let installed else { throw ProbeFailure("missing comparable helper code") }
    try require(SecStaticCodeCheckValidity(installed, [], nil) == errSecSuccess, "invalid expected helper signature")
    func codeHash(_ code: SecStaticCode) throws -> Data {
        var information: CFDictionary?
        try require(SecCodeCopySigningInformation(code, SecCSFlags(rawValue: UInt32(kSecCSSigningInformation)), &information) == errSecSuccess,
                    "missing signing information")
        guard let hash = (information as? [String: Any])?[kSecCodeInfoUnique as String] as? Data, !hash.isEmpty else {
            throw ProbeFailure("missing code directory hash")
        }
        return hash
    }
    let liveHash = try codeHash(live)
    let installedHash = try codeHash(installed)
    try require(liveHash == installedHash, "helper code identity differs")
}

@MainActor
func request(_ connection: LocalFrameConnection, id: String, method: String, params: [String: Any]) async throws -> [String: Any] {
    let payload = try JSONSerialization.data(withJSONObject: ["request_id": id, "method": method, "params": params])
    let body = try await connection.exchange(payload)
    guard let response = try JSONSerialization.jsonObject(with: body) as? [String: Any],
          response["request_id"] as? String == id, response["error"] == nil,
          let result = response["result"] as? [String: Any] else {
        throw ProbeFailure("unexpected protocol response")
    }
    return result
}

func verifyCancelledRead(helper: URL, directory: String, socket: String) throws {
    let child = Process()
    let lifetime = Pipe()
    child.executableURL = helper
    child.arguments = [directory, "--darwin-parent"]
    child.standardInput = lifetime
    try child.run()
    defer {
        if child.isRunning { child.terminate() }
        child.waitUntilExit()
    }
    let fd = try connect(to: socket, until: Date().addingTimeInterval(5))
    defer { Darwin.close(fd) }
    try requireChild(fd, pid: child.processIdentifier, executable: helper)
    var partialHeader: UInt8 = 0
    try require(Darwin.write(fd, &partialHeader, 1) == 1, "failed to start a partial frame")
    try lifetime.fileHandleForWriting.close()
    let deadline = Date().addingTimeInterval(2)
    while child.isRunning && Date() < deadline { Thread.sleep(forTimeInterval: 0.02) }
    try require(!child.isRunning && child.terminationStatus == 0, "closing the parent did not cancel the partial frame")
}

@main struct DarwinPeerProbe {
    @MainActor
    static func main() async throws {
        let args = CommandLine.arguments
        try require(args.count == 3, "expected helper and private directory")
        let helper = URL(fileURLWithPath: args[1])
        let socket = URL(fileURLWithPath: args[2]).appendingPathComponent("host.sock").path
        let child = Process()
        let lifetime = Pipe()
        child.executableURL = helper
        child.arguments = [args[2], "--darwin-parent"]
        child.standardInput = lifetime
        try child.run()
        defer {
            if child.isRunning { child.terminate() }
            child.waitUntilExit()
        }
        let fd = try connect(to: socket, until: Date().addingTimeInterval(5))
        var adopted = false
        defer { if !adopted { Darwin.close(fd) } }
        do {
            try requireChild(fd, pid: getpid(), executable: helper)
            throw ProbeFailure("accepted unexpected process")
        } catch let failure as ProbeFailure where failure.message == "incorrect socket process" {}
        do {
            try requireChild(fd, pid: child.processIdentifier, executable: URL(fileURLWithPath: "/usr/bin/true"))
            throw ProbeFailure("accepted unexpected code")
        } catch let failure as ProbeFailure where failure.message == "helper code identity differs" {}
        try requireChild(fd, pid: child.processIdentifier, executable: helper)
        let connection = LocalFrameConnection(verifiedSocket: fd)
        adopted = true
        let contender = Process()
        contender.executableURL = helper
        contender.arguments = ["--capture-lease-check"]
        contender.standardError = FileHandle.nullDevice
        try contender.run()
        contender.waitUntilExit()
        try require(contender.terminationStatus == 3, "second helper acquired the capture lease")
        let hello = try await request(connection, id: "swift-hello", method: "hello", params: [
            "protocol_version": ["major": 1, "minor": 0],
            "client_version": "darwin-fixture", "client_role": "cli",
            "capabilities": [], "required_capabilities": [],
        ])
        let state = hello["state"] as? [String: Any]
        try require(state?["service_instance_id"] as? String == "synthetic-instance", "wrong fixture instance")
        let clipboard = NSPasteboard.general
        let before = clipboard.changeCount
        let health = try await request(connection, id: "swift-health", method: "health", params: [:])
        guard let marker = health["clipboard_text"] as? String, marker.hasPrefix("darwin-pasteboard-") else {
            throw ProbeFailure("missing native clipboard receipt")
        }
        try require(clipboard.changeCount != before && clipboard.string(forType: .string) == marker,
                    "system pasteboard did not receive the helper's text")
        let list = try await request(connection, id: "swift-list", method: "surface.history.list", params: ["limit": 20, "offset": 0])
        try require((list["entries"] as? [Any])?.isEmpty == true, "invalid fixture page")
        try require(list["keychain_verified"] as? Bool == true, "missing helper keychain proof")
        child.waitUntilExit()
        try require(child.terminationStatus == 0, "helper exited unsuccessfully")
        try verifyCancelledRead(helper: helper, directory: args[2], socket: socket)
        print("Verified the Darwin UID, live signed helper, framed Rust wire responses, system pasteboard, isolated Keychain and cancelled partial read")
    }
}
