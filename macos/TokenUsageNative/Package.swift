// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "TokenUsageNative",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .executable(name: "TokenUsageNative", targets: ["TokenUsageNative"])
    ],
    targets: [
        .executableTarget(
            name: "TokenUsageNative",
            path: "Sources/TokenUsageNative",
            resources: [
                .process("Resources")
            ]
        )
    ]
)
