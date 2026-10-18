// swift-tools-version: 6.4
import PackageDescription

let package = Package(
    name: "StacklyClient",
    platforms: [.macOS("27.0")],
    products: [
        .library(name: "StacklyClient", targets: ["StacklyClient"]),
        // Evidence records for Swift tests (spec 0002), also used by the StacklyUI tests.
        .library(name: "StacklyTestSupport", targets: ["StacklyTestSupport"]),
    ],
    targets: [
        // No MainActor default isolation: socket IO runs off the main actor.
        .target(name: "StacklyClient"),
        // Evidence records for Swift tests (spec 0002). Only test targets depend on it.
        .target(name: "StacklyTestSupport"),
        .testTarget(
            name: "StacklyClientTests", dependencies: ["StacklyClient", "StacklyTestSupport"]),
        .testTarget(name: "StacklyTestSupportTests", dependencies: ["StacklyTestSupport"]),
    ]
)
