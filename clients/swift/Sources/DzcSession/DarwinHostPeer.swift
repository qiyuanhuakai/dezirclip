import Darwin
import Foundation
import Security

public struct DarwinHostLaunchError: LocalizedError, Equatable, Sendable {
    public let message: String
    public var errorDescription: String? { message }
    package init(_ message: String) { self.message = message }
}

package enum DarwinHostPeer {
    package static func connect(to path: String, timeout: Duration) async throws -> Int32 {
        var address = sockaddr_un()
        let bytes = path.utf8CString.map(UInt8.init(bitPattern:))
        try check(path.hasPrefix("/") && !bytes.dropLast().contains(0)
                  && bytes.count <= MemoryLayout.size(ofValue: address.sun_path), "invalid local socket path")
        let length = MemoryLayout.size(ofValue: address.sun_len) + MemoryLayout.size(ofValue: address.sun_family) + bytes.count
        address.sun_family = sa_family_t(AF_UNIX)
        address.sun_len = UInt8(length)
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: bytes) }

        let deadline = ContinuousClock.now + timeout
        while ContinuousClock.now < deadline {
            try Task.checkCancellation()
            let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
            try check(fd >= 0, "socket creation failed")
            var transferred = false
            defer { if !transferred { Darwin.close(fd) } }
            var noSignal: Int32 = 1
            try check(fcntl(fd, F_SETFD, FD_CLOEXEC) == 0 && fcntl(fd, F_SETFL, O_NONBLOCK) == 0
                      && setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout<Int32>.size)) == 0,
                      "could not configure local socket")
            let status = withUnsafePointer(to: &address) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    Darwin.connect(fd, $0, socklen_t(length))
                }
            }
            var failure = status == 0 ? 0 : errno
            if failure == EINPROGRESS {
                failure = try await pendingConnection(fd, until: deadline)
            }
            try Task.checkCancellation()
            if failure == 0 {
                transferred = true
                return fd
            }
            try check([ENOENT, ECONNREFUSED, EAGAIN, EINTR].contains(failure), "socket connection failed")
            try await Task.sleep(for: .milliseconds(20))
        }
        throw DarwinHostLaunchError("helper did not open its socket")
    }

    private static func pendingConnection(_ fd: Int32, until deadline: ContinuousClock.Instant) async throws -> Int32 {
        while ContinuousClock.now < deadline {
            var interest = pollfd(fd: fd, events: Int16(POLLOUT), revents: 0)
            let ready = Darwin.poll(&interest, 1, 0)
            if ready > 0 {
                var error: Int32 = 0
                var length = socklen_t(MemoryLayout<Int32>.size)
                try check(getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &length) == 0, "socket connection failed")
                return error
            }
            try check(ready == 0 || errno == EINTR, "socket connection failed")
            try await Task.sleep(for: .milliseconds(20))
        }
        throw DarwinHostLaunchError("helper socket connection timed out")
    }

    package static func requireChild(_ fd: Int32, pid: Int32, expectedHash: Data) throws {
        var uid: uid_t = 0
        var gid: gid_t = 0
        try check(getpeereid(fd, &uid, &gid) == 0 && uid == geteuid(), "incorrect socket user")
        var peerPID: pid_t = 0
        var length = socklen_t(MemoryLayout<pid_t>.size)
        try check(getsockopt(fd, SOL_LOCAL, LOCAL_PEERPID, &peerPID, &length) == 0
                  && length == MemoryLayout<pid_t>.size && peerPID == pid, "incorrect socket process")

        // Use the live process identified by the kernel's audit token.
        var token = [UInt32](repeating: 0, count: 8)
        let audit: Data = try token.withUnsafeMutableBytes { storage in
            var count = socklen_t(storage.count)
            try check(getsockopt(fd, SOL_LOCAL, LOCAL_PEERTOKEN, storage.baseAddress, &count) == 0
                      && count == storage.count, "socket audit token unavailable")
            return Data(storage)
        }
        var process: SecCode?
        let attributes = [kSecGuestAttributeAudit as String: audit] as CFDictionary
        try check(SecCodeCopyGuestWithAttributes(nil, attributes, [], &process) == errSecSuccess, "unrecognized signed process")
        guard let process else { throw DarwinHostLaunchError("missing signed process") }
        try check(SecCodeCheckValidity(process, [], nil) == errSecSuccess, "invalid live helper signature")
        var live: SecStaticCode?
        try check(SecCodeCopyStaticCode(process, [], &live) == errSecSuccess, "missing live helper code")
        guard let live else { throw DarwinHostLaunchError("missing comparable helper code") }
        try check(codeHash(live) == expectedHash, "helper code identity differs")
    }

    package static func installedCodeHash(_ executable: URL) throws -> Data {
        var installed: SecStaticCode?
        try check(SecStaticCodeCreateWithPath(executable as CFURL, [], &installed) == errSecSuccess, "missing expected helper code")
        guard let installed else { throw DarwinHostLaunchError("missing comparable helper code") }
        try check(SecStaticCodeCheckValidity(installed, [], nil) == errSecSuccess, "invalid expected helper signature")
        return try codeHash(installed)
    }

    package static func bundledHelper() throws -> (executable: URL, hash: Data) {
        let bundle = Bundle.main.bundleURL.resolvingSymlinksInPath()
        try check(bundle.pathExtension == "app", "the Host must be launched from the signed app bundle")
        let helper = bundle.appendingPathComponent("Contents/Helpers/dzc-darwin-host", isDirectory: false)
        try check(helper.resolvingSymlinksInPath().path == helper.path, "the bundled Host path contains a symbolic link")
        let values = try helper.resourceValues(forKeys: [.isRegularFileKey, .isExecutableKey])
        try check(values.isRegularFile == true && values.isExecutable == true, "the bundle does not contain an executable Host")
        // Pin the helper identity before validating the seal and launching it.
        let hash = try installedCodeHash(helper)
        var signedBundle: SecStaticCode?
        try check(SecStaticCodeCreateWithPath(bundle as CFURL, [], &signedBundle) == errSecSuccess,
                  "the app bundle signature is unavailable")
        guard let signedBundle else { throw DarwinHostLaunchError("the app bundle code object is missing") }
        let flags = SecCSFlags(rawValue: UInt32(kSecCSCheckNestedCode) | UInt32(kSecCSStrictValidate))
        try check(SecStaticCodeCheckValidity(signedBundle, flags, nil) == errSecSuccess,
                  "the app bundle or nested Host signature is invalid")
        return (helper, hash)
    }

    private static func codeHash(_ code: SecStaticCode) throws -> Data {
        var information: CFDictionary?
        try check(SecCodeCopySigningInformation(code, SecCSFlags(rawValue: UInt32(kSecCSSigningInformation)), &information) == errSecSuccess,
                  "missing signing information")
        guard let hash = (information as? [String: Any])?[kSecCodeInfoUnique as String] as? Data, !hash.isEmpty else {
            throw DarwinHostLaunchError("missing code directory hash")
        }
        return hash
    }

    private static func check(_ condition: Bool, _ message: String) throws {
        if !condition { throw DarwinHostLaunchError(message) }
    }
}
