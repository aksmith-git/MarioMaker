import Foundation

/// Per-course progress, keyed by the course's content hash (`CourseInfo.key`), so a course keeps
/// its record when the file is renamed, moved or re-added in another form (.bcd / zlib / raw).
struct CourseRecord: Codable, Equatable {
    var cleared = false
    /// Fastest clear in 60 Hz ticks (start or restart of the attempt → touching the goal).
    var bestTicks: Int?
    var attempts = 0
    var clears = 0
    var lastPlayed: Date?

    var bestTimeText: String? {
        bestTicks.map { CourseRecord.format(ticks: $0) }
    }

    static func format(ticks: Int) -> String {
        let ms = ticks * 1000 / 60
        return String(format: "%d:%02d.%03d", ms / 60000, ms / 1000 % 60, ms % 1000)
    }
}

/// `saves.json` in Application Support: `{"version": 1, "courses": {"<hash>": CourseRecord}}`.
@MainActor
final class SaveStore: ObservableObject {
    private struct File: Codable {
        var version = 1
        var courses: [String: CourseRecord] = [:]
    }

    @Published private(set) var courses: [String: CourseRecord] = [:]
    private let url: URL

    private var observer: NSObjectProtocol?

    init(url: URL = AppPaths.saves) {
        self.url = url
        reload()
        // the front end keeps the same file (mmk-front's `Saves`): read it again when it writes
        observer = NotificationCenter.default.addObserver(forName: .mmkSavesChanged, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.reload() }
        }
    }

    deinit { if let observer { NotificationCenter.default.removeObserver(observer) } }

    func reload() {
        if let data = try? Data(contentsOf: url), let f = try? JSONDecoder.saves.decode(File.self, from: data) {
            courses = f.courses
        }
    }

    func record(_ key: String) -> CourseRecord { courses[key] ?? CourseRecord() }

    func addAttempt(_ key: String) {
        update(key) { $0.attempts += 1; $0.lastPlayed = Date() }
    }

    /// Returns true if this is a new best time.
    @discardableResult
    func addClear(_ key: String, ticks: Int) -> Bool {
        var best = false
        update(key) {
            $0.cleared = true
            $0.clears += 1
            if $0.bestTicks.map({ ticks < $0 }) ?? true { $0.bestTicks = ticks; best = true }
        }
        return best
    }

    private func update(_ key: String, _ change: (inout CourseRecord) -> Void) {
        var r = record(key)
        change(&r)
        courses[key] = r
        save()
    }

    private func save() {
        let f = File(courses: courses)
        guard let data = try? JSONEncoder.saves.encode(f) else { return }
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: url, options: .atomic)
    }
}

private extension JSONEncoder {
    static var saves: JSONEncoder {
        let e = JSONEncoder()
        e.outputFormatting = [.prettyPrinted, .sortedKeys]
        e.dateEncodingStrategy = .iso8601
        return e
    }
}

private extension JSONDecoder {
    static var saves: JSONDecoder {
        let d = JSONDecoder()
        d.dateDecodingStrategy = .iso8601
        return d
    }
}
