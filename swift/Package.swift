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
            name: "CVox",
            linkerSettings: [
                .unsafeFlags([
                    "-L", "../target/debug",
                    "-lvox",
                    "-Xlinker", "-rpath",
                    "-Xlinker", "../target/debug",
                ]),
            ]
        ),
        .target(
            name: "VoxSwift",
            dependencies: [
                "CVox",
                .product(name: "BinetteSwiftProbes", package: "probes"),
            ]
        ),
        .target(
            name: "VoxSwiftGenerated",
            dependencies: [
                "VoxSwift",
                .product(name: "BinetteSwiftProbes", package: "probes"),
            ],
            exclude: ["TestbedEvolved.generated.swift"]
        ),
        .testTarget(
            name: "VoxSwiftBinetteCanariesTests",
            dependencies: [
                "VoxSwift",
                "VoxSwiftGenerated",
                .product(name: "BinetteSwiftProbes", package: "probes"),
            ]
        ),
    ]
)
