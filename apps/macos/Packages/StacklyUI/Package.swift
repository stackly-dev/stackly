// swift-tools-version: 6.4
import PackageDescription

let package = Package(
    name: "StacklyUI",
    platforms: [.macOS("27.0")],
    products: [
        .library(name: "StacklyUI", targets: ["StacklyUI"])
    ],
    dependencies: [
        // The local client package; its identity is its folder name, `swift`.
        .package(path: "../../../../clients/swift")
    ],
    targets: [
        .target(
            name: "StacklyUI",
            dependencies: [.product(name: "StacklyClient", package: "swift")],
            swiftSettings: [.defaultIsolation(MainActor.self)]
        ),
        .testTarget(
            name: "StacklyUITests",
            dependencies: [
                "StacklyUI",
                .product(name: "StacklyTestSupport", package: "swift"),
            ],
            // Reference PNGs of the snapshot suites (spec 0068); not test sources.
            exclude: ["SnapshotTests/__Snapshots__"],
            swiftSettings: [.defaultIsolation(MainActor.self)]
        ),
    ]
)
