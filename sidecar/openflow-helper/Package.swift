// swift-tools-version: 6.0
import PackageDescription

// openflow-helper: the macOS-only half of OpenFlow (FluidAudio ASR, CTC vocabulary boosting, SenseVoice, the Core
// Audio process tap, Contacts/Calendar names), driven by the Rust app over a framed stdio protocol
// (docs/ARCHITECTURE.md "Sidecar protocol").
let package = Package(
    name: "openflow-helper",
    platforms: [.macOS(.v15)],
    dependencies: [
        // Same pin as the Swift app (dep/Package.resolved: 0.16.1).
        .package(url: "https://github.com/FluidInference/FluidAudio.git", exact: "0.16.1"),
    ],
    targets: [
        .executableTarget(
            name: "openflow-helper",
            dependencies: [
                .product(name: "FluidAudio", package: "FluidAudio"),
            ],
            path: "Sources/openflow-helper",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
