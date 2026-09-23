// swift-tools-version: 6.0
import PackageDescription

// jezflow-helper: the macOS-only half of JezFlow (FluidAudio ASR, CTC vocabulary boosting, SenseVoice, the Core
// Audio process tap, Contacts/Calendar names), driven by the Rust app over a framed stdio protocol
// (docs/ARCHITECTURE.md "Sidecar protocol").
let package = Package(
    name: "jezflow-helper",
    platforms: [.macOS(.v15)],
    dependencies: [
        // Same pin as the Swift app (dep/Package.resolved: 0.16.1).
        .package(url: "https://github.com/FluidInference/FluidAudio.git", exact: "0.16.1"),
    ],
    targets: [
        .executableTarget(
            name: "jezflow-helper",
            dependencies: [
                .product(name: "FluidAudio", package: "FluidAudio"),
            ],
            path: "Sources/jezflow-helper",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
