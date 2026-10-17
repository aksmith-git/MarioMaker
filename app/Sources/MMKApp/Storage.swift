import Foundation

/// Where the app keeps its data (DECISIONS.md, "App storage locations"):
///
///     ~/Library/Application Support/<AppIdentity.name>/
///       assets/<build>/      asset cache written by the importer (e.g. smm2-3.0.3)
///       courses/             the course library (files the user added)
///       saves.json           per-course save data
///
/// Settings live in UserDefaults (see `SettingsKey`).
enum AppPaths {
    /// MMK_SUPPORT_DIR=<dir> replaces the whole folder (tests, screenshots).
    static var support: URL {
        if let dir = ProcessInfo.processInfo.environment["MMK_SUPPORT_DIR"], !dir.isEmpty {
            return URL(fileURLWithPath: dir, isDirectory: true)
        }
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent(AppIdentity.name, isDirectory: true)
    }

    static var assetsRoot: URL { support.appendingPathComponent("assets", isDirectory: true) }
    static var courses: URL { support.appendingPathComponent("courses", isDirectory: true) }
    static var saves: URL { support.appendingPathComponent("saves.json") }

    /// Build directory preferred when several imports exist.
    static let preferredBuild = "smm2-3.0.3"

    /// Asset cache directories to try, best first: `MMK_ASSETS`, then imports under the app's own
    /// folder, then older locations (`mmk/assets`, used by `mmk import` before the app existed;
    /// `~/smm-work/cache`, the development cache).
    static func assetCandidates() -> [URL] {
        var list: [URL] = []
        if let env = ProcessInfo.processInfo.environment["MMK_ASSETS"], !env.isEmpty {
            list.append(URL(fileURLWithPath: env, isDirectory: true))
        }
        let home = FileManager.default.homeDirectoryForCurrentUser
        let roots = [
            assetsRoot,
            home.appendingPathComponent("Library/Application Support/mmk/assets", isDirectory: true),
            home.appendingPathComponent("smm-work/cache", isDirectory: true),
        ]
        for root in roots {
            list.append(root.appendingPathComponent(preferredBuild, isDirectory: true))
            let others = (try? FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)) ?? []
            list += others.filter { $0.lastPathComponent.hasPrefix("smm2-") }.sorted { $0.path > $1.path }
        }
        var seen = Set<String>()
        return list.filter { seen.insert($0.standardizedFileURL.path).inserted }
    }

    static func isAssetCache(_ url: URL) -> Bool {
        FileManager.default.fileExists(atPath: url.appendingPathComponent("manifest.json").path)
    }

    /// MMK_ASSETS=placeholder: ignore every cache and run on placeholder art (tests, CI).
    static var forcePlaceholder: Bool { ProcessInfo.processInfo.environment["MMK_ASSETS"] == "placeholder" }

    /// First usable asset cache, if any.
    static func findAssetCache() -> URL? {
        forcePlaceholder ? nil : assetCandidates().first(where: isAssetCache)
    }

    static func ensureDirectories() {
        for dir in [support, assetsRoot, courses] {
            try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        }
    }
}
