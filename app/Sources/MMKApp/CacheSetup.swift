#if os(iOS)
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// iOS first launch (no asset cache found), or "Game data…" from the library. The importer needs
/// the game's RomFS / ExeFS and runs on the Mac (the macOS app, or `mmk import`); here the owner
/// brings the finished cache folder over: copied with the Files app into On My iPad › MMK ›
/// assets, or picked anywhere Files reaches (iCloud Drive, a USB drive, AirDrop) and copied in
/// by `CacheCopy`. Or continue with placeholder art.
struct ImportView: View {
    @EnvironmentObject private var model: AppModel

    @State private var picking = false
    @State private var copying = false
    @State private var error: String?

    private var device: String { UIDevice.current.model }   // "iPad" / "iPhone"

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                HStack(spacing: 14) {
                    Image(systemName: "gamecontroller.fill").font(.system(size: 40)).foregroundStyle(.tint)
                    VStack(alignment: .leading) {
                        Text("Set up game data").font(.largeTitle.weight(.bold))
                        Text("\(AppIdentity.name) plays courses with art and data from your own copy of the game.")
                            .foregroundStyle(.secondary)
                    }
                }

                GroupBox {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Bring the asset cache over from your Mac").font(.headline)
                        Text("Import your dump on a Mac first, with the macOS app or `mmk import`. That makes a folder such as **smm2-3.0.3**, and optionally **smm1-…** and **smm2-ui-…** for the menus.")
                        Text("Then copy those folders with the Files app into **On My \(device) › \(AppIdentity.name) › assets**, or choose them below from anywhere Files can reach (iCloud Drive, a USB drive, AirDrop) and they are copied in. Choosing the folder that holds several caches copies all of them.")
                        Text("Nothing is downloaded and nothing leaves this \(device). Menu layouts (smm1-…, smm2-ui-…) are used from the next launch.")
                            .foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(6)
                }

                if copying { ProgressView("Copying…") }
                if let error {
                    Label(error, systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.red)
                        .textSelection(.enabled)
                }

                ViewThatFits {
                    HStack { buttons }
                    VStack(alignment: .leading, spacing: 12) { buttons }
                }
                .disabled(copying)
            }
            .padding(28)
            .frame(maxWidth: 720)
            .frame(maxWidth: .infinity)
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            switch result {
            case .success(let url): copy(url)
            case .failure(let e): error = e.localizedDescription
            }
        }
    }

    @ViewBuilder private var buttons: some View {
        Button("Choose cache folder…") { picking = true }
            .buttonStyle(.borderedProminent)
        Button("Look again") { model.importFinished() }
        Button("Use placeholder art") { model.usePlaceholder() }
        if model.assets != nil {
            Button("Back to library") { model.showLibrary() }
        }
    }

    private func copy(_ url: URL) {
        copying = true
        error = nil
        CacheCopy.run(url) { err in
            copying = false
            if let err { error = err } else { model.importFinished() }
        }
    }
}

/// Copies asset caches (folders with a manifest.json) into `AppPaths.assetsRoot`: the picked
/// folder itself, or the caches directly inside it (the Mac's whole `cache/` folder). An older
/// copy of the same cache is replaced.
enum CacheCopy {
    static func run(_ src: URL, completion: @escaping (String?) -> Void) {
        DispatchQueue.global(qos: .userInitiated).async {
            // a folder picked in Files is readable only inside its security scope
            let scoped = src.startAccessingSecurityScopedResource()
            defer { if scoped { src.stopAccessingSecurityScopedResource() } }
            let fm = FileManager.default
            let children = (try? fm.contentsOfDirectory(at: src, includingPropertiesForKeys: nil)) ?? []
            let caches = AppPaths.isAssetCache(src) ? [src] : children.filter(AppPaths.isAssetCache)
            var err: String? = caches.isEmpty
                ? "\(src.lastPathComponent) is not an asset cache (no manifest.json in it or in its folders)." : nil
            for cache in caches {
                let dest = AppPaths.assetsRoot.appendingPathComponent(cache.lastPathComponent, isDirectory: true)
                // already in place (picked from the app's own assets folder)
                if cache.standardizedFileURL.resolvingSymlinksInPath() == dest.standardizedFileURL.resolvingSymlinksInPath() { continue }
                do {
                    try fm.createDirectory(at: AppPaths.assetsRoot, withIntermediateDirectories: true)
                    if fm.fileExists(atPath: dest.path) { try fm.removeItem(at: dest) }
                    try fm.copyItem(at: cache, to: dest)
                } catch {
                    err = "Copying \(cache.lastPathComponent) failed: \(error.localizedDescription)"
                }
            }
            DispatchQueue.main.async { completion(err) }
        }
    }
}
#endif
