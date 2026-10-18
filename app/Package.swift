// swift-tools-version:6.0
// Native Apple shell for the engine (macOS, iOS / iPadOS). Links the Rust core (core/crates/ffi)
// as a static library: build it first with `cargo build --release -p mmk-ffi` in core/ (macOS).
// iOS builds pass the per-target library folder on the command line instead (-Xlinker -L…,
// see scripts/make-ios-app.sh), because SwiftPM conditions can't tell the simulator from a
// device.
import Foundation
import PackageDescription

let repoRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().path

let package = Package(
    name: "MMKApp",
    platforms: [.macOS(.v14), .iOS(.v17)],
    targets: [
        .systemLibrary(name: "CMmk", path: "Sources/CMmk"),
        .executableTarget(
            name: "MMKApp",
            dependencies: ["CMmk"],
            path: "Sources/MMKApp",
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [
                .unsafeFlags(["-L", "\(repoRoot)/core/target/release"], .when(platforms: [.macOS])),
                .linkedLibrary("mmk_ffi"),
            ]
        ),
    ]
)
