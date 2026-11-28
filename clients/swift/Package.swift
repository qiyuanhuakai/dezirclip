// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "DezirClipLocalClient",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "DzcProtocol", targets: ["DzcProtocol"]),
        .library(name: "DzcTransport", targets: ["DzcTransport"]),
        .library(name: "DzcSession", targets: ["DzcSession"]),
        .executable(name: "DarwinPeerProbe", targets: ["DarwinPeerProbe"]),
    ],
    targets: [
        .target(name: "DzcProtocol"),
        .target(name: "DzcTransport", dependencies: ["DzcProtocol"]),
        .target(name: "DzcSession", dependencies: ["DzcProtocol", "DzcTransport"]),
        .executableTarget(name: "DarwinPeerProbe", dependencies: ["DzcProtocol", "DzcTransport", "DzcSession"]),
        .testTarget(name: "DzcTransportTests", dependencies: ["DzcProtocol", "DzcTransport", "DzcSession"]),
    ]
)
