// swift-tools-version: 6.0
import PackageDescription

let package = Package(
  name: "vox",
  platforms: [
    .macOS(.v15)
  ],
  products: [
    .library(name: "VoxRuntime", targets: ["VoxRuntime"])
  ],
  dependencies: [
    .package(
      url: "https://github.com/bearcove/phon.git",
      revision: "290bff341afad44f2d6193f86e61a3d78de6f8c6"),
    .package(url: "https://github.com/apple/swift-nio.git", from: "2.99.0"),
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
      path: "swift/vox-runtime/Sources/VoxRuntime",
      resources: [
        .copy("wireMessageSchemas.bin")
      ]
    ),
    .testTarget(
      name: "VoxRuntimeTests",
      dependencies: [
        "VoxRuntime",
        .product(name: "PhonSchema", package: "phon"),
      ],
      path: "swift/vox-runtime/Tests/VoxRuntimeTests"
    ),
  ]
)
