// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "VoxSwift",
    products: [
        .library(
            name: "VoxSwift",
            targets: ["VoxSwift"]
        ),
    ],
    dependencies: [
        .package(path: "/Users/amos/binette/swift/probes"),
    ],
    targets: [
        .target(
            name: "VoxSwift",
            dependencies: [
                .product(name: "BinetteSwiftProbes", package: "probes"),
            ]
        ),
        .testTarget(
            name: "VoxSwiftBinetteCanariesTests",
            dependencies: [
                "VoxSwift",
                .product(name: "BinetteSwiftProbes", package: "probes"),
            ]
        ),
    ]
)
