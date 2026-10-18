#if os(macOS)
import AppKit
import SwiftUI

/// First launch (no asset cache found), or "Import game data…" from the library: point the app at
/// the user's own extracted game files and run the importer, or continue with placeholder art.
/// (iOS has its own screen, CacheSetup.swift: the importer runs on the Mac.)
struct ImportView: View {
    @EnvironmentObject private var model: AppModel

    @State private var romfs: URL?
    @State private var exefs: URL?
    @State private var updateRomfs: URL?
    @State private var updateExefs: URL?
    @State private var running = false
    @State private var stage = ""
    @State private var progress: Double = 0
    @State private var error: String?
    @State private var importer: Importer?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                HStack(spacing: 14) {
                    Image(nsImage: NSApplication.shared.applicationIconImage)
                        .resizable().frame(width: 56, height: 56)
                    VStack(alignment: .leading) {
                        Text("Set up game data").font(.largeTitle.weight(.bold))
                        Text("\(AppIdentity.name) plays courses with art and data from your own copy of the game.")
                            .foregroundStyle(.secondary)
                    }
                }

                GroupBox {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("What you need").font(.headline)
                        Text("The **RomFS** and **ExeFS** folders extracted from your own dump of the Switch game (for example with an emulator's “Extract data” command, or hactool). Version 3.0.3 is recommended: either point at a RomFS/ExeFS that already has the update applied, or add the update's folders below.")
                        Text("Nothing is downloaded and nothing leaves this Mac. The importer converts what the engine needs into \(AppPaths.assetsRoot.path).")
                            .foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(6)
                }

                VStack(spacing: 10) {
                    FolderRow(title: "RomFS", hint: "contains Pack/, Course/, Model/ …", url: $romfs,
                              check: { looksLikeRomfs($0) ? nil : "No Pack/Static.pack or Course/ folder here" })
                    FolderRow(title: "ExeFS", hint: "contains main", url: $exefs,
                              check: { hasMain($0) ? nil : "No file named main here" })
                    FolderRow(title: "Update RomFS", hint: "optional", url: $updateRomfs, check: { _ in nil })
                    FolderRow(title: "Update ExeFS", hint: "optional", url: $updateExefs,
                              check: { hasMain($0) ? nil : "No file named main here" })
                }
                .disabled(running)

                if running {
                    VStack(alignment: .leading, spacing: 4) {
                        ProgressView(value: progress)
                        Text(stage).font(.caption).foregroundStyle(.secondary)
                    }
                }
                if let error {
                    Label(error, systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.red)
                        .textSelection(.enabled)
                }

                HStack {
                    Button("Use placeholder art") { model.usePlaceholder() }
                        .help("Play without game data: simple coloured shapes, unencrypted courses only")
                        .disabled(running)
                    if model.assets != nil {
                        Button("Back to library") { model.showLibrary() }.disabled(running)
                    }
                    Spacer()
                    Button(running ? "Importing…" : "Import") { start() }
                        .buttonStyle(.borderedProminent)
                        .keyboardShortcut(.defaultAction)
                        .disabled(running || !canStart)
                }
            }
            .padding(28)
            .frame(maxWidth: 720)
            .frame(maxWidth: .infinity)
        }
    }

    private var canStart: Bool {
        guard let romfs, let exefs else { return false }
        return (looksLikeRomfs(romfs) || updateRomfs != nil) && (hasMain(exefs) || updateExefs.map(hasMain) == true)
    }

    private func looksLikeRomfs(_ u: URL) -> Bool {
        let fm = FileManager.default
        return fm.fileExists(atPath: u.appendingPathComponent("Pack/Static.pack").path)
            || fm.fileExists(atPath: u.appendingPathComponent("Course").path)
    }

    private func hasMain(_ u: URL) -> Bool {
        FileManager.default.fileExists(atPath: u.appendingPathComponent("main").path)
    }

    private static let stageNames = [
        "identify": "Identifying the game version", "keys": "Finding the course key table",
        "tables": "Converting game data tables", "textures": "Converting textures",
    ]

    private func start() {
        guard let romfs, let exefs else { return }
        running = true
        error = nil
        progress = 0
        stage = "Starting…"
        let order = ["identify", "keys", "tables", "textures"]
        let imp = Importer { s, done, total in
            let i = Double(order.firstIndex(of: s) ?? 0)
            let frac = total > 0 ? Double(done) / Double(total) : 0
            progress = (i + frac) / Double(order.count)
            stage = "\(Self.stageNames[s] ?? s)… \(done)/\(total)"
        }
        importer = imp
        imp.run(.init(romfs: romfs, exefs: exefs, updateRomfs: updateRomfs, updateExefs: updateExefs),
                outRoot: AppPaths.assetsRoot) { err in
            running = false
            importer = nil
            if let err { error = "Import failed: \(err)" } else { model.importFinished() }
        }
    }
}

private struct FolderRow: View {
    let title: String
    let hint: String
    @Binding var url: URL?
    let check: (URL) -> String?

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(title).fontWeight(.semibold).frame(width: 110, alignment: .leading)
            VStack(alignment: .leading, spacing: 2) {
                Text(url?.path ?? hint)
                    .foregroundStyle(url == nil ? .secondary : .primary)
                    .lineLimit(1).truncationMode(.middle)
                if let url, let problem = check(url) {
                    Text(problem).font(.caption).foregroundStyle(.orange)
                }
            }
            Spacer()
            if url != nil { Button("Clear") { url = nil }.buttonStyle(.borderless) }
            Button("Choose…") { choose() }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.secondary.opacity(0.08)))
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.title = "Choose the \(title) folder"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK { url = panel.url }
    }
}
#endif
