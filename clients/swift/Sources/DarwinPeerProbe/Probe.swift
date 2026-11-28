import AppKit
import Darwin
import DzcProtocol
import DzcSession
import DzcTransport
import Foundation

struct ProbeFailure: Error {
    let message: String
    init(_ message: String) { self.message = message }
}

private struct HelloRequest: Encodable {
    let requestID = "swift-hello"
    let method = "hello"
    let params: HelloParams

    private enum CodingKeys: String, CodingKey {
        case requestID = "request_id"
        case method, params
    }
}

private struct HelloResponse: Decodable {
    let requestID: String
    let result: HelloResult

    private enum CodingKeys: String, CodingKey {
        case requestID = "request_id"
        case result, error
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        try require(!container.contains(.error), "hello returned an error")
        requestID = try container.decode(String.self, forKey: .requestID)
        result = try container.decode(HelloResult.self, forKey: .result)
    }
}

func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw ProbeFailure(message) }
}

private func requireChild(_ fd: Int32, pid: Int32, executable: URL) throws {
    try DarwinHostPeer.requireChild(fd, pid: pid, expectedHash: DarwinHostPeer.installedCodeHash(executable))
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

func verifyCancelledRead(helper: URL, directory: String, socket: String) async throws {
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
    let fd = try await DarwinHostPeer.connect(to: socket, timeout: .seconds(5))
    defer { Darwin.close(fd) }
    try requireChild(fd, pid: child.processIdentifier, executable: helper)
    var partialHeader: UInt8 = 0
    try require(Darwin.write(fd, &partialHeader, 1) == 1, "failed to start a partial frame")
    try lifetime.fileHandleForWriting.close()
    let deadline = Date().addingTimeInterval(2)
    while child.isRunning && Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
    try require(!child.isRunning && child.terminationStatus == 0, "closing the parent did not cancel the partial frame")
}

@main struct DarwinPeerProbe {
    @MainActor
    static func main() async throws {
        let args = CommandLine.arguments
        if args.count == 3, args[1] == "--foreign-host" {
            try await verifyForeignProcessRejected(socket: args[2])
            return
        }
        if args.count == 3, ["--bundled-host", "--bundled-production"].contains(args[1]) {
            do {
                let version = Bundle.main.object(forInfoDictionaryKey: "DezirClipCoreVersion") as? String ?? ""
                let directory = URL(fileURLWithPath: args[2], isDirectory: true)
                if args[1] == "--bundled-production" {
                    try await verifyProductionHost(directory: directory, version: version)
                } else {
                    try await verifySharedHost(directory: directory, version: version)
                }
            } catch {
                let message = (error as? DarwinHostLaunchError)?.message ?? (error as? ProbeFailure)?.message ?? String(describing: error)
                FileHandle.standardError.write(Data((message + "\n").utf8))
                exit(EXIT_FAILURE)
            }
            return
        }
        try require(args.count == 3, "expected the protocol helper and private directory")
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
        let fd = try await DarwinHostPeer.connect(to: socket, timeout: .seconds(5))
        var adopted = false
        defer { if !adopted { Darwin.close(fd) } }
        do {
            try requireChild(fd, pid: getpid(), executable: helper)
            throw ProbeFailure("accepted unexpected process")
        } catch let failure as DarwinHostLaunchError where failure.message == "incorrect socket process" {}
        do {
            try requireChild(fd, pid: child.processIdentifier, executable: URL(fileURLWithPath: "/usr/bin/true"))
            throw ProbeFailure("accepted unexpected code")
        } catch let failure as DarwinHostLaunchError where failure.message == "helper code identity differs" {}
        try requireChild(fd, pid: child.processIdentifier, executable: helper)
        adopted = true
        let connection = try LocalFrameConnection(verifiedSocket: fd)
        let contender = Process()
        contender.executableURL = helper
        contender.arguments = ["--capture-lease-check"]
        contender.standardError = FileHandle.nullDevice
        try contender.run()
        contender.waitUntilExit()
        try require(contender.terminationStatus == 3, "second helper acquired the capture lease")
        let helloRequest = try HelloRequest(params: HelloParams(
            protocolVersion: ProtocolVersion(major: 1, minor: 0),
            clientVersion: "darwin-fixture", clientRole: .cli,
            capabilities: [], requiredCapabilities: []
        ))
        let reply = try await connection.exchange(JSONEncoder().encode(helloRequest))
        let hello = try JSONDecoder().decode(HelloResponse.self, from: reply)
        try require(hello.requestID == helloRequest.requestID && hello.result.protocolVersion.major == 1,
                    "handshake request ID or version differs")
        try require(hello.result.permissions == [.historyRead], "wrong fixture permissions")
        try require(hello.result.state.serviceInstanceId == "synthetic-instance", "wrong fixture instance")
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
        try await verifyCancelledRead(helper: helper, directory: args[2], socket: socket)
        print("Verified the Darwin UID, live signed helper, framed Rust wire responses, system pasteboard, isolated Keychain and cancelled partial read")
    }
}
