// swift-tools-version: 6.4
import PackageDescription

let package = Package(
    name: "StacklyUI",
    platforms: [.macOS("27.0")],
    products: [
        .library(name: "StacklyUI", targets: ["StacklyUI"])
    ],
    targets: [
        .target(
            name: "StacklyUI",
            swiftSettings: [.defaultIsolation(MainActor.self)]
        ),
        .testTarget(
            name: "StacklyUITests",
            dependencies: ["StacklyUI"],
            swiftSettings: [.defaultIsolation(MainActor.self)]
        ),
    ]
)
