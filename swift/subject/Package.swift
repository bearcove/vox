// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "subject-swift",
    platforms: [
        .macOS(.v15)
    ],
    dependencies: [
        .package(path: "../vox-runtime"),
        .package(path: "../../../phon"),
    ],
    targets: [
        .executableTarget(
            name: "subject-swift",
            dependencies: [
                .product(name: "VoxRuntime", package: "vox-runtime")
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
                .product(name: "PhonEngineTestSupport", package: "phon"),
            ],
            // Preserved but not built: a real-socket handshake/frame harness whose
            // migration to the phon handshake protocol is Stage-4 work (it's the scaffold
            // Stage 4 will use). Re-include + migrate when wiring the cross-process matrix.
            exclude: ["ServerAndDispatcherIntegrationTests.swift"]
        )
    ]
)
