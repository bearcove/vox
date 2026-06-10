// swift-tools-version: 6.0
import PackageDescription

let package = Package(
  name: "subject-swift",
  platforms: [
    .macOS(.v15)
  ],
  dependencies: [
    .package(path: "../vox-runtime"),
    .package(
      url: "https://github.com/bearcove/phon.git",
      revision: "3f6bc6f75996a498a35606febb79430bce0f85d0"),
  ],
  targets: [
    .executableTarget(
      name: "subject-swift",
      dependencies: [
        .product(name: "VoxRuntime", package: "vox-runtime"),
        .product(name: "PhonSchema", package: "phon"),
        .product(name: "PhonIR", package: "phon"),
        .product(name: "PhonEngine", package: "phon"),
      ],
      sources: [
        "Server.swift",
        "Subject.swift",
        "Testbed.swift",
      ]
    ),
    .testTarget(
      name: "subject-swiftTests",
      dependencies: [
        .byName(name: "subject-swift"),
        .product(name: "VoxRuntime", package: "vox-runtime"),
        .product(name: "PhonSchema", package: "phon"),
        .product(name: "PhonIR", package: "phon"),
        .product(name: "PhonEngine", package: "phon"),
        .product(name: "PhonEngineTestSupport", package: "phon"),
      ]
    ),
  ]
)
