// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "DezirClipLocalClient",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "DzcTransport", targets: ["DzcTransport"]),
        .executable(name: "DarwinPeerProbe", targets: ["DarwinPeerProbe"]),
    ],
    targets: [
        .target(name: "DzcTransport"),
        .executableTarget(name: "DarwinPeerProbe", dependencies: ["DzcTransport"]),
        .testTarget(name: "DzcTransportTests", dependencies: ["DzcTransport"]),
    ]
)
