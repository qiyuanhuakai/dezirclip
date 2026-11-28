import AppKit
import Darwin
import Foundation

struct DarwinProcessUsage: Encodable {
    let sampledAtTicks: UInt64
    let startedAtTicks: UInt64
    let residentBytes: UInt64
    let physicalFootprintBytes: UInt64
    let userCpuTicks: UInt64
    let systemCpuTicks: UInt64
    let pageIns: UInt64
    let packageIdleWakeups: UInt64
    let interruptWakeups: UInt64

    init(_ pid: Int32) throws {
        var info = rusage_info_v0()
        let status = withUnsafeMutablePointer(to: &info) { pointer in
            pointer.withMemoryRebound(to: rusage_info_t?.self, capacity: 1) {
                proc_pid_rusage(pid, Int32(RUSAGE_INFO_V0), $0)
            }
        }
        try require(status == 0 && info.ri_proc_start_abstime > 0 && info.ri_proc_exit_abstime == 0
                    && info.ri_phys_footprint > 0, "could not sample the running process: errno \(errno)")
        sampledAtTicks = mach_absolute_time()
        startedAtTicks = info.ri_proc_start_abstime
        residentBytes = info.ri_resident_size
        physicalFootprintBytes = info.ri_phys_footprint
        userCpuTicks = info.ri_user_time
        systemCpuTicks = info.ri_system_time
        pageIns = info.ri_pageins
        packageIdleWakeups = info.ri_pkg_idle_wkups
        interruptWakeups = info.ri_interrupt_wkups
    }

    func requireContinuation(from earlier: Self) throws {
        try require(startedAtTicks == earlier.startedAtTicks && sampledAtTicks > earlier.sampledAtTicks
                    && userCpuTicks >= earlier.userCpuTicks && systemCpuTicks >= earlier.systemCpuTicks
                    && pageIns >= earlier.pageIns && packageIdleWakeups >= earlier.packageIdleWakeups
                    && interruptWakeups >= earlier.interruptWakeups, "resource sample lost process continuity")
    }
}

struct DarwinIdleResources: Encodable {
    let helperStart: DarwinProcessUsage
    let helperEnd: DarwinProcessUsage
    let launcherStart: DarwinProcessUsage
    let launcherEnd: DarwinProcessUsage

    @MainActor
    static func measure(helper: Int32, clipboard: NSPasteboard) async throws -> Self {
        let changeCount = clipboard.changeCount
        let launcherStart = try DarwinProcessUsage(getpid())
        let helperStart = try DarwinProcessUsage(helper)
        try await Task.sleep(for: .seconds(8))
        let helperEnd = try DarwinProcessUsage(helper)
        let launcherEnd = try DarwinProcessUsage(getpid())
        try require(clipboard.changeCount == changeCount, "pasteboard changed during the idle sample")
        try helperEnd.requireContinuation(from: helperStart)
        try launcherEnd.requireContinuation(from: launcherStart)
        return Self(helperStart: helperStart, helperEnd: helperEnd,
                    launcherStart: launcherStart, launcherEnd: launcherEnd)
    }
}

struct DarwinResourceReport: Encodable {
    let schemaVersion = 1
    let componentVersion: String
    let osVersion = ProcessInfo.processInfo.operatingSystemVersionString
    let sourceRevision = ProcessInfo.processInfo.environment["GITHUB_SHA"]
    let runID = ProcessInfo.processInfo.environment["GITHUB_RUN_ID"]
    let runAttempt = ProcessInfo.processInfo.environment["GITHUB_RUN_ATTEMPT"]
    let machTimeNumerator: UInt32
    let machTimeDenominator: UInt32
    let launchToFirstPageTicks: UInt64
    let stopTicks: UInt64
    let launcherBeforeLaunch: DarwinProcessUsage
    let launcherAtFirstPage: DarwinProcessUsage
    let idle: DarwinIdleResources

    init(version: String, launchToFirstPageTicks: UInt64, stopTicks: UInt64,
         launcherBeforeLaunch: DarwinProcessUsage, launcherAtFirstPage: DarwinProcessUsage,
         idle: DarwinIdleResources) throws {
        var timebase = mach_timebase_info_data_t()
        try require(mach_timebase_info(&timebase) == KERN_SUCCESS && timebase.numer > 0 && timebase.denom > 0,
                    "could not read the Darwin clock timebase")
        try launcherAtFirstPage.requireContinuation(from: launcherBeforeLaunch)
        try idle.launcherStart.requireContinuation(from: launcherAtFirstPage)
        machTimeNumerator = timebase.numer
        machTimeDenominator = timebase.denom
        componentVersion = version
        self.launchToFirstPageTicks = launchToFirstPageTicks
        self.stopTicks = stopTicks
        self.launcherBeforeLaunch = launcherBeforeLaunch
        self.launcherAtFirstPage = launcherAtFirstPage
        self.idle = idle
    }

    func write() throws {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        encoder.outputFormatting = .sortedKeys
        let data = try encoder.encode(self)
        if let path = ProcessInfo.processInfo.environment["DZC_DARWIN_RESOURCE_REPORT"] {
            try data.write(to: URL(fileURLWithPath: path), options: .withoutOverwriting)
        }
        print("Darwin resources \(String(decoding: data, as: UTF8.self))")
    }
}
