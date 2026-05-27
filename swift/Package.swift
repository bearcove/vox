// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "VoxSwiftBinetteCanaries",
    products: [
        .library(
            name: "VoxSwiftBinetteCanaries",
            targets: ["VoxSwiftBinetteCanaries"]
        ),
    ],
    dependencies: [
        .package(path: "/Users/amos/binette/swift/probes"),
    ],
    targets: [
        .target(
            name: "VoxSwiftBinetteCanaries",
            dependencies: [
                .product(name: "BinetteSwiftProbes", package: "probes"),
            ]
        ),
        .testTarget(
            name: "VoxSwiftBinetteCanariesTests",
            dependencies: [
                "VoxSwiftBinetteCanaries",
                .product(name: "BinetteSwiftProbes", package: "probes"),
            ]
        ),
    ]
)
