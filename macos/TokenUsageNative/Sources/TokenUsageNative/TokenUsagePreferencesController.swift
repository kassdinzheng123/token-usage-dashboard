import Foundation

@MainActor
public final class TokenUsagePreferencesController: ObservableObject {
    public static let briefSupportedSources: Set<TokenUsageSource> = [
        .claude, .codex, .opencode, .cursor, .zcode, .kimi
    ]

    @Published public var enabledSources: Set<TokenUsageSource> {
        didSet {
            let rawValues = enabledSources.map(\.rawValue).sorted()
            defaults.set(rawValues, forKey: Self.enabledSourcesKey)
        }
    }

    @Published public var briefBaseURL: String {
        didSet { defaults.set(briefBaseURL, forKey: Self.briefBaseURLKey) }
    }

    @Published public var briefModelId: String {
        didSet { defaults.set(briefModelId, forKey: Self.briefModelIdKey) }
    }

    /// Optional override. Prefer leaving empty and using local CPA `api-keys`.
    @Published public var briefApiKey: String {
        didSet { defaults.set(briefApiKey, forKey: Self.briefApiKeyKey) }
    }

    @Published public private(set) var connectionTestMessage: String?
    @Published public private(set) var isTestingConnection = false

    private let defaults: UserDefaults

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults

        if let stored = defaults.array(forKey: Self.enabledSourcesKey) as? [String] {
            let parsed = Set(stored.compactMap(TokenUsageSource.init(rawValue:)))
                .subtracting([.all])
            self.enabledSources = parsed.isEmpty ? Set(TokenUsageSource.allCases.filter { $0 != .all }) : parsed
        } else {
            self.enabledSources = Set(TokenUsageSource.allCases.filter { $0 != .all })
        }

        self.briefBaseURL = defaults.string(forKey: Self.briefBaseURLKey)
            ?? Self.defaultBriefBaseURL
        self.briefModelId = defaults.string(forKey: Self.briefModelIdKey)
            ?? Self.defaultBriefModelId
        self.briefApiKey = defaults.string(forKey: Self.briefApiKeyKey) ?? ""

        if briefBaseURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            briefBaseURL = Self.defaultBriefBaseURL
        }
        if briefModelId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            briefModelId = Self.defaultBriefModelId
        }

        // One-time migration: persisted lists from before the Kimi source was
        // added lack it, which would hide the new source. Append Kimi once;
        // the flag stays set afterwards, so a later manual disable is respected.
        if !defaults.bool(forKey: Self.enabledSourcesMigratedKimiKey) {
            if !enabledSources.contains(.kimi) {
                enabledSources.insert(.kimi)
                defaults.set(enabledSources.map(\.rawValue).sorted(), forKey: Self.enabledSourcesKey)
            }
            defaults.set(true, forKey: Self.enabledSourcesMigratedKimiKey)
        }
    }

    /// API key used for brief generation: explicit override, else local CPA config.
    public var resolvedBriefApiKey: String {
        let trimmed = briefApiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            return trimmed
        }
        return Self.readLocalCPAConfig()?.apiKey ?? ""
    }

    /// Fills Base URL / Model from local CPA; clears stored key so CPA yaml is used live.
    @discardableResult
    public func applyLocalCPADefaults(overwriteModel: Bool = true) -> Bool {
        guard let cpa = Self.readLocalCPAConfig() else {
            connectionTestMessage = "CPA config not found (~/.config/cliproxyapi/config.yaml)"
            return false
        }

        briefBaseURL = cpa.baseURL
        if overwriteModel || briefModelId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            briefModelId = Self.defaultBriefModelId
        }
        // Do not copy the CPA key into app storage — resolve it from yaml at use time.
        briefApiKey = ""
        connectionTestMessage = cpa.apiKey.isEmpty
            ? "CPA found, but api-keys is empty"
            : "Using local CPA (\(Self.defaultBriefModelId))"
        return !cpa.apiKey.isEmpty
    }

    private struct LocalCPAConfig {
        let baseURL: String
        let apiKey: String
    }

    private static func readLocalCPAConfig() -> LocalCPAConfig? {
        let path = NSHomeDirectory() + "/.config/cliproxyapi/config.yaml"
        guard let text = try? String(contentsOfFile: path, encoding: .utf8) else {
            return nil
        }

        var host = "127.0.0.1"
        var port = 8317
        var apiKey = ""
        var inApiKeys = false

        for rawLine in text.components(separatedBy: .newlines) {
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") {
                continue
            }

            if line.hasPrefix("host:") {
                host = yamlScalar(after: "host:", in: line) ?? host
                inApiKeys = false
                continue
            }
            if line.hasPrefix("port:") {
                if let value = yamlScalar(after: "port:", in: line), let parsed = Int(value) {
                    port = parsed
                }
                inApiKeys = false
                continue
            }
            if line == "api-keys:" || line.hasPrefix("api-keys:") {
                inApiKeys = true
                continue
            }
            if inApiKeys {
                if line.hasPrefix("-") {
                    let value = line.dropFirst().trimmingCharacters(in: .whitespaces)
                    apiKey = stripYAMLQuotes(value)
                    inApiKeys = false
                    continue
                }
                if !line.hasPrefix(" ") && line.contains(":") {
                    inApiKeys = false
                }
            }
        }

        let baseURL = "http://\(host):\(port)/v1"
        return LocalCPAConfig(baseURL: baseURL, apiKey: apiKey)
    }

    private static func yamlScalar(after key: String, in line: String) -> String? {
        guard let range = line.range(of: key) else { return nil }
        let value = line[range.upperBound...].trimmingCharacters(in: .whitespaces)
        guard !value.isEmpty else { return nil }
        return stripYAMLQuotes(value)
    }

    private static func stripYAMLQuotes(_ value: String) -> String {
        var result = value.trimmingCharacters(in: .whitespaces)
        if (result.hasPrefix("\"") && result.hasSuffix("\""))
            || (result.hasPrefix("'") && result.hasSuffix("'")) {
            result.removeFirst()
            result.removeLast()
        }
        return result
    }

    public var briefSupportedEnabledSources: [TokenUsageSource] {
        TokenUsageSource.allCases.filter { source in
            source != .all
                && Self.briefSupportedSources.contains(source)
                && enabledSources.contains(source)
        }
    }

    public func isEnabled(_ source: TokenUsageSource) -> Bool {
        source == .all || enabledSources.contains(source)
    }

    public func setEnabled(_ source: TokenUsageSource, enabled: Bool) {
        guard source != .all else { return }
        if enabled {
            enabledSources.insert(source)
        } else {
            enabledSources.remove(source)
        }
    }

    public func testConnection() async {
        guard !isTestingConnection else { return }
        isTestingConnection = true
        connectionTestMessage = nil
        defer { isTestingConnection = false }

        let trimmedBase = briefBaseURL.trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard let url = URL(string: "\(trimmedBase)/models") else {
            connectionTestMessage = "Invalid base URL"
            return
        }

        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.timeoutInterval = 15
        let apiKey = resolvedBriefApiKey
        if !apiKey.isEmpty {
            request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        }

        do {
            let (_, response) = try await URLSession.shared.data(for: request)
            guard let http = response as? HTTPURLResponse else {
                connectionTestMessage = "Unexpected response"
                return
            }
            if (200...299).contains(http.statusCode) {
                connectionTestMessage = "Connected"
            } else {
                connectionTestMessage = "HTTP \(http.statusCode)"
            }
        } catch {
            connectionTestMessage = error.localizedDescription
        }
    }

    private static let enabledSourcesKey = "TokenUsage.enabledSources"
    private static let enabledSourcesMigratedKimiKey = "TokenUsage.enabledSourcesMigratedKimi"
    private static let briefBaseURLKey = "TokenUsage.briefBaseURL"
    private static let briefModelIdKey = "TokenUsage.briefModelId"
    private static let briefApiKeyKey = "TokenUsage.briefApiKey"
    public static let defaultBriefBaseURL = "http://127.0.0.1:8317/v1"
    public static let defaultBriefModelId = "deepseek-v4-flash"
}
