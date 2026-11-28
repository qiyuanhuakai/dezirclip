import Darwin
import Foundation

private struct HostBootstrap: Decodable, Sendable {
    let instance: String
    let grant: String
    let socket: String

    func validate() throws {
        guard UUID(uuidString: instance) != nil, socket.hasPrefix("/"),
              grant.utf8.count == 64, grant.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }) else {
            throw DarwinHostLaunchError("invalid helper bootstrap receipt")
        }
    }
}

/// Owns the signed helper and the inherited pipe that bounds its lifetime.
/// Obtain independent sessions for queries, subscriptions and clipboard work.
public actor BundledDesktopHost {
    // The Host admits 16 streams. Leave room for its other verified clients
    // and short-lived native probes while bounding this launcher's work.
    public static let maximumSessions = 8
    private let process = Process()
    private let lifetime = Pipe()
    private let output = Pipe()
    private let clientVersion: String
    private var bootstrap: HostBootstrap?
    private var expectedHash: Data?
    private var readingReceipt = false
    private var inputClosed = false
    private var sessions: Set<UUID> = []

    private init(clientVersion: String) {
        self.clientVersion = clientVersion
    }

    deinit { try? lifetime.fileHandleForWriting.close() }

    /// Use the DezirClip component version expected from the bundled helper.
    public static func launch(in directory: URL, clientVersion: String) async throws -> BundledDesktopHost {
        try await launchWithReceipt(in: directory, clientVersion: clientVersion).host
    }

    // The signed integration probe also decodes its fixture-only receipt fields.
    package static func launchWithReceipt(in directory: URL, clientVersion: String) async throws -> (host: BundledDesktopHost, receipt: Data) {
        let host = BundledDesktopHost(clientVersion: clientVersion)
        let receipt = try await host.start(in: directory)
        return (host, receipt)
    }

    public func openSession(requireCopy: Bool = false, requirePrivacyWrite: Bool = false) async throws -> DesktopSession {
        guard let bootstrap else { throw DesktopSessionError.disconnected }
        try Task.checkCancellation()
        guard sessions.count < Self.maximumSessions else { throw DesktopSessionError.sessionLimitReached }
        let id = UUID()
        sessions.insert(id)
        do {
            let fd = try await verifiedSocket()
            let session = try await DesktopSession.open(
                verifiedSocket: fd, expectedInstance: bootstrap.instance, grant: bootstrap.grant,
                clientVersion: clientVersion, requireCopy: requireCopy, requirePrivacyWrite: requirePrivacyWrite,
                onClose: { [weak self] in await self?.releaseSession(id) }
            )
            if inputClosed || !process.isRunning || Task.isCancelled {
                await session.close()
                try Task.checkCancellation()
                throw DesktopSessionError.disconnected
            }
            return session
        } catch {
            releaseSession(id)
            throw error
        }
    }

    private func releaseSession(_ id: UUID) { sessions.remove(id) }

    package func idleHelperIdentifier() throws -> Int32 {
        guard bootstrap != nil, !inputClosed, process.isRunning else {
            throw DesktopSessionError.disconnected
        }
        guard sessions.isEmpty else { throw DesktopSessionError.exchangeInProgress }
        return process.processIdentifier
    }

    /// Closes the inherited pipe and gives admitted requests time to drain.
    public func stop() async throws {
        closeInput()
        let deadline = ContinuousClock.now + .seconds(3)
        do {
            while process.isRunning && ContinuousClock.now < deadline {
                try await Task.sleep(for: .milliseconds(20))
            }
            if process.isRunning { throw DarwinHostLaunchError("helper did not drain before the deadline") }
        } catch {
            if process.isRunning { process.terminate() }
            throw error
        }
        guard process.terminationReason == .exit && process.terminationStatus == 0 else {
            throw DarwinHostLaunchError("helper exited unsuccessfully")
        }
    }

    package func verifiedSocket() async throws -> Int32 {
        guard let bootstrap, let expectedHash, !inputClosed, process.isRunning else {
            throw DesktopSessionError.disconnected
        }
        let pid = process.processIdentifier
        let fd = try await DarwinHostPeer.connect(to: bootstrap.socket, timeout: .seconds(3))
        do {
            guard !inputClosed, process.isRunning else { throw DesktopSessionError.disconnected }
            try DarwinHostPeer.requireChild(fd, pid: pid, expectedHash: expectedHash)
            return fd
        } catch {
            Darwin.close(fd)
            throw error
        }
    }

    package func sendCommand(_ command: UInt8) throws {
        guard !inputClosed, process.isRunning else { throw DesktopSessionError.disconnected }
        try lifetime.fileHandleForWriting.write(contentsOf: Data([command]))
    }

    package func readReceipt() async throws -> Data {
        guard !readingReceipt else { throw DesktopSessionError.exchangeInProgress }
        readingReceipt = true
        defer { readingReceipt = false }

        let fd = output.fileHandleForReading.fileDescriptor
        let deadline = ContinuousClock.now + .seconds(5)
        var data = Data()
        while data.count < 4096 {
            try Task.checkCancellation()
            guard !inputClosed else { throw DesktopSessionError.disconnected }
            guard ContinuousClock.now < deadline else { throw DarwinHostLaunchError("helper receipt timed out") }
            var interest = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
            let ready = Darwin.poll(&interest, 1, 0)
            if ready == 0 || (ready == -1 && errno == EINTR) {
                try await Task.sleep(for: .milliseconds(10))
                continue
            }
            guard ready > 0 else { throw DarwinHostLaunchError("helper receipt pipe unavailable") }
            var byte: UInt8 = 0
            guard Darwin.read(fd, &byte, 1) == 1 else { throw DarwinHostLaunchError("incomplete helper receipt") }
            if byte == 10 { return data }
            data.append(byte)
        }
        throw DarwinHostLaunchError("helper receipt exceeded its limit")
    }

    private func start(in directory: URL) async throws -> Data {
        try Task.checkCancellation()
        let helper = try DarwinHostPeer.bundledHelper()
        guard directory.isFileURL, directory.path.hasPrefix("/") else {
            throw DarwinHostLaunchError("helper directory must be an absolute file URL")
        }
        if !FileManager.default.fileExists(atPath: directory.path) {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false,
                                                    attributes: [.posixPermissions: 0o700])
        }
        var metadata = stat()
        guard Darwin.lstat(directory.path, &metadata) == 0, metadata.st_uid == geteuid(),
              metadata.st_mode & mode_t(S_IFMT) == mode_t(S_IFDIR), metadata.st_mode & 0o077 == 0 else {
            throw DarwinHostLaunchError("helper directory must be private and owned by the current user")
        }
        expectedHash = helper.hash
        process.executableURL = helper.executable
        process.arguments = [directory.path]
        process.standardInput = lifetime
        process.standardOutput = output
        try process.run()
        do {
            let receipt = try await readReceipt()
            let credentials = try JSONDecoder().decode(HostBootstrap.self, from: receipt)
            try credentials.validate()
            bootstrap = credentials
            let verified = try await verifiedSocket()
            Darwin.close(verified)
            return receipt
        } catch {
            closeInput()
            if process.isRunning { process.terminate() }
            throw error
        }
    }

    private func closeInput() {
        guard !inputClosed else { return }
        inputClosed = true
        bootstrap = nil
        try? lifetime.fileHandleForWriting.close()
    }
}
