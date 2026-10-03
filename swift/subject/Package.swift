// swift-tools-version: 6.0
import PackageDescription

let package = Package(
  name: "subject-swift",
  platforms: [
    .macOS(.v15),
    .iOS(.v18),
  ],
  dependencies: [
    .package(path: "../vox-runtime"),
    .package(
      url: "https://github.com/bearcove/phon.git",
      revision: "a991d13c1cac3b092cae19efbf562c6a2610ac3d"),
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
