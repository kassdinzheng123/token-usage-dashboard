import Foundation

@MainActor
final class BackendLogStore: ObservableObject {
    static let shared = BackendLogStore()

    @Published private(set) var lines: [String] = []

    private let maximumLineCount = 2_000
    private var baselineLineCount = 0
    private var tailTask: Task<Void, Never>?
    private let logFileURL: URL

    private init() {
        let applicationSupportURL = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        logFileURL = applicationSupportURL
            .appendingPathComponent("Token Usage Dashboard", isDirectory: true)
            .appendingPathComponent("backend.log")
    }

    func makeBackendLogFileHandle() -> FileHandle? {
        let directoryURL = logFileURL.deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        try? Data().write(to: logFileURL)

        lines = []
        baselineLineCount = 0

        return try? FileHandle(forWritingTo: logFileURL)
    }

    func startTailing() {
        reload()
        guard tailTask == nil else {
            return
        }

        tailTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(500))
                await MainActor.run {
                    self?.reload()
                }
            }
        }
    }

    func stopTailing() {
        tailTask?.cancel()
        tailTask = nil
    }

    func reload() {
        let allLines = readAllLines()
        let visibleLines = allLines.dropFirst(min(baselineLineCount, allLines.count))
        lines = Array(visibleLines.suffix(maximumLineCount))
    }

    func clear() {
        baselineLineCount = readAllLines().count
        lines.removeAll()
    }

    private func readAllLines() -> [String] {
        guard let data = try? Data(contentsOf: logFileURL),
              let text = String(data: data, encoding: .utf8),
              !text.isEmpty else {
            return []
        }

        return text
            .split(whereSeparator: \.isNewline)
            .map(String.init)
    }
}
