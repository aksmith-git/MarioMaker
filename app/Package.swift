// swift-tools-version:6.0
// Native macOS shell for the engine. Links the Rust core (core/crates/ffi) as a static library:
// build it first with `cargo build --release -p mmk-ffi` in core/.
import Foundation
import PackageDescription

let repoRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().path

let package = Package(
    name: "MMKApp",
    platforms: [.macOS(.v14)],
    targets: [
        .systemLibrary(name: "CMmk", path: "Sources/CMmk"),
        .executableTarget(
            name: "MMKApp",
            dependencies: ["CMmk"],
            path: "Sources/MMKApp",
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [
                .unsafeFlags(["-L", "\(repoRoot)/core/target/release"]),
                .linkedLibrary("mmk_ffi"),
            ]
        ),
    ]
)
