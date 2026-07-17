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

struct HourlyBriefItem: Codable, Hashable, Identifiable {
    var hour: Int
    var headline: String
    var sessionCount: Int
    var tokens: Int

    var id: Int { hour }
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
