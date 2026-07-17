import Charts
import AppKit
import SwiftUI

// MARK: - Color System
// CLI brand colors and model provider colors come from `AppPalette` (Design.swift).
// Chart series domains are keyed by display label, so the lookup below stays a dictionary.

private let tokenTrendSourceColors: [String: Color] = AppPalette.cliColorsByLabel

private let chartTooltipWidth = 190.0
private let chartTooltipHeight = 86.0
private let maximumBarWidth: CGFloat = 96.0
private let dailyUsagePlotHeight: CGFloat = 220
private let dailyUsageContentHeight: CGFloat = 280
private let tokenTrendLegendPageSize = 6
private let modelDistributionLegendRowHeight: CGFloat = 40
private let modelDistributionLegendRowSpacing: CGFloat = 12
private let modelDistributionLegendMaxPageSize = 5
private let cliConsumptionPageSize = 5
private let allRangeBarPageSize = 60
private let todayOverviewContentHeight = 340.0
private let todaySourceRowHeight = 76.0
private let todaySourceRowSpacing = 10.0

public enum TokenUsageSource: String, CaseIterable, Identifiable, Sendable {
    case all
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

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .all: "All"
        case .claude: "Claude Code"
        case .codex: "Codex"
        case .opencode: "OpenCode"
        case .hermes: "Hermes"
        case .openclaw: "OpenClaw"
        case .pi: "Pi Agent"
        case .grok: "Grok CLI"
        case .cursor: "Cursor"
        case .cherry: "Cherry Studio"
        case .claudeScience: "Claude Science"
        case .zcode: "ZCode"
        case .kimi: "Kimi"
        }
    }
}

public enum TokenUsageViewMode: String, CaseIterable, Identifiable, Sendable {
    case daily
    case monthly
    case sessions

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .daily: "Daily"
        case .monthly: "Monthly"
        case .sessions: "Sessions"
        }
    }
}

private enum DashboardTimeRange: String, CaseIterable, Identifiable {
    case today
    case month
    case all

    var id: String { rawValue }

    var label: String {
        switch self {
        case .today: "Today"
        case .month: "This Month"
        case .all: "All"
        }
    }
}

private enum DashboardPage: String, CaseIterable, Identifiable {
    case overview
    case activity
    case models
    case brief
    case logs

    var id: String { rawValue }

    var label: String {
        switch self {
        case .overview: "Overview"
        case .activity: "Activity"
        case .models: "Models"
        case .brief: "Brief"
        case .logs: "Backend Logs"
        }
    }

    var systemImage: String {
        switch self {
        case .overview: "gauge"
        case .activity: "chart.bar.xaxis"
        case .models: "cpu"
        case .brief: "sparkles"
        case .logs: "terminal"
        }
    }

    /// Pages that show usage statistics and therefore expose the
    /// source / range / currency toolbar controls.
    var showsUsageControls: Bool {
        switch self {
        case .overview, .activity, .models: true
        case .brief, .logs: false
        }
    }
}

private enum DashboardCLICompositionPane: String, CaseIterable, Identifiable {
    case cli
    case composition

    var id: String { rawValue }

    var label: String {
        switch self {
        case .cli: "CLI Consumption"
        case .composition: "Token Composition"
        }
    }

    var pickerLabel: String {
        switch self {
        case .cli: "CLI"
        case .composition: "Token"
        }
    }
}

private enum DashboardModelCostMixPane: String, CaseIterable, Identifiable {
    case cost
    case mix

    var id: String { rawValue }

    var label: String {
        switch self {
        case .cost: "Model Cost"
        case .mix: "Token Usage"
        }
    }
}

private enum DashboardDailyUsageAggregation: String, CaseIterable, Identifiable {
    case cli
    case model

    var id: String { rawValue }

    var label: String {
        switch self {
        case .cli: "CLI"
        case .model: "Model"
        }
    }

    var seriesLabel: String {
        switch self {
        case .cli: "CLI"
        case .model: "Model"
        }
    }
}

/// Deterministic color for a model: the provider's brand hue with a stable
/// shade variation (see `AppPalette.modelColor`).
private func stableModelColor(for modelName: String) -> Color {
    AppPalette.modelColor(for: modelName)
}

public struct TokenUsageModelBreakdown: Identifiable, Hashable, Sendable {
    public var id: String { modelName }
    public let modelName: String
    public let inputTokens: Int
    public let outputTokens: Int
    public let cacheCreationTokens: Int
    public let cacheReadTokens: Int
    public let cost: Decimal

    public init(
        modelName: String,
        inputTokens: Int,
        outputTokens: Int,
        cacheCreationTokens: Int,
        cacheReadTokens: Int,
        cost: Decimal
    ) {
        self.modelName = modelName
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.cacheCreationTokens = cacheCreationTokens
        self.cacheReadTokens = cacheReadTokens
        self.cost = cost
    }
}

public struct TokenUsageRecord: Identifiable, Hashable, Sendable {
    public let id: String
    public let source: TokenUsageSource
    public let viewMode: TokenUsageViewMode
    public let date: Date
    public let sessionID: String?
    public let inputTokens: Int
    public let outputTokens: Int
    public let cacheCreationTokens: Int
    public let cacheReadTokens: Int
    public let totalTokens: Int
    public let totalCost: Decimal
    public let modelsUsed: [String]
    public let modelBreakdowns: [TokenUsageModelBreakdown]

    public init(
        id: String,
        source: TokenUsageSource,
        viewMode: TokenUsageViewMode,
        date: Date,
        sessionID: String? = nil,
        inputTokens: Int,
        outputTokens: Int,
        cacheCreationTokens: Int,
        cacheReadTokens: Int,
        totalTokens: Int,
        totalCost: Decimal,
        modelsUsed: [String],
        modelBreakdowns: [TokenUsageModelBreakdown] = []
    ) {
        self.id = id
        self.source = source
        self.viewMode = viewMode
        self.date = date
        self.sessionID = sessionID
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.cacheCreationTokens = cacheCreationTokens
        self.cacheReadTokens = cacheReadTokens
        self.totalTokens = totalTokens
        self.totalCost = totalCost
        self.modelsUsed = modelsUsed
        self.modelBreakdowns = modelBreakdowns
    }
}

@MainActor
protocol TokenUsageDashboardProviding: ObservableObject {
    var selectedSource: TokenUsageSource { get set }
    var selectedViewMode: TokenUsageViewMode { get set }
    var startDate: Date { get set }
    var endDate: Date { get set }
    var selectedModels: Set<String> { get set }
    var records: [TokenUsageRecord] { get }
    var todaySummary: TodaySummaryResponse { get }
    var todayHourly: HourlyUsageResponse? { get }
    /// Hourly usage keyed by date (YYYY-MM-DD) for the day drill-down view.
    var hourlyByDate: [String: HourlyUsageResponse] { get }
    var todayBrief: TodayBriefResponse? { get }
    var isLoading: Bool { get }
    var isGeneratingBrief: Bool { get }
    var briefErrorMessage: String? { get }
    var isBackendConnected: Bool { get }
    /// Cached briefs keyed by date (YYYY-MM-DD); today mirrors `todayBrief`.
    var briefCache: [String: TodayBriefResponse] { get }
    /// Dates confirmed to have no saved brief.
    var briefMissingDates: Set<String> { get }
    /// Day entries for the month currently shown in the brief month view.
    var briefDays: [BriefDayEntry] { get }
    /// Month entries for the brief all view.
    var briefMonths: [BriefMonthEntry] { get }

    func refresh() async
    func refreshToday() async
    func refreshDashboard(force: Bool) async
    func refreshToday(force: Bool) async
    func refreshTodayBrief() async
    func generateTodayBrief(force: Bool, trigger: String) async
    func loadBrief(for date: String) async
    func loadHourly(for date: String) async
    func loadBriefDays(month: String) async
    func loadBriefMonths() async
    func generateBrief(for date: String, mode: BriefRegenerateMode) async
    func updateDateRangeForViewMode()
}

struct TokenUsageDashboardView<Store: TokenUsageDashboardProviding>: View {
    @ObservedObject private var store: Store
    @ObservedObject private var currencyController: TokenUsageBillingCurrencyController
    @ObservedObject private var preferences: TokenUsagePreferencesController
    @StateObject private var backendLogs = BackendLogStore.shared
    @State private var modelSearchText = ""
    @State private var isModelFilterPresented = false
    @State private var tokenTrendLegendPage = 0
    @State private var modelCostLegendPage = 0
    @State private var tokenMixLegendPage = 0
    @State private var modelDistributionLegendCapacity = 1
    @State private var cliConsumptionPage = 0
    @State private var allRangeChartPage = 0
    @State private var heatmapPage = 0
    @State private var selectedPage: DashboardPage = .overview
    @State private var selectedTimeRange: DashboardTimeRange = .today
    @State private var cliCompositionPane: DashboardCLICompositionPane = .cli
    @State private var modelCostMixPane: DashboardModelCostMixPane = .cost
    @State private var dailyUsageAggregation: DashboardDailyUsageAggregation = .cli
    @State private var modelConsumptionGrouping: ModelConsumptionGrouping = .model
    @State private var modelConsumptionFilter = ""
    /// Day drill-down: when set, Overview/Activity show this single day.
    @State private var focusedDay: Date?
    @State private var isDayJumpPresented = false
    @State private var dayJumpSelection = Date()
    @State private var dashboardData: TokenUsageDashboardData

    init(
        store: Store,
        currencyController: TokenUsageBillingCurrencyController,
        preferences: TokenUsagePreferencesController
    ) {
        self.store = store
        self.currencyController = currencyController
        self.preferences = preferences
        _dashboardData = State(initialValue: Self.makeDashboardData(
            from: store,
            timeRange: .today,
            enabledSources: preferences.enabledSources
        ))
    }

    public var body: some View {
        applyDashboardSecondaryHandlers(
            to: applyDashboardPrimaryHandlers(
                to: applyDashboardTasks(to: dashboardRoot)
            )
        )
    }

    private var dashboardRoot: some View {
        NavigationSplitView {
            sidebar
                .navigationSplitViewColumnWidth(min: 200, ideal: 220, max: 260)
        } detail: {
            detailContent
                .frame(minWidth: 740, maxWidth: .infinity, minHeight: 680, maxHeight: .infinity)
        }
        .frame(minWidth: 960, maxWidth: .infinity, minHeight: 680, maxHeight: .infinity, alignment: .topLeading)
    }

    private func applyDashboardTasks<Content: View>(to content: Content) -> some View {
        content
            .task {
                await store.refresh()
            }
            .task {
                await store.refreshToday()
            }
            .task {
                await store.refreshTodayBrief()
            }
            .task {
                await currencyController.refreshExchangeRateIfNeeded()
            }
            .onChange(of: selectedPage) {
                if selectedPage == .logs {
                    backendLogs.startTailing()
                } else {
                    backendLogs.stopTailing()
                }
            }
            .onDisappear {
                backendLogs.stopTailing()
            }
    }

    private func applyDashboardPrimaryHandlers<Content: View>(to content: Content) -> some View {
        content
            .onChange(of: store.records) {
                updateDashboardData()
            }
            .onChange(of: store.selectedSource) {
                handleSelectedSourceChange()
            }
            .onChange(of: selectedTimeRange) {
                handleSelectedTimeRangeChange()
            }
            .onChange(of: dashboardData.dailyXAxisValues.count) {
                handleAllRangeAxisCountChange()
            }
            .onChange(of: dashboardData.heatmapDays.count) {
                handleHeatmapDaysCountChange()
            }
    }

    private func applyDashboardSecondaryHandlers<Content: View>(to content: Content) -> some View {
        content
            .onChange(of: store.startDate) {
                handleDateRangeChange()
            }
            .onChange(of: store.endDate) {
                handleDateRangeChange()
            }
            .onChange(of: store.selectedModels) {
                updateDashboardData()
                tokenTrendLegendPage = 0
            }
            .onChange(of: dailyUsageAggregation) {
                updateDashboardData()
                tokenTrendLegendPage = 0
            }
            .onChange(of: tokenTrendColorDomain) {
                tokenTrendLegendPage = 0
            }
            .onChange(of: currencyController.selectedCurrency) {
                Task { await currencyController.refreshExchangeRateIfNeeded() }
            }
            .onChange(of: preferences.enabledSources) {
                if store.selectedSource != .all,
                   !preferences.enabledSources.contains(store.selectedSource) {
                    store.selectedSource = .all
                }
                updateDashboardData()
                cliConsumptionPage = 0
            }
    }

    private func handleSelectedSourceChange() {
        updateDashboardData()
        Task { await store.refresh() }
    }

    private func handleDateRangeChange() {
        updateDashboardData()
        Task { await store.refresh() }
    }

    private func handleAllRangeAxisCountChange() {
        guard selectedTimeRange == .all else { return }
        jumpAllRangeChartPageToLatest()
    }

    private func handleHeatmapDaysCountChange() {
        guard selectedTimeRange == .all else { return }
        jumpHeatmapPageToLatest()
    }

    private static func makeDashboardData(
        from store: Store,
        timeRange: DashboardTimeRange = .today,
        dailyUsageAggregation: DashboardDailyUsageAggregation = .cli,
        enabledSources: Set<TokenUsageSource>
    ) -> TokenUsageDashboardData {
        TokenUsageDashboardData.make(
            records: store.records,
            selectedSource: store.selectedSource,
            selectedViewMode: store.selectedViewMode,
            startDate: store.startDate,
            endDate: store.endDate,
            selectedModels: store.selectedModels,
            timeRange: timeRange,
            dailyUsageAggregation: dailyUsageAggregation,
            enabledSources: enabledSources
        )
    }

    private func updateDashboardData() {
        dashboardData = Self.makeDashboardData(
            from: store,
            timeRange: selectedTimeRange,
            dailyUsageAggregation: dailyUsageAggregation,
            enabledSources: preferences.enabledSources
        )
        modelCostLegendPage = 0
        tokenMixLegendPage = 0
        cliConsumptionPage = 0
    }

    // MARK: - Sidebar

    private var sidebar: some View {
        List(selection: $selectedPage) {
            Section("Usage") {
                sidebarRow(for: .overview)
                sidebarRow(for: .activity)
                sidebarRow(for: .models)
                sidebarRow(for: .brief)
            }

            Section("System") {
                sidebarRow(for: .logs)
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
            sidebarStatusFooter
        }
    }

    private func sidebarRow(for page: DashboardPage) -> some View {
        Label(page.label, systemImage: page.systemImage)
            .tag(page)
    }

    private var sidebarStatusFooter: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(store.isBackendConnected ? Color.green : Color.red)
                .frame(width: 7, height: 7)
            Text(store.isBackendConnected ? "Backend connected" : "Backend disconnected")
                .font(.caption)
                .foregroundStyle(.secondary)
            Spacer()
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    // MARK: - Detail pages

    @ViewBuilder
    private var detailContent: some View {
        Group {
            switch selectedPage {
            case .overview:
                overviewPage
            case .activity:
                activityPage
            case .models:
                modelsPage
            case .brief:
                briefPage
            case .logs:
                logsPage
            }
        }
        .id(selectedPage)
        .transition(.opacity)
        .animation(.easeInOut(duration: 0.15), value: selectedPage)
        .navigationTitle("Token Usage")
        .navigationSubtitle(selectedPage.label)
        .toolbar {
            dashboardToolbar
        }
    }

    private var overviewPage: some View {
        ScrollView(.vertical) {
            LazyVStack(alignment: .leading, spacing: 20) {
                if store.records.isEmpty {
                    if focusedDay != nil && !store.isLoading {
                        emptyTodayState(text: "No usage recorded for this day")
                    } else {
                        dashboardLoadingView
                    }
                } else {
                    heroMetricsRow
                    chartSection
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
        .scrollIndicators(.visible)
    }

    private var activityPage: some View {
        ScrollView(.vertical) {
            LazyVStack(alignment: .leading, spacing: 20) {
                if isDayMode {
                    // Day view is driven by /api/hourly, not dashboard records.
                    // Don't wait on records — that raced and showed an empty state
                    // while hourly was still loading (or failed silently).
                    if activeHourly == nil {
                        dashboardLoadingView
                    } else if hourlyPoints.isEmpty {
                        activityTodayEmptyState
                    } else {
                        todayTimelineSection
                    }
                } else if store.records.isEmpty {
                    dashboardLoadingView
                } else {
                    dailyTokenUsageSection
                    activityInsightsSection
                    cacheEfficiencySection
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
        .scrollIndicators(.visible)
    }

    /// Day mode shows a single day's hourly strip: either the Today range or
    /// a day drilled into from the All/Month views.
    private var isDayMode: Bool {
        focusedDay != nil || selectedTimeRange == .today
    }

    /// Hourly data for the day currently shown in day mode.
    private var activeHourly: HourlyUsageResponse? {
        guard let focusedDayString, focusedDayString != todayDateString else {
            return store.todayHourly
        }
        return store.hourlyByDate[focusedDayString]
    }

    private var activityTodayEmptyState: some View {
        let isPastDay = focusedDayString != nil && focusedDayString != todayDateString
        return ContentUnavailableView {
            Label(
                isPastDay ? "No Activity This Day" : "No Activity Yet Today",
                systemImage: "chart.bar.xaxis"
            )
        } description: {
            Text(
                isPastDay
                    ? "No CLI recorded usage on this day."
                    : "Hourly usage appears here once a CLI records a session today. Daily trends live under This Month."
            )
        } actions: {
            if isPastDay {
                Button("Back to All") {
                    clearFocusedDay()
                }
            } else {
                Button("Show This Month") {
                    selectedTimeRange = .month
                }
            }
        }
        .frame(maxWidth: .infinity, minHeight: 420)
    }

    private var modelsPage: some View {
        ScrollView(.vertical) {
            LazyVStack(alignment: .leading, spacing: 20) {
                if store.records.isEmpty {
                    if focusedDay != nil && !store.isLoading {
                        emptyTodayState(text: "No usage recorded for this day")
                    } else {
                        dashboardLoadingView
                    }
                } else {
                    modelConsumptionSection
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
        .scrollIndicators(.visible)
    }

    private var briefPage: some View {
        ScrollView(.vertical) {
            BriefPageView(
                store: store,
                preferences: preferences,
                currencyController: currencyController
            )
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
        .scrollIndicators(.visible)
    }

    private var logsPage: some View {
        BackendLogsView(
            lines: backendLogs.lines,
            onClear: {
                backendLogs.clear()
            }
        )
        .padding(20)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private var dashboardLoadingView: some View {
        VStack(spacing: 16) {
            ProgressView()
                .scaleEffect(1.2)
            Text("Loading token usage data...")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, minHeight: 320)
    }

    // MARK: - Toolbar

    @ToolbarContentBuilder
    private var dashboardToolbar: some ToolbarContent {
        if selectedPage.showsUsageControls {
            ToolbarItemGroup(placement: .navigation) {
                sourcePicker

                Button {
                    isModelFilterPresented.toggle()
                } label: {
                    Label(
                        "Filter Models",
                        systemImage: store.selectedModels.isEmpty
                            ? "line.3.horizontal.decrease.circle"
                            : "line.3.horizontal.decrease.circle.fill"
                    )
                }
                .help(showsModelFilter ? "Filter models" : "Model filter is available for a specific source")
                .disabled(!showsModelFilter)
                .popover(isPresented: $isModelFilterPresented, arrowEdge: .bottom) {
                    modelFilterPopover
                }
            }

            ToolbarItem(placement: .principal) {
                HStack(spacing: 10) {
                    timeRangePicker

                    if let focusedDay {
                        focusedDayChip(focusedDay)
                    }

                    dayJumpButton
                }
            }

            ToolbarItem(placement: .primaryAction) {
                currencyPicker
            }
        }

        ToolbarItem(placement: .primaryAction) {
            settingsButton
        }

        ToolbarItem(placement: .primaryAction) {
            refreshButton
        }
    }

    private var currencyPicker: some View {
        Picker("Currency", selection: $currencyController.selectedCurrency) {
            ForEach(TokenUsageBillingCurrency.allCases) { currency in
                Text(currency.label).tag(currency)
            }
        }
        .labelsHidden()
        .pickerStyle(.menu)
        .fixedSize()
        .help("Billing currency")
    }

    private var refreshButton: some View {
        Button {
            Task { await refreshCurrentPage() }
        } label: {
            Label("Refresh", systemImage: "arrow.clockwise")
        }
        .keyboardShortcut("r", modifiers: .command)
        .help(selectedPage == .brief ? "Refresh brief" : "Refresh usage")
        .disabled(store.isLoading || store.isGeneratingBrief)
    }

    @Environment(\.openSettings) private var openSettings

    private var settingsButton: some View {
        Button {
            openSettings()
        } label: {
            Label("Settings", systemImage: "gearshape")
        }
        .help("Open settings")
    }

    private func refreshCurrentPage() async {
        switch selectedPage {
        case .brief:
            await store.refreshTodayBrief()
        case .logs:
            break
        case .overview, .activity, .models:
            await refreshDashboardAndToday(force: true)
        }
    }

    private var availableModels: [String] {
        dashboardData.availableModels
    }

    private var filteredAvailableModels: [String] {
        let searchText = modelSearchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !searchText.isEmpty else { return availableModels }

        return availableModels.filter { model in
            model.localizedCaseInsensitiveContains(searchText)
                || displayModelName(model).localizedCaseInsensitiveContains(searchText)
        }
    }

    private var modelSelectionSummary: String {
        store.selectedModels.isEmpty ? "All Models" : "\(store.selectedModels.count) selected"
    }

    private var totalCost: Decimal {
        dashboardData.totalCost
    }

    private var totalTokens: Int {
        dashboardData.totalTokens
    }

    private func refreshDashboardAndToday(force: Bool = false) async {
        await store.refreshDashboard(force: force)
        await store.refreshToday(force: force)
    }

    private func handleSelectedTimeRangeChange() {
        focusedDay = nil
        applyTimeRange(selectedTimeRange)
        updateDashboardData()
        tokenMixLegendPage = 0
        cliConsumptionPage = 0
        modelCostLegendPage = 0
        if selectedTimeRange == .today {
            cliCompositionPane = .cli
        }
        if selectedTimeRange == .all {
            jumpAllRangeChartPageToLatest()
            jumpHeatmapPageToLatest()
        } else {
            allRangeChartPage = 0
            heatmapPage = 0
        }
    }

    private func applyTimeRange(_ range: DashboardTimeRange) {
        let calendar = Calendar.current
        let today = Date()
        let tomorrow = calendar.date(byAdding: .day, value: 1, to: today) ?? today
        if let focusedDay {
            store.startDate = calendar.startOfDay(for: focusedDay)
            store.endDate = calendar.date(byAdding: .day, value: 1, to: calendar.startOfDay(for: focusedDay)) ?? tomorrow
            return
        }
        switch range {
        case .today:
            store.startDate = calendar.startOfDay(for: today)
        case .month:
            store.startDate = calendar.dateInterval(of: .month, for: today)?.start ?? calendar.startOfDay(for: today)
        case .all:
            var components = DateComponents()
            components.year = 2024
            components.month = 1
            components.day = 1
            store.startDate = calendar.date(from: components) ?? calendar.startOfDay(for: today)
        }
        store.endDate = tomorrow
    }

    // MARK: - Day drill-down

    private var todayDateString: String {
        DayDrillDownFormatters.dayString.string(from: Date())
    }

    private var focusedDayString: String? {
        focusedDay.map { DayDrillDownFormatters.dayString.string(from: $0) }
    }

    /// Enters the day view for an arbitrary day (from the heatmap or the
    /// calendar jump). Loads that day's records, hourly strip, and brief.
    private func selectDay(_ date: Date) {
        focusedDay = date
        applyTimeRange(selectedTimeRange)
        updateDashboardData()
        let dateString = DayDrillDownFormatters.dayString.string(from: date)
        Task {
            await store.refreshDashboard(force: false)
            await store.loadHourly(for: dateString)
            await store.loadBrief(for: dateString)
        }
    }

    private func clearFocusedDay() {
        focusedDay = nil
        applyTimeRange(selectedTimeRange)
        updateDashboardData()
        Task { await store.refreshDashboard(force: false) }
    }

    private var visibleSourcePickerOptions: [TokenUsageSource] {
        [.all] + TokenUsageSource.allCases.filter { source in
            source != .all && preferences.enabledSources.contains(source)
        }
    }

    private var timeRangePicker: some View {
        Picker("Range", selection: $selectedTimeRange) {
            ForEach(DashboardTimeRange.allCases) { range in
                Text(range.label).tag(range)
            }
        }
        .labelsHidden()
        .pickerStyle(.segmented)
        .frame(width: 260)
    }

    /// Chip shown while the day drill-down is active; clearing it returns to
    /// the selected time range.
    private func focusedDayChip(_ day: Date) -> some View {
        HStack(spacing: 5) {
            Image(systemName: "calendar.day.timeline.left")
                .font(.caption2)
            Text(DayDrillDownFormatters.dayTitle.string(from: day))
                .font(.caption.weight(.medium))
            Button {
                clearFocusedDay()
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 8, weight: .bold))
            }
            .buttonStyle(.plain)
        }
        .foregroundStyle(Color.accentColor)
        .padding(.horizontal, 9)
        .padding(.vertical, 5)
        .appGlassChip()
        .help("Day view — click × to return")
    }

    /// Calendar jump into the day view from any range.
    private var dayJumpButton: some View {
        Button {
            dayJumpSelection = focusedDay ?? Date()
            isDayJumpPresented = true
        } label: {
            Image(systemName: "calendar")
                .font(.system(size: 12, weight: .medium))
        }
        .help("Jump to a day")
        .popover(isPresented: $isDayJumpPresented, arrowEdge: .bottom) {
            VStack(alignment: .leading, spacing: 10) {
                Text("跳转到某天")
                    .font(.headline)
                DatePicker(
                    "日期",
                    selection: $dayJumpSelection,
                    in: ...Date(),
                    displayedComponents: .date
                )
                .datePickerStyle(.graphical)
                .labelsHidden()
                HStack {
                    Spacer()
                    Button("查看当天") {
                        isDayJumpPresented = false
                        selectDay(dayJumpSelection)
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                }
            }
            .padding(14)
        }
    }

    private var sourcePicker: some View {
        Picker("Source", selection: $store.selectedSource) {
            ForEach(visibleSourcePickerOptions) { source in
                HStack(spacing: 4) {
                    if let imageAssetName = source.imageAssetName {
                        BundledIconImage(imageAssetName: imageAssetName, padding: 1, size: 16)
                    } else {
                        Image(systemName: source.systemImage)
                            .font(.system(size: 11))
                            .frame(width: 16, height: 16)
                    }
                    Text(source.label)
                }
                .tag(source)
            }
        }
        .pickerStyle(.menu)
        .fixedSize()
        .help("Data source")
    }

    private func monthStart(for date: Date) -> Date {
        Calendar.current.dateInterval(of: .month, for: date)?.start ?? date
    }

    private func monthEnd(for date: Date) -> Date {
        let calendar = Calendar.current
        guard let interval = calendar.dateInterval(of: .month, for: date),
              let end = calendar.date(byAdding: .day, value: -1, to: interval.end) else {
            return date
        }
        return end
    }

    private var showsModelFilter: Bool {
        !availableModels.isEmpty && store.selectedSource != .all
    }

    private var modelFilterPopover: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Text("Models")
                    .font(.headline)
                Spacer()
                Text(modelSelectionSummary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            HStack(spacing: 8) {
                TextField("Search models", text: $modelSearchText)
                    .textFieldStyle(.roundedBorder)

                Button("All Models") {
                    store.selectedModels.removeAll()
                    modelSearchText = ""
                }
                .disabled(store.selectedModels.isEmpty && modelSearchText.isEmpty)
            }

            if filteredAvailableModels.isEmpty {
                ContentUnavailableView {
                    Label("No Matching Models", systemImage: "magnifyingglass")
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView(.vertical) {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(filteredAvailableModels, id: \.self) { model in
                            Toggle(isOn: modelBinding(model)) {
                                HStack(spacing: 8) {
                                    ProviderIconBadge(modelName: model)
                                    Text(displayModelName(model))
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                        .help(model)
                                }
                            }
                            .toggleStyle(.checkbox)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 5)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
            }

            Text("\(filteredAvailableModels.count) of \(availableModels.count) models")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(12)
        .frame(width: 340, height: 400)
    }

    private func todayOverviewResolvedContentHeight(for summary: TodaySummaryResponse) -> Double {
        let rowCount = Double(min(summary.sourceRows.count, cliConsumptionPageSize))
        let sourceRowsHeight = rowCount * todaySourceRowHeight + max(rowCount - 1, 0) * todaySourceRowSpacing
        return max(todayOverviewContentHeight, sourceRowsHeight)
    }

    @ViewBuilder
    private func todaySourceBreakdown(summary: TodaySummaryResponse) -> some View {
        if summary.sourceRows.isEmpty {
            emptyTodayState(text: store.isLoading ? "Loading usage..." : "No usage recorded today")
                .frame(maxWidth: .infinity, alignment: .center)
                .frame(height: todayOverviewResolvedContentHeight(for: summary))
        } else {
            VStack(alignment: .leading, spacing: 10) {
                ForEach(visibleCLIConsumptionRows(from: summary), id: \.source) { row in
                    TodaySourceRowView(
                        row: row,
                        maxTokens: summary.maxSourceTokens,
                        costText: currencyController.string(fromUSD: row.totalCostDecimal)
                    )
                    .frame(height: todaySourceRowHeight)
                }
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .frame(height: todayOverviewResolvedContentHeight(for: summary), alignment: .topLeading)
        }
    }

    private func cliConsumptionPageCount(for summary: TodaySummaryResponse) -> Int {
        max(Int(ceil(Double(summary.sourceRows.count) / Double(cliConsumptionPageSize))), 1)
    }

    private func clampedCLIConsumptionPage(for summary: TodaySummaryResponse) -> Int {
        min(max(cliConsumptionPage, 0), cliConsumptionPageCount(for: summary) - 1)
    }

    private func visibleCLIConsumptionRows(from summary: TodaySummaryResponse) -> [TodaySourceUsageRow] {
        let rows = summary.sourceRows
        let page = clampedCLIConsumptionPage(for: summary)
        let start = page * cliConsumptionPageSize
        guard start < rows.count else { return rows }
        let end = min(start + cliConsumptionPageSize, rows.count)
        return Array(rows[start..<end])
    }

    @ViewBuilder
    private func cliConsumptionPager(for summary: TodaySummaryResponse) -> some View {
        if !summary.sourceRows.isEmpty {
            LegendPageControls(
                currentPage: clampedCLIConsumptionPage(for: summary),
                pageCount: cliConsumptionPageCount(for: summary),
                totalCount: summary.sourceRows.count,
                onPrevious: {
                    cliConsumptionPage = max(clampedCLIConsumptionPage(for: summary) - 1, 0)
                },
                onNext: {
                    cliConsumptionPage = min(
                        clampedCLIConsumptionPage(for: summary) + 1,
                        cliConsumptionPageCount(for: summary) - 1
                    )
                }
            )
        }
    }

    @ViewBuilder
    private func todayTokenMix(summary: TodaySummaryResponse) -> some View {
        TokenMixDistributionChart(
            rows: summary.modelTokenRows,
            legendRows: visibleTokenMixRows(from: summary.modelTokenRows),
            totalTokens: summary.totalTokens,
            isLoading: store.isLoading
        )
    }

    private func tokenMixLegendPageCount(for rows: [TodayModelTokenRow]) -> Int {
        max(Int(ceil(Double(rows.count) / Double(modelDistributionLegendCapacity))), 1)
    }

    private func clampedTokenMixLegendPage(for rows: [TodayModelTokenRow]) -> Int {
        min(max(tokenMixLegendPage, 0), tokenMixLegendPageCount(for: rows) - 1)
    }

    private func visibleTokenMixRows(from rows: [TodayModelTokenRow]) -> [TodayModelTokenRow] {
        let page = clampedTokenMixLegendPage(for: rows)
        let start = page * modelDistributionLegendCapacity
        guard start < rows.count else { return rows }
        let end = min(start + modelDistributionLegendCapacity, rows.count)
        return Array(rows[start..<end])
    }

    @ViewBuilder
    private var tokenMixLegendPager: some View {
        let rows = dashboardSummary.modelTokenRows
        if !rows.isEmpty {
            LegendPageControls(
                currentPage: clampedTokenMixLegendPage(for: rows),
                pageCount: tokenMixLegendPageCount(for: rows),
                totalCount: rows.count,
                onPrevious: {
                    tokenMixLegendPage = max(clampedTokenMixLegendPage(for: rows) - 1, 0)
                },
                onNext: {
                    tokenMixLegendPage = min(
                        clampedTokenMixLegendPage(for: rows) + 1,
                        tokenMixLegendPageCount(for: rows) - 1
                    )
                }
            )
        }
    }

    private func emptyTodayState(text: String) -> some View {
        VStack(spacing: 8) {
            Image(systemName: "chart.bar.doc.horizontal")
                .font(.title2)
                .foregroundStyle(.secondary)
            Text(text)
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, minHeight: 220, alignment: .center)
    }

    private var heroMetricsRow: some View {
        VStack(alignment: .leading, spacing: 16) {
            DashboardHeroView(
                periodLabel: heroPeriodLabel,
                totalTokens: totalTokens,
                costText: currencyController.string(fromUSD: totalCost),
                subtitle: heroSubtitle,
                isRefreshing: store.isLoading
            )

            HStack(alignment: .top, spacing: 12) {
                TodayMetricCard(
                    title: "Cache Read",
                    value: dashboardData.cacheReadTokens.tokenText,
                    subtitle: "\(dashboardCacheReadShare.percentText) of tokens",
                    systemImage: "externaldrive.badge.icloud",
                    tint: .purple
                )
                TodayMetricCard(
                    title: "Cache Share",
                    value: dashboardCacheShare.percentText,
                    subtitle: "read + create",
                    systemImage: "chart.pie",
                    tint: .orange
                )
                TodayMetricCard(
                    title: "Input",
                    value: dashboardData.inputTokens.tokenText,
                    subtitle: dashboardInputShare.percentText,
                    systemImage: "arrow.down.to.line.compact",
                    tint: .cyan
                )
                TodayMetricCard(
                    title: "Output",
                    value: dashboardData.outputTokens.tokenText,
                    subtitle: dashboardOutputShare.percentText,
                    systemImage: "arrow.up.to.line.compact",
                    tint: .pink
                )
            }
        }
    }

    private var heroPeriodLabel: String {
        if let focusedDay {
            let dateString = DayDrillDownFormatters.dayString.string(from: focusedDay)
            return dateString == todayDateString
                ? "Today"
                : DayDrillDownFormatters.dayTitle.string(from: focusedDay)
        }
        return switch selectedTimeRange {
        case .today: "Today"
        case .month: "This Month"
        case .all: "All Time"
        }
    }

    private var heroSubtitle: String {
        "\(dashboardSummary.activeSourceCount) active CLI · \(dashboardSummary.modelCount) models"
    }

    private var dashboardCacheReadShare: Double {
        dashboardShare(dashboardData.cacheReadTokens)
    }

    private var dashboardCacheShare: Double {
        dashboardShare(dashboardData.cacheReadTokens + dashboardData.cacheCreationTokens)
    }

    private var dashboardInputShare: Double {
        dashboardShare(dashboardData.inputTokens)
    }

    private var dashboardOutputShare: Double {
        dashboardShare(dashboardData.outputTokens)
    }

    private func dashboardShare(_ tokens: Int) -> Double {
        guard totalTokens > 0 else { return 0 }
        return Double(tokens) / Double(totalTokens)
    }

    private var dashboardSummary: TodaySummaryResponse {
        dashboardData.summary
    }

    private var chartSection: some View {
        let paneHeight = todayOverviewResolvedContentHeight(for: dashboardSummary)
        let showsCompositionToggle = selectedTimeRange != .today

        return Grid(horizontalSpacing: 16, verticalSpacing: 16) {
            GridRow {
                ChartCard(title: showsCompositionToggle ? cliCompositionPane.label : DashboardCLICompositionPane.cli.label) {
                    HStack(spacing: 10) {
                        if showsCompositionToggle {
                            Picker("", selection: $cliCompositionPane) {
                                ForEach(DashboardCLICompositionPane.allCases) { pane in
                                    Text(pane.pickerLabel).tag(pane)
                                }
                            }
                            .labelsHidden()
                            .pickerStyle(.segmented)
                            .frame(width: 140)
                        }

                        if showsCompositionToggle && cliCompositionPane == .composition {
                            allRangeBarPager
                        } else {
                            cliConsumptionPager(for: dashboardSummary)
                        }
                    }
                } content: {
                    if showsCompositionToggle {
                        switch cliCompositionPane {
                        case .cli:
                            todaySourceBreakdown(summary: dashboardSummary)
                        case .composition:
                            compositionChart
                                .frame(maxWidth: .infinity, maxHeight: .infinity)
                        }
                    } else {
                        todaySourceBreakdown(summary: dashboardSummary)
                    }
                }
                .frame(height: paneHeight + 66, alignment: .top)

                ChartCard(title: modelCostMixPane.label) {
                    HStack(spacing: 10) {
                        Picker("", selection: $modelCostMixPane) {
                            ForEach(DashboardModelCostMixPane.allCases) { pane in
                                Text(pane.label).tag(pane)
                            }
                        }
                        .labelsHidden()
                        .pickerStyle(.segmented)
                        .frame(width: 180)

                        if modelCostMixPane == .cost {
                            modelCostLegendPager
                        } else {
                            tokenMixLegendPager
                        }
                    }
                } content: {
                    modelDistributionContent(summary: dashboardSummary)
                }
                .frame(height: paneHeight + 66, alignment: .top)
            }
        }
    }

    @ViewBuilder
    private func modelDistributionContent(summary: TodaySummaryResponse) -> some View {
        GeometryReader { geometry in
            Group {
                switch modelCostMixPane {
                case .cost:
                    modelCostChart
                case .mix:
                    todayTokenMix(summary: summary)
                }
            }
            .frame(width: geometry.size.width, height: geometry.size.height)
            .onAppear {
                updateModelDistributionLegendCapacity(for: geometry.size.height)
            }
            .onChange(of: geometry.size.height) { _, height in
                updateModelDistributionLegendCapacity(for: height)
            }
        }
    }

    private func updateModelDistributionLegendCapacity(for availableHeight: CGFloat) {
        let stride = modelDistributionLegendRowHeight + modelDistributionLegendRowSpacing
        let fitted = max(Int((availableHeight + modelDistributionLegendRowSpacing) / stride), 1)
        let capacity = min(fitted, modelDistributionLegendMaxPageSize)
        guard capacity != modelDistributionLegendCapacity else { return }
        modelDistributionLegendCapacity = capacity
    }

    private var dailyUsageCardHeight: CGFloat {
        dailyUsageContentHeight + (selectedTimeRange == .all ? 92 : 68)
    }

    private var dailyTokenUsageSection: some View {
        GeometryReader { geometry in
            let spacing: CGFloat = 20
            let heatmapWidth = max((geometry.size.width - spacing) / 3, 0)

            HStack(alignment: .top, spacing: spacing) {
                ChartCard(title: "Daily Heatmap") {
                    heatmapPager
                } content: {
                    GeometryReader { heatmapGeometry in
                        DashboardHeatmapView(
                            days: visibleHeatmapDays,
                            availableWidth: heatmapGeometry.size.width,
                            costText: { currencyController.string(fromUSD: $0) },
                            onSelectDay: { selectDay($0.date) }
                        )
                        .frame(width: heatmapGeometry.size.width, height: dailyUsageContentHeight)
                    }
                    .frame(height: dailyUsageContentHeight)
                }
                .frame(width: heatmapWidth, height: dailyUsageCardHeight)

                ChartCard(title: "Daily Token Usage") {
                    VStack(alignment: .trailing, spacing: 6) {
                        Picker("", selection: $dailyUsageAggregation) {
                            ForEach(DashboardDailyUsageAggregation.allCases) { aggregation in
                                Text(aggregation.label).tag(aggregation)
                            }
                        }
                        .labelsHidden()
                        .pickerStyle(.segmented)
                        .frame(width: 132)

                        allRangeBarPager
                    }
                } content: {
                    tokenTrendChart
                        .frame(
                            maxWidth: .infinity,
                            minHeight: dailyUsageContentHeight,
                            maxHeight: dailyUsageContentHeight,
                            alignment: .topLeading
                        )
                }
                .frame(width: heatmapWidth * 2, height: dailyUsageCardHeight)
            }
        }
        .frame(height: dailyUsageCardHeight)
    }

    private var modelConsumptionSection: some View {
        ChartCard(title: "Model Consumption") {
            HStack(spacing: 10) {
                Picker("Group by", selection: $modelConsumptionGrouping) {
                    ForEach(ModelConsumptionGrouping.allCases) { grouping in
                        Text(grouping.label).tag(grouping)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 140)

                modelConsumptionFilterField
            }
        } content: {
            dashboardModelConsumption
        }
    }

    private var modelConsumptionFilterField: some View {
        HStack(spacing: 5) {
            Image(systemName: "magnifyingglass")
                .font(.caption2)
                .foregroundStyle(.tertiary)
            TextField(
                modelConsumptionGrouping == .model ? "Filter models" : "Filter CLIs",
                text: $modelConsumptionFilter
            )
            .textFieldStyle(.plain)
            .font(.caption)
            if !modelConsumptionFilter.isEmpty {
                Button {
                    modelConsumptionFilter = ""
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 9)
        .padding(.vertical, 5)
        .appGlassChip()
        .frame(width: 170)
    }

    @ViewBuilder
    private var dashboardModelConsumption: some View {
        let rows = filteredModelConsumptionRows
        if dashboardData.modelUsageRows.isEmpty {
            emptyTodayState(text: store.isLoading ? "Loading models..." : "No model usage recorded")
        } else if rows.isEmpty {
            emptyTodayState(text: "No matching entries")
        } else {
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 10) {
                    Text(modelConsumptionGrouping == .model ? "Model" : "CLI")
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Text(modelConsumptionGrouping == .model ? "Used by" : "Models")
                        .frame(width: 150, alignment: .leading)
                    Text("Tokens")
                        .frame(width: 92, alignment: .trailing)
                    Text("Cache")
                        .frame(width: 78, alignment: .trailing)
                    Text("Cost")
                        .frame(width: 86, alignment: .trailing)
                }
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 2)
                .padding(.bottom, 6)

                Divider()

                ForEach(Array(rows.enumerated()), id: \.element.id) { index, row in
                    ModelConsumptionGroupRowView(
                        row: row,
                        grouping: modelConsumptionGrouping,
                        costText: currencyController.string(fromUSD: row.totalCost)
                    )
                    if index < rows.count - 1 {
                        Divider()
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
    }

    /// Groups the (CLI × model) usage rows by the selected dimension and
    /// keeps the other dimension as hoverable icon parts.
    private var groupedModelConsumptionRows: [ModelConsumptionGroupRow] {
        var totals: [String: ModelConsumptionTotals] = [:]
        for row in dashboardData.modelUsageRows {
            let key = modelConsumptionGrouping == .model ? row.modelName : row.source.rawValue
            var entry = totals[key] ?? ModelConsumptionTotals()
            entry.total += row.totalTokens
            entry.cache += row.cacheCreationTokens + row.cacheReadTokens
            entry.cost += row.totalCostDecimal
            let partCost = currencyController.string(fromUSD: row.totalCostDecimal)
            let partLabel: String
            let partModel: String?
            let partSource: UsageSource?
            switch modelConsumptionGrouping {
            case .model:
                partLabel = row.source.label
                partModel = nil
                partSource = row.source
            case .cli:
                partLabel = displayModelName(row.modelName)
                partModel = row.modelName
                partSource = nil
            }
            entry.parts.append(
                ModelConsumptionPart(
                    id: modelConsumptionGrouping == .model ? row.source.rawValue : row.modelName,
                    label: partLabel,
                    modelName: partModel,
                    source: partSource,
                    tokens: row.totalTokens,
                    tooltip: "\(partLabel)\n\(row.totalTokens.tokenText) tokens · \(partCost)"
                )
            )
            totals[key] = entry
        }

        return totals.map { key, entry in
            ModelConsumptionGroupRow(
                id: key,
                title: modelConsumptionGrouping == .model ? displayModelName(key) : (UsageSource(rawValue: key)?.label ?? key),
                modelName: modelConsumptionGrouping == .model ? key : nil,
                source: modelConsumptionGrouping == .cli ? UsageSource(rawValue: key) : nil,
                totalTokens: entry.total,
                cacheShare: entry.total > 0 ? Double(entry.cache) / Double(entry.total) : 0,
                totalCost: entry.cost,
                parts: entry.parts.sorted { $0.tokens > $1.tokens }
            )
        }
        .sorted { $0.totalCost > $1.totalCost }
    }

    private var filteredModelConsumptionRows: [ModelConsumptionGroupRow] {
        let query = modelConsumptionFilter.trimmingCharacters(in: .whitespaces)
        guard !query.isEmpty else { return groupedModelConsumptionRows }
        return groupedModelConsumptionRows.filter { row in
            row.title.localizedCaseInsensitiveContains(query)
                || row.parts.contains { $0.label.localizedCaseInsensitiveContains(query) }
        }
    }

    // MARK: - Activity insight sections (Daily Cost / By Weekday / Cache Efficiency)

    private var activityInsightPlotHeight: CGFloat { 180 }
    private var activityInsightCardHeight: CGFloat { activityInsightPlotHeight + 66 }

    private var activityCostRangeTotal: String {
        let total = dashboardData.costTrendRows.reduce(Decimal.zero) { $0 + Decimal($1.value) }
        return "Total \(currencyController.string(fromUSD: total))"
    }

    private var activityWeekdayPeakSummary: String? {
        guard let peak = dashboardData.weekdayAverageRows.max(by: { $0.averageTokens < $1.averageTokens }),
              peak.averageTokens > 0 else { return nil }
        return "Peak \(peak.label) · \(peak.averageTokens.tokenAxisText)"
    }

    private var activityCacheAverageSummary: String? {
        let rows = dashboardData.cacheShareRows
        guard !rows.isEmpty else { return nil }
        let average = rows.reduce(0.0) { $0 + $1.value } / Double(rows.count)
        return "Avg \(average.percentText)"
    }

    private var activityInsightsSection: some View {
        GeometryReader { geometry in
            let spacing: CGFloat = 20
            let weekdayWidth = max((geometry.size.width - spacing) / 3, 0)

            HStack(alignment: .top, spacing: spacing) {
                ChartCard(title: "Daily Cost") {
                    Text(activityCostRangeTotal)
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                } content: {
                    CostTrendChartView(
                        rows: dashboardData.costTrendRows,
                        formatCost: { currencyController.string(fromUSD: $0) }
                    )
                    .frame(height: activityInsightPlotHeight)
                }
                .frame(width: weekdayWidth * 2, height: activityInsightCardHeight)

                ChartCard(title: "By Weekday") {
                    if let activityWeekdayPeakSummary {
                        Text(activityWeekdayPeakSummary)
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                } content: {
                    WeekdayAverageChartView(rows: dashboardData.weekdayAverageRows)
                        .frame(height: activityInsightPlotHeight)
                }
                .frame(width: weekdayWidth, height: activityInsightCardHeight)
            }
        }
        .frame(height: activityInsightCardHeight)
    }

    private var cacheEfficiencySection: some View {
        ChartCard(title: "Cache Efficiency") {
            if let activityCacheAverageSummary {
                Text(activityCacheAverageSummary)
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        } content: {
            CacheShareChartView(rows: dashboardData.cacheShareRows)
                .frame(height: activityInsightPlotHeight)
        }
        .frame(height: activityInsightCardHeight)
    }

    // MARK: - Hourly (Today range)

    private var hourlyPoints: [HourlyUsagePoint] {
        guard let hours = activeHourly?.hours else { return [] }
        return hours.compactMap { row in
            guard let tokenSource = TokenUsageSource(rawValue: row.source.rawValue),
                  preferences.enabledSources.contains(tokenSource),
                  store.selectedSource == .all || store.selectedSource == tokenSource
            else { return nil }
            return HourlyUsagePoint(hour: row.hour, series: row.source.displayName, tokens: row.totalTokens)
        }
    }

    private var hourlyColorDomain: [String] {
        Array(Set(hourlyPoints.map(\.series))).sorted()
    }

    private var hourlyColorRange: [Color] {
        hourlyColorDomain.map { tokenTrendSourceColors[$0] ?? .blue }
    }

    private var hourlyPeakSummary: String? {
        var totals: [Int: Int] = [:]
        for point in hourlyPoints where (0...23).contains(point.hour) {
            totals[point.hour, default: 0] += point.tokens
        }
        guard let peak = totals.max(by: { $0.value < $1.value }), peak.value > 0 else {
            return nil
        }
        return "Peak \(TodayHourlyTimelineView.hourLabel(peak.key)) · \(peak.value.tokenText)"
    }

    private var hourlyShowsNowIndicator: Bool {
        guard let date = activeHourly?.date else { return false }
        return date == todayDateString
    }

    /// Brief hour headlines for the day shown in day mode.
    private var dayTimelineBriefHours: [HourlyBriefItem] {
        let dateString = focusedDayString ?? todayDateString
        if dateString == todayDateString, let todayBrief = store.todayBrief {
            return todayBrief.hours ?? []
        }
        return store.briefCache[dateString]?.hours ?? []
    }

    /// 24h usage strip + hour event rail (no Daily Brief kanban board).
    private var todayTimelineSection: some View {
        let title = focusedDayString.map { dateString in
            dateString == todayDateString
                ? "Today Timeline"
                : "\(DayDrillDownFormatters.dayTitle.string(from: focusedDay ?? Date())) Timeline"
        } ?? "Today Timeline"
        return ChartCard(title: title) {
            if let hourlyPeakSummary {
                Text(hourlyPeakSummary)
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        } content: {
            TodayHourlyTimelineView(
                points: hourlyPoints,
                briefHours: dayTimelineBriefHours,
                colorDomain: hourlyColorDomain,
                colorRange: hourlyColorRange,
                showsNowIndicator: hourlyShowsNowIndicator
            )
        }
    }

    private var tokenTrendChart: some View {
        VStack(alignment: .leading, spacing: 10) {
            TokenTrendChartView(
                rows: pagedTokenTrendRows,
                colorDomain: tokenTrendColorDomain,
                colorRange: tokenTrendColorRange,
                seriesLabel: tokenTrendSeriesLabel,
                viewMode: store.selectedViewMode,
                dateColumnTitle: dateColumnTitle,
                monthlyXAxisValues: monthlyXAxisValues,
                dailyXAxisValues: pagedDailyXAxisValues,
                chartXScaleDomain: pagedChartXScaleDomain,
                tooltipDateText: tooltipDateText(for:),
                monthAxisLabel: monthAxisLabel(for:),
                monthSeparatorLabel: monthSeparatorLabel(for:),
                isFirstDayOfMonth: isFirstDayOfMonth(_:)
            )

            TokenTrendLegend(
                entries: visibleTokenTrendLegendEntries,
                currentPage: clampedTokenTrendLegendPage,
                pageCount: tokenTrendLegendPageCount,
                totalCount: tokenTrendColorDomain.count,
                usesCompactLayout: true,
                isCLIGrouping: usesCLITokenTrendGrouping,
                onPrevious: {
                    tokenTrendLegendPage = max(clampedTokenTrendLegendPage - 1, 0)
                },
                onNext: {
                    tokenTrendLegendPage = min(clampedTokenTrendLegendPage + 1, tokenTrendLegendPageCount - 1)
                }
            )
        }
    }

    private var compositionChart: some View {
        CompositionChartView(
            inputRows: pagedCompositionRows(dashboardData.compositionInputRows),
            cacheReadRows: pagedCompositionRows(dashboardData.compositionCacheReadRows),
            outputRows: pagedCompositionRows(dashboardData.compositionOutputRows),
            byDateKey: dashboardData.compositionByDateKey,
            viewMode: store.selectedViewMode,
            dateColumnTitle: dateColumnTitle,
            monthlyXAxisValues: monthlyXAxisValues,
            dailyXAxisValues: pagedDailyXAxisValues,
            chartXScaleDomain: pagedChartXScaleDomain,
            tooltipDateText: tooltipDateText(for:),
            monthAxisLabel: monthAxisLabel(for:),
            monthSeparatorLabel: monthSeparatorLabel(for:),
            isFirstDayOfMonth: isFirstDayOfMonth(_:)
        )
    }

    private var modelCostChart: some View {
        ModelCostDistributionChart(
            slices: modelCostSlices,
            legendSlices: visibleModelCostLegendSlices,
            totalCost: modelCostTotalCost,
            currencyController: currencyController
        )
    }

    @ViewBuilder
    private var modelCostLegendPager: some View {
        if !modelCostSlices.isEmpty {
            LegendPageControls(
                currentPage: clampedModelCostLegendPage,
                pageCount: modelCostLegendPageCount,
                totalCount: modelCostSlices.count,
                onPrevious: {
                    modelCostLegendPage = max(clampedModelCostLegendPage - 1, 0)
                },
                onNext: {
                    modelCostLegendPage = min(clampedModelCostLegendPage + 1, modelCostLegendPageCount - 1)
                }
            )
        }
    }

    private var modelCostSlices: [ModelCostSlice] {
        dashboardData.modelCostSlices
    }

    private var modelCostTotalCost: Decimal {
        dashboardData.modelCostTotalCost
    }

    private var modelCostLegendPageCount: Int {
        max(Int(ceil(Double(modelCostSlices.count) / Double(modelDistributionLegendCapacity))), 1)
    }

    private var clampedModelCostLegendPage: Int {
        min(max(modelCostLegendPage, 0), modelCostLegendPageCount - 1)
    }

    private var visibleModelCostLegendSlices: [ModelCostSlice] {
        let start = clampedModelCostLegendPage * modelDistributionLegendCapacity
        guard start < modelCostSlices.count else { return modelCostSlices }
        let end = min(start + modelDistributionLegendCapacity, modelCostSlices.count)
        return Array(modelCostSlices[start..<end])
    }

    private var tokenTrendRows: [TokenTrendRow] {
        dashboardData.tokenTrendRows
    }

    private var tokenTrendSeriesLabel: String {
        dailyUsageAggregation.seriesLabel
    }

    private var usesCLITokenTrendGrouping: Bool {
        dailyUsageAggregation == .cli
    }

    private var tokenTrendColorDomain: [String] {
        dashboardData.tokenTrendColorDomain
    }

    private var tokenTrendColorRange: [Color] {
        if usesCLITokenTrendGrouping {
            return tokenTrendColorDomain.map { tokenTrendSourceColors[$0] ?? .blue }
        }

        return tokenTrendColorDomain.map { stableModelColor(for: $0) }
    }

    private var tokenTrendLegendPageCount: Int {
        max(Int(ceil(Double(tokenTrendColorDomain.count) / Double(tokenTrendLegendPageSize))), 1)
    }

    private var clampedTokenTrendLegendPage: Int {
        min(max(tokenTrendLegendPage, 0), tokenTrendLegendPageCount - 1)
    }

    private var visibleTokenTrendColorDomain: [String] {
        let start = clampedTokenTrendLegendPage * tokenTrendLegendPageSize
        guard start < tokenTrendColorDomain.count else { return tokenTrendColorDomain }
        let end = min(start + tokenTrendLegendPageSize, tokenTrendColorDomain.count)
        return Array(tokenTrendColorDomain[start..<end])
    }

    private var visibleTokenTrendLegendEntries: [TokenTrendLegendEntry] {
        visibleTokenTrendColorDomain.map { series in
            TokenTrendLegendEntry(label: series, color: tokenTrendColor(for: series))
        }
    }

    private func tokenTrendColor(for series: String) -> Color {
        if usesCLITokenTrendGrouping {
            return tokenTrendSourceColors[series] ?? .blue
        }

        return stableModelColor(for: series)
    }

    private var compositionRows: [TokenCompositionRow] {
        dashboardData.compositionRows
    }

    private var dateColumnTitle: String {
        store.selectedViewMode == .monthly ? "Month" : "Date"
    }

    private var monthlyXAxisValues: [Date] {
        dashboardData.monthlyXAxisValues
    }

    private var spansMultipleYears: Bool {
        dashboardData.spansMultipleYears
    }

    private func monthAxisLabel(for date: Date) -> String {
        if spansMultipleYears {
            return date.formatted(.dateTime.year().month(.abbreviated))
        }
        return date.formatted(.dateTime.month(.abbreviated))
    }

    private var dailyXAxisValues: [Date] {
        dashboardData.dailyXAxisValues
    }

    private var shouldPageAllRangeBars: Bool {
        selectedTimeRange == .all && dailyXAxisValues.count > allRangeBarPageSize
    }

    private var allRangeChartPageCount: Int {
        max(Int(ceil(Double(dailyXAxisValues.count) / Double(allRangeBarPageSize))), 1)
    }

    private var clampedAllRangeChartPage: Int {
        min(max(allRangeChartPage, 0), allRangeChartPageCount - 1)
    }

    private var visibleAllBarDays: [Date] {
        let days = dailyXAxisValues
        guard shouldPageAllRangeBars else { return days }
        let start = clampedAllRangeChartPage * allRangeBarPageSize
        let end = min(start + allRangeBarPageSize, days.count)
        guard start < days.count else { return days }
        return Array(days[start..<end])
    }

    private var visibleAllBarDaySet: Set<Date> {
        Set(visibleAllBarDays.map { Calendar.current.startOfDay(for: $0) })
    }

    private var pagedDailyXAxisValues: [Date] {
        visibleAllBarDays
    }

    private var pagedTokenTrendRows: [TokenTrendRow] {
        guard shouldPageAllRangeBars else { return tokenTrendRows }
        return tokenTrendRows.filter { visibleAllBarDaySet.contains(Calendar.current.startOfDay(for: $0.date)) }
    }

    private func pagedCompositionRows(_ rows: [TokenCompositionRow]) -> [TokenCompositionRow] {
        guard shouldPageAllRangeBars else { return rows }
        return rows.filter { visibleAllBarDaySet.contains(Calendar.current.startOfDay(for: $0.date)) }
    }

    private var pagedChartXScaleDomain: ClosedRange<Date> {
        guard shouldPageAllRangeBars, let first = visibleAllBarDays.first, let last = visibleAllBarDays.last else {
            return chartXScaleDomain
        }
        let start = Calendar.current.startOfDay(for: first)
        let end = Calendar.current.date(byAdding: .day, value: 1, to: Calendar.current.startOfDay(for: last)) ?? last
        return start...end
    }

    private var allRangeVisibleDateLabel: String {
        guard let first = visibleAllBarDays.first, let last = visibleAllBarDays.last else { return "" }
        let firstText = first.formatted(.dateTime.month(.abbreviated).day())
        let lastText = last.formatted(.dateTime.month(.abbreviated).day())
        return "\(firstText) – \(lastText)"
    }

    @ViewBuilder
    private var allRangeBarPager: some View {
        if shouldPageAllRangeBars {
            HStack(spacing: 8) {
                Text(allRangeVisibleDateLabel)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .help("Showing up to \(allRangeBarPageSize) days")

                LegendPageControls(
                    currentPage: clampedAllRangeChartPage,
                    pageCount: allRangeChartPageCount,
                    totalCount: dailyXAxisValues.count,
                    onPrevious: {
                        allRangeChartPage = max(clampedAllRangeChartPage - 1, 0)
                    },
                    onNext: {
                        allRangeChartPage = min(clampedAllRangeChartPage + 1, allRangeChartPageCount - 1)
                    }
                )
            }
        }
    }

    private func jumpAllRangeChartPageToLatest() {
        allRangeChartPage = max(allRangeChartPageCount - 1, 0)
    }

    // MARK: - Heatmap pagination (All view: sliding 2-calendar-month window)

    /// All heatmap windows contain two consecutive calendar months and advance one month at a time.
    /// The newest window is always the previous month plus the current month.
    private var heatmapRounds: [(start: Date, end: Date)] {
        guard selectedTimeRange == .all, let firstDay = dashboardData.heatmapDays.first else { return [] }
        let calendar = Calendar.current
        let firstMonthStart = monthStart(for: firstDay.date)
        let currentMonthStart = monthStart(for: Date())
        guard let latestWindowStart = calendar.date(byAdding: .month, value: -1, to: currentMonthStart) else {
            return []
        }

        // Even if the available history starts this month, retain the required
        // previous-month + current-month window and render missing days as zero.
        guard firstMonthStart <= latestWindowStart else {
            return [(start: latestWindowStart, end: monthEnd(for: currentMonthStart))]
        }

        var rounds: [(Date, Date)] = []
        var cursor = firstMonthStart
        while cursor <= latestWindowStart {
            let roundStart = cursor
            guard let secondMonthStart = calendar.date(byAdding: .month, value: 1, to: roundStart) else {
                break
            }
            rounds.append((roundStart, monthEnd(for: secondMonthStart)))

            guard let nextWindowStart = calendar.date(byAdding: .month, value: 1, to: roundStart) else {
                break
            }
            cursor = nextWindowStart
            if cursor <= roundStart { break }
        }
        return rounds
    }

    private var heatmapRoundCount: Int {
        max(heatmapRounds.count, 1)
    }

    private var clampedHeatmapRound: Int {
        min(max(heatmapPage, 0), heatmapRoundCount - 1)
    }

    private var visibleHeatmapDays: [DashboardHeatmapDay] {
        switch selectedTimeRange {
        case .month, .today:
            return dashboardData.heatmapDays
        case .all:
            guard heatmapRounds.indices.contains(clampedHeatmapRound) else {
                return dashboardData.heatmapDays
            }
            return heatmapDays(in: heatmapRounds[clampedHeatmapRound])
        }
    }

    /// Supplies every day in a displayed All-view window so both months retain
    /// their calendar geometry even when the current month is still in progress.
    private func heatmapDays(in round: (start: Date, end: Date)) -> [DashboardHeatmapDay] {
        let calendar = Calendar.current
        let tokensByDay = dashboardData.heatmapDays.reduce(into: [Date: Int]()) { totals, day in
            totals[calendar.startOfDay(for: day.date)] = day.tokens
        }
        let costByDay = dashboardData.heatmapDays.reduce(into: [Date: Decimal]()) { totals, day in
            totals[calendar.startOfDay(for: day.date)] = day.cost
        }

        var days: [DashboardHeatmapDay] = []
        var date = calendar.startOfDay(for: round.start)
        let end = calendar.startOfDay(for: round.end)
        while date <= end {
            days.append(
                DashboardHeatmapDay(
                    date: date,
                    tokens: tokensByDay[date] ?? 0,
                    cost: costByDay[date] ?? .zero
                )
            )
            guard let nextDate = calendar.date(byAdding: .day, value: 1, to: date) else {
                break
            }
            date = nextDate
        }
        return days
    }

    private var heatmapRoundLabel: String {
        guard let round = heatmapRounds.indices.contains(clampedHeatmapRound) ? heatmapRounds[clampedHeatmapRound] : nil else { return "" }
        let startText = round.start.formatted(.dateTime.month(.abbreviated).year())
        let endText = round.end.formatted(.dateTime.month(.abbreviated).year())
        return "\(startText) – \(endText)"
    }

    @ViewBuilder
    private var heatmapPager: some View {
        if selectedTimeRange == .all && heatmapRoundCount > 1 {
            HStack(spacing: 8) {
                Text(heatmapRoundLabel)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)

                LegendPageControls(
                    currentPage: clampedHeatmapRound,
                    pageCount: heatmapRoundCount,
                    totalCount: visibleHeatmapDays.count,
                    onPrevious: {
                        // Left arrow → older round (lower page index)
                        heatmapPage = max(clampedHeatmapRound - 1, 0)
                    },
                    onNext: {
                        // Right arrow → newer round (higher page index)
                        heatmapPage = min(clampedHeatmapRound + 1, heatmapRoundCount - 1)
                    }
                )
            }
        }
    }

    private func jumpHeatmapPageToLatest() {
        heatmapPage = max(heatmapRoundCount - 1, 0)
    }

    private func isFirstDayOfMonth(_ date: Date) -> Bool {
        Calendar.current.component(.day, from: date) == 1
    }

    private func monthSeparatorLabel(for date: Date) -> String {
        if spansMultipleYears {
            return date.formatted(.dateTime.year().month(.abbreviated))
        }
        return date.formatted(.dateTime.month(.abbreviated))
    }

    private var chartXScaleDomain: ClosedRange<Date> {
        switch store.selectedViewMode {
        case .monthly:
            let start = monthlyXAxisValues.first ?? store.startDate
            let lastMonth = monthlyXAxisValues.last ?? store.endDate
            let end = Calendar.current.date(byAdding: .month, value: 1, to: lastMonth) ?? lastMonth
            return start...end
        case .daily, .sessions:
            let start = Calendar.current.startOfDay(for: store.startDate)
            let end = Calendar.current.date(byAdding: .day, value: 1, to: Calendar.current.startOfDay(for: store.endDate)) ?? store.endDate
            return start...end
        }
    }

    private func modelBinding(_ model: String) -> Binding<Bool> {
        Binding {
            store.selectedModels.contains(model)
        } set: { isSelected in
            if isSelected {
                store.selectedModels.insert(model)
            } else {
                store.selectedModels.remove(model)
            }
        }
    }

    private func tooltipDateText(for date: Date) -> String {
        switch store.selectedViewMode {
        case .daily, .sessions:
            date.formatted(.dateTime.year().month().day())
        case .monthly:
            date.formatted(.dateTime.year().month())
        }
    }
}

private struct TokenUsageDashboardData {
    let filteredRecords: [TokenUsageRecord]
    let availableModels: [String]
    let totalCost: Decimal
    let totalTokens: Int
    let inputTokens: Int
    let outputTokens: Int
    let cacheCreationTokens: Int
    let cacheReadTokens: Int
    let activePeriodCount: Int
    let uniqueModelCount: Int
    let summary: TodaySummaryResponse
    let modelUsageRows: [TodayModelUsageRow]
    let heatmapDays: [DashboardHeatmapDay]
    let modelCostRows: [ModelCostRow]
    let modelCostSlices: [ModelCostSlice]
    let modelCostTotalCost: Decimal
    let tokenTrendRows: [TokenTrendRow]
    let tokenTrendColorDomain: [String]
    let compositionRows: [TokenCompositionRow]
    let compositionInputRows: [TokenCompositionRow]
    let compositionCacheReadRows: [TokenCompositionRow]
    let compositionOutputRows: [TokenCompositionRow]
    let compositionByDateKey: [String: (input: Int, cacheRead: Int, output: Int)]
    let monthlyXAxisValues: [Date]
    let dailyXAxisValues: [Date]
    let spansMultipleYears: Bool
    let costTrendRows: [DailyValuePoint]
    let cacheShareRows: [DailyValuePoint]
    let weekdayAverageRows: [WeekdayAveragePoint]

    static func make(
        records: [TokenUsageRecord],
        selectedSource: TokenUsageSource,
        selectedViewMode: TokenUsageViewMode,
        startDate: Date,
        endDate: Date,
        selectedModels: Set<String>,
        timeRange: DashboardTimeRange = .today,
        dailyUsageAggregation: DashboardDailyUsageAggregation = .cli,
        enabledSources: Set<TokenUsageSource> = Set(TokenUsageSource.allCases.filter { $0 != .all })
    ) -> TokenUsageDashboardData {
        let calendar = Calendar.current
        let rangeStart = selectedViewMode == .monthly
            ? monthStart(for: startDate, calendar: calendar)
            : calendar.startOfDay(for: startDate)
        let rangeEnd = selectedViewMode == .monthly
            ? monthEnd(for: endDate, calendar: calendar)
            : calendar.startOfDay(for: endDate)

        let filteredRecords = records
            .filter { record in
                enabledSources.contains(record.source)
                    && (selectedSource == .all || record.source == selectedSource)
                    && record.viewMode == selectedViewMode
                    && calendar.startOfDay(for: record.date) >= rangeStart
                    && calendar.startOfDay(for: record.date) <= rangeEnd
            }
            .sorted { $0.date > $1.date }
        let availableModels = Array(Set(filteredRecords.flatMap(\.modelsUsed))).sorted()
        let totalCost = filteredRecords.reduce(Decimal.zero) { $0 + $1.totalCost }
        let totalTokens = filteredRecords.reduce(0) { $0 + $1.totalTokens }
        let inputTokens = filteredRecords.reduce(0) { $0 + $1.inputTokens }
        let outputTokens = filteredRecords.reduce(0) { $0 + $1.outputTokens }
        let cacheCreationTokens = filteredRecords.reduce(0) { $0 + $1.cacheCreationTokens }
        let cacheReadTokens = filteredRecords.reduce(0) { $0 + $1.cacheReadTokens }
        let activePeriodCount = Set(filteredRecords.map { periodKey(for: $0.date, viewMode: selectedViewMode) }).count
        let uniqueModelCount = Set(filteredRecords.flatMap(\.modelsUsed)).count
        let modelUsageRows = makeModelUsageRows(from: filteredRecords)
        let summary = makeSummary(
            from: filteredRecords,
            inputTokens: inputTokens,
            outputTokens: outputTokens,
            cacheCreationTokens: cacheCreationTokens,
            cacheReadTokens: cacheReadTokens,
            totalTokens: totalTokens,
            totalCost: totalCost,
            uniqueModelCount: uniqueModelCount,
            modelUsageRows: modelUsageRows
        )
        let modelCostRows = makeModelCostRows(from: filteredRecords)
        let modelCostSlices = makeModelCostSlices(from: modelCostRows)
        let tokenTrendRows = makeTokenTrendRows(
            from: filteredRecords,
            aggregation: dailyUsageAggregation,
            selectedModels: selectedModels
        )
        let tokenTrendColorDomain = Array(Set(tokenTrendRows.map(\.series))).sorted()
        let compositionRows = makeCompositionRows(from: filteredRecords, viewMode: selectedViewMode)
        let compositionInputRows = compositionRows.filter { $0.kind == .input }
        let compositionCacheReadRows = compositionRows.filter { $0.kind == .cacheRead }
        let compositionOutputRows = compositionRows.filter { $0.kind == .output }
        var compositionByDateKey: [String: (input: Int, cacheRead: Int, output: Int)] = [:]
        for row in compositionRows {
            let key = periodKey(for: row.date, viewMode: selectedViewMode)
            var entry = compositionByDateKey[key] ?? (0, 0, 0)
            switch row.kind {
            case .input: entry.input = row.tokens
            case .cacheRead: entry.cacheRead = row.tokens
            case .output: entry.output = row.tokens
            case .cacheCreation: break
            }
            compositionByDateKey[key] = entry
        }
        let monthlyXAxisValues = makeMonthlyXAxisValues(rangeStart: rangeStart, rangeEnd: rangeEnd, calendar: calendar)
        let dailyXAxisValues = makeDailyXAxisValues(from: filteredRecords, rangeStart: rangeStart, rangeEnd: rangeEnd, calendar: calendar)
        let spansMultipleYears = Set(monthlyXAxisValues.map { calendar.component(.year, from: $0) }).count > 1
        let heatmapAxisDays = makeHeatmapAxisDays(
            timeRange: timeRange,
            startDate: startDate,
            endDate: endDate,
            calendar: calendar
        )
        let heatmapDays = makeHeatmapDays(from: filteredRecords, days: heatmapAxisDays, calendar: calendar)
        let costTrendRows = makeDailyTotalPoints(from: filteredRecords, calendar: calendar) { $0.totalCost.doubleValue }
        let cacheShareRows = makeCacheSharePoints(from: filteredRecords, calendar: calendar)
        let weekdayAverageRows = makeWeekdayAveragePoints(from: filteredRecords, calendar: calendar)

        return TokenUsageDashboardData(
            filteredRecords: filteredRecords,
            availableModels: availableModels,
            totalCost: totalCost,
            totalTokens: totalTokens,
            inputTokens: inputTokens,
            outputTokens: outputTokens,
            cacheCreationTokens: cacheCreationTokens,
            cacheReadTokens: cacheReadTokens,
            activePeriodCount: activePeriodCount,
            uniqueModelCount: uniqueModelCount,
            summary: summary,
            modelUsageRows: modelUsageRows,
            heatmapDays: heatmapDays,
            modelCostRows: modelCostRows,
            modelCostSlices: modelCostSlices,
            modelCostTotalCost: modelCostRows.reduce(Decimal.zero) { $0 + $1.cost },
            tokenTrendRows: tokenTrendRows,
            tokenTrendColorDomain: tokenTrendColorDomain,
            compositionRows: compositionRows,
            compositionInputRows: compositionInputRows,
            compositionCacheReadRows: compositionCacheReadRows,
            compositionOutputRows: compositionOutputRows,
            compositionByDateKey: compositionByDateKey,
            monthlyXAxisValues: monthlyXAxisValues,
            dailyXAxisValues: dailyXAxisValues,
            spansMultipleYears: spansMultipleYears,
            costTrendRows: costTrendRows,
            cacheShareRows: cacheShareRows,
            weekdayAverageRows: weekdayAverageRows
        )
    }

    private static func makeDailyTotalPoints(
        from records: [TokenUsageRecord],
        calendar: Calendar,
        measure: (TokenUsageRecord) -> Double
    ) -> [DailyValuePoint] {
        let grouped = records.reduce(into: [Date: Double]()) { totals, record in
            totals[calendar.startOfDay(for: record.date), default: 0] += measure(record)
        }
        return grouped
            .map { DailyValuePoint(date: $0.key, value: $0.value) }
            .sorted { $0.date < $1.date }
    }

    private static func makeCacheSharePoints(
        from records: [TokenUsageRecord],
        calendar: Calendar
    ) -> [DailyValuePoint] {
        let grouped = records.reduce(into: [Date: (cache: Int, total: Int)]()) { totals, record in
            let day = calendar.startOfDay(for: record.date)
            var entry = totals[day] ?? (0, 0)
            entry.cache += record.cacheCreationTokens + record.cacheReadTokens
            entry.total += record.totalTokens
            totals[day] = entry
        }
        return grouped
            .compactMap { day, entry in
                guard entry.total > 0 else { return nil }
                return DailyValuePoint(date: day, value: Double(entry.cache) / Double(entry.total))
            }
            .sorted { $0.date < $1.date }
    }

    private static func makeWeekdayAveragePoints(
        from records: [TokenUsageRecord],
        calendar: Calendar
    ) -> [WeekdayAveragePoint] {
        let tokensByDay = records.reduce(into: [Date: Int]()) { totals, record in
            totals[calendar.startOfDay(for: record.date), default: 0] += record.totalTokens
        }
        var sums: [Int: Int] = [:]
        var counts: [Int: Int] = [:]
        for (day, tokens) in tokensByDay {
            // Remap Sunday-first weekday (1...7) to Monday-first (1...7).
            let weekday = (calendar.component(.weekday, from: day) + 5) % 7 + 1
            sums[weekday, default: 0] += tokens
            counts[weekday, default: 0] += 1
        }
        let labels = ["", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        return (1...7).map { weekday in
            let average = counts[weekday].map { Double(sums[weekday] ?? 0) / Double($0) } ?? 0
            return WeekdayAveragePoint(weekday: weekday, label: labels[weekday], averageTokens: average)
        }
    }

    private static func makeSummary(
        from records: [TokenUsageRecord],
        inputTokens: Int,
        outputTokens: Int,
        cacheCreationTokens: Int,
        cacheReadTokens: Int,
        totalTokens: Int,
        totalCost: Decimal,
        uniqueModelCount: Int,
        modelUsageRows: [TodayModelUsageRow]
    ) -> TodaySummaryResponse {
        let sourceRows = makeSummarySourceRows(from: records)
        return TodaySummaryResponse(
            date: "",
            inputTokens: inputTokens,
            outputTokens: outputTokens,
            cacheCreationTokens: cacheCreationTokens,
            cacheReadTokens: cacheReadTokens,
            totalTokens: totalTokens,
            totalCost: totalCost.doubleValue,
            activeSourceCount: sourceRows.count,
            modelCount: uniqueModelCount,
            sourceRows: sourceRows,
            modelRows: modelUsageRows
        )
    }

    private static func makeSummarySourceRows(from records: [TokenUsageRecord]) -> [TodaySourceUsageRow] {
        let grouped = Dictionary(grouping: records, by: \.source)
        var rows: [TodaySourceUsageRow] = []
        for (source, sourceRecords) in grouped {
            var input = 0
            var output = 0
            var cacheCreation = 0
            var cacheRead = 0
            var total = 0
            var cost = Decimal.zero
            var models = Set<String>()
            for record in sourceRecords {
                input += record.inputTokens
                output += record.outputTokens
                cacheCreation += record.cacheCreationTokens
                cacheRead += record.cacheReadTokens
                total += record.totalTokens
                cost += record.totalCost
                if record.modelsUsed.isEmpty {
                    models.insert("Unknown")
                } else {
                    models.formUnion(record.modelsUsed)
                }
            }
            rows.append(
                TodaySourceUsageRow(
                    source: source.usageSource,
                    inputTokens: input,
                    outputTokens: output,
                    cacheCreationTokens: cacheCreation,
                    cacheReadTokens: cacheRead,
                    totalTokens: total,
                    totalCost: cost.doubleValue,
                    modelCount: models.count
                )
            )
        }
        return rows.sorted { $0.totalTokens > $1.totalTokens }
    }

    private static func makeModelUsageRows(from records: [TokenUsageRecord]) -> [TodayModelUsageRow] {
        struct Key: Hashable {
            let source: UsageSource
            let model: String
        }
        struct Totals {
            var input = 0
            var output = 0
            var cacheCreation = 0
            var cacheRead = 0
            var total = 0
            var cost = Decimal.zero
        }

        var totals: [Key: Totals] = [:]
        for record in records {
            let source = record.source.usageSource
            if record.modelBreakdowns.isEmpty {
                let key = Key(source: source, model: record.modelsUsed.first ?? "Unknown")
                var entry = totals[key] ?? Totals()
                entry.input += record.inputTokens
                entry.output += record.outputTokens
                entry.cacheCreation += record.cacheCreationTokens
                entry.cacheRead += record.cacheReadTokens
                entry.total += record.totalTokens
                entry.cost += record.totalCost
                totals[key] = entry
            } else {
                for breakdown in record.modelBreakdowns {
                    let key = Key(source: source, model: breakdown.modelName)
                    var entry = totals[key] ?? Totals()
                    entry.input += breakdown.inputTokens
                    entry.output += breakdown.outputTokens
                    entry.cacheCreation += breakdown.cacheCreationTokens
                    entry.cacheRead += breakdown.cacheReadTokens
                    entry.total += breakdown.totalTokens
                    entry.cost += breakdown.cost
                    totals[key] = entry
                }
            }
        }

        return totals.map { key, entry in
            TodayModelUsageRow(
                source: key.source,
                modelName: key.model,
                inputTokens: entry.input,
                outputTokens: entry.output,
                cacheCreationTokens: entry.cacheCreation,
                cacheReadTokens: entry.cacheRead,
                totalTokens: entry.total,
                totalCost: entry.cost.doubleValue
            )
        }
        .sorted { $0.totalCost > $1.totalCost }
    }

    private static func makeHeatmapDays(
        from records: [TokenUsageRecord],
        days: [Date],
        calendar: Calendar
    ) -> [DashboardHeatmapDay] {
        let tokensByDay = records.reduce(into: [Date: Int]()) { totals, record in
            totals[calendar.startOfDay(for: record.date), default: 0] += record.totalTokens
        }
        let costByDay = records.reduce(into: [Date: Decimal]()) { totals, record in
            totals[calendar.startOfDay(for: record.date), default: .zero] += record.totalCost
        }
        return days.map { day in
            let date = calendar.startOfDay(for: day)
            return DashboardHeatmapDay(
                date: day,
                tokens: tokensByDay[date] ?? 0,
                cost: costByDay[date] ?? .zero
            )
        }
    }

    private static func makeHeatmapAxisDays(
        timeRange: DashboardTimeRange,
        startDate: Date,
        endDate: Date,
        calendar: Calendar
    ) -> [Date] {
        switch timeRange {
        case .today:
            return [calendar.startOfDay(for: startDate)]
        case .month:
            let start = monthStart(for: startDate, calendar: calendar)
            let end = monthEnd(for: startDate, calendar: calendar)
            return makeContiguousDays(from: start, through: end, calendar: calendar)
        case .all:
            let start = calendar.startOfDay(for: startDate)
            let today = calendar.startOfDay(for: Date())
            let rangeEnd = calendar.startOfDay(for: endDate)
            let end = min(today, calendar.date(byAdding: .day, value: -1, to: rangeEnd) ?? today)
            guard start <= end else { return [today] }
            return makeContiguousDays(from: start, through: end, calendar: calendar)
        }
    }

    private static func makeContiguousDays(from start: Date, through end: Date, calendar: Calendar) -> [Date] {
        var dates: [Date] = []
        var current = calendar.startOfDay(for: start)
        let last = calendar.startOfDay(for: end)
        while current <= last {
            dates.append(current)
            current = calendar.date(byAdding: .day, value: 1, to: current) ?? current
        }
        return dates
    }

    private static func makeModelCostRows(from records: [TokenUsageRecord]) -> [ModelCostRow] {
        let grouped = records
            .flatMap { record in
                if record.modelBreakdowns.isEmpty {
                    return record.modelsUsed.map {
                        ModelCostRow(model: $0, cost: record.totalCost / Decimal(max(record.modelsUsed.count, 1)))
                    }
                }

                return record.modelBreakdowns.map { ModelCostRow(model: $0.modelName, cost: $0.cost) }
            }
            .reduce(into: [String: Decimal]()) { totals, row in
                totals[row.model, default: .zero] += row.cost
            }

        return grouped
            .map { ModelCostRow(model: $0.key, cost: $0.value) }
            .sorted { $0.cost > $1.cost }
    }

    private static func makeModelCostSlices(from rows: [ModelCostRow]) -> [ModelCostSlice] {
        let visibleRows = rows.filter { $0.cost.doubleValue > 0 }
        let total = visibleRows.reduce(0.0) { $0 + $1.cost.doubleValue }
        guard total > 0 else { return [] }

        return visibleRows.map { row in
            ModelCostSlice(
                model: row.model,
                cost: row.cost,
                percent: row.cost.doubleValue / total,
                color: stableModelColor(for: row.model)
            )
        }
    }

    private static func makeTokenTrendRows(
        from records: [TokenUsageRecord],
        aggregation: DashboardDailyUsageAggregation,
        selectedModels: Set<String>
    ) -> [TokenTrendRow] {
        let rows: [TokenTrendRow]

        switch aggregation {
        case .cli:
            rows = records.compactMap { record in
                tokenTrendRowByCLI(for: record, selectedModels: selectedModels)
            }
        case .model:
            rows = records.flatMap { record in
                tokenTrendRowsByModel(for: record, selectedModels: selectedModels)
            }
        }

        return rows.sorted {
            if $0.date == $1.date {
                return $0.series < $1.series
            }
            return $0.date < $1.date
        }
    }

    private static func tokenTrendRowByCLI(
        for record: TokenUsageRecord,
        selectedModels: Set<String>
    ) -> TokenTrendRow? {
        let tokens: Int
        if selectedModels.isEmpty {
            tokens = record.totalTokens
        } else if !record.modelBreakdowns.isEmpty {
            tokens = record.modelBreakdowns
                .filter { selectedModels.contains($0.modelName) }
                .reduce(0) { $0 + $1.totalTokens }
            guard tokens > 0 else { return nil }
        } else {
            let matchingModels = record.modelsUsed.filter { selectedModels.contains($0) }
            guard matchingModels.count == 1 else { return nil }
            tokens = record.totalTokens
        }

        return TokenTrendRow(
            date: record.date,
            series: record.source.label,
            tokens: tokens
        )
    }

    private static func tokenTrendRowsByModel(
        for record: TokenUsageRecord,
        selectedModels: Set<String>
    ) -> [TokenTrendRow] {
        if !record.modelBreakdowns.isEmpty {
            return record.modelBreakdowns.compactMap { breakdown in
                guard selectedModels.isEmpty || selectedModels.contains(breakdown.modelName) else {
                    return nil
                }

                return TokenTrendRow(
                    date: record.date,
                    series: breakdown.modelName,
                    tokens: breakdown.totalTokens
                )
            }
        }

        let activeModels = selectedModels.isEmpty
            ? record.modelsUsed
            : record.modelsUsed.filter { selectedModels.contains($0) }

        guard activeModels.count == 1, let model = activeModels.first else {
            return []
        }

        return [
            TokenTrendRow(
                date: record.date,
                series: model,
                tokens: record.totalTokens
            ),
        ]
    }

    private static func makeCompositionRows(
        from records: [TokenUsageRecord],
        viewMode: TokenUsageViewMode
    ) -> [TokenCompositionRow] {
        let grouped = records.reduce(into: [String: TokenCompositionTotals]()) { totals, record in
            let key = periodKey(for: record.date, viewMode: viewMode)
            totals[key, default: TokenCompositionTotals(date: record.date)].add(record)
        }

        return grouped.values
            .flatMap { $0.rows }
            .sorted {
                if $0.date == $1.date {
                    return $0.kind.sortOrder < $1.kind.sortOrder
                }
                return $0.date < $1.date
            }
    }

    private static func makeMonthlyXAxisValues(rangeStart: Date, rangeEnd: Date, calendar: Calendar) -> [Date] {
        var dates: [Date] = []
        var current = monthStart(for: rangeStart, calendar: calendar)
        let end = monthStart(for: rangeEnd, calendar: calendar)

        while current <= end {
            dates.append(current)
            current = calendar.date(byAdding: .month, value: 1, to: current) ?? current
        }

        return dates
    }

    private static func makeDailyXAxisValues(from records: [TokenUsageRecord], rangeStart: Date, rangeEnd: Date, calendar: Calendar) -> [Date] {
        var dates: [Date] = []
        var current = calendar.startOfDay(for: rangeStart)
        let end = calendar.startOfDay(for: rangeEnd)

        while current <= end {
            dates.append(current)
            current = calendar.date(byAdding: .day, value: 1, to: current) ?? current
        }

        return dates
    }

    private static func monthStart(for date: Date, calendar: Calendar) -> Date {
        calendar.dateInterval(of: .month, for: date)?.start ?? date
    }

    private static func monthEnd(for date: Date, calendar: Calendar) -> Date {
        guard let interval = calendar.dateInterval(of: .month, for: date),
              let end = calendar.date(byAdding: .day, value: -1, to: interval.end) else {
            return date
        }
        return end
    }

    private static func periodKey(for date: Date, viewMode: TokenUsageViewMode) -> String {
        switch viewMode {
        case .daily, .sessions:
            date.formatted(.iso8601.year().month().day())
        case .monthly:
            date.formatted(.iso8601.year().month())
        }
    }
}

// MARK: - Chart helpers (file-private)

private func dashboardPeriodKey(for date: Date, viewMode: TokenUsageViewMode) -> String {
    switch viewMode {
    case .daily, .sessions:
        date.formatted(.iso8601.year().month().day())
    case .monthly:
        date.formatted(.iso8601.year().month())
    }
}

private func dashboardPeriodStart(for date: Date, viewMode: TokenUsageViewMode) -> Date {
    switch viewMode {
    case .daily, .sessions:
        Calendar.current.startOfDay(for: date)
    case .monthly:
        Calendar.current.dateInterval(of: .month, for: date)?.start ?? date
    }
}

private func dashboardRepresentativeDate(for date: Date, in dates: [Date], viewMode: TokenUsageViewMode) -> Date? {
    let period = dashboardPeriodKey(for: date, viewMode: viewMode)
    return dates
        .filter { dashboardPeriodKey(for: $0, viewMode: viewMode) == period }
        .min()
}

private func dashboardHistogramBarWidth(for viewMode: TokenUsageViewMode) -> MarkDimension {
    viewMode == .monthly ? .fixed(maximumBarWidth) : .automatic
}

private func dashboardTooltipPosition(for point: CGPoint, in size: CGSize) -> CGPoint {
    let tooltipWidth = chartTooltipWidth
    let tooltipHeight = chartTooltipHeight
    let horizontalPadding = 10.0
    let verticalPadding = 10.0

    let x = min(
        max(point.x + 18 + tooltipWidth / 2, tooltipWidth / 2 + horizontalPadding),
        max(tooltipWidth / 2 + horizontalPadding, size.width - tooltipWidth / 2 - horizontalPadding)
    )
    let preferredY = point.y - 18 - tooltipHeight / 2
    let fallbackY = point.y + 18 + tooltipHeight / 2
    let unclampedY = preferredY >= tooltipHeight / 2 + verticalPadding ? preferredY : fallbackY
    let y = min(
        max(unclampedY, tooltipHeight / 2 + verticalPadding),
        max(tooltipHeight / 2 + verticalPadding, size.height - tooltipHeight / 2 - verticalPadding)
    )

    return CGPoint(x: x, y: y)
}

private func dashboardStackedBarHit(
    at point: CGPoint,
    proxy: ChartProxy,
    geometry: GeometryProxy,
    dates: [Date],
    viewMode: TokenUsageViewMode
) -> (date: Date, tokens: Double)? {
    guard let plotFrame = proxy.plotFrame else {
        return nil
    }

    let plotRect = geometry[plotFrame]
    guard plotRect.contains(point) else {
        return nil
    }

    let plotPoint = CGPoint(
        x: min(max(point.x - plotRect.minX, 0), plotRect.width),
        y: min(max(point.y - plotRect.minY, 0), plotRect.height)
    )
    guard
        let date = proxy.value(atX: plotPoint.x, as: Date.self),
        let tokens = proxy.value(atY: plotPoint.y, as: Double.self),
        tokens >= 0
    else {
        return nil
    }

    let hitDate = dashboardPeriodStart(for: date, viewMode: viewMode)
    guard let barDate = dashboardRepresentativeDate(for: hitDate, in: dates, viewMode: viewMode) else {
        return nil
    }
    let uniqueCount = Set(dates.map { dashboardPeriodKey(for: $0, viewMode: viewMode) }).count
    let barCenterX: CGFloat
    let barHalfWidth: CGFloat
    if viewMode == .monthly,
       let monthEnd = Calendar.current.date(byAdding: .month, value: 1, to: hitDate),
       let monthStartX = proxy.position(forX: hitDate),
       let monthEndX = proxy.position(forX: monthEnd) {
        barCenterX = (monthStartX + monthEndX) / 2
        barHalfWidth = maximumBarWidth / 2 + 4
    } else if let barX = proxy.position(forX: barDate) {
        barCenterX = barX
        if uniqueCount <= 1 {
            barHalfWidth = 52
        } else {
            barHalfWidth = min(max(plotRect.width / CGFloat(uniqueCount * 2), 8), 80) + 4
        }
    } else {
        return nil
    }
    guard abs(plotPoint.x - barCenterX) <= barHalfWidth else {
        return nil
    }

    return (hitDate, tokens)
}

private func dashboardHitStackedRow<Row>(
    date: Date,
    tokens: Double,
    rows: [Row],
    rowDate: KeyPath<Row, Date>,
    rowTokens: KeyPath<Row, Int>,
    rowOrder: (Row) -> Int,
    viewMode: TokenUsageViewMode
) -> Row? {
    let period = dashboardPeriodKey(for: date, viewMode: viewMode)
    let periodRows = rows
        .filter { dashboardPeriodKey(for: $0[keyPath: rowDate], viewMode: viewMode) == period && $0[keyPath: rowTokens] > 0 }
        .sorted {
            let leftOrder = rowOrder($0)
            let rightOrder = rowOrder($1)
            if leftOrder == rightOrder {
                return $0[keyPath: rowDate] < $1[keyPath: rowDate]
            }
            return leftOrder < rightOrder
        }
    let total = periodRows.reduce(0) { $0 + $1[keyPath: rowTokens] }
    let tolerance = max(Double(total) * 0.015, 1.0)
    guard tokens >= -tolerance, tokens <= Double(total) + tolerance else {
        return nil
    }

    var lowerBound = 0.0
    for row in periodRows {
        let upperBound = lowerBound + Double(row[keyPath: rowTokens])
        if tokens >= lowerBound - tolerance, tokens <= upperBound + tolerance {
            return row
        }
        lowerBound = upperBound
    }

    return nil
}

// MARK: - TokenTrendChartView

private struct TokenTrendChartView: View {
    let rows: [TokenTrendRow]
    let colorDomain: [String]
    let colorRange: [Color]
    let seriesLabel: String
    let viewMode: TokenUsageViewMode
    let dateColumnTitle: String
    let monthlyXAxisValues: [Date]
    let dailyXAxisValues: [Date]
    let chartXScaleDomain: ClosedRange<Date>
    let tooltipDateText: (Date) -> String
    let monthAxisLabel: (Date) -> String
    let monthSeparatorLabel: (Date) -> String
    let isFirstDayOfMonth: (Date) -> Bool

    @State private var hoveredRow: TokenTrendRow?
    @State private var hoveredPoint: CGPoint?

    var body: some View {
        let barWidth = dashboardHistogramBarWidth(for: viewMode)
        let dates = rows.map(\.date)

        Chart {
            ForEach(rows) { row in
                BarMark(
                    x: .value(dateColumnTitle, row.date, unit: viewMode == .monthly ? .month : .day),
                    y: .value("Tokens", row.tokens),
                    width: barWidth
                )
                .foregroundStyle(by: .value(seriesLabel, row.series))
                .cornerRadius(3)
            }
        }
        .chartForegroundStyleScale(domain: colorDomain, range: colorRange)
        .chartLegend(.hidden)
        .chartXAxis {
            xAxisMarks
        }
        .chartXScale(domain: chartXScaleDomain)
        .chartOverlay { proxy in
            GeometryReader { geometry in
                ZStack(alignment: .topLeading) {
                    Rectangle()
                        .fill(.clear)
                        .contentShape(Rectangle())
                        .onContinuousHover { phase in
                            switch phase {
                            case .active(let point):
                                hoveredRow = tokenTrendRow(at: point, proxy: proxy, geometry: geometry, dates: dates)
                                hoveredPoint = hoveredRow == nil ? nil : point
                            case .ended:
                                hoveredRow = nil
                                hoveredPoint = nil
                            }
                        }

                    if let row = hoveredRow, let point = hoveredPoint {
                        ChartTooltipPanel(
                            title: tooltipDateText(row.date),
                            rows: [
                                ChartTooltipRow(row.series, row.tokens.tokenText, color: seriesColor(for: row.series)),
                                ChartTooltipRow("Day Total", dayTotalText(for: row.date), emphasized: true),
                            ]
                        )
                        .position(dashboardTooltipPosition(for: point, in: geometry.size))
                        .zIndex(1)
                    }
                }
                .allowsHitTesting(true)
                .animation(nil, value: hoveredRow?.id)
            }
        }
        .transaction { transaction in
            transaction.animation = nil
        }
        .chartYAxis {
            AxisMarks { value in
                AxisGridLine(stroke: StrokeStyle(lineWidth: 0.5))
                    .foregroundStyle(Color(nsColor: .separatorColor).opacity(0.35))
                AxisValueLabel {
                    if let tokens = value.as(Double.self) {
                        Text(tokens.tokenAxisText)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .frame(height: dailyUsagePlotHeight)
    }

    @AxisContentBuilder
    private var xAxisMarks: some AxisContent {
        if viewMode == .monthly {
            AxisMarks(values: monthlyXAxisValues) { value in
                AxisValueLabel {
                    if let date = value.as(Date.self) {
                        Text(monthAxisLabel(date))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        } else {
            AxisMarks(values: dailyXAxisValues) { value in
                if let date = value.as(Date.self), isFirstDayOfMonth(date) {
                    AxisGridLine(stroke: StrokeStyle(lineWidth: 0.5))
                        .foregroundStyle(Color(nsColor: .separatorColor).opacity(0.45))
                    AxisValueLabel {
                        Text(monthSeparatorLabel(date))
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                    }
                } else {
                    AxisValueLabel {
                        if let date = value.as(Date.self) {
                            Text(date.formatted(.dateTime.day()))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
    }

    private func tokenTrendRow(at point: CGPoint, proxy: ChartProxy, geometry: GeometryProxy, dates: [Date]) -> TokenTrendRow? {
        guard let hit = dashboardStackedBarHit(at: point, proxy: proxy, geometry: geometry, dates: dates, viewMode: viewMode) else {
            return nil
        }

        return dashboardHitStackedRow(
            date: hit.date,
            tokens: hit.tokens,
            rows: rows,
            rowDate: \.date,
            rowTokens: \.tokens,
            rowOrder: { colorDomain.firstIndex(of: $0.series) ?? Int.max },
            viewMode: viewMode
        )
    }

    private func seriesColor(for series: String) -> Color {
        guard let index = colorDomain.firstIndex(of: series), index < colorRange.count else {
            return .blue
        }
        return colorRange[index]
    }

    private func dayTotalText(for date: Date) -> String {
        let period = dashboardPeriodKey(for: date, viewMode: viewMode)
        let total = rows
            .filter { dashboardPeriodKey(for: $0.date, viewMode: viewMode) == period }
            .reduce(0) { $0 + $1.tokens }
        return total.tokenText
    }
}

// MARK: - CompositionChartView

private struct CompositionChartView: View {
    let inputRows: [TokenCompositionRow]
    let cacheReadRows: [TokenCompositionRow]
    let outputRows: [TokenCompositionRow]
    let byDateKey: [String: (input: Int, cacheRead: Int, output: Int)]
    let viewMode: TokenUsageViewMode
    let dateColumnTitle: String
    let monthlyXAxisValues: [Date]
    let dailyXAxisValues: [Date]
    let chartXScaleDomain: ClosedRange<Date>
    let tooltipDateText: (Date) -> String
    let monthAxisLabel: (Date) -> String
    let monthSeparatorLabel: (Date) -> String
    let isFirstDayOfMonth: (Date) -> Bool

    @State private var hoveredRow: TokenCompositionRow?
    @State private var hoveredPoint: CGPoint?

    private var allRows: [TokenCompositionRow] {
        inputRows + cacheReadRows + outputRows
    }

    var body: some View {
        let allDates = allRows.map(\.date)
        let barWidth = dashboardHistogramBarWidth(for: viewMode)

        GeometryReader { geometry in
            Chart {
                ForEach(outputRows) { row in
                    BarMark(
                        x: .value("Date", row.date, unit: viewMode == .monthly ? .month : .day),
                        y: .value("Tokens", row.tokens),
                        width: barWidth
                    )
                    .foregroundStyle(AppPalette.compositionOutput)
                    .cornerRadius(3)
                }
                ForEach(inputRows) { row in
                    BarMark(
                        x: .value("Date", row.date, unit: viewMode == .monthly ? .month : .day),
                        y: .value("Tokens", row.tokens),
                        width: barWidth
                    )
                    .foregroundStyle(AppPalette.compositionInput)
                    .cornerRadius(3)
                }
                ForEach(cacheReadRows) { row in
                    BarMark(
                        x: .value("Date", row.date, unit: viewMode == .monthly ? .month : .day),
                        y: .value("Tokens", row.tokens),
                        width: barWidth
                    )
                    .foregroundStyle(AppPalette.compositionCacheRead)
                    .cornerRadius(3)
                }
            }
            .chartLegend(position: .bottom, alignment: .center)
            .chartXAxis {
                xAxisMarks
            }
            .chartXScale(domain: chartXScaleDomain)
            .chartOverlay { proxy in
                GeometryReader { overlayGeometry in
                    ZStack(alignment: .topLeading) {
                        Rectangle()
                            .fill(.clear)
                            .contentShape(Rectangle())
                            .onContinuousHover { phase in
                                switch phase {
                                case .active(let point):
                                    hoveredRow = compositionRow(at: point, proxy: proxy, geometry: overlayGeometry, dates: allDates)
                                    hoveredPoint = hoveredRow == nil ? nil : point
                                case .ended:
                                    hoveredRow = nil
                                    hoveredPoint = nil
                                }
                            }

                        if let hovered = hoveredRow, let point = hoveredPoint {
                            let dateKey = dashboardPeriodKey(for: hovered.date, viewMode: viewMode)
                            let entry = byDateKey[dateKey] ?? (0, 0, 0)
                            let inputTokens = entry.input
                            let cacheReadTokens = entry.cacheRead
                            let outputTokens = entry.output
                            let totalInput = cacheReadTokens + inputTokens
                            let rawCoverage = totalInput > 0 ? Double(cacheReadTokens) / Double(totalInput) * 100 : 0
                            let cacheCoverage = min(floor(rawCoverage * 10) / 10, 99.9)

                            ChartTooltipPanel(
                                title: tooltipDateText(hovered.date),
                                rows: [
                                    ChartTooltipRow("Input", inputTokens.tokenText, color: AppPalette.compositionInput),
                                    ChartTooltipRow("Cache Read", cacheReadTokens.tokenText, color: AppPalette.compositionCacheRead),
                                    ChartTooltipRow("Output", outputTokens.tokenText, color: AppPalette.compositionOutput),
                                    ChartTooltipRow("Cache Coverage", String(format: "%.1f%%", cacheCoverage), emphasized: true),
                                ]
                            )
                            .position(dashboardTooltipPosition(for: point, in: overlayGeometry.size))
                            .zIndex(1)
                        }
                    }
                    .allowsHitTesting(true)
                    .animation(nil, value: hoveredRow?.id)
                }
            }
            .transaction { transaction in
                transaction.animation = nil
            }
            .chartYAxis {
                AxisMarks { value in
                    AxisGridLine(stroke: StrokeStyle(lineWidth: 0.5))
                        .foregroundStyle(Color(nsColor: .separatorColor).opacity(0.35))
                    AxisValueLabel {
                        if let tokens = value.as(Double.self) {
                            Text(tokens.tokenAxisText)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
            .frame(width: geometry.size.width, height: geometry.size.height)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @AxisContentBuilder
    private var xAxisMarks: some AxisContent {
        if viewMode == .monthly {
            AxisMarks(values: monthlyXAxisValues) { value in
                AxisValueLabel {
                    if let date = value.as(Date.self) {
                        Text(monthAxisLabel(date))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        } else {
            AxisMarks(values: dailyXAxisValues) { value in
                if let date = value.as(Date.self), isFirstDayOfMonth(date) {
                    AxisGridLine(stroke: StrokeStyle(lineWidth: 0.5))
                        .foregroundStyle(Color(nsColor: .separatorColor).opacity(0.45))
                    AxisValueLabel {
                        Text(monthSeparatorLabel(date))
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                    }
                } else {
                    AxisValueLabel {
                        if let date = value.as(Date.self) {
                            Text(date.formatted(.dateTime.day()))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
    }

    private func compositionRow(at point: CGPoint, proxy: ChartProxy, geometry: GeometryProxy, dates: [Date]) -> TokenCompositionRow? {
        guard let hit = dashboardStackedBarHit(at: point, proxy: proxy, geometry: geometry, dates: dates, viewMode: viewMode) else {
            return nil
        }

        return dashboardHitStackedRow(
            date: hit.date,
            tokens: hit.tokens,
            rows: allRows,
            rowDate: \.date,
            rowTokens: \.tokens,
            rowOrder: { $0.kind.sortOrder },
            viewMode: viewMode
        )
    }
}

// MARK: - Activity insight charts

private struct DailyValuePoint: Identifiable {
    var id: Date { date }
    let date: Date
    let value: Double
}

private struct WeekdayAveragePoint: Identifiable {
    var id: Int { weekday }
    let weekday: Int
    let label: String
    let averageTokens: Double
}

/// Shared hover plumbing for single-series daily line charts: tracks the
/// pointer, resolves the nearest row by x position, and floats a tooltip.
private protocol DailyLineChartRow {
    var date: Date { get }
    var value: Double { get }
}

extension DailyValuePoint: DailyLineChartRow {}

private struct DailyLineChartHover<Row: DailyLineChartRow>: View {
    let rows: [Row]
    let proxy: ChartProxy
    let geometry: GeometryProxy
    let tooltip: (Row) -> ChartTooltipPanel

    @State private var hoveredRow: Row?
    @State private var hoveredPoint: CGPoint?

    var body: some View {
        ZStack(alignment: .topLeading) {
            Rectangle()
                .fill(.clear)
                .contentShape(Rectangle())
                .onContinuousHover { phase in
                    switch phase {
                    case .active(let point):
                        hoveredRow = row(at: point)
                        hoveredPoint = hoveredRow == nil ? nil : point
                    case .ended:
                        hoveredRow = nil
                        hoveredPoint = nil
                    }
                }

            if let row = hoveredRow, let point = hoveredPoint {
                tooltip(row)
                    .position(dashboardTooltipPosition(for: point, in: geometry.size))
                    .zIndex(1)
            }
        }
        .allowsHitTesting(true)
        .animation(nil, value: hoveredRow?.date)
    }

    private func row(at point: CGPoint) -> Row? {
        guard let plotFrame = proxy.plotFrame, !rows.isEmpty else { return nil }
        let plotRect = geometry[plotFrame]
        let x = point.x - plotRect.origin.x
        guard let date: Date = proxy.value(atX: x) else { return nil }
        return rows.min { left, right in
            abs(left.date.timeIntervalSince(date)) < abs(right.date.timeIntervalSince(date))
        }
    }
}

private struct CostTrendChartView: View {
    let rows: [DailyValuePoint]
    let formatCost: (Decimal) -> String

    private var averageCost: Double {
        guard !rows.isEmpty else { return 0 }
        return rows.reduce(0.0) { $0 + $1.value } / Double(rows.count)
    }

    private var peakRow: DailyValuePoint? {
        rows.max { $0.value < $1.value }
    }

    var body: some View {
        if rows.isEmpty {
            ChartEmptyState(symbol: "chart.xyaxis.line", text: "No cost data in range")
        } else {
            Chart {
                RuleMark(y: .value("Average", averageCost))
                    .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 4]))
                    .foregroundStyle(Color.secondary.opacity(0.55))

                ForEach(rows) { row in
                    AreaMark(
                        x: .value("Date", row.date),
                        y: .value("Cost", row.value)
                    )
                    .interpolationMethod(.catmullRom)
                    .foregroundStyle(
                        LinearGradient(
                            colors: [AppPalette.chartCost.opacity(0.22), AppPalette.chartCost.opacity(0.02)],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                    )

                    LineMark(
                        x: .value("Date", row.date),
                        y: .value("Cost", row.value)
                    )
                    .interpolationMethod(.catmullRom)
                    .lineStyle(StrokeStyle(lineWidth: 2, lineCap: .round))
                    .foregroundStyle(AppPalette.chartCost)
                }

                if let peak = peakRow, rows.count > 1 {
                    PointMark(
                        x: .value("Date", peak.date),
                        y: .value("Cost", peak.value)
                    )
                    .symbolSize(24)
                    .foregroundStyle(AppPalette.chartCost)
                    .annotation(position: .top, spacing: 2) {
                        Text(formatCost(Decimal(peak.value)))
                            .font(.system(size: 9, weight: .semibold, design: .monospaced))
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .chartLegend(.hidden)
            .chartXAxis {
                AxisMarks(preset: .aligned, values: .automatic(desiredCount: 6)) { _ in
                    AxisValueLabel()
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }
            .chartYAxis {
                AxisMarks(preset: .aligned, values: .automatic(desiredCount: 4)) { value in
                    AxisGridLine(stroke: StrokeStyle(lineWidth: 0.5))
                        .foregroundStyle(Color(nsColor: .separatorColor).opacity(0.3))
                    AxisValueLabel {
                        if let cost = value.as(Double.self) {
                            Text("$\(cost.tokenAxisText)")
                                .font(.caption2)
                                .foregroundStyle(.tertiary)
                        }
                    }
                }
            }
            .chartOverlay { proxy in
                GeometryReader { geometry in
                    DailyLineChartHover(rows: rows, proxy: proxy, geometry: geometry) { row in
                        ChartTooltipPanel(
                            title: row.date.formatted(.dateTime.year().month().day()),
                            rows: [ChartTooltipRow("Cost", formatCost(Decimal(row.value)), color: AppPalette.chartCost)]
                        )
                    }
                }
            }
            .transaction { transaction in
                transaction.animation = nil
            }
        }
    }
}

private struct CacheShareChartView: View {
    let rows: [DailyValuePoint]

    private var averageShare: Double {
        guard !rows.isEmpty else { return 0 }
        return rows.reduce(0.0) { $0 + $1.value } / Double(rows.count)
    }

    var body: some View {
        if rows.isEmpty {
            ChartEmptyState(symbol: "chart.xyaxis.line", text: "No cache data in range")
        } else {
            Chart {
                RuleMark(y: .value("Average", averageShare * 100))
                    .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 4]))
                    .foregroundStyle(Color.secondary.opacity(0.55))

                ForEach(rows) { row in
                    AreaMark(
                        x: .value("Date", row.date),
                        y: .value("Cache", row.value * 100)
                    )
                    .interpolationMethod(.catmullRom)
                    .foregroundStyle(
                        LinearGradient(
                            colors: [AppPalette.chartCache.opacity(0.20), AppPalette.chartCache.opacity(0.02)],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                    )

                    LineMark(
                        x: .value("Date", row.date),
                        y: .value("Cache", row.value * 100)
                    )
                    .interpolationMethod(.catmullRom)
                    .lineStyle(StrokeStyle(lineWidth: 2, lineCap: .round))
                    .foregroundStyle(AppPalette.chartCache)
                }
            }
            .chartLegend(.hidden)
            .chartYScale(domain: 0...100)
            .chartXAxis {
                AxisMarks(preset: .aligned, values: .automatic(desiredCount: 6)) { _ in
                    AxisValueLabel()
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }
            .chartYAxis {
                AxisMarks(values: [0, 25, 50, 75, 100]) { value in
                    AxisGridLine(stroke: StrokeStyle(lineWidth: 0.5))
                        .foregroundStyle(Color(nsColor: .separatorColor).opacity(0.3))
                    AxisValueLabel {
                        if let percent = value.as(Double.self) {
                            Text("\(Int(percent))%")
                                .font(.caption2)
                                .foregroundStyle(.tertiary)
                        }
                    }
                }
            }
            .chartOverlay { proxy in
                GeometryReader { geometry in
                    DailyLineChartHover(rows: rows, proxy: proxy, geometry: geometry) { row in
                        ChartTooltipPanel(
                            title: row.date.formatted(.dateTime.year().month().day()),
                            rows: [
                                ChartTooltipRow("Cache Share", (row.value).percentText, color: AppPalette.chartCache),
                                ChartTooltipRow("Average", averageShare.percentText, emphasized: true),
                            ]
                        )
                    }
                }
            }
            .transaction { transaction in
                transaction.animation = nil
            }
        }
    }
}

private struct WeekdayAverageChartView: View {
    let rows: [WeekdayAveragePoint]

    @State private var hoveredRow: WeekdayAveragePoint?
    @State private var hoveredPoint: CGPoint?

    private var peakRow: WeekdayAveragePoint? {
        rows.max { $0.averageTokens < $1.averageTokens }
    }

    var body: some View {
        if rows.allSatisfy({ $0.averageTokens <= 0 }) {
            ChartEmptyState(symbol: "chart.bar.xaxis", text: "No usage in range")
        } else {
            Chart(rows) { row in
                BarMark(
                    x: .value("Weekday", row.label),
                    y: .value("Avg Tokens", row.averageTokens)
                )
                .foregroundStyle(
                    LinearGradient(
                        colors: [AppPalette.chartWeekday, AppPalette.chartWeekday.opacity(0.6)],
                        startPoint: .top,
                        endPoint: .bottom
                    )
                )
                .cornerRadius(4)
                .opacity(hoveredRow == nil || hoveredRow?.id == row.id ? 1 : 0.35)
                .annotation(position: .top, spacing: 2) {
                    if row.id == peakRow?.id {
                        Text(row.averageTokens.tokenAxisText)
                            .font(.system(size: 9, weight: .semibold, design: .monospaced))
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .chartLegend(.hidden)
            .chartXAxis {
                AxisMarks { _ in
                    AxisValueLabel()
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }
            .chartYAxis {
                AxisMarks(preset: .aligned, values: .automatic(desiredCount: 4)) { value in
                    AxisGridLine(stroke: StrokeStyle(lineWidth: 0.5))
                        .foregroundStyle(Color(nsColor: .separatorColor).opacity(0.3))
                    AxisValueLabel {
                        if let tokens = value.as(Double.self) {
                            Text(tokens.tokenAxisText)
                                .font(.caption2)
                                .foregroundStyle(.tertiary)
                        }
                    }
                }
            }
            .chartOverlay { proxy in
                GeometryReader { geometry in
                    ZStack(alignment: .topLeading) {
                        Rectangle()
                            .fill(.clear)
                            .contentShape(Rectangle())
                            .onContinuousHover { phase in
                                switch phase {
                                case .active(let point):
                                    hoveredRow = row(at: point, proxy: proxy, geometry: geometry)
                                    hoveredPoint = hoveredRow == nil ? nil : point
                                case .ended:
                                    hoveredRow = nil
                                    hoveredPoint = nil
                                }
                            }

                        if let row = hoveredRow, let point = hoveredPoint {
                            ChartTooltipPanel(
                                title: row.label,
                                rows: [ChartTooltipRow("Avg Tokens", Int(row.averageTokens.rounded()).fullTokenText, color: AppPalette.chartWeekday)]
                            )
                            .position(dashboardTooltipPosition(for: point, in: geometry.size))
                            .zIndex(1)
                            .allowsHitTesting(false)
                        }
                    }
                    .animation(nil, value: hoveredRow?.id)
                }
            }
            .transaction { transaction in
                transaction.animation = nil
            }
        }
    }

    private func row(at point: CGPoint, proxy: ChartProxy, geometry: GeometryProxy) -> WeekdayAveragePoint? {
        guard let plotFrame = proxy.plotFrame else { return nil }
        let plotRect = geometry[plotFrame]
        let x = point.x - plotRect.origin.x
        guard let label: String = proxy.value(atX: x) else { return nil }
        return rows.first { $0.label == label }
    }
}

private struct ChartEmptyState: View {
    let symbol: String
    let text: String

    var body: some View {
        VStack(spacing: 8) {
            Image(systemName: symbol)
                .font(.title2)
                .foregroundStyle(.secondary)
            Text(text)
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct HourlyUsagePoint: Identifiable {
    var id: String { "\(hour)-\(series)" }
    let hour: Int
    let series: String
    let tokens: Int
}

private struct HourlyStackSegment: Identifiable {
    var id: String { "\(hour)-\(series)" }
    let hour: Int
    let series: String
    let yStart: Double
    let yEnd: Double
    var isTop: Bool = false
}

/// 24h timeline: wide equal-slot histogram (Monthly axis styling) + brief event rail.
/// Daily Brief kanban lives only on the Brief page.
private struct TodayHourlyTimelineView: View {
    let points: [HourlyUsagePoint]
    let briefHours: [HourlyBriefItem]
    let colorDomain: [String]
    let colorRange: [Color]
    let showsNowIndicator: Bool

    @State private var hoveredHour: Int?
    @State private var hoveredPoint: CGPoint?

    private var hourlyTotals: [Int] {
        var totals = Array(repeating: 0, count: 24)
        for point in points where (0...23).contains(point.hour) {
            totals[point.hour] += point.tokens
        }
        return totals
    }

    private var yUpperBound: Double {
        let peak = hourlyTotals.max() ?? 0
        return max(Double(peak) * 1.12, 1)
    }

    private var nowHourPosition: Double {
        let components = Calendar.current.dateComponents([.hour, .minute], from: Date())
        let hour = Double(components.hour ?? 0)
        let minute = Double(components.minute ?? 0)
        return min(hour + minute / 60.0, 23.999)
    }

    private var currentHour: Int {
        Calendar.current.component(.hour, from: Date())
    }

    /// Half-gap on each side of an hour slot (unit = 1 hour). 0.14 → ~72% bar width.
    private static let hourBarInset = 0.14

    /// Pre-stacked segments so RectangleMark can draw wide bars with gaps.
    /// NOTE: do not use BarMark(width: .ratio) on a custom continuous x scale —
    /// it resolves to zero-width marks and every bar disappears.
    private var stackedSegments: [HourlyStackSegment] {
        var segments: [HourlyStackSegment] = []
        var baseByHour = Array(repeating: 0.0, count: 24)
        let ordered = points
            .filter { (0...23).contains($0.hour) && $0.tokens > 0 }
            .sorted {
                if $0.hour != $1.hour { return $0.hour < $1.hour }
                return $0.series < $1.series
            }
        for point in ordered {
            let base = baseByHour[point.hour]
            let top = base + Double(point.tokens)
            segments.append(
                HourlyStackSegment(
                    hour: point.hour,
                    series: point.series,
                    yStart: base,
                    yEnd: top
                )
            )
            baseByHour[point.hour] = top
        }
        // Segments are appended hour-by-hour, so the last segment of each hour
        // is the stack's top; only that one gets rounded corners.
        var lastIndexByHour: [Int: Int] = [:]
        for (index, segment) in segments.enumerated() {
            lastIndexByHour[segment.hour] = index
        }
        for (index, segment) in segments.enumerated() where lastIndexByHour[segment.hour] == index {
            segments[index].isTop = true
        }
        return segments
    }

    private var briefByHour: [Int: HourlyBriefItem] {
        Dictionary(uniqueKeysWithValues: briefHours.map { ($0.hour, $0) })
    }

    private var timelineEventHours: [Int] {
        var hours = Set(points.filter { $0.tokens > 0 }.map(\.hour))
        hours.formUnion(briefHours.map(\.hour))
        return hours.filter { (0...23).contains($0) }.sorted()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            usageStrip
                .frame(height: dailyUsagePlotHeight)

            if !timelineEventHours.isEmpty {
                eventRail
            }
        }
    }

    private var usageStrip: some View {
        Chart {
            ForEach(stackedSegments) { segment in
                // Inset x range creates gaps between hours; clipShape rounds only
                // the top of each stack (BarMark width:.ratio on a continuous
                // scale draws zero-size marks).
                RectangleMark(
                    xStart: .value("Start", Double(segment.hour) + Self.hourBarInset),
                    xEnd: .value("End", Double(segment.hour) + 1.0 - Self.hourBarInset),
                    yStart: .value("Base", segment.yStart),
                    yEnd: .value("Top", segment.yEnd)
                )
                .foregroundStyle(by: .value("CLI", segment.series))
                .clipShape(
                    UnevenRoundedRectangle(
                        topLeadingRadius: segment.isTop ? 3 : 0,
                        bottomLeadingRadius: 0,
                        bottomTrailingRadius: 0,
                        topTrailingRadius: segment.isTop ? 3 : 0,
                        style: .continuous
                    )
                )
                .opacity(hoveredHour == nil || hoveredHour == segment.hour ? 1 : 0.35)
            }

            if showsNowIndicator {
                RuleMark(x: .value("Now", nowHourPosition))
                    .lineStyle(StrokeStyle(lineWidth: 1, dash: [3, 3]))
                    .foregroundStyle(Color.secondary.opacity(0.55))
                    .annotation(position: .top, alignment: .center, spacing: 1) {
                        Text("Now")
                            .font(.system(size: 9, weight: .medium))
                            .foregroundStyle(.secondary)
                    }
            }
        }
        .chartForegroundStyleScale(domain: colorDomain, range: colorRange)
        .chartLegend(.hidden)
        .chartXScale(domain: 0.0...24.0)
        .chartYScale(domain: 0...yUpperBound)
        .chartXAxis {
            AxisMarks(values: [0.0, 6.0, 12.0, 18.0]) { value in
                AxisValueLabel(anchor: .top) {
                    if let hour = value.as(Double.self) {
                        Text(Self.hourLabel(Int(hour.rounded(.towardZero))))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .chartYAxis {
            AxisMarks { value in
                AxisGridLine(stroke: StrokeStyle(lineWidth: 0.5))
                    .foregroundStyle(Color(nsColor: .separatorColor).opacity(0.35))
                AxisValueLabel {
                    if let tokens = value.as(Double.self) {
                        Text(tokens.tokenAxisText)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .chartOverlay { proxy in
            GeometryReader { geometry in
                ZStack(alignment: .topLeading) {
                    Rectangle()
                        .fill(.clear)
                        .contentShape(Rectangle())
                        .onContinuousHover { phase in
                            switch phase {
                            case .active(let point):
                                hoveredHour = hour(at: point, proxy: proxy, geometry: geometry)
                                hoveredPoint = hoveredHour == nil ? nil : point
                            case .ended:
                                hoveredHour = nil
                                hoveredPoint = nil
                            }
                        }

                    if let hour = hoveredHour, let point = hoveredPoint {
                        ChartTooltipPanel(
                            title: Self.hourLabel(hour),
                            rows: tooltipRows(for: hour)
                        )
                        .position(dashboardTooltipPosition(for: point, in: geometry.size))
                        .zIndex(1)
                        .allowsHitTesting(false)
                    }
                }
                .animation(nil, value: hoveredHour)
            }
        }
        .transaction { transaction in
            transaction.animation = nil
        }
    }

    private var eventRail: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(timelineEventHours, id: \.self) { hour in
                timelineEventRow(hour: hour)
            }
        }
    }

    private func timelineEventRow(hour: Int) -> some View {
        let total = hourlyTotals[hour]
        let brief = briefByHour[hour]
        let isNow = showsNowIndicator && hour == currentHour
        let isHovered = hoveredHour == hour

        return HStack(alignment: .top, spacing: 0) {
            HStack(alignment: .top, spacing: 10) {
                Text(Self.hourLabel(hour))
                    .font(.system(.caption, design: .monospaced).weight(isNow ? .semibold : .regular))
                    .foregroundStyle(isNow ? Color.primary : Color.secondary)
                    .frame(width: 44, alignment: .trailing)
                    .padding(.top, 2)

                VStack(spacing: 0) {
                    Circle()
                        .fill(isNow ? Color.accentColor : Color.secondary.opacity(0.45))
                        .frame(width: 8, height: 8)
                        .padding(.top, 5)
                    Rectangle()
                        .fill(Color.secondary.opacity(0.18))
                        .frame(width: 1)
                        .frame(maxHeight: .infinity)
                }
                .frame(width: 8)
            }
            .frame(width: 70, alignment: .topTrailing)

            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    if let brief {
                        Text(brief.headline)
                            .font(.subheadline.weight(.medium))
                            .foregroundStyle(.primary)
                            .lineLimit(2)
                            .fixedSize(horizontal: false, vertical: true)
                    } else {
                        Text("Activity")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }

                    Spacer(minLength: 8)

                    if total > 0 {
                        Text(total.tokenText)
                            .font(.caption.monospacedDigit().weight(.medium))
                            .foregroundStyle(.secondary)
                    }
                }

                if total > 0 {
                    seriesChipRow(for: hour)
                }

                if let brief, brief.sessionCount > 0 {
                    Text("\(brief.sessionCount) sessions")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }
            .padding(.leading, 12)
            .padding(.vertical, 8)
            .padding(.trailing, 4)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(isHovered || isNow ? Color.primary.opacity(0.04) : Color.clear)
            )
        }
        .padding(.bottom, 2)
        .contentShape(Rectangle())
        .onHover { inside in
            hoveredHour = inside ? hour : (hoveredHour == hour ? nil : hoveredHour)
        }
    }

    private func seriesChipRow(for hour: Int) -> some View {
        let series = points
            .filter { $0.hour == hour && $0.tokens > 0 }
            .sorted { $0.tokens > $1.tokens }
        return HStack(spacing: 6) {
            ForEach(series.prefix(5), id: \.id) { point in
                HStack(spacing: 4) {
                    Circle()
                        .fill(seriesColor(for: point.series))
                        .frame(width: 6, height: 6)
                    Text(point.series)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(Color.primary.opacity(0.04), in: Capsule())
            }
            if series.count > 5 {
                Text("+\(series.count - 5)")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
    }

    private func hour(at point: CGPoint, proxy: ChartProxy, geometry: GeometryProxy) -> Int? {
        guard let plotFrame = proxy.plotFrame else { return nil }
        let plotRect = geometry[plotFrame]
        let x = point.x - plotRect.origin.x
        guard let value: Double = proxy.value(atX: x) else { return nil }
        let hour = Int(value.rounded(.down))
        return (0...23).contains(hour) ? hour : nil
    }

    private func seriesColor(for series: String) -> Color {
        guard let index = colorDomain.firstIndex(of: series), index < colorRange.count else {
            return .blue
        }
        return colorRange[index]
    }

    private func tooltipRows(for hour: Int) -> [ChartTooltipRow] {
        let rows = points
            .filter { $0.hour == hour && $0.tokens > 0 }
            .sorted { $0.tokens > $1.tokens }
        let total = rows.reduce(0) { $0 + $1.tokens }
        var tooltip = rows.prefix(4).map { row in
            ChartTooltipRow(row.series, row.tokens.tokenText, color: seriesColor(for: row.series))
        }
        if rows.count > 4 {
            let others = rows.dropFirst(4).reduce(0) { $0 + $1.tokens }
            tooltip.append(ChartTooltipRow("Others", others.tokenText))
        }
        if let brief = briefByHour[hour] {
            tooltip.append(ChartTooltipRow(brief.headline, "", emphasized: false))
        }
        tooltip.append(ChartTooltipRow("Total", total.tokenText, emphasized: true))
        return tooltip
    }

    static func hourLabel(_ hour: Int) -> String {
        switch hour {
        case 0: "12AM"
        case 12: "12PM"
        case 1...11: "\(hour)AM"
        default: "\(hour - 12)PM"
        }
    }
}

struct ProviderIconBadge: View {
    private let metadata: ProviderMetadata
    private let size: CGFloat

    init(modelName: String, size: CGFloat = 22) {
        metadata = ProviderMetadata.forModel(modelName)
        self.size = size
    }

    var body: some View {
        badgeBody
            .help(metadata.label)
            .accessibilityLabel(metadata.label)
    }

    @ViewBuilder
    private var badgeBody: some View {
        let shape = RoundedRectangle(cornerRadius: size * 0.23, style: .continuous)
        if let imageAssetName = metadata.imageAssetName {
            BundledIconImage(
                imageAssetName: imageAssetName,
                tint: metadata.preservesOriginalImageColor ? nil : metadata.color,
                padding: 1
            )
            .frame(width: size, height: size)
            .background(metadata.color.opacity(0.10), in: shape)
            .background(.ultraThinMaterial, in: shape)
            .overlay(shape.stroke(metadata.color.opacity(0.18), lineWidth: 0.5))
        } else {
            Text(metadata.abbreviation)
                .font(.system(size: size * 0.41, weight: .bold, design: .rounded))
                .foregroundStyle(metadata.color)
                .frame(width: size + 2, height: size - 4)
                .background(metadata.color.opacity(0.13), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 5, style: .continuous)
                        .stroke(metadata.color.opacity(0.28), lineWidth: 1)
                )
        }
    }
}

struct UsageSourceIconBadge: View {
    let source: UsageSource
    var size: CGFloat = 22

    var body: some View {
        sourceIconBadge
            .help(source.displayName)
            .accessibilityLabel(source.displayName)
    }

    @ViewBuilder
    private var sourceIconBadge: some View {
        let shape = RoundedRectangle(cornerRadius: size * 0.23, style: .continuous)
        if let imageAssetName = source.imageAssetName {
            BundledIconImage(imageAssetName: imageAssetName, padding: 1)
                .frame(width: size, height: size)
                .background(source.iconBadgeBackgroundColor, in: shape)
                .background(.ultraThinMaterial, in: shape)
                .overlay(shape.stroke(source.tintColor.opacity(0.16), lineWidth: 0.5))
        } else {
            Image(systemName: source.systemImage)
                .font(.system(size: size * 0.5, weight: .semibold))
                .foregroundStyle(source.tintColor)
                .frame(width: size, height: size - 4)
                .background(source.tintColor.opacity(0.12), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
        }
    }
}

private struct TokenUsageSourceLabel: View {
    let source: TokenUsageSource

    var body: some View {
        HStack(spacing: 7) {
            sourceIconBadge
            Text(source.label)
                .lineLimit(1)
        }
    }

    @ViewBuilder
    private var sourceIconBadge: some View {
        if let imageAssetName = source.imageAssetName {
            BundledIconImage(imageAssetName: imageAssetName, padding: 1)
                .frame(width: 22, height: 22)
                .background(source.iconBadgeBackgroundColor, in: RoundedRectangle(cornerRadius: 5, style: .continuous))
        } else {
            Image(systemName: source.systemImage)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(source.tintColor)
                .frame(width: 22, height: 18)
                .background(source.tintColor.opacity(0.12), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
        }
    }
}

struct BundledIconImage: View {
    let imageAssetName: String
    var tint: Color? = nil
    var padding: CGFloat = 0
    var size: CGFloat = 22

    var body: some View {
        icon
            .frame(width: size, height: size)
    }

    @ViewBuilder
    private var icon: some View {
        if let image = IconImageLoader.image(named: imageAssetName, pointSize: max(1, size - padding * 2)) {
            if let tint {
                Image(nsImage: image)
                    .resizable()
                    .renderingMode(.template)
                    .scaledToFit()
                    .foregroundStyle(tint)
                    .padding(padding)
            } else {
                Image(nsImage: image)
                    .resizable()
                    .renderingMode(.original)
                    .scaledToFit()
                    .padding(padding)
            }
        } else {
            Color.clear
                .padding(padding)
        }
    }
}

@MainActor
private enum IconImageLoader {
    private static let imageCache = NSCache<NSString, NSImage>()

    static func image(named name: String, pointSize: CGFloat) -> NSImage? {
        let cacheKey = "\(name):\(pointSize)" as NSString
        if let cachedImage = imageCache.object(forKey: cacheKey) {
            return cachedImage
        }

        for bundle in fallbackBundles() {
            if let image = image(named: name, in: bundle) {
                let sizedImage = image.copy() as? NSImage ?? image
                sizedImage.size = NSSize(width: pointSize, height: pointSize)
                imageCache.setObject(sizedImage, forKey: cacheKey)
                return sizedImage
            }
        }
        return nil
    }

    private static func image(named name: String, in bundle: Bundle) -> NSImage? {
        for fileExtension in ["svg", "png"] {
            if let url = bundle.url(forResource: name, withExtension: fileExtension),
               let image = NSImage(contentsOf: url) {
                return image
            }
        }

        return bundle.image(forResource: name)
    }

    private static func fallbackBundles() -> [Bundle] {
        let bundleName = "TokenUsageNative_TokenUsageNative.bundle"
        let candidates = [
            Bundle.main.bundleURL.appendingPathComponent(bundleName),
            Bundle.main.resourceURL?.appendingPathComponent(bundleName),
        ].compactMap(\.self)

        return candidates.compactMap { Bundle(url: $0) }
    }
}

private struct DashboardHeroView: View {
    let periodLabel: String
    let totalTokens: Int
    let costText: String
    let subtitle: String
    let isRefreshing: Bool

    @State private var displayedValue = 0
    @State private var hasAppeared = false
    @State private var wasRefreshing = false
    @State private var animationTask: Task<Void, Never>?

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(periodLabel.uppercased())
                .font(.callout.weight(.semibold))
                .tracking(0.8)
                .foregroundStyle(.secondary)

            Text(displayedValue.fullTokenText)
                .font(.system(size: 40, weight: .bold, design: .rounded))
                .monospacedDigit()
                .lineLimit(1)
                .minimumScaleFactor(0.45)
                .contentTransition(.numericText())

            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(costText)
                    .font(.system(.title3, design: .rounded).weight(.semibold))
                    .monospacedDigit()
                Text(subtitle)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 4)
        .onAppear {
            wasRefreshing = isRefreshing
            guard !hasAppeared else { return }
            hasAppeared = true
            if !isRefreshing {
                animate(to: totalTokens, fromZero: true)
            }
        }
        .onChange(of: totalTokens) { _, newValue in
            guard !isRefreshing else { return }
            animate(to: newValue, fromZero: false)
        }
        .onChange(of: isRefreshing) { _, refreshing in
            if wasRefreshing && !refreshing {
                animate(to: totalTokens, fromZero: true)
            }
            wasRefreshing = refreshing
        }
        .onDisappear {
            animationTask?.cancel()
            animationTask = nil
        }
    }

    private func animate(to target: Int, fromZero: Bool) {
        animationTask?.cancel()

        let end = max(target, 0)
        let start = fromZero ? 0 : displayedValue
        guard start != end else {
            displayedValue = end
            return
        }

        animationTask = Task { @MainActor in
            let duration = 0.85
            let startedAt = Date()
            displayedValue = start

            while !Task.isCancelled {
                let progress = min(Date().timeIntervalSince(startedAt) / duration, 1)
                let eased = 1 - pow(1 - progress, 3)
                let next = start + Int((Double(end - start) * eased).rounded())
                if next != displayedValue {
                    withAnimation(.linear(duration: 0.05)) {
                        displayedValue = next
                    }
                }
                if progress >= 1 {
                    break
                }
                try? await Task.sleep(nanoseconds: 16_000_000)
            }

            if !Task.isCancelled {
                displayedValue = end
            }
        }
    }
}

private struct DashboardHeatmapDay: Identifiable {
    var id: Date { date }
    let date: Date
    let tokens: Int
    let cost: Decimal
}

private struct DashboardHeatmapView: View {
    let days: [DashboardHeatmapDay]
    let availableWidth: CGFloat
    let costText: (Decimal) -> String
    /// Day tap for the day drill-down; nil disables cell interaction.
    var onSelectDay: ((DashboardHeatmapDay) -> Void)? = nil

    @State private var hoveredDay: DashboardHeatmapDay?

    private let maximumCellSize: CGFloat = 30
    private let minimumCellSize: CGFloat = 16
    private let cellSpacing: CGFloat = 3
    private let weekdayLabelWidth: CGFloat = 24
    private let monthLabelHeight: CGFloat = 16
    private let dayOfMonthLabelHeight: CGFloat = 12

    /// The weekday labels are fixed to Mon/Wed/Fri, so the grid must use the
    /// same Monday-first calendar regardless of the system's locale setting.
    private var calendar: Calendar {
        var calendar = Calendar.current
        calendar.firstWeekday = 2
        return calendar
    }

    private var columnStride: CGFloat {
        cellSize + cellSpacing
    }

    private var gridWidth: CGFloat {
        CGFloat(weeks.count) * cellSize + CGFloat(max(weeks.count - 1, 0)) * cellSpacing
    }

    private var heatmapWidth: CGFloat {
        weekdayLabelWidth + cellSpacing + gridWidth
    }

    private var maxTokens: Int {
        days.map(\.tokens).max() ?? 0
    }

    /// Keeps cells square while shrinking just enough for every week column to
    /// fit in the heatmap card.
    private var cellSize: CGFloat {
        guard !weeks.isEmpty else { return maximumCellSize }
        let columnCount = CGFloat(weeks.count)
        let gridAvailableWidth = max(availableWidth - weekdayLabelWidth - cellSpacing, 0)
        let totalSpacing = CGFloat(max(weeks.count - 1, 0)) * cellSpacing
        let widthLimitedSize = (gridAvailableWidth - totalSpacing) / columnCount
        return min(maximumCellSize, max(widthLimitedSize, minimumCellSize))
    }

    /// Weeks arranged as columns (GitHub style): each column = one week, 7 rows = days of week.
    private var weeks: [[DashboardHeatmapDay?]] {
        let sortedDays = days.sorted { $0.date < $1.date }
        guard let first = sortedDays.first else { return [] }
        var result: [[DashboardHeatmapDay?]] = []
        var current: [DashboardHeatmapDay?] = []
        let leadingEmpty = calendar.component(.weekday, from: first.date) - calendar.firstWeekday
        let normalizedLeading = (leadingEmpty + 7) % 7
        current.append(contentsOf: Array(repeating: nil, count: normalizedLeading))
        for day in sortedDays {
            current.append(day)
            if current.count == 7 {
                result.append(current)
                current = []
            }
        }
        if !current.isEmpty {
            current.append(contentsOf: Array(repeating: nil, count: 7 - current.count))
            result.append(current)
        }
        return result
    }

    /// Weekday labels to show alongside rows (GitHub shows Mon, Wed, Fri).
    private let weekdayLabels: [Int: String] = [
        0: "Mon", 2: "Wed", 4: "Fri"
    ]

    /// Month labels positioned at the precise week column where each month starts.
    private var monthLabels: [(text: String, weekIndex: Int)] {
        var labels: [(text: String, weekIndex: Int)] = []
        var lastMonth: (month: Int, year: Int)?
        for (weekIndex, week) in weeks.enumerated() {
            for day in week {
                guard let day else { continue }
                let month = calendar.component(.month, from: day.date)
                let year = calendar.component(.year, from: day.date)
                guard lastMonth?.month != month || lastMonth?.year != year else { continue }

                let text = lastMonth == nil || lastMonth?.year != year
                    ? day.date.formatted(.dateTime.month(.abbreviated).year())
                    : day.date.formatted(.dateTime.month(.abbreviated))
                labels.append((text: text, weekIndex: weekIndex))
                lastMonth = (month, year)
            }
        }
        return labels
    }

    /// One date tick per week column prevents labels such as 1 and 5 from
    /// occupying the same horizontal coordinate. Month starts take precedence.
    private var dayOfMonthLabels: [(weekIndex: Int, text: String)] {
        weeks.enumerated().compactMap { weekIndex, week in
            let dates = week.compactMap { $0 }
            guard let firstDay = dates.first else { return nil }
            let tickDay = dates.first { calendar.component(.day, from: $0.date) == 1 } ?? firstDay
            return (weekIndex, "\(calendar.component(.day, from: tickDay.date))")
        }
    }

    var body: some View {
        if days.isEmpty {
            VStack(spacing: 8) {
                Image(systemName: "square.grid.3x3")
                    .font(.title2)
                    .foregroundStyle(.secondary)
                Text("No token usage in range")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, minHeight: 160, alignment: .center)
        } else {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: cellSpacing) {
                    Color.clear
                        .frame(width: weekdayLabelWidth, height: monthLabelHeight)
                    monthLabelRow
                }
                HStack(alignment: .top, spacing: cellSpacing) {
                    weekdayLabelColumn
                    heatmapGrid
                }
                HStack(spacing: cellSpacing) {
                    Color.clear
                        .frame(width: weekdayLabelWidth, height: dayOfMonthLabelHeight)
                    dayOfMonthLabelRow
                }
            }
            .padding(.vertical, 8)
            .frame(width: heatmapWidth, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .center)
            .overlay(alignment: .topTrailing) {
                if let hoveredDay {
                    ChartTooltipPanel(
                        title: hoveredDay.date.formatted(.dateTime.year().month().day()),
                        rows: [
                            ChartTooltipRow("Tokens", hoveredDay.tokens.fullTokenText),
                            ChartTooltipRow("Cost", costText(hoveredDay.cost), emphasized: true)
                        ]
                    )
                    .padding(4)
                }
            }
        }
    }

    /// Row of month labels aligned above the week columns.
    private var monthLabelRow: some View {
        return ZStack(alignment: .topLeading) {
            Color.clear
                .frame(width: gridWidth, height: monthLabelHeight)
            ForEach(Array(monthLabels.enumerated()), id: \.offset) { _, label in
                let xOffset = CGFloat(label.weekIndex) * columnStride
                Text(label.text)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .frame(width: max(cellSize * 2, 50), height: monthLabelHeight, alignment: .leading)
                    .offset(x: xOffset)
            }
        }
    }

    /// Column of weekday labels (Mon, Wed, Fri) aligned with the heatmap rows.
    private var weekdayLabelColumn: some View {
        VStack(spacing: cellSpacing) {
            ForEach(0..<7, id: \.self) { row in
                Group {
                    if let label = weekdayLabels[row] {
                        Text(label)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    } else {
                        Color.clear
                    }
                }
                .frame(width: weekdayLabelWidth, height: cellSize, alignment: .trailing)
            }
        }
    }

    /// The actual heatmap grid: weeks as columns, days as rows.
    private var heatmapGrid: some View {
        HStack(spacing: cellSpacing) {
            ForEach(Array(weeks.enumerated()), id: \.offset) { _, week in
                VStack(spacing: cellSpacing) {
                    ForEach(Array(week.enumerated()), id: \.offset) { _, day in
                        cell(for: day)
                    }
                }
            }
        }
    }

    /// Row of non-overlapping date labels, one for each week column.
    private var dayOfMonthLabelRow: some View {
        return ZStack(alignment: .topLeading) {
            Color.clear
                .frame(width: gridWidth, height: dayOfMonthLabelHeight)
            ForEach(Array(dayOfMonthLabels.enumerated()), id: \.offset) { _, mark in
                let xOffset = CGFloat(mark.weekIndex) * columnStride
                Text(mark.text)
                    .font(.system(size: 10))
                    .foregroundStyle(.secondary)
                    .frame(width: cellSize, height: dayOfMonthLabelHeight, alignment: .center)
                    .offset(x: xOffset)
            }
        }
        .padding(.top, 2)
    }

    @ViewBuilder
    private func cell(for day: DashboardHeatmapDay?) -> some View {
        if let day {
            RoundedRectangle(cornerRadius: 3, style: .continuous)
                .fill(color(for: day.tokens))
                .frame(width: cellSize, height: cellSize)
                .help("\(day.date.formatted(.dateTime.year().month().day()))\n\(day.tokens.fullTokenText) tokens\n\(costText(day.cost))")
                .onHover { isHovering in
                    if isHovering {
                        hoveredDay = day
                    } else if hoveredDay?.date == day.date {
                        hoveredDay = nil
                    }
                }
                .onTapGesture {
                    onSelectDay?(day)
                }
        } else {
            RoundedRectangle(cornerRadius: 3, style: .continuous)
                .fill(Color.clear)
                .frame(width: cellSize, height: cellSize)
        }
    }

    private func color(for tokens: Int) -> Color {
        guard tokens > 0, maxTokens > 0 else {
            // Zero-token days: a faint neutral cell that recedes into the card.
            return Color(nsColor: .separatorColor).opacity(0.18)
        }
        let ratio = Double(tokens) / Double(maxTokens)
        let intensity = 0.25 + 0.75 * pow(ratio, 0.5)
        return Color.accentColor.opacity(intensity)
    }
}

private struct TodayMetricCard: View {
    let title: String
    let value: String
    let subtitle: String
    let systemImage: String
    let tint: Color

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: systemImage)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: 28, height: 28)
                .background(tint.opacity(0.14), in: RoundedRectangle(cornerRadius: 7, style: .continuous))

            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(value)
                    .font(.system(.title2, design: .rounded).weight(.semibold))
                    .monospacedDigit()
                    .lineLimit(1)
                    .minimumScaleFactor(0.7)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
            }
        }
        .frame(maxWidth: .infinity, minHeight: 84, alignment: .topLeading)
        .padding(14)
        .appCard()
        .drawingGroup()
    }
}

private struct TodaySourceRowView: View {
    let row: TodaySourceUsageRow
    let maxTokens: Int
    let costText: String

    private var fillWidthRatio: Double {
        guard maxTokens > 0 else { return 0 }
        return Double(row.totalTokens) / Double(maxTokens)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                UsageSourceIconBadge(source: row.source)
                Text(row.source.label)
                    .font(.callout.weight(.medium))
                    .lineLimit(1)
                Spacer()
                Text(row.totalTokens.tokenText)
                    .font(.system(.callout, design: .monospaced))
                Text(costText)
                    .font(.system(.callout, design: .monospaced))
                    .frame(width: 86, alignment: .trailing)
            }

            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(Color(nsColor: .separatorColor).opacity(0.25))
                    Capsule()
                        .fill(row.source.tintColor)
                        .frame(width: max(geometry.size.width * fillWidthRatio, row.totalTokens > 0 ? 3 : 0))
                }
            }
            .frame(height: 6)

            HStack(spacing: 12) {
                Label(row.cacheReadTokens.tokenText, systemImage: "externaldrive")
                Label(row.cacheShare.percentText, systemImage: "chart.pie")
                Label("\(row.modelCount)", systemImage: "cpu")
                Spacer()
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 2)
        .padding(.vertical, 6)
    }
}

private struct TokenMixDistributionChart: View {
    let rows: [TodayModelTokenRow]
    let legendRows: [TodayModelTokenRow]
    let totalTokens: Int
    let isLoading: Bool

    var body: some View {
        if rows.isEmpty {
            VStack(spacing: 8) {
                Image(systemName: "chart.pie")
                    .font(.title2)
                    .foregroundStyle(.secondary)
                Text(isLoading ? "Loading models..." : "No model usage recorded")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            HStack(alignment: .center, spacing: 32) {
                ZStack {
                    TokenMixSectorChart(rows: rows)
                        .zIndex(1)

                    VStack(spacing: 4) {
                        Text("Total")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(totalTokens.tokenText)
                            .font(.system(size: 20, weight: .semibold, design: .monospaced))
                            .lineLimit(1)
                            .minimumScaleFactor(0.7)
                    }
                    .frame(width: 116)
                    .zIndex(0)
                }
                .frame(width: 220, height: 220)
                .frame(maxWidth: .infinity, maxHeight: .infinity)

                TokenMixLegend(rows: legendRows)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

private struct TokenMixSectorChart: View {
    let rows: [TodayModelTokenRow]
    @State private var hoveredRow: TodayModelTokenRow?
    @State private var hoveredPoint: CGPoint?

    var body: some View {
        Chart(rows) { row in
            SectorMark(
                angle: .value("Tokens", row.tokens),
                innerRadius: .ratio(0.62),
                angularInset: 1.6
            )
            .cornerRadius(6)
            .foregroundStyle(row.color)
            .opacity(hoveredRow == nil || hoveredRow?.id == row.id ? 1 : 0.55)
        }
        .chartLegend(.hidden)
        .chartBackground { _ in Color.clear }
        .chartOverlay { proxy in
            GeometryReader { geometry in
                ZStack(alignment: .topLeading) {
                    Rectangle()
                        .fill(.clear)
                        .contentShape(Rectangle())
                        .onContinuousHover { phase in
                            switch phase {
                            case .active(let point):
                                hoveredRow = row(at: point, proxy: proxy, geometry: geometry)
                                hoveredPoint = hoveredRow == nil ? nil : point
                            case .ended:
                                hoveredRow = nil
                                hoveredPoint = nil
                            }
                        }

                    if let row = hoveredRow, let point = hoveredPoint {
                        ChartTooltipPanel(
                            title: displayModelName(row.modelName),
                            rows: [
                                ChartTooltipRow("Tokens", row.tokens.tokenText, color: row.color),
                                ChartTooltipRow("Share", row.percentText, emphasized: true),
                            ]
                        )
                        .position(tooltipPosition(for: point, in: geometry.size))
                        .zIndex(1)
                    }
                }
                .animation(nil, value: hoveredRow?.id)
            }
        }
        .transaction { transaction in
            transaction.animation = nil
        }
        .chartPlotStyle { plot in
            plot
                .frame(width: 220, height: 220)
        }
    }

    private func row(at point: CGPoint, proxy: ChartProxy, geometry: GeometryProxy) -> TodayModelTokenRow? {
        guard let plotFrame = proxy.plotFrame else {
            return nil
        }

        let plotRect = geometry[plotFrame]
        let vectorX = point.x - plotRect.midX
        let vectorY = point.y - plotRect.midY
        let radius = hypot(vectorX, vectorY)
        let outerRadius = min(plotRect.width, plotRect.height) / 2
        let innerRadius = outerRadius * 0.62
        guard radius >= innerRadius, radius <= outerRadius else {
            return nil
        }

        let total = rows.reduce(0) { $0 + $1.tokens }
        guard total > 0 else {
            return nil
        }

        let rawAngle = atan2(vectorX, -vectorY)
        let angle = rawAngle >= 0 ? rawAngle : rawAngle + (2 * .pi)
        let target = Double(angle / (2 * .pi)) * Double(total)

        var lowerBound = 0.0
        for row in rows {
            let upperBound = lowerBound + Double(row.tokens)
            if target >= lowerBound, target <= upperBound {
                return row
            }
            lowerBound = upperBound
        }

        return nil
    }

    private func tooltipPosition(for point: CGPoint, in size: CGSize) -> CGPoint {
        let tooltipWidth = chartTooltipWidth
        let tooltipHeight = chartTooltipHeight
        let horizontalPadding = 10.0
        let verticalPadding = 10.0

        let x = min(
            max(point.x + 18 + tooltipWidth / 2, tooltipWidth / 2 + horizontalPadding),
            max(tooltipWidth / 2 + horizontalPadding, size.width - tooltipWidth / 2 - horizontalPadding)
        )
        let preferredY = point.y - 18 - tooltipHeight / 2
        let fallbackY = point.y + 18 + tooltipHeight / 2
        let unclampedY = preferredY >= tooltipHeight / 2 + verticalPadding ? preferredY : fallbackY
        let y = min(
            max(unclampedY, tooltipHeight / 2 + verticalPadding),
            max(tooltipHeight / 2 + verticalPadding, size.height - tooltipHeight / 2 - verticalPadding)
        )

        return CGPoint(x: x, y: y)
    }
}

private struct TokenMixLegend: View {
    let rows: [TodayModelTokenRow]

    var body: some View {
        VStack(alignment: .leading, spacing: modelDistributionLegendRowSpacing) {
            ForEach(rows) { row in
                legendItem(row)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
    }

    private func legendItem(_ row: TodayModelTokenRow) -> some View {
        let modelLabel = displayModelName(row.modelName)
        return HStack(alignment: .firstTextBaseline, spacing: 8) {
            Circle()
                .fill(row.color)
                .frame(width: 9, height: 9)
            ProviderIconBadge(modelName: row.modelName)

            VStack(alignment: .leading, spacing: 2) {
                Text("\(modelLabel) (\(row.percentText))")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(row.tokens.tokenText)
                    .font(.system(size: 12, weight: .medium, design: .monospaced))
                    .foregroundStyle(.primary)
            }
        }
        .frame(
            maxWidth: .infinity,
            minHeight: modelDistributionLegendRowHeight,
            maxHeight: modelDistributionLegendRowHeight,
            alignment: .leading
        )
    }
}

private enum ModelConsumptionGrouping: String, CaseIterable, Identifiable {
    case model
    case cli

    var id: String { rawValue }

    var label: String {
        switch self {
        case .model: "Model"
        case .cli: "CLI"
        }
    }
}

private struct ModelConsumptionPart: Identifiable, Hashable {
    let id: String
    let label: String
    let modelName: String?
    let source: UsageSource?
    let tokens: Int
    let tooltip: String
}

private struct ModelConsumptionTotals {
    var total = 0
    var cache = 0
    var cost = Decimal.zero
    var parts: [ModelConsumptionPart] = []
}

private struct ModelConsumptionGroupRow: Identifiable, Hashable {
    let id: String
    let title: String
    let modelName: String?
    let source: UsageSource?
    let totalTokens: Int
    let cacheShare: Double
    let totalCost: Decimal
    let parts: [ModelConsumptionPart]
}

private struct ModelConsumptionGroupRowView: View {
    let row: ModelConsumptionGroupRow
    let grouping: ModelConsumptionGrouping
    let costText: String

    var body: some View {
        HStack(spacing: 10) {
            HStack(spacing: 8) {
                if grouping == .model, let modelName = row.modelName {
                    ProviderIconBadge(modelName: modelName)
                } else if let source = row.source {
                    UsageSourceIconBadge(source: source)
                }
                Text(row.title)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(row.modelName ?? row.title)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            ModelConsumptionPartStack(parts: row.parts, grouping: grouping)
                .frame(width: 150, alignment: .leading)

            Text(row.totalTokens.tokenText)
                .font(.system(.caption, design: .monospaced))
                .frame(width: 92, alignment: .trailing)
            Text(row.cacheShare.percentText)
                .font(.system(.caption, design: .monospaced))
                .foregroundStyle(.secondary)
                .frame(width: 78, alignment: .trailing)
            Text(costText)
                .font(.system(.caption, design: .monospaced))
                .frame(width: 86, alignment: .trailing)
        }
        .font(.caption)
        .padding(.horizontal, 2)
        .padding(.vertical, 7)
    }
}

/// Inline row of counterpart icons (CLIs for a model, models for a CLI).
/// Hovering an icon shows the counterpart's name and its usage.
private struct ModelConsumptionPartStack: View {
    let parts: [ModelConsumptionPart]
    let grouping: ModelConsumptionGrouping
    @State private var hoveredPartID: String?
    private let maxVisible = 6

    var body: some View {
        HStack(spacing: 4) {
            ForEach(parts.prefix(maxVisible)) { part in
                partIcon(part)
                    .scaleEffect(hoveredPartID == part.id ? 1.18 : 1)
                    .zIndex(hoveredPartID == part.id ? 1 : 0)
                    .onHover { hovering in
                        withAnimation(.easeInOut(duration: 0.12)) {
                            hoveredPartID = hovering ? part.id : nil
                        }
                    }
                    .help(part.tooltip)
            }
            if parts.count > maxVisible {
                Text("+\(parts.count - maxVisible)")
                    .font(.system(size: 9, weight: .semibold, design: .rounded))
                    .foregroundStyle(.secondary)
                    .frame(width: 22, height: 20)
                    .background(
                        Color.primary.opacity(0.06),
                        in: RoundedRectangle(cornerRadius: 5, style: .continuous)
                    )
                    .help(parts.dropFirst(maxVisible).map(\.label).joined(separator: ", "))
            }
        }
    }

    @ViewBuilder
    private func partIcon(_ part: ModelConsumptionPart) -> some View {
        if grouping == .model, let source = part.source {
            UsageSourceIconBadge(source: source, size: 20)
        } else if let modelName = part.modelName {
            ProviderIconBadge(modelName: modelName, size: 20)
        }
    }
}

private struct ChartCard<Content: View, TitleAccessory: View>: View {
    let title: String
    let titleAccessory: TitleAccessory
    let content: Content

    init(
        title: String,
        @ViewBuilder titleAccessory: () -> TitleAccessory,
        @ViewBuilder content: () -> Content
    ) {
        self.title = title
        self.titleAccessory = titleAccessory()
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .center) {
                Text(title)
                    .font(.headline)
                Spacer()
                titleAccessory
            }

            content
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .appCard()
    }
}

private extension ChartCard where TitleAccessory == EmptyView {
    init(title: String, @ViewBuilder content: () -> Content) {
        self.init(title: title) {
            EmptyView()
        } content: {
            content()
        }
    }
}

private struct BackendLogsView: View {
    let lines: [String]
    let onClear: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Text("\(lines.count) lines")
                    .font(.callout)
                    .foregroundStyle(.secondary)

                Spacer()

                Button("Clear") {
                    onClear()
                }
                .disabled(lines.isEmpty)
                .help("Clear backend logs")
            }

            if lines.isEmpty {
                ContentUnavailableView {
                    Label("No Backend Logs", systemImage: "terminal")
                } description: {
                    Text("Refresh or change dashboard data to generate backend activity.")
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollViewReader { proxy in
                    ScrollView(.vertical, showsIndicators: true) {
                        LazyVStack(alignment: .leading, spacing: 4) {
                            ForEach(Array(lines.enumerated()), id: \.offset) { index, line in
                                Text(line)
                                    .font(.system(.caption, design: .monospaced))
                                    .foregroundStyle(line.contains("[stderr]") ? AppPalette.semanticError : .primary)
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .id(index)
                            }
                        }
                        .padding(10)
                    }
                    .background(
                        Color(nsColor: .textBackgroundColor),
                        in: RoundedRectangle(cornerRadius: AppDesign.cardCornerRadius, style: .continuous)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: AppDesign.cardCornerRadius, style: .continuous)
                            .stroke(AppDesign.hairline.opacity(0.45), lineWidth: 1)
                    )
                    .onAppear {
                        scrollToBottom(proxy)
                    }
                    .onChange(of: lines.count) {
                        scrollToBottom(proxy)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private func scrollToBottom(_ proxy: ScrollViewProxy) {
        guard let lastIndex = lines.indices.last else {
            return
        }

        DispatchQueue.main.async {
            proxy.scrollTo(lastIndex, anchor: .bottom)
        }
    }
}

/// A single key/value line in a chart hover panel. `color` draws a small
/// series dot in front of the label; `emphasized` renders the value in the
/// primary style (used for totals).
private struct ChartTooltipRow {
    let label: String
    let value: String
    var color: Color? = nil
    var emphasized: Bool = false
    var multiline: Bool = false

    init(
        _ label: String,
        _ value: String,
        color: Color? = nil,
        emphasized: Bool = false,
        multiline: Bool = false
    ) {
        self.label = label
        self.value = value
        self.color = color
        self.emphasized = emphasized
        self.multiline = multiline
    }
}

private struct ChartTooltipPanel: View {
    let title: String
    let rows: [ChartTooltipRow]
    var width: CGFloat = chartTooltipWidth

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(title)
                .font(.caption.weight(.semibold))
                .lineLimit(1)
                .truncationMode(.middle)
                .padding(.bottom, 1)

            ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                if row.multiline {
                    Text(row.label)
                        .foregroundStyle(row.emphasized ? Color.primary : Color.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                } else {
                    HStack(spacing: 6) {
                        if let color = row.color {
                            Circle()
                                .fill(color)
                                .frame(width: 6, height: 6)
                        }
                        Text(row.label)
                            .foregroundStyle(row.emphasized ? Color.primary : Color.secondary)
                            .lineLimit(1)
                        Spacer(minLength: 12)
                        Text(row.value)
                            .font(.system(.caption, design: .monospaced))
                            .fontWeight(row.emphasized ? .semibold : .regular)
                            .foregroundStyle(.primary)
                            .lineLimit(1)
                            .minimumScaleFactor(0.85)
                    }
                }
            }
        }
        .font(.caption)
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .frame(width: width, alignment: .leading)
        .fixedSize(horizontal: false, vertical: true)
        .appFloatingOverlaySurface(cornerRadius: 10)
        .allowsHitTesting(false)
    }
}

private struct TokenTrendLegendEntry: Identifiable {
    var id: String { label }
    let label: String
    let color: Color
}

private struct TokenTrendLegend: View {
    let entries: [TokenTrendLegendEntry]
    let currentPage: Int
    let pageCount: Int
    let totalCount: Int
    let usesCompactLayout: Bool
    let isCLIGrouping: Bool
    let onPrevious: () -> Void
    let onNext: () -> Void

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            Button(action: onPrevious) {
                Image(systemName: "chevron.left")
                    .frame(width: 18, height: 18)
            }
            .buttonStyle(.borderless)
            .disabled(currentPage == 0)
            .help(isCLIGrouping ? "Previous CLIs" : "Previous models")

            Group {
                if usesCompactLayout {
                    ViewThatFits(in: .horizontal) {
                        legendGrid(columnCount: 4, itemWidth: 124, horizontalSpacing: 8)
                        legendGrid(columnCount: 3, itemWidth: 132, horizontalSpacing: 8)
                        legendGrid(columnCount: 2, itemWidth: 154, horizontalSpacing: 10)
                        legendGrid(columnCount: 1, itemWidth: 190, horizontalSpacing: 0)
                    }
                } else {
                    ViewThatFits(in: .horizontal) {
                        legendGrid(columnCount: 3, itemWidth: 190, horizontalSpacing: 18)
                        legendGrid(columnCount: 2, itemWidth: 190, horizontalSpacing: 18)
                        legendGrid(columnCount: 1, itemWidth: 230, horizontalSpacing: 0)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .center)

            Text("\(currentPage + 1)/\(pageCount)")
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 42)
                .help("\(totalCount) \(isCLIGrouping ? "CLIs" : "models")")

            Button(action: onNext) {
                Image(systemName: "chevron.right")
                    .frame(width: 18, height: 18)
            }
            .buttonStyle(.borderless)
            .disabled(currentPage >= pageCount - 1)
            .help(isCLIGrouping ? "Next CLIs" : "Next models")
        }
        .frame(maxWidth: .infinity, minHeight: 48, alignment: .center)
    }

    private func legendGrid(
        columnCount: Int,
        itemWidth: CGFloat,
        horizontalSpacing: CGFloat
    ) -> some View {
        VStack(alignment: .center, spacing: usesCompactLayout ? 6 : 8) {
            ForEach(Array(legendRows(columnCount: columnCount).enumerated()), id: \.offset) { _, row in
                HStack(spacing: horizontalSpacing) {
                    ForEach(row) { entry in
                        legendItem(entry, width: itemWidth)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .center)
            }
        }
        .fixedSize(horizontal: true, vertical: true)
    }

    private func legendItem(_ entry: TokenTrendLegendEntry, width: CGFloat) -> some View {
        let label = displayModelName(entry.label)
        return HStack(spacing: usesCompactLayout ? 6 : 8) {
            Circle()
                .fill(entry.color)
                .frame(width: 9, height: 9)
            LegendBadge(label: entry.label, compact: usesCompactLayout)
            Text(label)
                .font(usesCompactLayout ? .caption2 : .caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .frame(width: width, alignment: usesCompactLayout ? .leading : .center)
    }

    private func legendRows(columnCount: Int) -> [[TokenTrendLegendEntry]] {
        let columnCount = max(columnCount, 1)
        return stride(from: 0, to: entries.count, by: columnCount).map { start in
            Array(entries[start..<min(start + columnCount, entries.count)])
        }
    }
}

private struct LegendBadge: View {
    let label: String
    let compact: Bool

    var body: some View {
        if let source = UsageSource(label: label) {
            UsageSourceIconBadge(source: source)
                .scaleEffect(compact ? 0.82 : 1)
                .frame(width: compact ? 18 : 22, height: compact ? 18 : 22)
        } else {
            ProviderIconBadge(modelName: label)
                .scaleEffect(compact ? 0.82 : 1)
                .frame(width: compact ? 18 : 22, height: compact ? 18 : 22)
        }
    }
}

private struct LegendPageControls: View {
    let currentPage: Int
    let pageCount: Int
    let totalCount: Int
    let onPrevious: () -> Void
    let onNext: () -> Void

    /// Dots replace the "n/m" readout for a handful of pages; beyond that the
    /// readout stays compact.
    private var showsDots: Bool { pageCount <= 8 }

    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            Button(action: onPrevious) {
                Image(systemName: "chevron.left")
                    .font(.system(size: 9, weight: .bold))
                    .frame(width: 18, height: 18)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .foregroundStyle(currentPage == 0 ? Color.primary.opacity(0.25) : Color.primary.opacity(0.75))
            .disabled(currentPage == 0)
            .help("Previous page")

            if showsDots {
                HStack(spacing: 4) {
                    ForEach(0..<pageCount, id: \.self) { page in
                        Circle()
                            .fill(page == currentPage ? Color.accentColor : Color.primary.opacity(0.18))
                            .frame(width: page == currentPage ? 6 : 4, height: page == currentPage ? 6 : 4)
                            .animation(.easeInOut(duration: 0.15), value: currentPage)
                    }
                }
                .frame(minWidth: 34)
                .help("\(totalCount) items · page \(currentPage + 1) of \(pageCount)")
            } else {
                Text("\(currentPage + 1)/\(pageCount)")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                    .frame(minWidth: 34)
                    .help("\(totalCount) items")
            }

            Button(action: onNext) {
                Image(systemName: "chevron.right")
                    .font(.system(size: 9, weight: .bold))
                    .frame(width: 18, height: 18)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .foregroundStyle(currentPage >= pageCount - 1 ? Color.primary.opacity(0.25) : Color.primary.opacity(0.75))
            .disabled(currentPage >= pageCount - 1)
            .help("Next page")
        }
        .padding(.horizontal, 7)
        .padding(.vertical, 4)
        .appGlassChip()
        .fixedSize()
    }
}

private struct ModelCostDistributionChart: View {
    let slices: [ModelCostSlice]
    let legendSlices: [ModelCostSlice]
    let totalCost: Decimal
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController

    var body: some View {
        if slices.isEmpty {
            VStack(spacing: 8) {
                Image(systemName: "chart.pie")
                    .font(.title2)
                    .foregroundStyle(.secondary)
                Text("No model cost data")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            HStack(alignment: .center, spacing: 32) {
                ZStack {
                    ModelCostSectorChart(slices: slices, formatCost: { currencyController.string(fromUSD: $0) })
                        .zIndex(1)

                    VStack(spacing: 4) {
                        Text("Total")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(currencyController.string(fromUSD: totalCost))
                            .font(.system(size: 20, weight: .semibold, design: .monospaced))
                            .lineLimit(1)
                            .minimumScaleFactor(0.7)
                    }
                    .frame(width: 116)
                    .zIndex(0)
                }
                .frame(width: 220, height: 220)
                .frame(maxWidth: .infinity, maxHeight: .infinity)

                ModelCostLegend(
                    slices: legendSlices,
                    currencyController: currencyController
                )
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

private struct ModelCostSectorChart: View {
    let slices: [ModelCostSlice]
    let formatCost: (Decimal) -> String
    @State private var hoveredSlice: ModelCostSlice?
    @State private var hoveredPoint: CGPoint?

    var body: some View {
        Chart(slices) { slice in
            SectorMark(
                angle: .value("Cost", slice.cost.doubleValue),
                innerRadius: .ratio(0.62),
                angularInset: 1.6
            )
            .cornerRadius(6)
            .foregroundStyle(slice.color)
            .opacity(hoveredSlice == nil || hoveredSlice?.id == slice.id ? 1 : 0.55)
        }
        .chartLegend(.hidden)
        .chartBackground { _ in Color.clear }
        .chartOverlay { proxy in
            GeometryReader { geometry in
                ZStack(alignment: .topLeading) {
                    Rectangle()
                        .fill(.clear)
                        .contentShape(Rectangle())
                        .onContinuousHover { phase in
                            switch phase {
                            case .active(let point):
                                hoveredSlice = slice(at: point, proxy: proxy, geometry: geometry)
                                hoveredPoint = hoveredSlice == nil ? nil : point
                            case .ended:
                                hoveredSlice = nil
                                hoveredPoint = nil
                            }
                        }

                    if let slice = hoveredSlice, let point = hoveredPoint {
                        ChartTooltipPanel(
                            title: displayModelName(slice.model),
                            rows: [
                                ChartTooltipRow("Cost", formatCost(slice.cost), color: slice.color),
                                ChartTooltipRow("Share", slice.percentText, emphasized: true),
                            ]
                        )
                        .position(tooltipPosition(for: point, in: geometry.size))
                        .zIndex(1)
                    }
                }
                .animation(nil, value: hoveredSlice?.id)
            }
        }
        .transaction { transaction in
            transaction.animation = nil
        }
        .chartPlotStyle { plot in
            plot
                .frame(width: 220, height: 220)
        }
    }

    private func slice(at point: CGPoint, proxy: ChartProxy, geometry: GeometryProxy) -> ModelCostSlice? {
        guard let plotFrame = proxy.plotFrame else {
            return nil
        }

        let plotRect = geometry[plotFrame]
        let vectorX = point.x - plotRect.midX
        let vectorY = point.y - plotRect.midY
        let radius = hypot(vectorX, vectorY)
        let outerRadius = min(plotRect.width, plotRect.height) / 2
        let innerRadius = outerRadius * 0.62
        guard radius >= innerRadius, radius <= outerRadius else {
            return nil
        }

        let total = slices.reduce(0.0) { $0 + $1.cost.doubleValue }
        guard total > 0 else {
            return nil
        }

        let rawAngle = atan2(vectorX, -vectorY)
        let angle = rawAngle >= 0 ? rawAngle : rawAngle + (2 * .pi)
        let target = Double(angle / (2 * .pi)) * total

        var lowerBound = 0.0
        for slice in slices {
            let upperBound = lowerBound + slice.cost.doubleValue
            if target >= lowerBound, target <= upperBound {
                return slice
            }
            lowerBound = upperBound
        }

        return nil
    }

    private func tooltipPosition(for point: CGPoint, in size: CGSize) -> CGPoint {
        let tooltipWidth = chartTooltipWidth
        let tooltipHeight = chartTooltipHeight
        let horizontalPadding = 10.0
        let verticalPadding = 10.0

        let x = min(
            max(point.x + 18 + tooltipWidth / 2, tooltipWidth / 2 + horizontalPadding),
            max(tooltipWidth / 2 + horizontalPadding, size.width - tooltipWidth / 2 - horizontalPadding)
        )
        let preferredY = point.y - 18 - tooltipHeight / 2
        let fallbackY = point.y + 18 + tooltipHeight / 2
        let unclampedY = preferredY >= tooltipHeight / 2 + verticalPadding ? preferredY : fallbackY
        let y = min(
            max(unclampedY, tooltipHeight / 2 + verticalPadding),
            max(tooltipHeight / 2 + verticalPadding, size.height - tooltipHeight / 2 - verticalPadding)
        )

        return CGPoint(x: x, y: y)
    }
}

private struct ModelCostLegend: View {
    let slices: [ModelCostSlice]
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController

    var body: some View {
        VStack(alignment: .leading, spacing: modelDistributionLegendRowSpacing) {
            ForEach(slices) { slice in
                legendItem(slice)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
    }

    private func legendItem(_ slice: ModelCostSlice) -> some View {
        let modelLabel = displayModelName(slice.model)
        return HStack(alignment: .firstTextBaseline, spacing: 8) {
            Circle()
                .fill(slice.color)
                .frame(width: 9, height: 9)
            ProviderIconBadge(modelName: slice.model)

            VStack(alignment: .leading, spacing: 2) {
                Text("\(modelLabel) (\(slice.percentText))")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(currencyController.string(fromUSD: slice.cost))
                    .font(.system(size: 12, weight: .medium, design: .monospaced))
                    .foregroundStyle(.primary)
            }
        }
        .frame(
            maxWidth: .infinity,
            minHeight: modelDistributionLegendRowHeight,
            maxHeight: modelDistributionLegendRowHeight,
            alignment: .leading
        )
    }
}

private struct ModelCostRow: Identifiable {
    var id: String { model }
    let model: String
    let cost: Decimal
}

private struct ModelCostSlice: Identifiable {
    var id: String { model }
    let model: String
    let cost: Decimal
    let percent: Double
    let color: Color

    var percentText: String {
        String(format: "%.1f%%", percent * 100)
    }
}

private extension TodaySummaryResponse {
    var totalCostDecimal: Decimal {
        Decimal(totalCost)
    }

    var inputShare: Double {
        share(inputTokens)
    }

    var outputShare: Double {
        share(outputTokens)
    }

    var cacheReadShare: Double {
        share(cacheReadTokens)
    }

    var cacheShare: Double {
        share(cacheCreationTokens + cacheReadTokens)
    }

    var maxSourceTokens: Int {
        sourceRows.map(\.totalTokens).max() ?? 0
    }

    var modelTokenRows: [TodayModelTokenRow] {
        let grouped = modelRows.reduce(into: [String: Int]()) { totals, row in
            totals[row.modelName, default: 0] += row.totalTokens
        }
        let total = grouped.values.reduce(0, +)
        guard total > 0 else { return [] }

        return grouped
            .map { modelName, tokens in (modelName: modelName, tokens: tokens) }
            .sorted {
                if $0.tokens == $1.tokens {
                    return $0.modelName < $1.modelName
                }
                return $0.tokens > $1.tokens
            }
            .map { row in
                TodayModelTokenRow(
                    modelName: row.modelName,
                    tokens: row.tokens,
                    percent: Double(row.tokens) / Double(total),
                    color: stableModelColor(for: row.modelName)
                )
            }
    }

    private func share(_ tokens: Int) -> Double {
        guard totalTokens > 0 else { return 0 }
        return Double(tokens) / Double(totalTokens)
    }
}

private extension TodaySourceUsageRow {
    var totalCostDecimal: Decimal {
        Decimal(totalCost)
    }

    var cacheShare: Double {
        guard totalTokens > 0 else { return 0 }
        return Double(cacheCreationTokens + cacheReadTokens) / Double(totalTokens)
    }
}

private struct TodayModelTokenRow: Identifiable {
    var id: String { modelName }
    let modelName: String
    let tokens: Int
    let percent: Double
    let color: Color

    var percentText: String {
        percent.percentText
    }
}

private extension TodayModelUsageRow {
    var dashboardID: String {
        "\(source.rawValue)-\(modelName)"
    }

    var totalCostDecimal: Decimal {
        Decimal(totalCost)
    }

    var cacheShare: Double {
        guard totalTokens > 0 else { return 0 }
        return Double(cacheCreationTokens + cacheReadTokens) / Double(totalTokens)
    }
}

private struct TokenTrendRow: Identifiable {
    var id: String { "\(date.timeIntervalSince1970)-\(series)" }
    let date: Date
    let series: String
    let tokens: Int
}

private enum TokenCompositionKind: String, CaseIterable {
    case input = "Input"
    case output = "Output"
    case cacheCreation = "Cache Creation"
    case cacheRead = "Cache Read"

    var label: String { rawValue }

    var sortOrder: Int {
        switch self {
        case .input: 0
        case .output: 1
        case .cacheCreation: 2
        case .cacheRead: 3
        }
    }

    static let foregroundStyles: KeyValuePairs<String, Color> = [
        TokenCompositionKind.input.label: .blue,
        TokenCompositionKind.output.label: .green,
        TokenCompositionKind.cacheCreation.label: .orange,
        TokenCompositionKind.cacheRead.label: .purple,
    ]
}

private struct TokenCompositionRow: Identifiable {
    var id: String { "\(date.timeIntervalSince1970)-\(kind.rawValue)" }
    let date: Date
    let kind: TokenCompositionKind
    let tokens: Int
}

private struct TokenCompositionTotals {
    let date: Date
    var inputTokens = 0
    var outputTokens = 0
    var cacheCreationTokens = 0
    var cacheReadTokens = 0

    mutating func add(_ record: TokenUsageRecord) {
        inputTokens += record.inputTokens
        outputTokens += record.outputTokens
        cacheCreationTokens += record.cacheCreationTokens
        cacheReadTokens += record.cacheReadTokens
    }

    var rows: [TokenCompositionRow] {
        [
            TokenCompositionRow(date: date, kind: .input, tokens: inputTokens),
            TokenCompositionRow(date: date, kind: .output, tokens: outputTokens),
            TokenCompositionRow(date: date, kind: .cacheRead, tokens: cacheReadTokens),
        ]
    }
}

private let fullTokenCountFormatter: NumberFormatter = {
    let formatter = NumberFormatter()
    formatter.numberStyle = .decimal
    formatter.locale = Locale(identifier: "en_US")
    formatter.usesGroupingSeparator = true
    formatter.groupingSeparator = ","
    return formatter
}()

private extension Int {
    var tokenText: String {
        if self >= 1_000_000_000 { return String(format: "%.2fB", Double(self) / 1_000_000_000) }
        if self >= 1_000_000 { return String(format: "%.2fM", Double(self) / 1_000_000) }
        if self >= 1_000 { return String(format: "%.1fK", Double(self) / 1_000) }
        return formatted()
    }

    var fullTokenText: String {
        fullTokenCountFormatter.string(from: NSNumber(value: self)) ?? formatted()
    }
}

private extension Double {
    var tokenAxisText: String {
        if self >= 1_000_000_000 { return String(format: "%.1fB", self / 1_000_000_000) }
        if self >= 1_000_000 { return String(format: "%.1fM", self / 1_000_000) }
        if self >= 1_000 { return String(format: "%.1fK", self / 1_000) }
        return String(Int(self.rounded()))
    }

    var percentText: String {
        String(format: "%.1f%%", self * 100)
    }
}

private extension TokenUsageModelBreakdown {
    var totalTokens: Int {
        inputTokens + outputTokens + cacheCreationTokens + cacheReadTokens
    }
}

/// Maps `claude-{family}-4.8` / `claude-{family}-4-8` to `claude-{family}-4-8`.
private func canonicalClaude4xModelName(_ modelName: String) -> String? {
    let lower = modelName.lowercased()
    guard let range = lower.range(of: "claude-") else { return nil }
    let segment = String(lower[range.lowerBound...])
    let afterClaude = segment.dropFirst("claude-".count)
    guard let dash = afterClaude.firstIndex(of: "-") else { return nil }
    let family = afterClaude[..<dash]
    guard !family.isEmpty else { return nil }
    let version = afterClaude[afterClaude.index(after: dash)...]
    let minor: String
    let suffix: Substring
    if version.hasPrefix("4.") {
        let rest = version.dropFirst(2)
        minor = String(rest.prefix(while: { $0.isNumber }))
        suffix = rest.dropFirst(minor.count)
    } else if version.hasPrefix("4-") {
        let rest = version.dropFirst(2)
        minor = String(rest.prefix(while: { $0.isNumber }))
        suffix = rest.dropFirst(minor.count)
    } else {
        return nil
    }
    guard !minor.isEmpty else { return nil }
    let prefix = modelName[..<range.lowerBound]
    return "\(prefix)claude-\(family)-4-\(minor)\(suffix)".lowercased()
}

func displayModelName(_ modelName: String) -> String {
    let stripped = modelName.split(separator: "/").last.map(String.init) ?? modelName
    let normalized = stripped.lowercased()
    let displayName: String
    if normalized.contains("ark-code-latest") || normalized == "ark-code" {
        displayName = "glm-5.2"
    } else if normalized.contains("glm-5.2") || normalized.contains("glm-5-2") {
        displayName = "glm-5.2"
    } else if normalized.contains("step-3.7-flash") {
        displayName = "step-3.7-flash"
    } else if normalized.contains("grok4.5")
        || normalized.contains("grok-4.5")
        || normalized.contains("grok-4-5") {
        displayName = "grok4.5"
    } else if normalized.contains("claude-fable-5") {
        displayName = "claude-fable-5"
    } else if normalized.contains("composer-2.5-fast") || normalized.contains("composer-2-5-fast") {
        displayName = "composer-2.5-fast"
    } else if normalized.contains("grok-composer-2.5-fast") || normalized.contains("grok-composer-2-5-fast") {
        displayName = "composer-2.5-fast"
    } else if normalized.contains("composer-2.5") || normalized.contains("composer-2-5") {
        displayName = "composer-2.5"
    } else if modelName.localizedCaseInsensitiveContains("kiro-claude-opus-4.7")
        || modelName.localizedCaseInsensitiveContains("kiro-claude-opus-4-7") {
        displayName = "kiro-claude-opus-4-7"
    } else if let canon = canonicalClaude4xModelName(stripped) {
        displayName = canon
    } else if stripped.hasPrefix("[pi] ") {
        displayName = String(stripped.dropFirst(5))
    } else {
        displayName = stripped
    }
    return displayName.lowercased()
}

private extension Decimal {
    var doubleValue: Double {
        NSDecimalNumber(decimal: self).doubleValue
    }
}

extension TokenUsageSource {
    var usageSource: UsageSource {
        switch self {
        case .all, .claude: .claude
        case .codex: .codex
        case .opencode: .opencode
        case .hermes: .hermes
        case .openclaw: .openclaw
        case .pi: .pi
        case .grok: .grok
        case .cursor: .cursor
        case .cherry: .cherry
        case .claudeScience: .claudeScience
        case .zcode: .zcode
        case .kimi: .kimi
        }
    }

    var tintColor: Color {
        tokenTrendSourceColors[label] ?? .blue
    }

    var systemImage: String {
        switch self {
        case .all: "square.grid.2x2"
        case .claude: "terminal"
        case .codex: "curlybraces"
        case .opencode: "chevron.left.forwardslash.chevron.right"
        case .hermes: "paperplane"
        case .openclaw: "hammer"
        case .pi: "p.circle"
        case .grok: "sparkles"
        case .cursor: "cursorarrow.rays"
        case .cherry: "leaf"
        case .claudeScience: "flask"
        case .zcode: "bolt.horizontal.circle"
        case .kimi: "moon.stars.fill"
        }
    }

    var imageAssetName: String? {
        switch self {
        case .all: nil
        case .claude: "anthropic-mark"
        case .codex: "codex-mark"
        case .opencode: "opencode-mark"
        case .hermes: "hermes-mark"
        case .openclaw: "openclaw-mark"
        case .pi: "pi-mark"
        case .grok: "grok-mark"
        case .cursor: "cursor-mark"
        case .cherry: "cherrystudio-mark"
        case .claudeScience: "anthropic-mark"
        case .zcode: "zai-mark"
        case .kimi: "kimi-mark"
        }
    }

    var iconBadgeBackgroundColor: Color {
        switch self {
        case .codex, .opencode, .cherry:
            .clear
        default:
            tintColor.opacity(0.12)
        }
    }
}

extension UsageSource {
    var label: String {
        displayName
    }

    var tintColor: Color {
        tokenTrendSourceColors[label] ?? .blue
    }

    var systemImage: String {
        switch self {
        case .claude: "terminal"
        case .codex: "curlybraces"
        case .opencode: "chevron.left.forwardslash.chevron.right"
        case .hermes: "paperplane"
        case .openclaw: "hammer"
        case .pi: "p.circle"
        case .grok: "sparkles"
        case .cursor: "cursorarrow.rays"
        case .cherry: "leaf"
        case .claudeScience: "flask"
        case .zcode: "bolt.horizontal.circle"
        case .kimi: "moon.stars.fill"
        }
    }

    var imageAssetName: String? {
        switch self {
        case .claude: "anthropic-mark"
        case .codex: "codex-mark"
        case .opencode: "opencode-mark"
        case .hermes: "hermes-mark"
        case .openclaw: "openclaw-mark"
        case .pi: "pi-mark"
        case .grok: "grok-mark"
        case .cursor: "cursor-mark"
        case .cherry: "cherrystudio-mark"
        case .claudeScience: "anthropic-mark"
        case .zcode: "zai-mark"
        case .kimi: "kimi-mark"
        }
    }

    var iconBadgeBackgroundColor: Color {
        switch self {
        case .codex, .opencode, .cherry:
            .clear
        default:
            tintColor.opacity(0.12)
        }
    }

    init?(label: String) {
        guard let source = Self.allCases.first(where: { $0.displayName == label }) else {
            return nil
        }
        self = source
    }
}

final class TokenUsageDashboardMockStore: TokenUsageDashboardProviding {
    @Published var selectedSource: TokenUsageSource = .all
    @Published var selectedViewMode: TokenUsageViewMode = .daily
    @Published var startDate: Date
    @Published var endDate: Date
    @Published var selectedModels: Set<String> = []
    @Published private(set) var records: [TokenUsageRecord]
    @Published private(set) var todaySummary: TodaySummaryResponse
    @Published private(set) var todayHourly: HourlyUsageResponse?
    @Published private(set) var hourlyByDate: [String: HourlyUsageResponse] = [:]
    @Published private(set) var todayBrief: TodayBriefResponse?
    @Published private(set) var briefCache: [String: TodayBriefResponse] = [:]
    @Published private(set) var briefMissingDates: Set<String> = []
    @Published private(set) var briefDays: [BriefDayEntry] = []
    @Published private(set) var briefMonths: [BriefMonthEntry] = []
    @Published private(set) var isLoading = false
    @Published private(set) var isGeneratingBrief = false
    @Published private(set) var briefErrorMessage: String?
    @Published private(set) var isBackendConnected = true

    init(
        startDate: Date = Calendar.current.date(byAdding: .day, value: -14, to: Date()) ?? Date(),
        endDate: Date = Calendar.current.date(byAdding: .day, value: 1, to: Date()) ?? Date(),
        records: [TokenUsageRecord]? = nil
    ) {
        self.startDate = startDate
        self.endDate = endDate
        self.records = records ?? Self.makePreviewRecords(startDate: startDate)
        self.todaySummary = Self.makePreviewTodaySummary(
            records: records ?? Self.makePreviewRecords(startDate: Calendar.current.startOfDay(for: Date()))
        )
        self.todayHourly = Self.makePreviewHourly()
        self.todayBrief = Self.makePreviewBrief()
    }

    private static func makePreviewHourly() -> HourlyUsageResponse {
        let sources: [UsageSource] = [.claude, .codex, .opencode]
        let baseByHour: [Int: Int] = [
            8: 2_000_000, 9: 6_500_000, 10: 11_000_000, 11: 8_000_000,
            12: 3_500_000, 13: 5_000_000, 14: 12_500_000, 15: 9_000_000,
            16: 7_500_000, 17: 4_000_000, 18: 1_500_000,
        ]
        let hours = baseByHour.flatMap { hour, base in
            sources.enumerated().map { index, source in
                let total = base / (index + 2)
                let input = total / 5
                let output = total / 20
                let cacheRead = total - input - output
                return HourlyUsageRow(
                    hour: hour,
                    source: source,
                    inputTokens: input,
                    outputTokens: output,
                    cacheCreationTokens: 0,
                    cacheReadTokens: cacheRead,
                    totalTokens: total,
                    totalCost: Double(total) / 1_000_000 * 3.2
                )
            }
        }
        return HourlyUsageResponse(date: todayFormatter.string(from: Date()), hours: hours)
    }

    private static func makePreviewBrief() -> TodayBriefResponse {
        TodayBriefResponse(
            date: todayFormatter.string(from: Date()),
            status: "ok",
            generatedAt: "",
            trigger: "preview",
            model: BriefModelInfo(baseUrl: "", modelId: "preview"),
            enabledSources: ["claude", "codex"],
            contentFingerprint: "preview",
            summary: "3 个项目：claude·token-usage；codex·backend；kimi·token-usage",
            cards: nil,
            sections: nil,
            error: nil,
            hours: [
                HourlyBriefItem(hour: 9, headline: "晨间集中重构认证模块", sessionCount: 3, tokens: 6_500_000),
                HourlyBriefItem(hour: 10, headline: "API 集成与联调", sessionCount: 2, tokens: 11_000_000),
                HourlyBriefItem(hour: 14, headline: "修复 token 统计的边界问题", sessionCount: 4, tokens: 12_500_000),
                HourlyBriefItem(hour: 16, headline: "Daily Brief 卡片联调", sessionCount: 2, tokens: 7_500_000),
            ]
        )
    }

    func refresh() async {
        isLoading = true
        isLoading = false
    }

    func refreshToday() async {
        isLoading = true
        isLoading = false
    }

    func refreshDashboard(force: Bool) async {
        await refresh()
    }

    func refreshToday(force: Bool) async {
        await refreshToday()
    }

    func refreshTodayBrief() async {}

    func generateTodayBrief(force: Bool, trigger: String) async {}

    func loadBrief(for date: String) async {}

    func loadHourly(for date: String) async {}

    func loadBriefDays(month: String) async {}

    func loadBriefMonths() async {}

    func generateBrief(for date: String, mode: BriefRegenerateMode) async {}

    func updateDateRangeForViewMode() {
        // Mock: no-op
    }

    private static func makePreviewRecords(startDate: Date) -> [TokenUsageRecord] {
        let calendar = Calendar.current
        let sources: [TokenUsageSource] = [.claude, .codex, .opencode, .grok]
        let models = ["claude-sonnet-4", "gpt-5.2-codex", "qwen3-coder", "grok-composer-2.5-fast"]

        return (0..<14).flatMap { dayOffset in
            sources.enumerated().map { sourceIndex, source in
                let date = calendar.date(byAdding: .day, value: dayOffset, to: startDate) ?? startDate
                let input = 120_000 + (dayOffset * 12_000) + (sourceIndex * 9_000)
                let output = 32_000 + (dayOffset * 4_000) + (sourceIndex * 3_000)
                let cacheCreate = 8_000 + (sourceIndex * 1_500)
                let cacheRead = 24_000 + (dayOffset * 2_000)
                let total = input + output + cacheCreate + cacheRead
                let cost = Decimal(total) / Decimal(1_000_000) * Decimal(string: "3.20")!
                let model = models[sourceIndex]

                return TokenUsageRecord(
                    id: "\(source.rawValue)-\(dayOffset)",
                    source: source,
                    viewMode: .daily,
                    date: date,
                    inputTokens: input,
                    outputTokens: output,
                    cacheCreationTokens: cacheCreate,
                    cacheReadTokens: cacheRead,
                    totalTokens: total,
                    totalCost: cost,
                    modelsUsed: [model],
                    modelBreakdowns: [
                        TokenUsageModelBreakdown(
                            modelName: model,
                            inputTokens: input,
                            outputTokens: output,
                            cacheCreationTokens: cacheCreate,
                            cacheReadTokens: cacheRead,
                            cost: cost
                        )
                    ]
                )
            }
        }
    }

    private static func makePreviewTodaySummary(records: [TokenUsageRecord]) -> TodaySummaryResponse {
        let calendar = Calendar.current
        let todayRecords = records.filter { record in
            record.viewMode == .daily && calendar.isDateInToday(record.date)
        }
        let inputTokens = todayRecords.reduce(0) { $0 + $1.inputTokens }
        let outputTokens = todayRecords.reduce(0) { $0 + $1.outputTokens }
        let cacheCreationTokens = todayRecords.reduce(0) { $0 + $1.cacheCreationTokens }
        let cacheReadTokens = todayRecords.reduce(0) { $0 + $1.cacheReadTokens }
        let totalTokens = todayRecords.reduce(0) { $0 + $1.totalTokens }
        let totalCost = todayRecords.reduce(Decimal.zero) { $0 + $1.totalCost }
        let modelNames = Set(todayRecords.flatMap { $0.modelsUsed.isEmpty ? ["Unknown"] : $0.modelsUsed })
        let sourceRows = makePreviewSourceRows(records: todayRecords)

        return TodaySummaryResponse(
            date: Self.todayFormatter.string(from: Date()),
            inputTokens: inputTokens,
            outputTokens: outputTokens,
            cacheCreationTokens: cacheCreationTokens,
            cacheReadTokens: cacheReadTokens,
            totalTokens: totalTokens,
            totalCost: totalCost.doubleValue,
            activeSourceCount: sourceRows.count,
            modelCount: modelNames.count,
            sourceRows: sourceRows,
            modelRows: makePreviewModelRows(records: todayRecords)
        )
    }

    private static func makePreviewSourceRows(records: [TokenUsageRecord]) -> [TodaySourceUsageRow] {
        let grouped = Dictionary(grouping: records, by: \.source)

        return grouped.map { source, records in
            TodaySourceUsageRow(
                source: source.usageSource,
                inputTokens: records.reduce(0) { $0 + $1.inputTokens },
                outputTokens: records.reduce(0) { $0 + $1.outputTokens },
                cacheCreationTokens: records.reduce(0) { $0 + $1.cacheCreationTokens },
                cacheReadTokens: records.reduce(0) { $0 + $1.cacheReadTokens },
                totalTokens: records.reduce(0) { $0 + $1.totalTokens },
                totalCost: records.reduce(Decimal.zero) { $0 + $1.totalCost }.doubleValue,
                modelCount: Set(records.flatMap { $0.modelsUsed.isEmpty ? ["Unknown"] : $0.modelsUsed }).count
            )
        }
        .sorted { $0.totalTokens > $1.totalTokens }
    }

    private static func makePreviewModelRows(records: [TokenUsageRecord]) -> [TodayModelUsageRow] {
        records.flatMap { record in
            let breakdowns = record.modelBreakdowns
            if breakdowns.isEmpty {
                return [
                    TodayModelUsageRow(
                        source: record.source.usageSource,
                        modelName: record.modelsUsed.first ?? "Unknown",
                        inputTokens: record.inputTokens,
                        outputTokens: record.outputTokens,
                        cacheCreationTokens: record.cacheCreationTokens,
                        cacheReadTokens: record.cacheReadTokens,
                        totalTokens: record.totalTokens,
                        totalCost: record.totalCost.doubleValue
                    )
                ]
            }

            return breakdowns.map { breakdown in
                TodayModelUsageRow(
                    source: record.source.usageSource,
                    modelName: breakdown.modelName,
                    inputTokens: breakdown.inputTokens,
                    outputTokens: breakdown.outputTokens,
                    cacheCreationTokens: breakdown.cacheCreationTokens,
                    cacheReadTokens: breakdown.cacheReadTokens,
                    totalTokens: breakdown.totalTokens,
                    totalCost: breakdown.cost.doubleValue
                )
            }
        }
        .sorted { $0.totalCost > $1.totalCost }
    }

    private static let todayFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter
    }()
}

struct TokenUsageDashboardView_Previews: PreviewProvider {
    static var previews: some View {
        TokenUsageDashboardView(
            store: TokenUsageDashboardMockStore(),
            currencyController: TokenUsageBillingCurrencyController(),
            preferences: TokenUsagePreferencesController()
        )
    }
}

/// Formatters shared by the day drill-down. File scope because stored statics
/// are not allowed inside the generic dashboard view.
private enum DayDrillDownFormatters {
    static let dayString: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter
    }()

    static let dayTitle: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale.current
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter
    }()
}
