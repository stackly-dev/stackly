// swift-tools-version: 6.4
import PackageDescription

let package = Package(
    name: "StacklyClient",
    platforms: [.macOS("27.0")],
    products: [
        .library(name: "StacklyClient", targets: ["StacklyClient"]),
        // Evidence records for Swift tests (spec 0002), also used by the StacklyUI tests.
        .library(name: "StacklyTestSupport", targets: ["StacklyTestSupport"]),
        // The same evidence records for XCTest, which UI tests need (spec 0068).
        .library(name: "StacklyXCTestSupport", targets: ["StacklyXCTestSupport"]),
    ],
    targets: [
        // No MainActor default isolation: socket IO runs off the main actor.
        .target(name: "StacklyClient"),
        // The record format, argument rules, and record writer shared by the Swift Testing
        // trait and the XCTest helper (spec 0068). Internal: no product exposes it.
        .target(name: "StacklyEvidenceCore"),
        // Evidence records for Swift tests (spec 0002). Only test targets depend on it.
        .target(name: "StacklyTestSupport", dependencies: ["StacklyEvidenceCore"]),
        // Evidence records for XCTest, such as UI tests (spec 0068). Only test targets depend
        // on it.
        .target(name: "StacklyXCTestSupport", dependencies: ["StacklyEvidenceCore"]),
        .testTarget(
            name: "StacklyClientTests", dependencies: ["StacklyClient", "StacklyTestSupport"]),
        .testTarget(
            name: "StacklyTestSupportTests",
            dependencies: ["StacklyTestSupport", "StacklyEvidenceCore"]),
        .testTarget(
            name: "StacklyXCTestSupportTests",
            dependencies: ["StacklyXCTestSupport", "StacklyEvidenceCore"]),
    ]
)
