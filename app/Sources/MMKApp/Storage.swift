import Foundation

/// Where the app keeps its data (DECISIONS.md, "App storage locations"):
///
///     ~/Library/Application Support/<AppIdentity.name>/
///       assets/<build>/      asset cache written by the importer (e.g. smm2-3.0.3)
///       courses/             the course library (files the user added)
///       saves.json           per-course save data
///       hundred.json         100 Mario Challenge save (Continue records, clears, unlocks)
///
/// The SMM1 UI import (`mmk import-smm1`, drives the 100 Mario screens) is looked for in
/// `MMK_UI_CACHE`, then `assets/smm1-<build>/` here, then `~/smm-work/cache/smm1-<build>/`
/// (DECISIONS.md, "SMM1 UI cache location"). Without one the app uses its stand-in screens.
///
/// On iOS the same layout sits in the app's Documents folder, which the Files app shows as
/// On My iPad / iPhone › <name>: the owner copies an asset cache made on the Mac into `assets/`
/// there (or picks one anywhere and the app copies it in, `CacheCopy`), and course files into
/// `courses/`.
///
/// Settings live in UserDefaults (see `SettingsKey`).
enum AppPaths {
    /// MMK_SUPPORT_DIR=<dir> replaces the whole folder (tests, screenshots).
    static var support: URL {
        if let dir = ProcessInfo.processInfo.environment["MMK_SUPPORT_DIR"], !dir.isEmpty {
            return URL(fileURLWithPath: dir, isDirectory: true)
        }
        #if os(macOS)
        _ = migrated
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent(AppIdentity.name, isDirectory: true)
        #else
        return FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        #endif
    }

    #if os(macOS)
    /// Once per launch: a data folder from before the rename (`Application Support/MMK`) moves
    /// to the new name if that doesn't exist yet.
    private static let migrated: Void = {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let old = base.appendingPathComponent("MMK", isDirectory: true)
        let new = base.appendingPathComponent(AppIdentity.name, isDirectory: true)
        let fm = FileManager.default
        if fm.fileExists(atPath: old.path) && !fm.fileExists(atPath: new.path) {
            try? fm.moveItem(at: old, to: new)
        }
    }()
    #endif

    /// Home folder holding the development caches (`~/smm-work/cache`): the user's on macOS, the
    /// Mac's inside the iOS Simulator (so a simulator build finds the same caches), none on a
    /// device.
    static var devHome: URL? {
        #if os(macOS)
        return FileManager.default.homeDirectoryForCurrentUser
        #else
        return ProcessInfo.processInfo.environment["SIMULATOR_HOST_HOME"].map { URL(fileURLWithPath: $0, isDirectory: true) }
        #endif
    }

    /// `~/smm-work/cache` under `devHome`, if there is one.
    private static var devCache: [URL] {
        devHome.map { [$0.appendingPathComponent("smm-work/cache", isDirectory: true)] } ?? []
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
        var roots = [assetsRoot]
        #if os(macOS)
        roots.append(FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/mmk/assets", isDirectory: true))
        #endif
        roots += devCache
        for root in roots {
            list.append(root.appendingPathComponent(preferredBuild, isDirectory: true))
            let others = (try? FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)) ?? []
            // (smm2-ui-* folders are the SMM2 UI import, not an asset cache)
            list += others.filter { $0.lastPathComponent.hasPrefix("smm2-") && !$0.lastPathComponent.hasPrefix("smm2-ui-") }
                .sorted { $0.path > $1.path }
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

    /// SMM1 UI import folders to try, best first (see the type comment).
    static func smm1Candidates() -> [URL] {
        var list: [URL] = []
        if let d = ProcessInfo.processInfo.environment["MMK_UI_CACHE"], !d.isEmpty {
            list.append(URL(fileURLWithPath: d, isDirectory: true))
        }
        for root in [assetsRoot] + devCache {
            let dirs = (try? FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)) ?? []
            list += dirs.filter { $0.lastPathComponent.hasPrefix("smm1-") }.sorted { $0.path > $1.path }
        }
        return list
    }

    static func findSMM1Cache() -> URL? { smm1Candidates().first(where: isAssetCache) }

    /// SMM2 UI import folders (`mmk import-smm2-ui`, drives the title screen and menus), best
    /// first: `MMK_UI2_CACHE`, then `assets/smm2-ui-<build>/` here, then
    /// `~/smm-work/cache/smm2-ui-<build>/`, newest build name first.
    static func smm2UICandidates() -> [URL] {
        var list: [URL] = []
        if let d = ProcessInfo.processInfo.environment["MMK_UI2_CACHE"], !d.isEmpty {
            list.append(URL(fileURLWithPath: d, isDirectory: true))
        }
        for root in [assetsRoot] + devCache {
            let dirs = (try? FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)) ?? []
            list += dirs.filter { $0.lastPathComponent.hasPrefix("smm2-ui-") }.sorted { $0.path > $1.path }
        }
        return list
    }

    static func findSMM2UICache() -> URL? { smm2UICandidates().first(where: isAssetCache) }

    static func ensureDirectories() {
        for dir in [support, assetsRoot, courses] {
            try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        }
    }
}
