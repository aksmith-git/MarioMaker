import CMmk
import Foundation
import Metal

/// Swift-side owners of the engine's C handles (mmk.h).

func lastEngineError() -> String { String(cString: mmk_last_error()) }

/// An open asset cache (or the empty placeholder set). Shared by the library (course info for
/// encrypted courses) and every game session; immutable after opening, so safe to read from any
/// thread.
final class EngineAssets: @unchecked Sendable {
    let handle: OpaquePointer
    /// nil for the placeholder set.
    let url: URL?
    var isPlaceholder: Bool { url == nil }
    /// GPU textures by cache index, uploaded when a sprite first uses one (see Renderer); a
    /// cached nil means the texture can't be drawn. The pixels are read lazily too.
    var gpuTextures: [Int: MTLTexture?] = [:]

    private init(handle: OpaquePointer, url: URL?) {
        self.handle = handle
        self.url = url
    }

    static func open(_ url: URL) throws -> EngineAssets {
        guard let h = mmk_assets_open(url.path) else { throw EngineError(lastEngineError()) }
        // SMM1 costumes and Lanky Mario, when the SMM1 import has them (before any engine)
        if let smm1 = AppPaths.findSMM1Cache() {
            mmk_assets_add_smm1_sprites(h, smm1.appendingPathComponent("sprites").path)
        }
        let costumes = CostumeList.read(h)
        Task { @MainActor in CostumeList.entries = costumes }
        return EngineAssets(handle: h, url: url)
    }

    /// No game data: the engine runs and the renderer draws stand-in shapes.
    static func placeholder() -> EngineAssets {
        EngineAssets(handle: mmk_assets_placeholder(), url: nil)
    }

    var textureCount: Int { mmk_assets_texture_count(handle) }

    deinit { mmk_assets_free(handle) }
}

/// Summary of a course file (`mmk_course_info`).
struct CourseInfo: Hashable, Sendable {
    var name: String
    var description: String
    var style: String
    var theme: Int
    var subTheme: Int
    var night: Bool
    var timer: Int
    var encrypted: Bool
    var objectCount: Int
    /// Objects the engine can't run yet (hook; 0 until the engine reports them).
    var unsupported: Int
    var playable: Bool
    var contentHash: UInt64

    var key: String { String(format: "%016llx", contentHash) }

    static let themeNames = ["Ground", "Underground", "Castle", "Airship", "Underwater",
                             "Ghost house", "Snow", "Desert", "Sky", "Forest"]
    var themeName: String {
        let t = theme < Self.themeNames.count ? Self.themeNames[theme] : "Theme \(theme)"
        return night ? "\(t) (night)" : t
    }

    /// Why a course can't be played yet, nil if it can.
    var unsupportedReason: String? {
        if playable { return nil }
        if unsupported > 0 { return "\(unsupported) object\(unsupported == 1 ? "" : "s") not supported yet" }
        return "Style \(style) not supported yet"
    }

    static func read(_ data: Data, assets: EngineAssets?) -> Result<CourseInfo, EngineError> {
        var c = MmkCourseInfo()
        let ok = data.withUnsafeBytes { raw in
            mmk_course_info(assets?.handle, raw.bindMemory(to: UInt8.self).baseAddress, data.count, &c)
        }
        guard ok else { return .failure(EngineError(lastEngineError())) }
        func str<T>(_ tuple: T) -> String {
            withUnsafeBytes(of: tuple) { String(cString: $0.bindMemory(to: CChar.self).baseAddress!) }
        }
        return .success(CourseInfo(
            name: str(c.name), description: str(c.description), style: str(c.style),
            theme: Int(c.theme), subTheme: Int(c.sub_theme), night: c.night & 1 != 0,
            timer: Int(c.timer), encrypted: c.encrypted != 0, objectCount: Int(c.object_count),
            unsupported: Int(c.unsupported), playable: c.playable != 0, contentHash: c.content_hash))
    }
}

/// Runs `mmk_import_run` on a background thread and reports progress on the main thread.
final class Importer: @unchecked Sendable {
    struct Paths {
        var romfs: URL
        var exefs: URL
        var updateRomfs: URL?
        var updateExefs: URL?
    }

    private let onProgress: (String, Int, Int) -> Void

    init(onProgress: @escaping (String, Int, Int) -> Void) { self.onProgress = onProgress }

    /// Completion gets the error message on failure.
    func run(_ p: Paths, outRoot: URL, completion: @escaping (String?) -> Void) {
        let thread = Thread { [self] in
            try? FileManager.default.createDirectory(at: outRoot, withIntermediateDirectories: true)
            let user = Unmanaged.passUnretained(self).toOpaque()
            let ok = mmk_import_run(p.romfs.path, p.exefs.path, p.updateRomfs?.path, p.updateExefs?.path,
                                    outRoot.path, { user, stage, done, total in
                guard let user, let stage else { return }
                let me = Unmanaged<Importer>.fromOpaque(user).takeUnretainedValue()
                let s = String(cString: stage)
                DispatchQueue.main.async { me.onProgress(s, done, total) }
            }, user)
            let err = ok ? nil : lastEngineError()
            DispatchQueue.main.async { completion(err) }
        }
        thread.stackSize = 16 << 20
        thread.start()
    }
}

struct EngineError: Error, CustomStringConvertible {
    let description: String
    init(_ d: String) { description = d }
}
