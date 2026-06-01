// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "vox-runtime",
    platforms: [
        // macOS 15 to match the phon package (native UInt128 / String(validating:)).
        .macOS(.v15)
    ],
    products: [
        .library(name: "VoxRuntime", targets: ["VoxRuntime"])
    ],
    dependencies: [
        .package(url: "https://github.com/apple/swift-nio.git", from: "2.99.0"),
        .package(path: "../../../phon"),
    ],
    targets: [
        .target(
            name: "VoxRuntime",
            dependencies: [
                .product(name: "NIO", package: "swift-nio"),
                .product(name: "NIOCore", package: "swift-nio"),
                .product(name: "NIOPosix", package: "swift-nio"),
                .product(name: "PhonSchema", package: "phon"),
                .product(name: "PhonIR", package: "phon"),
                .product(name: "PhonEngine", package: "phon"),
            ],
            path: "Sources/VoxRuntime",
            resources: [
                .copy("wireMessageSchemas.bin")
            ]
        ),
        .testTarget(
            name: "VoxRuntimeTests",
            dependencies: ["VoxRuntime"],
            path: "Tests/VoxRuntimeTests"
        ),
    ]
)
