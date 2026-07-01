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
