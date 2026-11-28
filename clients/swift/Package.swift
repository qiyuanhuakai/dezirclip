// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "DezirClipLocalClient",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "DzcProtocol", targets: ["DzcProtocol"]),
        .library(name: "DzcTransport", targets: ["DzcTransport"]),
        .executable(name: "DarwinPeerProbe", targets: ["DarwinPeerProbe"]),
    ],
    targets: [
        .target(name: "DzcProtocol"),
        .target(name: "DzcTransport", dependencies: ["DzcProtocol"]),
        .executableTarget(name: "DarwinPeerProbe", dependencies: ["DzcProtocol", "DzcTransport"]),
        .testTarget(name: "DzcTransportTests", dependencies: ["DzcProtocol", "DzcTransport"]),
    ]
)
