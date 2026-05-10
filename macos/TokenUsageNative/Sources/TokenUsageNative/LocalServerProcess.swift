import Darwin
import Foundation

actor LocalServerProcess {
    enum ServerError: LocalizedError {
        case bundledBackendNotFound
        case failedToStart
        case healthCheckTimedOut

        var errorDescription: String? {
            switch self {
            case .bundledBackendNotFound:
                "Could not find the bundled token usage server."
            case .failedToStart:
                "Could not start the local token usage server."
            case .healthCheckTimedOut:
                "Local token usage server did not become ready in time."
            }
        }
    }

    private var process: Process?
    private var backendLogFileHandle: FileHandle?
    private let healthURL = URL(string: "http://127.0.0.1:3456/api/health")!

    deinit {
        process?.terminate()
        try? backendLogFileHandle?.close()
    }

    func ensureRunning() async throws {
        if process?.isRunning == true, await isHealthy() {
            return
        }

        let launch = try Self.productionLaunchConfiguration()
        if launch.ownsPort, await isHealthy() {
            Self.terminateExistingTokenUsageServerListeners(on: 3456)
        } else if await isHealthy() {
            return
        }

        if process == nil || process?.isRunning == false {
            try await start(using: launch)
        }

        for _ in 0..<50 {
            if await isHealthy() {
                return
            }
            try await Task.sleep(for: .milliseconds(300))
        }

        throw ServerError.healthCheckTimedOut
    }

    private func start(using launch: LaunchConfiguration) async throws {
        let process = Process()
        process.executableURL = launch.executableURL
        process.arguments = launch.arguments
        process.currentDirectoryURL = launch.currentDirectoryURL
        process.environment = launch.environment
        try? backendLogFileHandle?.close()
        backendLogFileHandle = await BackendLogStore.shared.makeBackendLogFileHandle()
        process.standardOutput = backendLogFileHandle ?? FileHandle.nullDevice
        process.standardError = backendLogFileHandle ?? FileHandle.nullDevice

        do {
            try process.run()
        } catch {
            throw ServerError.failedToStart
        }

        self.process = process
    }

    private static func productionLaunchConfiguration() throws -> LaunchConfiguration {
        guard let resourceURL = Bundle.main.resourceURL else {
            throw ServerError.bundledBackendNotFound
        }

        let backendURL = resourceURL.appendingPathComponent("Backend", isDirectory: true)
        let rustURL = backendURL.appendingPathComponent("token-usage-server")

        if FileManager.default.isExecutableFile(atPath: rustURL.path) {
            var environment = ProcessInfo.processInfo.environment
            environment["PORT"] = "3456"
            environment["NODE_ENV"] = "production"

            return LaunchConfiguration(
                executableURL: rustURL,
                arguments: [],
                currentDirectoryURL: backendURL,
                environment: environment,
                ownsPort: true
            )
        }

        throw ServerError.bundledBackendNotFound
    }

    private struct LaunchConfiguration {
        let executableURL: URL
        let arguments: [String]
        let currentDirectoryURL: URL
        let environment: [String: String]
        let ownsPort: Bool
    }

    private func isHealthy() async -> Bool {
        var request = URLRequest(url: healthURL)
        request.timeoutInterval = 1

        do {
            let (_, response) = try await URLSession.shared.data(for: request)
            guard let http = response as? HTTPURLResponse else {
                return false
            }
            return (200...299).contains(http.statusCode)
        } catch {
            return false
        }
    }

    private static func terminateExistingTokenUsageServerListeners(on port: Int) {
        let pidsOutput = commandOutput(
            executableURL: URL(fileURLWithPath: "/usr/sbin/lsof"),
            arguments: ["-tiTCP:\(port)", "-sTCP:LISTEN"]
        )

        let pids = pidsOutput
            .split(whereSeparator: \.isNewline)
            .compactMap { Int32($0.trimmingCharacters(in: .whitespacesAndNewlines)) }

        for pid in pids where pid > 0 {
            let command = commandOutput(
                executableURL: URL(fileURLWithPath: "/bin/ps"),
                arguments: ["-p", "\(pid)", "-o", "command="]
            )

            guard command.contains("token-usage-server") else {
                continue
            }

            Darwin.kill(pid, SIGTERM)
        }

        if !pids.isEmpty {
            Thread.sleep(forTimeInterval: 0.25)
        }
    }

    private static func commandOutput(executableURL: URL, arguments: [String]) -> String {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = executableURL
        process.arguments = arguments
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice

        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            return ""
        }

        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        return String(data: data, encoding: .utf8) ?? ""
    }

}
