import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// One course file in the library folder.
struct CourseEntry: Identifiable, Hashable {
    var id: String { url.path }
    let url: URL
    let info: CourseInfo

    var name: String { info.name.isEmpty ? url.deletingPathExtension().lastPathComponent : info.name }
}

/// The course library: every course file under `AppPaths.courses` (any depth). Files that are
/// not courses are ignored; encrypted `.bcd` files need imported game data for their key table.
@MainActor
final class CourseLibrary: ObservableObject {
    @Published private(set) var entries: [CourseEntry] = []
    @Published private(set) var scanning = false
    /// Files that looked like courses but couldn't be read (path → reason).
    @Published private(set) var unreadable: [(String, String)] = []
    @Published var message: String?

    let folder: URL

    init(folder: URL = AppPaths.courses) { self.folder = folder }

    /// Likely course files: dataset `.course.zlib`, `.zlib`, `.bcd`, raw data (`.bin`, `.dat`,
    /// `.course`, or no extension).
    nonisolated static func isCandidate(_ url: URL) -> Bool {
        let name = url.lastPathComponent
        if name.hasPrefix(".") { return false }
        let ext = url.pathExtension.lowercased()
        return ["zlib", "bcd", "bin", "dat", "course", ""].contains(ext)
    }

    nonisolated static func files(under root: URL) -> [URL] {
        let keys: Set<URLResourceKey> = [.isRegularFileKey, .fileSizeKey]
        guard let e = FileManager.default.enumerator(at: root, includingPropertiesForKeys: Array(keys),
                                                     options: [.skipsHiddenFiles]) else { return [] }
        var out: [URL] = []
        for case let url as URL in e {
            // course data is 0x5BFC0 bytes (a .bcd 0x5C000); anything much bigger isn't one
            guard let v = try? url.resourceValues(forKeys: keys), v.isRegularFile == true,
                  (v.fileSize ?? 0) <= 1 << 20, isCandidate(url) else { continue }
            out.append(url)
        }
        return out
    }

    nonisolated static func relativePath(_ file: URL, in root: URL) -> String {
        let f = file.resolvingSymlinksInPath().path, r = root.resolvingSymlinksInPath().path + "/"
        return f.hasPrefix(r) ? String(f.dropFirst(r.count)) : file.lastPathComponent
    }

    func scan(assets: EngineAssets?) {
        scanning = true
        let folder = folder
        DispatchQueue.global(qos: .userInitiated).async {
            var found: [CourseEntry] = []
            var bad: [(String, String)] = []
            for url in Self.files(under: folder) {
                guard let data = try? Data(contentsOf: url) else { continue }
                switch CourseInfo.read(data, assets: assets) {
                case .success(let info): found.append(CourseEntry(url: url, info: info))
                case .failure(let e): bad.append((url.path, e.description))
                }
            }
            found.sort { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
            DispatchQueue.main.async {
                self.entries = found
                self.unreadable = bad
                self.scanning = false
            }
        }
    }

    /// Copy course files (and course files inside folders, keeping their sub-paths) into the
    /// library. Skips courses already present (same content hash).
    func add(_ urls: [URL], assets: EngineAssets?) {
        let folder = folder
        let known = Set(entries.map(\.info.contentHash))
        scanning = true
        DispatchQueue.global(qos: .userInitiated).async {
            var added = 0, dupes = 0, skipped = 0
            var seen = known
            let fm = FileManager.default
            try? fm.createDirectory(at: folder, withIntermediateDirectories: true)
            for src in urls {
                var isDir: ObjCBool = false
                guard fm.fileExists(atPath: src.path, isDirectory: &isDir) else { continue }
                let pairs: [(URL, String)] = isDir.boolValue
                    ? Self.files(under: src).map { ($0, src.lastPathComponent + "/" + Self.relativePath($0, in: src)) }
                    : [(src, src.lastPathComponent)]
                for (file, rel) in pairs {
                    guard let data = try? Data(contentsOf: file),
                          case .success(let info) = CourseInfo.read(data, assets: assets) else { skipped += 1; continue }
                    guard seen.insert(info.contentHash).inserted else { dupes += 1; continue }
                    var dest = folder.appendingPathComponent(rel)
                    if fm.fileExists(atPath: dest.path) {
                        dest = dest.deletingLastPathComponent().appendingPathComponent("\(info.key)-\(dest.lastPathComponent)")
                    }
                    try? fm.createDirectory(at: dest.deletingLastPathComponent(), withIntermediateDirectories: true)
                    if (try? fm.copyItem(at: file, to: dest)) != nil { added += 1 }
                }
            }
            DispatchQueue.main.async {
                var parts = ["Added \(added) course\(added == 1 ? "" : "s")"]
                if dupes > 0 { parts.append("\(dupes) already in the library") }
                if skipped > 0 { parts.append("\(skipped) not readable as courses") }
                self.message = parts.joined(separator: ", ") + "."
                self.scan(assets: assets)
            }
        }
    }
}

// MARK: - view

struct LibraryView: View {
    @EnvironmentObject private var model: AppModel
    @ObservedObject var library: CourseLibrary
    @ObservedObject var saves: SaveStore

    enum Filter: String, CaseIterable, Identifiable {
        case all = "All", playable = "Playable", notCleared = "Not cleared", cleared = "Cleared"
        var id: String { rawValue }
    }

    @State private var search = ""
    @State private var filter: Filter = .all
    @State private var selection: CourseEntry.ID?
    @State private var sortOrder = [KeyPathComparator(\Row.name, comparator: .localizedStandard)]

    /// Flattened row for the table (sortable columns).
    struct Row: Identifiable {
        let entry: CourseEntry
        let record: CourseRecord
        var id: String { entry.id }
        var name: String { entry.name }
        var style: String { entry.info.style }
        var theme: String { entry.info.themeName }
        var timer: Int { entry.info.timer }
        var best: Int { record.bestTicks ?? .max }
        var attempts: Int { record.attempts }
        var playable: Int { entry.info.playable ? 0 : 1 }
        var cleared: Int { record.cleared ? 0 : 1 }
    }

    private var rows: [Row] {
        library.entries.lazy
            .map { Row(entry: $0, record: saves.record($0.info.key)) }
            .filter { r in
                switch filter {
                case .all: true
                case .playable: r.entry.info.playable
                case .notCleared: !r.record.cleared
                case .cleared: r.record.cleared
                }
            }
            .filter { r in
                search.isEmpty || r.name.localizedCaseInsensitiveContains(search)
                    || r.entry.info.description.localizedCaseInsensitiveContains(search)
                    || r.style.localizedCaseInsensitiveContains(search) || r.theme.localizedCaseInsensitiveContains(search)
            }
            .sorted(using: sortOrder)
    }

    var body: some View {
        VStack(spacing: 0) {
            if library.entries.isEmpty && !library.scanning {
                emptyState
            } else {
                table
                Divider()
                detail
            }
        }
        .navigationTitle(AppIdentity.name)
        .navigationSubtitle(subtitle)
        .searchable(text: $search, placement: .toolbar, prompt: "Search courses")
        .toolbar {
            ToolbarItemGroup {
                Menu {
                    Picker("Show", selection: $filter) {
                        ForEach(Filter.allCases) { Text($0.rawValue).tag($0) }
                    }
                    .pickerStyle(.inline)
                } label: {
                    Label("Show: \(filter.rawValue)", systemImage: filter == .all ? "line.3.horizontal.decrease.circle"
                                                                     : "line.3.horizontal.decrease.circle.fill")
                }
                .help("Show: \(filter.rawValue)")
                Button { addCourses() } label: { Label("Add courses…", systemImage: "plus") }
                    .help("Copy course files or folders into the library")
                Button { model.playDemo() } label: { Label("Demo", systemImage: "gamecontroller") }
                    .help("Play the built-in test strip")
                Menu {
                    Button("Rescan library") { library.scan(assets: model.assets) }
                    Button("Show library folder in Finder") { NSWorkspace.shared.open(library.folder) }
                    Divider()
                    Button("Import game data…") { model.showImporter() }
                } label: { Label("More", systemImage: "ellipsis.circle") }
            }
        }
        .onChange(of: model.addCoursesRequested) {
            if model.addCoursesRequested { model.addCoursesRequested = false; addCourses() }
        }
        .alert(library.message ?? "", isPresented: Binding(get: { library.message != nil }, set: { if !$0 { library.message = nil } })) {
            Button("OK", role: .cancel) {}
        }
    }

    private var subtitle: String {
        let n = library.entries.count
        let cleared = library.entries.filter { saves.record($0.info.key).cleared }.count
        var s = library.scanning ? "Scanning…" : "\(n) course\(n == 1 ? "" : "s") · \(cleared) cleared"
        if model.assets?.isPlaceholder == true { s += " · placeholder art" }
        return s
    }

    private var table: some View {
        Table(rows, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("", value: \.cleared) { r in
                Image(systemName: r.record.cleared ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(r.record.cleared ? Color.green : Color.secondary.opacity(0.4))
                    .help(r.record.cleared ? "Cleared" : "Not cleared yet")
            }
            .width(22)
            TableColumn("Name", value: \.name) { r in
                Text(r.name).fontWeight(.medium)
            }
            .width(min: 180, ideal: 280)
            TableColumn("Style", value: \.style).width(48)
            TableColumn("Theme", value: \.theme).width(min: 80, ideal: 110)
            TableColumn("Time limit", value: \.timer) { r in
                Text(r.timer > 0 ? "\(r.timer) s" : "—").monospacedDigit()
            }
            .width(70)
            TableColumn("Best", value: \.best) { r in
                Text(r.record.bestTimeText ?? "—").monospacedDigit()
            }
            .width(78)
            TableColumn("Tries", value: \.attempts) { r in
                Text(r.attempts > 0 ? "\(r.attempts)" : "—").monospacedDigit()
            }
            .width(44)
            TableColumn("Status", value: \.playable) { r in
                StatusBadge(info: r.entry.info)
            }
            .width(min: 110, ideal: 150)
        }
        .contextMenu(forSelectionType: CourseEntry.ID.self) { ids in
            if let e = entry(ids.first) {
                Button("Play") { model.play(e) }.disabled(!e.info.playable)
                if !e.info.playable { Button("Play anyway (unsupported)") { model.play(e) } }
                Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([e.url]) }
            }
        } primaryAction: { ids in
            if let e = entry(ids.first), e.info.playable { model.play(e) }
        }
    }

    private func entry(_ id: CourseEntry.ID?) -> CourseEntry? {
        library.entries.first { $0.id == id }
    }

    @ViewBuilder private var detail: some View {
        if let e = entry(selection) {
            let rec = saves.record(e.info.key)
            HStack(alignment: .top, spacing: 16) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(e.name).font(.title3.weight(.semibold))
                    Text(e.info.description.isEmpty ? "No description." : e.info.description)
                        .foregroundStyle(.secondary).lineLimit(3)
                    Text(detailLine(e, rec)).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button { model.play(e) } label: { Label("Play", systemImage: "play.fill").frame(minWidth: 80) }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .keyboardShortcut(.defaultAction)
                    .disabled(!e.info.playable)
                    .help(e.info.unsupportedReason ?? "Play this course")
            }
            .padding(14)
        } else {
            HStack {
                Text(library.unreadable.isEmpty ? "Select a course. Double-click to play."
                     : "Select a course. \(library.unreadable.count) file(s) couldn't be read: \(library.unreadable.first!.1)")
                    .foregroundStyle(.secondary).lineLimit(2)
                Spacer()
            }
            .padding(14)
        }
    }

    private func detailLine(_ e: CourseEntry, _ r: CourseRecord) -> String {
        var parts = ["Style \(e.info.style)", e.info.themeName, "\(e.info.objectCount) objects"]
        if e.info.encrypted { parts.append("encrypted .bcd") }
        if r.attempts > 0 { parts.append("\(r.attempts) attempt\(r.attempts == 1 ? "" : "s")") }
        if r.clears > 0 { parts.append("\(r.clears) clear\(r.clears == 1 ? "" : "s")") }
        parts.append(e.url.lastPathComponent)
        return parts.joined(separator: " · ")
    }

    private var emptyState: some View {
        VStack(spacing: 14) {
            Image(systemName: "square.stack.3d.up").font(.system(size: 46)).foregroundStyle(.secondary)
            Text("No courses yet").font(.title2.weight(.semibold))
            Text("Add course files (.course.zlib, .bcd or raw course data) or whole folders of them.\nThey are copied into \(library.folder.path).")
                .multilineTextAlignment(.center).foregroundStyle(.secondary)
            HStack {
                Button("Add courses…") { addCourses() }.buttonStyle(.borderedProminent)
                Button("Play demo strip") { model.playDemo() }
            }
            if !library.unreadable.isEmpty {
                Text("\(library.unreadable.count) file(s) in the folder couldn't be read: \(library.unreadable.first!.1)")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(40)
    }

    private func addCourses() {
        let panel = NSOpenPanel()
        panel.title = "Add courses"
        panel.prompt = "Add"
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = true
        panel.canChooseFiles = true
        guard panel.runModal() == .OK else { return }
        library.add(panel.urls, assets: model.assets)
    }
}

private struct StatusBadge: View {
    let info: CourseInfo

    var body: some View {
        let ok = info.playable
        Text(ok ? "Playable" : "Not yet supported")
            .font(.caption.weight(.semibold))
            .padding(.horizontal, 7).padding(.vertical, 2)
            .background(Capsule().fill(ok ? Color.green.opacity(0.2) : Color.orange.opacity(0.18)))
            .foregroundStyle(ok ? Color.green : Color.orange)
            .help(info.unsupportedReason ?? "The engine can play this course")
    }
}
