import Foundation

enum UsageSource: String, CaseIterable, Codable, Identifiable {
    case claude
    case codex
    case opencode
    case hermes
    case openclaw
    case pi
    case grok
    case cursor
    case cherry
    case claudeScience = "claude-science"
    case zcode
    case kimi
    case reasonix

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .claude:
            "Claude Code"
        case .codex:
            "Codex"
        case .opencode:
            "OpenCode"
        case .hermes:
            "Hermes"
        case .openclaw:
            "OpenClaw"
        case .pi:
            "Pi Agent"
        case .grok:
            "Grok CLI"
        case .cursor:
            "Cursor"
        case .cherry:
            "Cherry Studio"
        case .claudeScience:
            "Claude Science"
        case .zcode:
            "ZCode"
        case .kimi:
            "Kimi"
        case .reasonix:
            "ReasonIX"
        }
    }
}

enum UsageView: String, CaseIterable, Codable, Identifiable {
    case daily
    case monthly
    case sessions
    case blocks

    var id: String { rawValue }
}

struct HealthStatus: Codable, Equatable {
    var status: String
    var cached: Int
    var expected: Int
    var keys: [String]
    var errors: [String: String]
    var warm: WarmStatus
}

struct WarmStatus: Codable, Equatable {
    var warming: Bool
    var total: Int
    var completed: Int
    var currentKey: String?
    var currentLabel: String?
    var startedAt: String?
    var finishedAt: String?
}

struct GitSyncSummary: Codable, Hashable {
    var files: Int
    var sessions: Int
    var blocks: Int
    var messages: Int

    var records: Int {
        sessions + blocks + messages
    }
}

struct GitSyncResponse: Codable, Hashable {
    var imported: GitSyncSummary
    var exported: GitSyncSummary
    var committed: Bool
    var pushed: Bool
    var commit: String?
    var attempts: Int
}

struct ModelBreakdown: Codable, Hashable {
    var modelName: String
    var inputTokens: Int
    var outputTokens: Int
    var cacheCreationTokens: Int
    var cacheReadTokens: Int
    var cost: Double
}

struct TodaySummaryResponse: Codable, Hashable {
    var date: String
    var inputTokens: Int
    var outputTokens: Int
    var cacheCreationTokens: Int
    var cacheReadTokens: Int
    var totalTokens: Int
    var totalCost: Double
    var activeSourceCount: Int
    var modelCount: Int
    var sourceRows: [TodaySourceUsageRow]
    var modelRows: [TodayModelUsageRow]

    static let empty = TodaySummaryResponse(
        date: "",
        inputTokens: 0,
        outputTokens: 0,
        cacheCreationTokens: 0,
        cacheReadTokens: 0,
        totalTokens: 0,
        totalCost: 0,
        activeSourceCount: 0,
        modelCount: 0,
        sourceRows: [],
        modelRows: []
    )

    func filtered(byEnabledSources enabledSources: Set<TokenUsageSource>) -> TodaySummaryResponse {
        let enabledRaw = Set(enabledSources.map(\.rawValue))
        let sourceRows = self.sourceRows.filter { enabledRaw.contains($0.source.rawValue) }
        let modelRows = self.modelRows.filter { enabledRaw.contains($0.source.rawValue) }
        let inputTokens = sourceRows.reduce(0) { $0 + $1.inputTokens }
        let outputTokens = sourceRows.reduce(0) { $0 + $1.outputTokens }
        let cacheCreationTokens = sourceRows.reduce(0) { $0 + $1.cacheCreationTokens }
        let cacheReadTokens = sourceRows.reduce(0) { $0 + $1.cacheReadTokens }
        let totalTokens = sourceRows.reduce(0) { $0 + $1.totalTokens }
        let totalCost = sourceRows.reduce(0.0) { $0 + $1.totalCost }
        return TodaySummaryResponse(
            date: date,
            inputTokens: inputTokens,
            outputTokens: outputTokens,
            cacheCreationTokens: cacheCreationTokens,
            cacheReadTokens: cacheReadTokens,
            totalTokens: totalTokens,
            totalCost: totalCost,
            activeSourceCount: sourceRows.count,
            modelCount: Set(modelRows.map(\.modelName)).count,
            sourceRows: sourceRows,
            modelRows: modelRows
        )
    }
}

struct TodaySourceUsageRow: Codable, Hashable {
    var source: UsageSource
    var inputTokens: Int
    var outputTokens: Int
    var cacheCreationTokens: Int
    var cacheReadTokens: Int
    var totalTokens: Int
    var totalCost: Double
    var modelCount: Int
}

struct TodayModelUsageRow: Codable, Hashable {
    var source: UsageSource
    var modelName: String
    var inputTokens: Int
    var outputTokens: Int
    var cacheCreationTokens: Int
    var cacheReadTokens: Int
    var totalTokens: Int
    var totalCost: Double
}

struct HourlyUsageRow: Codable, Hashable {
    var hour: Int
    var source: UsageSource
    var inputTokens: Int
    var outputTokens: Int
    var cacheCreationTokens: Int
    var cacheReadTokens: Int
    var totalTokens: Int
    var totalCost: Double
}

struct HourlyUsageResponse: Codable, Hashable {
    var date: String
    var hours: [HourlyUsageRow]
}

struct BriefModelInfo: Codable, Hashable {
    var baseUrl: String
    var modelId: String
}

struct TodayBriefCardItem: Codable, Hashable, Identifiable {
    var id: String
    var source: String
    var project: String
    var headline: String
    var bullets: [String]
    var sessionCount: Int
    var coverage: String
}

struct TodayBriefSection: Codable, Hashable, Identifiable {
    var source: String
    var headline: String
    var bullets: [String]
    var sessionCount: Int
    var coverage: String

    var id: String { "\(source)-\(headline)" }
}

struct HourlyBriefProject: Codable, Hashable, Identifiable {
    var source: String
    var project: String
    var tokens: Int
    var sessionCount: Int
    var headline: String

    var id: String { "\(source):\(project)" }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        source = try container.decode(String.self, forKey: .source)
        project = try container.decode(String.self, forKey: .project)
        tokens = try container.decode(Int.self, forKey: .tokens)
        sessionCount = try container.decode(Int.self, forKey: .sessionCount)
        headline = try container.decodeIfPresent(String.self, forKey: .headline) ?? ""
    }

    private enum CodingKeys: String, CodingKey {
        case source, project, tokens, sessionCount, headline
    }

    init(source: String, project: String, tokens: Int, sessionCount: Int, headline: String = "") {
        self.source = source
        self.project = project
        self.tokens = tokens
        self.sessionCount = sessionCount
        self.headline = headline
    }
}

struct HourlyBriefItem: Codable, Hashable, Identifiable {
    var hour: Int
    var headline: String
    var sessionCount: Int
    var tokens: Int
    var projects: [HourlyBriefProject]

    var id: Int { hour }

    /// Older briefs (saved before `projects` existed) omit the field; decode
    /// it as an empty array so the UI degrades to per-CLI chips.
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        hour = try container.decode(Int.self, forKey: .hour)
        headline = try container.decode(String.self, forKey: .headline)
        sessionCount = try container.decode(Int.self, forKey: .sessionCount)
        tokens = try container.decode(Int.self, forKey: .tokens)
        projects = try container.decodeIfPresent([HourlyBriefProject].self, forKey: .projects) ?? []
    }

    private enum CodingKeys: String, CodingKey {
        case hour, headline, sessionCount, tokens, projects
    }

    /// Memberwise initializer; `projects` defaults to empty for call sites
    /// (e.g. previews) that don't carry per-project data.
    init(
        hour: Int,
        headline: String,
        sessionCount: Int,
        tokens: Int,
        projects: [HourlyBriefProject] = []
    ) {
        self.hour = hour
        self.headline = headline
        self.sessionCount = sessionCount
        self.tokens = tokens
        self.projects = projects
    }
}

struct TodayBriefResponse: Codable, Hashable {
    var date: String
    var status: String
    var generatedAt: String
    var trigger: String
    var model: BriefModelInfo
    var enabledSources: [String]
    var contentFingerprint: String
    var summary: String?
    var cards: [TodayBriefCardItem]?
    var sections: [TodayBriefSection]?
    var error: String?
    var hours: [HourlyBriefItem]?

    var boardCards: [TodayBriefCardItem] {
        if let cards, !cards.isEmpty {
            return cards
        }
        return (sections ?? []).map { section in
            TodayBriefCardItem(
                id: "\(section.source):\(section.headline)",
                source: section.source,
                project: section.source,
                headline: section.headline,
                bullets: section.bullets,
                sessionCount: section.sessionCount,
                coverage: section.coverage
            )
        }
    }

    var boardSummary: String {
        if let summary, !summary.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return summary
        }
        let cards = boardCards
        guard !cards.isEmpty else { return "今日暂无项目摘要" }
        let previews = cards.prefix(3).map { "\($0.source)·\($0.project)" }
        if cards.count <= 3 {
            return "\(cards.count) 个项目：\(previews.joined(separator: "；"))"
        }
        return "\(cards.count) 个项目：\(previews.joined(separator: "；")) 等"
    }
}

struct BriefModelConfig: Encodable {
    var baseUrl: String
    var apiKey: String
    var modelId: String
}

struct BriefGenerateRequest: Encodable {
    var force: Bool
    var trigger: String
    var sources: [String]
    var model: BriefModelConfig
    var hours: [Int]?
    var mergeSources: Bool?
    var date: String?
}

/// How a brief regeneration should be scoped.
enum BriefRegenerateMode: Hashable {
    /// Regenerate the whole day.
    case full
    /// Regenerate only the given hours, keeping the cached headlines of the rest.
    case hours([Int])
    /// Regenerate only the given CLIs' project cards, keeping the rest.
    case sources([String])
}

/// One day in the brief month view.
struct BriefDayEntry: Codable, Hashable, Identifiable {
    var date: String
    var totalTokens: Int
    var totalCost: Double
    var sessions: Int
    var sources: [String]
    var projects: Int?
    var briefSummary: String?
    var topProjects: [String]
    var hasBrief: Bool

    var id: String { date }
}

/// One month in the brief all view.
struct BriefMonthEntry: Codable, Hashable, Identifiable {
    var month: String
    var totalTokens: Int
    var totalCost: Double
    var sessions: Int
    var activeDays: Int
    var sources: [String]
    var projects: Int
    var briefDays: Int
    var topProjects: [String]

    var id: String { month }
}

struct BriefDaysResponse: Codable, Hashable {
    var days: [BriefDayEntry]
}

struct BriefMonthsResponse: Codable, Hashable {
    var months: [BriefMonthEntry]
}

/// A named binding between a display name and a local project path. The path
/// is the authoritative identity for daily report aggregation and git lookup.
struct ProjectBinding: Codable, Hashable, Identifiable {
    var name: String
    var path: String
    var addedAt: String

    var id: String { name }
}

struct ProjectsResponse: Codable, Hashable {
    var projects: [ProjectBinding]
}

/// One completed work item of a daily report.
struct DailyWorkItem: Codable, Hashable, Identifiable {
    var title: String
    var detail: String

    var id: String { title }
}

/// A generated daily work summary for one project on one date.
struct DailyReport: Codable, Hashable {
    var date: String
    var project: String
    var path: String
    var status: String
    var overview: String
    var workItems: [DailyWorkItem]
    var sessionCount: Int
    var commitCount: Int
    var tokenTotal: Int
    var coverage: String
    var generatedAt: String
    var model: BriefModelInfo
    var error: String?

    var coverageLabel: String {
        switch coverage {
        case "exact": "路径精确匹配"
        case "decoded": "路径解码匹配"
        case "fallback": "含按项目名近似匹配的会话"
        default: "无会话"
        }
    }
}

struct DailyGenerateRequest: Encodable {
    var project: String
    var date: String?
    var force: Bool?
    var model: BriefModelConfig?
}

struct ProjectUpsertRequest: Encodable {
    var name: String
    var path: String
}

protocol UsageSummary: Codable, Hashable {
    var inputTokens: Int { get }
    var outputTokens: Int { get }
    var cacheCreationTokens: Int { get }
    var cacheReadTokens: Int { get }
    var totalTokens: Int { get }
}

struct DailyUsageEntry: UsageSummary {
    var date: String
    var inputTokens: Int
    var outputTokens: Int
    var cacheCreationTokens: Int
    var cacheReadTokens: Int
    var totalTokens: Int
    var totalCost: Double
    var modelsUsed: [String]
    var modelBreakdowns: [ModelBreakdown]
}

struct MonthlyUsageEntry: UsageSummary {
    var month: String
    var inputTokens: Int
    var outputTokens: Int
    var cacheCreationTokens: Int
    var cacheReadTokens: Int
    var totalTokens: Int
    var totalCost: Double
    var modelsUsed: [String]
    var modelBreakdowns: [ModelBreakdown]
}

struct SessionUsageEntry: UsageSummary {
    var sessionId: String
    var date: String
    var time: String
    var inputTokens: Int
    var outputTokens: Int
    var cacheCreationTokens: Int
    var cacheReadTokens: Int
    var totalTokens: Int
    var totalCost: Double
    var modelsUsed: [String]
    var modelBreakdowns: [ModelBreakdown]
}

struct BlockUsageEntry: UsageSummary {
    var blockId: String
    var sessionId: String
    var modelName: String
    var timestamp: String
    var date: String
    var time: String
    var inputTokens: Int
    var outputTokens: Int
    var cacheCreationTokens: Int
    var cacheReadTokens: Int
    var totalTokens: Int
    var cost: Double
}
