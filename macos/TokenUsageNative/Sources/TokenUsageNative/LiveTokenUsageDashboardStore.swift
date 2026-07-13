import Foundation

@MainActor
final class LiveTokenUsageDashboardStore: TokenUsageDashboardProviding {
    @Published var selectedSource: TokenUsageSource = .all
    @Published var selectedViewMode: TokenUsageViewMode = .daily
    @Published var startDate: Date
    @Published var endDate: Date
    @Published var selectedModels: Set<String> = []
    @Published private(set) var records: [TokenUsageRecord] = []
    @Published private(set) var todaySummary: TodaySummaryResponse = .empty
    @Published private(set) var isLoading = false
    @Published private(set) var errorMessage: String?
    @Published private(set) var isBackendConnected = false

    private let client: TokenUsageAPIClient
    private let serverProcess: LocalServerProcess
    private let calendar = Calendar.current
    private var activeLoadCount = 0
    private var isDashboardRefreshInFlight = false
    private var isTodayRefreshInFlight = false
    private var dashboardWarmTask: Task<Void, Never>?
    private var dashboardCacheRefreshTask: Task<Void, Never>?
    private var dashboardCacheRefreshTasks: [DashboardRecordRequest: Task<Void, Never>] = [:]
    private var backendPollerTask: Task<Void, Never>?
    private var activeDashboardRequest: DashboardRecordRequest?
    private let dashboardRecordsCache: DashboardRecordsCache

    init(
        client: TokenUsageAPIClient = TokenUsageAPIClient(),
        serverProcess: LocalServerProcess = LocalServerProcess()
    ) {
        self.client = client
        self.serverProcess = serverProcess
        self.dashboardRecordsCache = DashboardRecordsCache(client: client)
        let today = Date()
        self.endDate = calendar.date(byAdding: .day, value: 1, to: today) ?? today
        self.startDate = calendar.startOfDay(for: today)
        startDashboardCacheRefreshTimer()
        startBackendPoller()
    }

    deinit {
        dashboardWarmTask?.cancel()
        dashboardCacheRefreshTask?.cancel()
        dashboardCacheRefreshTasks.values.forEach { $0.cancel() }
        backendPollerTask?.cancel()
    }

    private func startBackendPoller() {
        backendPollerTask?.cancel()
        backendPollerTask = Task { [weak self] in
            while !Task.isCancelled {
                do {
                    _ = try await self?.client.fetchHealth()
                    await MainActor.run { self?.isBackendConnected = true }
                } catch {
                    await MainActor.run { self?.isBackendConnected = false }
                }
                try? await Task.sleep(for: .seconds(5))
            }
        }
    }

    func refresh() async {
        await refreshDashboard()
    }

    func refreshDashboard(force: Bool = false) async {
        guard !isDashboardRefreshInFlight else {
            return
        }

        let request = dashboardRecordRequest()
        activeDashboardRequest = request

        if !force, let snapshot = await dashboardRecordsCache.snapshot(for: request) {
            records = snapshot.records
            if !snapshot.isFresh {
                refreshDashboardCacheInBackground(request: request)
            }
            return
        }

        isDashboardRefreshInFlight = true
        beginLoading()
        errorMessage = nil
        defer {
            isDashboardRefreshInFlight = false
            endLoading()
        }

        do {
            try await serverProcess.ensureRunning()
            records = try await dashboardRecordsCache.records(for: request, force: force)
        } catch {
            errorMessage = error.localizedDescription
            records = []
        }
    }

    func refreshToday() async {
        await refreshToday(force: false)
    }

    func refreshToday(force: Bool) async {
        guard !isTodayRefreshInFlight else {
            return
        }

        isTodayRefreshInFlight = true
        beginLoading()
        errorMessage = nil
        defer {
            isTodayRefreshInFlight = false
            endLoading()
        }

        do {
            try await serverProcess.ensureRunning()
            todaySummary = try await client.fetchTodaySummary(refresh: force)
            warmDashboardCacheIfNeeded()
        } catch {
            errorMessage = error.localizedDescription
            todaySummary = .empty
        }
    }

    private func beginLoading() {
        activeLoadCount += 1
        isLoading = true
    }

    private func endLoading() {
        activeLoadCount = max(0, activeLoadCount - 1)
        isLoading = activeLoadCount > 0
    }

    private func warmDashboardCacheIfNeeded() {
        guard dashboardWarmTask == nil else {
            return
        }

        let requests = dashboardWarmRequests()

        dashboardWarmTask = Task { [weak self, client, dashboardRecordsCache] in
            do {
                try await self?.serverProcess.ensureRunning()
                let health = try await client.fetchHealth()
                if !health.warm.warming && health.cached < health.expected {
                    try await client.warmDashboardCache()
                }
                for request in requests {
                    _ = try await dashboardRecordsCache.records(for: request, force: false)
                }
            } catch {
                // Dashboard records remain lazy-loaded if backend warmup is unavailable.
            }

            await MainActor.run {
                self?.dashboardWarmTask = nil
            }
        }
    }

    private func startDashboardCacheRefreshTimer() {
        guard dashboardCacheRefreshTask == nil else {
            return
        }

        dashboardCacheRefreshTask = Task { [weak self] in
            while !Task.isCancelled {
                self?.refreshDashboardCacheInBackground()
                try? await Task.sleep(nanoseconds: DashboardRecordsCache.refreshIntervalNanoseconds)
            }
        }
    }

    private func refreshDashboardCacheInBackground() {
        let requests = dashboardWarmRequests()
        for request in requests {
            refreshDashboardCacheInBackground(request: request)
        }
    }

    private func refreshDashboardCacheInBackground(request: DashboardRecordRequest) {
        guard dashboardCacheRefreshTasks[request] == nil else {
            return
        }

        let task = Task { [weak self, dashboardRecordsCache] in
            defer {
                Task { @MainActor in
                    self?.dashboardCacheRefreshTasks[request] = nil
                }
            }

            do {
                try await self?.serverProcess.ensureRunning()
                guard !Task.isCancelled else { return }
                guard let refreshedRecords = try await dashboardRecordsCache.refreshIncrementally(for: request) else {
                    return
                }
                await MainActor.run {
                    guard let self else { return }
                    guard self.activeDashboardRequest == request else {
                        return
                    }
                    guard self.records != refreshedRecords else {
                        return
                    }
                    self.records = refreshedRecords
                }
            } catch {
                // Stale dashboard cache is preferable to surfacing background refresh noise.
            }
        }

        dashboardCacheRefreshTasks[request] = task
    }

    private func dashboardRecordRequest() -> DashboardRecordRequest {
        let range = dashboardDateRange()

        return DashboardRecordRequest(
            source: selectedSource,
            viewMode: selectedViewMode,
            since: range.since,
            until: range.until
        )
    }

    private func dashboardWarmRequests() -> [DashboardRecordRequest] {
        let range = dashboardDateRange()

        return TokenUsageViewMode.allCases.map { viewMode in
            DashboardRecordRequest(
                source: .all,
                viewMode: viewMode,
                since: viewMode == .monthly ? range.monthlySince : range.since,
                until: viewMode == .monthly ? range.monthlyUntil : range.until
            )
        }
    }

    private func dashboardDateRange() -> DashboardDateRange {
        let since = Self.dayFormatter.string(from: startDate)
        let until = Self.dayFormatter.string(from: endDate)
        let monthlyStartDate = calendar.dateInterval(of: .month, for: startDate)?.start ?? startDate
        let monthlyEndDate = Self.endOfMonth(for: endDate, calendar: calendar)

        return DashboardDateRange(
            since: selectedViewMode == .monthly ? Self.dayFormatter.string(from: monthlyStartDate) : since,
            until: selectedViewMode == .monthly ? Self.dayFormatter.string(from: monthlyEndDate) : until,
            monthlySince: Self.dayFormatter.string(from: monthlyStartDate),
            monthlyUntil: Self.dayFormatter.string(from: monthlyEndDate)
        )
    }

    func updateDateRangeForViewMode() {
        switch selectedViewMode {
        case .monthly:
            let today = Date()
            let prevMonth = calendar.date(byAdding: .month, value: -1, to: today) ?? today
            let nextMonth = calendar.date(byAdding: .month, value: 1, to: today) ?? today
            startDate = calendar.dateInterval(of: .month, for: prevMonth)?.start ?? prevMonth
            endDate = Self.endOfMonth(for: nextMonth, calendar: calendar)
        case .daily, .sessions:
            let today = Date()
            endDate = calendar.date(byAdding: .day, value: 1, to: today) ?? today
            startDate = calendar.date(byAdding: .day, value: -14, to: today) ?? today
        }
    }

    private static func endOfMonth(for date: Date, calendar: Calendar) -> Date {
        guard let interval = calendar.dateInterval(of: .month, for: date),
              let endOfMonth = calendar.date(byAdding: .day, value: -1, to: interval.end) else {
            return date
        }
        return endOfMonth
    }

    fileprivate static let dayFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter
    }()

    fileprivate static let monthFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM"
        return formatter
    }()
}

private struct DashboardDateRange {
    let since: String
    let until: String
    let monthlySince: String
    let monthlyUntil: String
}

private struct DashboardRecordRequest: Hashable, Sendable {
    let source: TokenUsageSource
    let viewMode: TokenUsageViewMode
    let since: String
    let until: String
}

private struct DashboardRecordsSnapshot: Sendable {
    let records: [TokenUsageRecord]
    let isFresh: Bool
}

private struct DashboardRecordsCacheEntry: Sendable {
    let records: [TokenUsageRecord]
    let refreshedAt: Date
    var lastAccessed: Date

    init(records: [TokenUsageRecord], refreshedAt: Date = Date(), lastAccessed: Date? = nil) {
        self.records = records
        self.refreshedAt = refreshedAt
        self.lastAccessed = lastAccessed ?? refreshedAt
    }
}

private actor DashboardRecordsCache {
    static let refreshIntervalNanoseconds: UInt64 = 5 * 60 * 1_000_000_000
    private static let maximumEntryAge: TimeInterval = 30 * 60
    private static let maximumEntryCount = 24

    private let client: TokenUsageAPIClient
    private var entries: [DashboardRecordRequest: DashboardRecordsCacheEntry] = [:]
    private var inFlightRequests: [DashboardRecordRequest: Task<[TokenUsageRecord], Error>] = [:]

    init(client: TokenUsageAPIClient) {
        self.client = client
    }

    func snapshot(for request: DashboardRecordRequest) -> DashboardRecordsSnapshot? {
        pruneEntries()
        guard var entry = entries[request] else {
            return nil
        }
        entry.lastAccessed = Date()
        entries[request] = entry

        return DashboardRecordsSnapshot(
            records: entry.records,
            isFresh: Self.isFresh(entry)
        )
    }

    func records(for request: DashboardRecordRequest, force: Bool) async throws -> [TokenUsageRecord] {
        pruneEntries()
        if !force, var entry = entries[request], Self.isFresh(entry) {
            entry.lastAccessed = Date()
            entries[request] = entry
            return entry.records
        }

        if let inFlightRequest = inFlightRequests[request] {
            return try await inFlightRequest.value
        }

        let task = Task { [client] in
            try await Self.fetchRecords(client: client, request: request, refresh: force)
        }
        inFlightRequests[request] = task

        do {
            let records = try await task.value
            entries[request] = DashboardRecordsCacheEntry(records: records, refreshedAt: Date())
            inFlightRequests[request] = nil
            pruneEntries(protecting: request)
            return records
        } catch {
            inFlightRequests[request] = nil
            throw error
        }
    }

    func refreshIncrementally(for request: DashboardRecordRequest) async throws -> [TokenUsageRecord]? {
        pruneEntries()
        guard var entry = entries[request], request.viewMode != .sessions else {
            return nil
        }
        entry.lastAccessed = Date()
        entries[request] = entry

        if let inFlightRequest = inFlightRequests[request] {
            return try await inFlightRequest.value
        }

        let task = Task { [client] in
            try await Self.fetchIncrementalRecords(
                client: client,
                request: request,
                cachedRecords: entry.records
            )
        }
        inFlightRequests[request] = task

        do {
            let records = try await task.value
            entries[request] = DashboardRecordsCacheEntry(records: records, refreshedAt: Date())
            inFlightRequests[request] = nil
            pruneEntries(protecting: request)
            return records
        } catch {
            inFlightRequests[request] = nil
            throw error
        }
    }

    private func pruneEntries(protecting protectedRequest: DashboardRecordRequest? = nil) {
        let now = Date()
        entries = entries.filter { request, entry in
            request == protectedRequest || now.timeIntervalSince(entry.lastAccessed) < Self.maximumEntryAge
        }

        while entries.count > Self.maximumEntryCount {
            let oldestRequest = entries
                .filter { request, _ in request != protectedRequest }
                .min { left, right in left.value.lastAccessed < right.value.lastAccessed }?
                .key

            guard let oldestRequest else { break }
            entries.removeValue(forKey: oldestRequest)
        }
    }

    private static func isFresh(_ entry: DashboardRecordsCacheEntry) -> Bool {
        Date().timeIntervalSince(entry.refreshedAt) < TimeInterval(refreshIntervalNanoseconds) / 1_000_000_000
    }

    private static func fetchIncrementalRecords(
        client: TokenUsageAPIClient,
        request: DashboardRecordRequest,
        cachedRecords: [TokenUsageRecord]
    ) async throws -> [TokenUsageRecord] {
        switch request.viewMode {
        case .daily:
            return try await fetchIncrementalDailyRecords(
                client: client,
                request: request,
                cachedRecords: cachedRecords
            )
        case .monthly:
            return try await fetchIncrementalMonthlyRecords(
                client: client,
                request: request,
                cachedRecords: cachedRecords
            )
        case .sessions:
            return cachedRecords
        }
    }

    private static func fetchIncrementalDailyRecords(
        client: TokenUsageAPIClient,
        request: DashboardRecordRequest,
        cachedRecords: [TokenUsageRecord]
    ) async throws -> [TokenUsageRecord] {
        guard let since = latestDateString(in: cachedRecords) else {
            return cachedRecords
        }

        let incrementalRequest = DashboardRecordRequest(
            source: request.source,
            viewMode: .daily,
            since: maxDateString(since, request.since),
            until: request.until
        )
        let updates = try await fetchRecords(client: client, request: incrementalRequest, refresh: true)
        return mergeRecords(
            cachedRecords,
            updates: updates,
            replacingFrom: incrementalRequest.since,
            until: incrementalRequest.until,
            viewMode: .daily
        )
    }

    private static func fetchIncrementalMonthlyRecords(
        client: TokenUsageAPIClient,
        request: DashboardRecordRequest,
        cachedRecords: [TokenUsageRecord]
    ) async throws -> [TokenUsageRecord] {
        guard let monthStart = latestMonthStartString(in: cachedRecords) else {
            return cachedRecords
        }

        let dailyRequest = DashboardRecordRequest(
            source: request.source,
            viewMode: .daily,
            since: maxDateString(monthStart, request.since),
            until: request.until
        )
        let dailyUpdates = try await fetchRecords(client: client, request: dailyRequest, refresh: true)
        let monthlyUpdates = aggregateDailyRecordsToMonthly(dailyUpdates)
        return mergeRecords(
            cachedRecords,
            updates: monthlyUpdates,
            replacingFrom: dailyRequest.since,
            until: dailyRequest.until,
            viewMode: .monthly
        )
    }

    private static func fetchRecords(
        client: TokenUsageAPIClient,
        request: DashboardRecordRequest,
        refresh: Bool
    ) async throws -> [TokenUsageRecord] {
        let sources = request.source == .all
            ? TokenUsageSource.apiSources
            : [request.source]

        if sources.count == 1, let source = sources.first {
            return try await fetchRecords(client: client, request: request, source: source, refresh: refresh)
        }

        return try await withThrowingTaskGroup(of: [TokenUsageRecord].self) { group in
            for source in sources {
                group.addTask {
                    try await fetchRecords(client: client, request: request, source: source, refresh: refresh)
                }
            }

            var allRecords: [TokenUsageRecord] = []
            for try await sourceRecords in group {
                allRecords.append(contentsOf: sourceRecords)
            }
            return allRecords
        }
    }

    private static func fetchRecords(
        client: TokenUsageAPIClient,
        request: DashboardRecordRequest,
        source: TokenUsageSource,
        refresh: Bool
    ) async throws -> [TokenUsageRecord] {
        switch request.viewMode {
        case .daily:
            let rows = try await client.fetchDaily(
                source: source.apiSource,
                since: request.since,
                until: request.until,
                refresh: refresh
            )
            return rows.map { record(from: $0, source: source) }
        case .monthly:
            let rows = try await client.fetchMonthly(
                source: source.apiSource,
                since: request.since,
                until: request.until,
                refresh: refresh
            )
            return rows.map { record(from: $0, source: source) }
        case .sessions:
            let rows = try await client.fetchSessions(
                source: source.apiSource,
                since: request.since,
                until: request.until,
                refresh: refresh
            )
            return rows.map { record(from: $0, source: source) }
        }
    }

    private static func record(from row: DailyUsageEntry, source: TokenUsageSource) -> TokenUsageRecord {
        TokenUsageRecord(
            id: "\(source.rawValue)-daily-\(row.date)",
            source: source,
            viewMode: .daily,
            date: dayFormatter().date(from: row.date) ?? Date.distantPast,
            inputTokens: row.inputTokens,
            outputTokens: row.outputTokens,
            cacheCreationTokens: row.cacheCreationTokens,
            cacheReadTokens: row.cacheReadTokens,
            totalTokens: row.totalTokens,
            totalCost: Decimal(row.totalCost),
            modelsUsed: row.modelsUsed,
            modelBreakdowns: row.modelBreakdowns.map(TokenUsageModelBreakdown.init)
        )
    }

    private static func record(from row: MonthlyUsageEntry, source: TokenUsageSource) -> TokenUsageRecord {
        TokenUsageRecord(
            id: "\(source.rawValue)-monthly-\(row.month)",
            source: source,
            viewMode: .monthly,
            date: monthFormatter().date(from: row.month) ?? Date.distantPast,
            inputTokens: row.inputTokens,
            outputTokens: row.outputTokens,
            cacheCreationTokens: row.cacheCreationTokens,
            cacheReadTokens: row.cacheReadTokens,
            totalTokens: row.totalTokens,
            totalCost: Decimal(row.totalCost),
            modelsUsed: row.modelsUsed,
            modelBreakdowns: row.modelBreakdowns.map(TokenUsageModelBreakdown.init)
        )
    }

    private static func record(from row: SessionUsageEntry, source: TokenUsageSource) -> TokenUsageRecord {
        TokenUsageRecord(
            id: "\(source.rawValue)-session-\(row.sessionId)",
            source: source,
            viewMode: .sessions,
            date: dayFormatter().date(from: row.date) ?? Date.distantPast,
            sessionID: row.sessionId,
            inputTokens: row.inputTokens,
            outputTokens: row.outputTokens,
            cacheCreationTokens: row.cacheCreationTokens,
            cacheReadTokens: row.cacheReadTokens,
            totalTokens: row.totalTokens,
            totalCost: Decimal(row.totalCost),
            modelsUsed: row.modelsUsed,
            modelBreakdowns: row.modelBreakdowns.map(TokenUsageModelBreakdown.init)
        )
    }

    private static func mergeRecords(
        _ cachedRecords: [TokenUsageRecord],
        updates: [TokenUsageRecord],
        replacingFrom since: String,
        until: String,
        viewMode: TokenUsageViewMode
    ) -> [TokenUsageRecord] {
        let calendar = Calendar(identifier: .gregorian)
        guard let startDate = dayFormatter().date(from: since),
              let endDate = dayFormatter().date(from: until) else {
            return cachedRecords
        }

        let affectedMonths = monthsBetween(startDate, and: endDate)
        let retained = cachedRecords.filter { record in
            guard record.viewMode == viewMode else {
                return true
            }
            switch viewMode {
            case .daily:
                let day = calendar.startOfDay(for: record.date)
                return day < calendar.startOfDay(for: startDate) || day > calendar.startOfDay(for: endDate)
            case .monthly:
                return !affectedMonths.contains(monthKey(for: record.date))
            case .sessions:
                return true
            }
        }

        return (retained + updates).sorted { left, right in
            if left.date == right.date {
                return left.id < right.id
            }
            return left.date < right.date
        }
    }

    private static func aggregateDailyRecordsToMonthly(_ records: [TokenUsageRecord]) -> [TokenUsageRecord] {
        let grouped = Dictionary(grouping: records) { record in
            "\(record.source.rawValue)-\(monthKey(for: record.date))"
        }

        return grouped.values.compactMap { rows in
            guard let first = rows.first,
                  let monthDate = monthFormatter().date(from: monthKey(for: first.date)) else {
                return nil
            }

            let modelBreakdowns = aggregateModelBreakdowns(rows.flatMap(\.modelBreakdowns))
            return TokenUsageRecord(
                id: "\(first.source.rawValue)-monthly-\(monthKey(for: first.date))",
                source: first.source,
                viewMode: .monthly,
                date: monthDate,
                inputTokens: rows.reduce(0) { $0 + $1.inputTokens },
                outputTokens: rows.reduce(0) { $0 + $1.outputTokens },
                cacheCreationTokens: rows.reduce(0) { $0 + $1.cacheCreationTokens },
                cacheReadTokens: rows.reduce(0) { $0 + $1.cacheReadTokens },
                totalTokens: rows.reduce(0) { $0 + $1.totalTokens },
                totalCost: rows.reduce(Decimal.zero) { $0 + $1.totalCost },
                modelsUsed: Array(Set(rows.flatMap(\.modelsUsed))).sorted(),
                modelBreakdowns: modelBreakdowns
            )
        }
    }

    private static func aggregateModelBreakdowns(
        _ breakdowns: [TokenUsageModelBreakdown]
    ) -> [TokenUsageModelBreakdown] {
        let grouped = Dictionary(grouping: breakdowns, by: \.modelName)
        return grouped.map { modelName, rows in
            TokenUsageModelBreakdown(
                modelName: modelName,
                inputTokens: rows.reduce(0) { $0 + $1.inputTokens },
                outputTokens: rows.reduce(0) { $0 + $1.outputTokens },
                cacheCreationTokens: rows.reduce(0) { $0 + $1.cacheCreationTokens },
                cacheReadTokens: rows.reduce(0) { $0 + $1.cacheReadTokens },
                cost: rows.reduce(Decimal.zero) { $0 + $1.cost }
            )
        }
        .sorted { $0.modelName < $1.modelName }
    }

    private static func latestDateString(in records: [TokenUsageRecord]) -> String? {
        records.map(\.date).max().map { dayFormatter().string(from: $0) }
    }

    private static func latestMonthStartString(in records: [TokenUsageRecord]) -> String? {
        records.map(\.date).max().map { monthKey(for: $0) + "-01" }
    }

    private static func maxDateString(_ left: String, _ right: String) -> String {
        left < right ? right : left
    }

    private static func monthsBetween(_ startDate: Date, and endDate: Date) -> Set<String> {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .current
        guard let startMonth = calendar.dateInterval(of: .month, for: startDate)?.start,
              let endMonth = calendar.dateInterval(of: .month, for: endDate)?.start else {
            return []
        }

        var months = Set<String>()
        var month = startMonth
        while month <= endMonth {
            months.insert(monthKey(for: month))
            guard let nextMonth = calendar.date(byAdding: .month, value: 1, to: month) else {
                break
            }
            month = nextMonth
        }
        return months
    }

    private static func monthKey(for date: Date) -> String {
        monthFormatter().string(from: date)
    }

    private static func dayFormatter() -> DateFormatter {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter
    }

    private static func monthFormatter() -> DateFormatter {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM"
        return formatter
    }
}

private extension TokenUsageSource {
    static let apiSources: [TokenUsageSource] = [.claude, .codex, .opencode, .hermes, .openclaw, .pi, .grok, .cursor, .cherry, .claudeScience]

    var apiSource: UsageSource {
        switch self {
        case .all:
            .claude
        case .claude:
            .claude
        case .codex:
            .codex
        case .opencode:
            .opencode
        case .hermes:
            .hermes
        case .openclaw:
            .openclaw
        case .pi:
            .pi
        case .grok:
            .grok
        case .cursor:
            .cursor
        case .cherry:
            .cherry
        case .claudeScience:
            .claudeScience
        }
    }
}

private extension TokenUsageModelBreakdown {
    init(_ breakdown: ModelBreakdown) {
        self.init(
            modelName: breakdown.modelName,
            inputTokens: breakdown.inputTokens,
            outputTokens: breakdown.outputTokens,
            cacheCreationTokens: breakdown.cacheCreationTokens,
            cacheReadTokens: breakdown.cacheReadTokens,
            cost: Decimal(breakdown.cost)
        )
    }
}
