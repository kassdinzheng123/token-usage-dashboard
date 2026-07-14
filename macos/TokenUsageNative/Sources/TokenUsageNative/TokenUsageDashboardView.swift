import Charts
import AppKit
import SwiftUI

// MARK: - Cohesive Color Family System
// Each color family provides primary / light / dark variants for visual consistency.
// Sources, providers, and chart palettes all derive from these families.

private struct ColorFamily {
    let primary: Color
    let light: Color
    let dark: Color
}

private let colorRose    = ColorFamily(primary: Color(red: 0.85, green: 0.38, blue: 0.30), light: Color(red: 0.95, green: 0.72, blue: 0.68), dark: Color(red: 0.65, green: 0.22, blue: 0.18))
private let colorOcean   = ColorFamily(primary: Color(red: 0.18, green: 0.46, blue: 0.85), light: Color(red: 0.55, green: 0.72, blue: 0.95), dark: Color(red: 0.10, green: 0.30, blue: 0.65))
private let colorAmber   = ColorFamily(primary: Color(red: 0.88, green: 0.62, blue: 0.18), light: Color(red: 0.96, green: 0.85, blue: 0.58), dark: Color(red: 0.70, green: 0.46, blue: 0.08))
private let colorEmerald = ColorFamily(primary: Color(red: 0.15, green: 0.62, blue: 0.42), light: Color(red: 0.52, green: 0.85, blue: 0.70), dark: Color(red: 0.06, green: 0.42, blue: 0.28))
private let colorViolet  = ColorFamily(primary: Color(red: 0.52, green: 0.35, blue: 0.90), light: Color(red: 0.76, green: 0.68, blue: 0.98), dark: Color(red: 0.38, green: 0.20, blue: 0.72))
private let colorTeal    = ColorFamily(primary: Color(red: 0.12, green: 0.58, blue: 0.72), light: Color(red: 0.48, green: 0.82, blue: 0.90), dark: Color(red: 0.05, green: 0.40, blue: 0.52))
private let colorCoral   = ColorFamily(primary: Color(red: 0.92, green: 0.38, blue: 0.30), light: Color(red: 0.98, green: 0.70, blue: 0.64), dark: Color(red: 0.72, green: 0.22, blue: 0.16))
private let colorSlate   = ColorFamily(primary: Color(red: 0.38, green: 0.42, blue: 0.52), light: Color(red: 0.68, green: 0.72, blue: 0.80), dark: Color(red: 0.22, green: 0.25, blue: 0.34))

private let allColorFamilies: [ColorFamily] = [
    colorRose, colorOcean, colorAmber, colorEmerald, colorViolet, colorTeal, colorCoral, colorSlate
]

// Source -> Color Family mapping
private let tokenTrendSourceColors: [String: Color] = [
    TokenUsageSource.claude.label:   colorRose.primary,
    TokenUsageSource.codex.label:    colorEmerald.primary,
    TokenUsageSource.opencode.label: colorTeal.primary,
    TokenUsageSource.hermes.label:   colorViolet.primary,
    TokenUsageSource.openclaw.label: colorAmber.primary,
    TokenUsageSource.pi.label:       colorCoral.primary,
    TokenUsageSource.grok.label:     colorSlate.primary,
    TokenUsageSource.cursor.label: colorSlate.primary,
    TokenUsageSource.cherry.label: colorRose.light,
    TokenUsageSource.claudeScience.label: colorOcean.primary,
    TokenUsageSource.zcode.label: colorOcean.light,
]

// Model trend chart palette (single-source view): primary shades from each family
private let tokenTrendChartPalette: [Color] = allColorFamilies.map(\.primary)

// Model cost chart palette: primary + light alternation for visual richness
private let modelCostPalette: [Color] = allColorFamilies.flatMap { [$0.primary, $0.light] }

private let chartTooltipWidth = 190.0
private let chartTooltipHeight = 86.0
private let maximumBarWidth: CGFloat = 96.0
private let dailyUsagePlotHeight: CGFloat = 220
private let dailyUsageContentHeight: CGFloat = 280
private let tokenTrendLegendPageSize = 6
private let modelCostLegendPageSize = 5
private let cliConsumptionPageSize = 5
private let todayModelPageSize = 10
private let allRangeBarPageSize = 60
private let todayOverviewContentHeight = 340.0
private let todaySourceRowHeight = 76.0
private let todaySourceRowSpacing = 10.0

private let dashboardMonthFormatter: DateFormatter = {
    let formatter = DateFormatter()
    formatter.calendar = Calendar(identifier: .gregorian)
    formatter.locale = Locale.current
    return formatter
}()

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
        case .mix: "Token Mix"
        }
    }
}

// Expanded stable model palette: primary/light/dark from every family.
private let expandedModelPalette: [Color] = allColorFamilies.flatMap { [$0.primary, $0.light, $0.dark] }

/// Deterministic color for a model, hashed from its display name so a model
/// always keeps the same color regardless of ordering or filters.
private func stableModelColor(for modelName: String) -> Color {
    let key = displayModelName(modelName)
    var hash: UInt64 = 14695981039346656037
    for byte in key.utf8 {
        hash = (hash ^ UInt64(byte)) &* 1099511628211
    }
    return expandedModelPalette[Int(hash % UInt64(expandedModelPalette.count))]
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
    var isLoading: Bool { get }
    var isBackendConnected: Bool { get }

    func refresh() async
    func refreshToday() async
    func refreshDashboard(force: Bool) async
    func refreshToday(force: Bool) async
    func updateDateRangeForViewMode()
}

struct TokenUsageDashboardView<Store: TokenUsageDashboardProviding>: View {
    @ObservedObject private var store: Store
    @ObservedObject private var currencyController: TokenUsageBillingCurrencyController
    @StateObject private var backendLogs = BackendLogStore.shared
    @State private var modelSearchText = ""
    @State private var isModelFilterExpanded = false
    @State private var tokenTrendLegendPage = 0
    @State private var modelCostLegendPage = 0
    @State private var tokenMixLegendPage = 0
    @State private var cliConsumptionPage = 0
    @State private var todayModelPage = 0
    @State private var allRangeChartPage = 0
    @State private var heatmapPage = 0
    @State private var isViewModeTransitioning = false
    @State private var viewModeTransitionGeneration = 0
    @State private var viewModeTransitionTask: Task<Void, Never>?
    @State private var dateRangeRefreshSuppressionGeneration = 0
    @State private var isLogViewerPresented = false
    @State private var selectedTimeRange: DashboardTimeRange = .today
    @State private var cliCompositionPane: DashboardCLICompositionPane = .cli
    @State private var modelCostMixPane: DashboardModelCostMixPane = .cost
    @State private var dashboardData: TokenUsageDashboardData

    init(store: Store, currencyController: TokenUsageBillingCurrencyController) {
        self.store = store
        self.currencyController = currencyController
        _dashboardData = State(initialValue: Self.makeDashboardData(from: store, timeRange: .today))
    }

    public var body: some View {
        applyDashboardSecondaryHandlers(
            to: applyDashboardPrimaryHandlers(
                to: applyDashboardTasks(to: dashboardRoot)
            )
        )
    }

    private var dashboardRoot: some View {
        ZStack {
            dashboardContent
                .disabled(isViewModeTransitioning)
                .opacity(isViewModeTransitioning ? 0.55 : 1)
                .transaction { transaction in
                    transaction.animation = nil
                }

            if isViewModeTransitioning {
                viewModeTransitionOverlay
                    .transition(.opacity.combined(with: .scale(scale: 0.98)))
                    .zIndex(1)
            }

            if isLogViewerPresented {
                logViewerOverlay
                    .transition(.opacity.combined(with: .scale(scale: 0.98)))
                    .zIndex(2)
            }
        }
        .frame(minWidth: 960, maxWidth: .infinity, minHeight: 680, maxHeight: .infinity, alignment: .topLeading)
        .background(Color(nsColor: .windowBackgroundColor))
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
                await currencyController.refreshExchangeRateIfNeeded()
            }
            .onDisappear {
                viewModeTransitionTask?.cancel()
                viewModeTransitionTask = nil
                isViewModeTransitioning = false
                isLogViewerPresented = false
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
            .onChange(of: tokenTrendColorDomain) {
                tokenTrendLegendPage = 0
            }
            .onChange(of: dashboardData.modelUsageRows.count) {
                todayModelPage = 0
            }
            .onChange(of: currencyController.selectedCurrency) {
                Task { await currencyController.refreshExchangeRateIfNeeded() }
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
        timeRange: DashboardTimeRange = .today
    ) -> TokenUsageDashboardData {
        TokenUsageDashboardData.make(
            records: store.records,
            selectedSource: store.selectedSource,
            selectedViewMode: store.selectedViewMode,
            startDate: store.startDate,
            endDate: store.endDate,
            selectedModels: store.selectedModels,
            timeRange: timeRange
        )
    }

    private func updateDashboardData() {
        dashboardData = Self.makeDashboardData(
            from: store,
            timeRange: selectedTimeRange
        )
        clearChartInteractionState()
        modelCostLegendPage = 0
        tokenMixLegendPage = 0
        cliConsumptionPage = 0
    }

    private func clearChartInteractionState() {
    }

    private var dashboardContent: some View {
        ScrollView(.vertical, showsIndicators: true) {
            LazyVStack(alignment: .leading, spacing: 20) {
                header

                filterBar

                if store.records.isEmpty {
                    dashboardLoadingView
                } else {
                    heroMetricsRow
                    if selectedTimeRange != .today {
                        dailyTokenUsageSection
                    }
                    chartSection
                    modelConsumptionSection
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
        .scrollIndicators(.visible)
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

    private var viewModeTransitionOverlay: some View {
        VStack(spacing: 10) {
            ProgressView()
                .controlSize(.regular)

            Text("Loading \(store.selectedViewMode.label.lowercased()) view...")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 16)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
        )
        .shadow(color: Color.black.opacity(0.14), radius: 14, y: 8)
    }

    private var logViewerOverlay: some View {
        ZStack {
            Color.black.opacity(0.18)
                .ignoresSafeArea()
                .onTapGesture {
                    dismissLogViewer()
                }

            BackendLogViewerPanel(
                lines: backendLogs.lines,
                onClear: {
                    backendLogs.clear()
                },
                onClose: {
                    dismissLogViewer()
                }
            )
        }
    }

    private func presentLogViewer() {
        backendLogs.startTailing()
        isLogViewerPresented = true
    }

    private func dismissLogViewer() {
        isLogViewerPresented = false
        backendLogs.stopTailing()
    }

    private func refreshForViewModeChange() {
        viewModeTransitionGeneration += 1
        let generation = viewModeTransitionGeneration

        clearChartInteractionState()
        viewModeTransitionTask?.cancel()

        viewModeTransitionTask = Task { @MainActor in
            withAnimation(.easeInOut(duration: 0.16)) {
                isViewModeTransitioning = true
            }

            async let delay: Void = Self.minimumViewModeTransitionDelay()
            await store.refresh()
            await delay

            guard !Task.isCancelled, generation == viewModeTransitionGeneration else { return }

            withAnimation(.easeInOut(duration: 0.16)) {
                isViewModeTransitioning = false
            }
            viewModeTransitionTask = nil
        }
    }

    private static func minimumViewModeTransitionDelay() async {
        try? await Task.sleep(nanoseconds: 180_000_000)
    }

    private var filteredRecords: [TokenUsageRecord] {
        dashboardData.filteredRecords
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

    private var modelFilterListHeight: CGFloat {
        min(max(CGFloat(filteredAvailableModels.count) * 32, 44), 180)
    }

    private var modelSelectionSummary: String {
        store.selectedModels.isEmpty ? "All Models" : "\(store.selectedModels.count) selected"
    }



    private func sessionIDText(for record: TokenUsageRecord) -> String {
        guard store.selectedViewMode == .sessions else { return "-" }

        let sessionID = record.sessionID?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        guard !sessionID.isEmpty else { return "-" }

        return sessionID.count > 12 ? "\(sessionID.prefix(12))..." : sessionID
    }

    private var totalCost: Decimal {
        dashboardData.totalCost
    }

    private var totalTokens: Int {
        dashboardData.totalTokens
    }

    private var todaySummary: TodaySummaryResponse {
        store.todaySummary
    }

    private var activePeriodCount: Int {
        dashboardData.activePeriodCount
    }

    private var header: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Token Usage Dashboard")
                    .font(.title2.weight(.semibold))
                Text("Local token, model, and cost usage across supported sources.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            HStack(spacing: 8) {
                Button {
                    presentLogViewer()
                } label: {
                    Label("Logs", systemImage: "doc.text.magnifyingglass")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .help("View backend logs")

                Picker("Currency", selection: $currencyController.selectedCurrency) {
                    ForEach(TokenUsageBillingCurrency.allCases) { currency in
                        Text(currency.label).tag(currency)
                    }
                }
                .labelsHidden()
                .pickerStyle(.segmented)
                .frame(width: 120)
            }

            VStack(alignment: .trailing, spacing: 6) {
                Button {
                    Task { await refreshDashboardAndToday(force: true) }
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.bordered)
                .help("Refresh usage")
                .disabled(store.isLoading)

                HStack(spacing: 4) {
                    Circle()
                        .fill(store.isBackendConnected ? Color.green : Color.red)
                        .frame(width: 7, height: 7)
                    Text(store.isBackendConnected ? "Connected" : "Disconnected")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    private func refreshDashboardAndToday(force: Bool = false) async {
        await store.refreshDashboard(force: force)
        await store.refreshToday(force: force)
    }

    private func handleSelectedTimeRangeChange() {
        applyTimeRange(selectedTimeRange)
        updateDashboardData()
        tokenMixLegendPage = 0
        cliConsumptionPage = 0
        todayModelPage = 0
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

    private var filterBar: some View {
        VStack(alignment: .leading, spacing: 12) {
            filterControls

            modelFilter
        }
        .padding(16)
        .background(.background)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
        )
    }

    private var filterControls: some View {
        HStack(spacing: 16) {
            sourcePicker
            timeRangePicker
            Spacer(minLength: 0)
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

    private var sourcePicker: some View {
        Picker("Source", selection: $store.selectedSource) {
            ForEach(TokenUsageSource.allCases) { source in
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
        .frame(width: 180)
    }

    private var viewModePicker: some View {
        Picker("View", selection: $store.selectedViewMode) {
            ForEach(TokenUsageViewMode.allCases) { mode in
                Text(mode.label).tag(mode)
            }
        }
        .pickerStyle(.segmented)
        .frame(width: 280)
    }

    @ViewBuilder
    private var dateRangePickers: some View {
        HStack(spacing: 16) {
            if store.selectedViewMode == .monthly {
                monthPickerField("Month")
            } else {
                datePickerField("From", selection: $store.startDate)
                datePickerField("To", selection: $store.endDate)
            }
        }
    }

    private func datePickerField(_ title: String, selection: Binding<Date>) -> some View {
        HStack(spacing: 8) {
            Text(title)
                .frame(width: 42, alignment: .trailing)

            DatePicker("", selection: selection, displayedComponents: .date)
                .labelsHidden()
                .datePickerStyle(.compact)
                .frame(width: 190, alignment: .leading)
        }
        .frame(width: 242, alignment: .leading)
        .layoutPriority(1)
    }

    private func monthPickerField(_ title: String) -> some View {
        HStack(spacing: 8) {
            Text(title)
                .frame(width: 42, alignment: .trailing)

            Picker("Year", selection: monthYearBinding) {
                ForEach(monthYearOptions, id: \.self) { year in
                    Text(year.formatted(.number.grouping(.never))).tag(year)
                }
            }
            .labelsHidden()
            .pickerStyle(.menu)
            .frame(width: 92)

            Picker("Month", selection: monthNumberBinding) {
                ForEach(1...12, id: \.self) { month in
                    Text(dashboardMonthFormatter.monthSymbols[month - 1]).tag(month)
                }
            }
            .labelsHidden()
            .pickerStyle(.menu)
            .frame(width: 118)
        }
        .frame(width: 268, alignment: .leading)
        .layoutPriority(1)
    }

    private var monthYearOptions: [Int] {
        let calendar = Calendar.current
        let currentYear = calendar.component(.year, from: Date())
        let startYear = calendar.component(.year, from: store.startDate)
        let endYear = calendar.component(.year, from: store.endDate)
        let lowerBound = min(currentYear, startYear, endYear) - 5
        let upperBound = max(currentYear, startYear, endYear) + 1
        return Array(lowerBound...upperBound)
    }

    private var monthYearBinding: Binding<Int> {
        Binding {
            Calendar.current.component(.year, from: selectedMonthDate)
        } set: { year in
            updateSelectedMonth(year: year, month: Calendar.current.component(.month, from: selectedMonthDate))
        }
    }

    private var monthNumberBinding: Binding<Int> {
        Binding {
            Calendar.current.component(.month, from: selectedMonthDate)
        } set: { month in
            updateSelectedMonth(year: Calendar.current.component(.year, from: selectedMonthDate), month: month)
        }
    }

    private var selectedMonthDate: Date {
        monthStart(for: store.startDate)
    }

    private func updateSelectedMonth(year: Int, month: Int) {
        var components = DateComponents()
        components.calendar = Calendar.current
        components.year = year
        components.month = month
        components.day = 1

        guard let monthStart = Calendar.current.date(from: components) else {
            return
        }

        store.startDate = self.monthStart(for: monthStart)
        store.endDate = monthEnd(for: monthStart)
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

    @ViewBuilder
    private var modelFilter: some View {
        if showsModelFilter {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Button {
                        withAnimation(.easeInOut(duration: 0.16)) {
                            isModelFilterExpanded.toggle()
                        }
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: isModelFilterExpanded ? "chevron.down" : "chevron.right")
                                .font(.caption.weight(.semibold))
                                .frame(width: 12)
                            Text("Models")
                                .font(.callout.weight(.semibold))
                        }
                    }
                    .buttonStyle(.plain)
                    .contentShape(Rectangle())
                    .help(isModelFilterExpanded ? "Collapse model filters" : "Expand model filters")

                    Text(modelSelectionSummary)
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    Text("\(availableModels.count) models")
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    Spacer()

                    if !store.selectedModels.isEmpty {
                        Button("All Models") {
                            store.selectedModels.removeAll()
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                    }
                }

                if isModelFilterExpanded {
                    HStack(spacing: 8) {
                        TextField("Search models", text: $modelSearchText)
                            .textFieldStyle(.roundedBorder)
                            .frame(width: 320)

                        Button("All Models") {
                            store.selectedModels.removeAll()
                            modelSearchText = ""
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                        .disabled(store.selectedModels.isEmpty && modelSearchText.isEmpty)

                        Text("\(filteredAvailableModels.count) of \(availableModels.count) models")
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        Spacer()
                    }

                    if filteredAvailableModels.isEmpty {
                        Text("No matching models")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, minHeight: 48, alignment: .center)
                            .background(Color(nsColor: .controlBackgroundColor))
                            .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
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
                                    .padding(.horizontal, 10)
                                    .padding(.vertical, 6)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                }
                            }
                        }
                        .frame(height: modelFilterListHeight)
                        .background(Color(nsColor: .controlBackgroundColor))
                        .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 6, style: .continuous)
                                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
                        )
                    }
                }
            }
        }
    }

    private var todayDashboard: some View {
        let summary = todaySummary
        let overviewContentHeight = todayOverviewResolvedContentHeight(for: summary)

        return VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top, spacing: 16) {
                TodayTokenHeroView(
                    totalTokens: summary.totalTokens,
                    modelCount: summary.modelCount,
                    isRefreshing: store.isLoading
                )
                .frame(minWidth: 280, idealWidth: 320, maxWidth: 360, alignment: .leading)

                LazyVGrid(
                    columns: [
                        GridItem(.adaptive(minimum: 148), spacing: 12, alignment: .top),
                    ],
                    alignment: .leading,
                    spacing: 12
                ) {
                    TodayMetricCard(
                        title: "Cost",
                        value: currencyController.string(fromUSD: summary.totalCostDecimal),
                        subtitle: "\(summary.activeSourceCount) active CLI",
                        systemImage: "creditcard",
                        tint: .blue
                    )
                    TodayMetricCard(
                        title: "Cache Read",
                        value: summary.cacheReadTokens.tokenText,
                        subtitle: "\(summary.cacheReadShare.percentText) of tokens",
                        systemImage: "externaldrive.badge.icloud",
                        tint: .purple
                    )
                    TodayMetricCard(
                        title: "Cache Share",
                        value: summary.cacheShare.percentText,
                        subtitle: "read + create",
                        systemImage: "chart.pie",
                        tint: .orange
                    )
                    TodayMetricCard(
                        title: "Input",
                        value: summary.inputTokens.tokenText,
                        subtitle: summary.inputShare.percentText,
                        systemImage: "arrow.down.to.line.compact",
                        tint: .cyan
                    )
                    TodayMetricCard(
                        title: "Output",
                        value: summary.outputTokens.tokenText,
                        subtitle: summary.outputShare.percentText,
                        systemImage: "arrow.up.to.line.compact",
                        tint: .pink
                    )
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }

            Grid(horizontalSpacing: 16, verticalSpacing: 16) {
                GridRow {
                    ChartCard(title: "CLI Consumption") {
                        cliConsumptionPager(for: summary)
                    } content: {
                        todaySourceBreakdown(summary: summary)
                    }
                    .frame(height: overviewContentHeight + 66, alignment: .top)

                    ChartCard(title: "Token Mix") {
                        tokenMixLegendPager
                    } content: {
                        todayTokenMix(summary: summary)
                    }
                    .frame(height: overviewContentHeight + 66, alignment: .top)
                }

                GridRow {
                    ChartCard(title: "Model Consumption") {
                        if todayModelPageCount(for: summary) > 1 {
                            LegendPageControls(
                                currentPage: clampedTodayModelPage(for: summary),
                                pageCount: todayModelPageCount(for: summary),
                                totalCount: summary.modelRows.count,
                                onPrevious: {
                                    todayModelPage = max(clampedTodayModelPage(for: summary) - 1, 0)
                                },
                                onNext: {
                                    todayModelPage = min(
                                        clampedTodayModelPage(for: summary) + 1,
                                        todayModelPageCount(for: summary) - 1
                                    )
                                }
                            )
                        }
                    } content: {
                        todayModelBreakdown(summary: summary)
                    }
                    .gridCellColumns(2)
                }
            }
        }
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
        max(Int(ceil(Double(rows.count) / Double(modelCostLegendPageSize))), 1)
    }

    private func clampedTokenMixLegendPage(for rows: [TodayModelTokenRow]) -> Int {
        min(max(tokenMixLegendPage, 0), tokenMixLegendPageCount(for: rows) - 1)
    }

    private func visibleTokenMixRows(from rows: [TodayModelTokenRow]) -> [TodayModelTokenRow] {
        let page = clampedTokenMixLegendPage(for: rows)
        let start = page * modelCostLegendPageSize
        guard start < rows.count else { return rows }
        let end = min(start + modelCostLegendPageSize, rows.count)
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

    @ViewBuilder
    private func todayModelBreakdown(summary: TodaySummaryResponse) -> some View {
        if summary.modelRows.isEmpty {
            emptyTodayState(text: store.isLoading ? "Loading models..." : "No model usage recorded today")
        } else {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 10) {
                    Text("Model")
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Text("Source")
                        .frame(width: 120, alignment: .leading)
                    Text("Tokens")
                        .frame(width: 92, alignment: .trailing)
                    Text("Cache")
                        .frame(width: 78, alignment: .trailing)
                    Text("Cost")
                        .frame(width: 86, alignment: .trailing)
                }
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)

                ForEach(visibleTodayModelRows(for: summary), id: \.dashboardID) { row in
                    TodayModelRowView(row: row, costText: currencyController.string(fromUSD: row.totalCostDecimal))
                }
            }
            .frame(maxWidth: .infinity, minHeight: 260, alignment: .topLeading)
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
                totalTokens: totalTokens,
                costText: currencyController.string(fromUSD: totalCost),
                isRefreshing: store.isLoading
            )
            .frame(maxWidth: .infinity, minHeight: 140)

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
                    switch modelCostMixPane {
                    case .cost:
                        modelCostChart
                    case .mix:
                        todayTokenMix(summary: dashboardSummary)
                            .frame(height: 280)
                    }
                }
                .frame(height: paneHeight + 66, alignment: .top)
            }
        }
    }

    private var dailyTokenUsageSection: some View {
        HStack(alignment: .top, spacing: 20) {
            ChartCard(title: "Daily Heatmap") {
                heatmapPager
            } content: {
                GeometryReader { geometry in
                    DashboardHeatmapView(
                        days: visibleHeatmapDays,
                        availableWidth: geometry.size.width
                    )
                    .frame(width: geometry.size.width, height: dailyUsageContentHeight)
                }
                .frame(height: dailyUsageContentHeight)
            }
            .frame(maxWidth: .infinity, alignment: .topLeading)

            ChartCard(title: "Daily Token Usage") {
                allRangeBarPager
            } content: {
                tokenTrendChart
                    .frame(
                        maxWidth: .infinity,
                        minHeight: dailyUsageContentHeight,
                        maxHeight: dailyUsageContentHeight,
                        alignment: .topLeading
                    )
            }
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
    }

    private var modelConsumptionSection: some View {
        ChartCard(title: "Model Consumption") {
            if dashboardModelPageCount > 1 {
                LegendPageControls(
                    currentPage: clampedDashboardModelPage,
                    pageCount: dashboardModelPageCount,
                    totalCount: dashboardData.modelUsageRows.count,
                    onPrevious: {
                        todayModelPage = max(clampedDashboardModelPage - 1, 0)
                    },
                    onNext: {
                        todayModelPage = min(clampedDashboardModelPage + 1, dashboardModelPageCount - 1)
                    }
                )
            }
        } content: {
            dashboardModelConsumption
        }
    }

    @ViewBuilder
    private var dashboardModelConsumption: some View {
        let rows = dashboardData.modelUsageRows
        if rows.isEmpty {
            emptyTodayState(text: store.isLoading ? "Loading models..." : "No model usage recorded")
        } else {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 10) {
                    Text("Model")
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Text("Source")
                        .frame(width: 120, alignment: .leading)
                    Text("Tokens")
                        .frame(width: 92, alignment: .trailing)
                    Text("Cache")
                        .frame(width: 78, alignment: .trailing)
                    Text("Cost")
                        .frame(width: 86, alignment: .trailing)
                }
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)

                ForEach(visibleDashboardModelRows, id: \.dashboardID) { row in
                    TodayModelRowView(row: row, costText: currencyController.string(fromUSD: row.totalCostDecimal))
                }
            }
            .frame(maxWidth: .infinity, minHeight: 260, alignment: .topLeading)
        }
    }

    private var dashboardModelPageCount: Int {
        max(Int(ceil(Double(dashboardData.modelUsageRows.count) / Double(todayModelPageSize))), 1)
    }

    private var clampedDashboardModelPage: Int {
        min(max(todayModelPage, 0), dashboardModelPageCount - 1)
    }

    private var visibleDashboardModelRows: [TodayModelUsageRow] {
        let rows = dashboardData.modelUsageRows
        let start = clampedDashboardModelPage * todayModelPageSize
        guard start < rows.count else { return rows }
        let end = min(start + todayModelPageSize, rows.count)
        return Array(rows[start..<end])
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
            .frame(height: 280)
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
        max(Int(ceil(Double(modelCostSlices.count) / Double(modelCostLegendPageSize))), 1)
    }

    private var clampedModelCostLegendPage: Int {
        min(max(modelCostLegendPage, 0), modelCostLegendPageCount - 1)
    }

    private var visibleModelCostLegendSlices: [ModelCostSlice] {
        let start = clampedModelCostLegendPage * modelCostLegendPageSize
        guard start < modelCostSlices.count else { return modelCostSlices }
        let end = min(start + modelCostLegendPageSize, modelCostSlices.count)
        return Array(modelCostSlices[start..<end])
    }

    private func todayModelPageCount(for summary: TodaySummaryResponse) -> Int {
        max(Int(ceil(Double(summary.modelRows.count) / Double(todayModelPageSize))), 1)
    }

    private func clampedTodayModelPage(for summary: TodaySummaryResponse) -> Int {
        min(max(todayModelPage, 0), todayModelPageCount(for: summary) - 1)
    }

    private func visibleTodayModelRows(for summary: TodaySummaryResponse) -> [TodayModelUsageRow] {
        let start = clampedTodayModelPage(for: summary) * todayModelPageSize
        guard start < summary.modelRows.count else { return summary.modelRows }
        let end = min(start + todayModelPageSize, summary.modelRows.count)
        return Array(summary.modelRows[start..<end])
    }

    private var tokenTrendRows: [TokenTrendRow] {
        dashboardData.tokenTrendRows
    }

    private var tokenTrendSeriesLabel: String {
        usesCLITokenTrendGrouping ? "Source" : "Model"
    }

    private var usesCLITokenTrendGrouping: Bool {
        store.selectedSource == .all
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

    private var primaryChartTitle: String {
        switch store.selectedViewMode {
        case .daily: "Daily Token Usage"
        case .monthly: "Monthly Token Usage"
        case .sessions: "Session Token Usage"
        }
    }

    private var activePeriodLabel: String {
        switch store.selectedViewMode {
        case .daily: "Active Days"
        case .monthly: "Active Months"
        case .sessions: "Active Sessions"
        }
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

        var days: [DashboardHeatmapDay] = []
        var date = calendar.startOfDay(for: round.start)
        let end = calendar.startOfDay(for: round.end)
        while date <= end {
            days.append(DashboardHeatmapDay(date: date, tokens: tokensByDay[date] ?? 0))
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

    static func make(
        records: [TokenUsageRecord],
        selectedSource: TokenUsageSource,
        selectedViewMode: TokenUsageViewMode,
        startDate: Date,
        endDate: Date,
        selectedModels: Set<String>,
        timeRange: DashboardTimeRange = .today
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
                (selectedSource == .all || record.source == selectedSource)
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
            selectedSource: selectedSource,
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
            spansMultipleYears: spansMultipleYears
        )
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
        return days.map { day in
            DashboardHeatmapDay(date: day, tokens: tokensByDay[calendar.startOfDay(for: day)] ?? 0)
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
        selectedSource: TokenUsageSource,
        selectedModels: Set<String>
    ) -> [TokenTrendRow] {
        let rows: [TokenTrendRow]

        if selectedSource == .all {
            rows = records.map { record in
                TokenTrendRow(
                    date: record.date,
                    series: record.source.label,
                    tokens: record.totalTokens
                )
            }
        } else {
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

        Chart {
            ForEach(rows) { row in
                BarMark(
                    x: .value(dateColumnTitle, row.date, unit: viewMode == .monthly ? .month : .day),
                    y: .value("Tokens", row.tokens),
                    width: barWidth
                )
                .foregroundStyle(by: .value(seriesLabel, row.series))
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
                                hoveredRow = tokenTrendRow(at: point, proxy: proxy, geometry: geometry)
                                hoveredPoint = hoveredRow == nil ? nil : point
                            case .ended:
                                hoveredRow = nil
                                hoveredPoint = nil
                            }
                        }

                    if let row = hoveredRow, let point = hoveredPoint {
                        ChartTooltipPanel(
                            title: row.series,
                            rows: [
                                (dateColumnTitle, tooltipDateText(row.date)),
                                ("Tokens", row.tokens.tokenText),
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
                AxisGridLine()
                AxisTick()
                AxisValueLabel {
                    if let tokens = value.as(Double.self) {
                        Text(tokens.tokenAxisText)
                    }
                }
            }
        }
        .chartYAxisLabel("Tokens")
        .frame(height: dailyUsagePlotHeight)
    }

    @AxisContentBuilder
    private var xAxisMarks: some AxisContent {
        if viewMode == .monthly {
            AxisMarks(values: monthlyXAxisValues) { value in
                AxisGridLine()
                AxisTick()
                AxisValueLabel {
                    if let date = value.as(Date.self) {
                        Text(monthAxisLabel(date))
                    }
                }
            }
        } else {
            AxisMarks(values: dailyXAxisValues) { value in
                if let date = value.as(Date.self), isFirstDayOfMonth(date) {
                    AxisGridLine(stroke: StrokeStyle(lineWidth: 0.8))
                    AxisTick(stroke: StrokeStyle(lineWidth: 0.8))
                    AxisValueLabel {
                        Text(monthSeparatorLabel(date))
                            .font(.caption.weight(.semibold))
                    }
                } else {
                    AxisGridLine()
                    AxisTick()
                    AxisValueLabel {
                        if let date = value.as(Date.self) {
                            Text(date.formatted(.dateTime.day()))
                        }
                    }
                }
            }
        }
    }

    private func tokenTrendRow(at point: CGPoint, proxy: ChartProxy, geometry: GeometryProxy) -> TokenTrendRow? {
        guard let hit = dashboardStackedBarHit(at: point, proxy: proxy, geometry: geometry, dates: rows.map(\.date), viewMode: viewMode) else {
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
                    .foregroundStyle(colorRose.primary)
                }
                ForEach(inputRows) { row in
                    BarMark(
                        x: .value("Date", row.date, unit: viewMode == .monthly ? .month : .day),
                        y: .value("Tokens", row.tokens),
                        width: barWidth
                    )
                    .foregroundStyle(colorOcean.primary)
                }
                ForEach(cacheReadRows) { row in
                    BarMark(
                        x: .value("Date", row.date, unit: viewMode == .monthly ? .month : .day),
                        y: .value("Tokens", row.tokens),
                        width: barWidth
                    )
                    .foregroundStyle(colorOcean.light)
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
                                title: "Token Composition",
                                rows: [
                                    (dateColumnTitle, tooltipDateText(hovered.date)),
                                    ("Input", inputTokens.tokenText),
                                    ("Cache Read", cacheReadTokens.tokenText),
                                    ("Output", outputTokens.tokenText),
                                    ("Cache Coverage", String(format: "%.1f%%", cacheCoverage)),
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
                    AxisGridLine()
                    AxisTick()
                    AxisValueLabel {
                        if let tokens = value.as(Double.self) {
                            Text(tokens.tokenAxisText)
                        }
                    }
                }
            }
            .chartYAxisLabel("Tokens")
            .frame(width: geometry.size.width, height: geometry.size.height)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @AxisContentBuilder
    private var xAxisMarks: some AxisContent {
        if viewMode == .monthly {
            AxisMarks(values: monthlyXAxisValues) { value in
                AxisGridLine()
                AxisTick()
                AxisValueLabel {
                    if let date = value.as(Date.self) {
                        Text(monthAxisLabel(date))
                    }
                }
            }
        } else {
            AxisMarks(values: dailyXAxisValues) { value in
                if let date = value.as(Date.self), isFirstDayOfMonth(date) {
                    AxisGridLine(stroke: StrokeStyle(lineWidth: 0.8))
                    AxisTick(stroke: StrokeStyle(lineWidth: 0.8))
                    AxisValueLabel {
                        Text(monthSeparatorLabel(date))
                            .font(.caption.weight(.semibold))
                    }
                } else {
                    AxisGridLine()
                    AxisTick()
                    AxisValueLabel {
                        if let date = value.as(Date.self) {
                            Text(date.formatted(.dateTime.day()))
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

private struct SummaryCard: View {
    let title: String
    let value: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(value)
                .font(.system(size: 26, weight: .semibold, design: .monospaced))
                .lineLimit(1)
                .minimumScaleFactor(0.75)
            Text(title.uppercased())
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(18)
        .background(.background)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
        )
        .drawingGroup()
    }
}

private struct ProviderMetadata {
    let label: String
    let abbreviation: String
    let color: Color
    let imageAssetName: String?
    let preservesOriginalImageColor: Bool

    static func forModel(_ modelName: String) -> ProviderMetadata {
        let model = modelName.lowercased()

        if model.contains("deepseek") {
            return ProviderMetadata(label: "DeepSeek", abbreviation: "DS", color: colorOcean.primary, imageAssetName: "deepseek-mark", preservesOriginalImageColor: true)
        }
        if model.contains("longcat") {
            return ProviderMetadata(label: "LongCat", abbreviation: "LC", color: colorEmerald.light, imageAssetName: "longcat-mark", preservesOriginalImageColor: true)
        }
        if model.contains("kimi") || model.contains("moonshot") {
            return ProviderMetadata(label: "Kimi", abbreviation: "KM", color: colorSlate.primary, imageAssetName: "kimi-mark")
        }
        if model.contains("minimax") {
            return ProviderMetadata(label: "MiniMax", abbreviation: "MM", color: colorCoral.dark, imageAssetName: "minimax-mark", preservesOriginalImageColor: true)
        }
        if model.contains("mimo") || model.contains("xiaomi") {
            return ProviderMetadata(label: "MiMo", abbreviation: "MO", color: colorAmber.dark, imageAssetName: "xiaomi-mi-mark", preservesOriginalImageColor: true)
        }
        if model.contains("claude") || model.contains("opus") || model.contains("sonnet") || model.contains("haiku") {
            return ProviderMetadata(label: "Claude", abbreviation: "CL", color: colorRose.primary, imageAssetName: "anthropic-mark", preservesOriginalImageColor: true)
        }
        if model.contains("gpt") || model.contains("openai") || model.contains("chatgpt") || model.hasPrefix("o1") || model.hasPrefix("o3") || model.hasPrefix("o4") {
            return ProviderMetadata(label: "OpenAI", abbreviation: "AI", color: colorEmerald.primary, imageAssetName: "openai-mark", preservesOriginalImageColor: true)
        }
        if model.contains("glm") || model.contains("zai") || model.contains("z.ai") {
            return ProviderMetadata(label: "GLM", abbreviation: "GL", color: colorOcean.light, imageAssetName: "zai-mark", preservesOriginalImageColor: true)
        }
        if model.contains("gemini") || model.contains("google") {
            return ProviderMetadata(label: "Gemini", abbreviation: "GM", color: colorViolet.primary, imageAssetName: "gemini-mark", preservesOriginalImageColor: true)
        }
        if model.contains("composer-2.5") || model.contains("composer-2-5") {
            return ProviderMetadata(label: "Cursor", abbreviation: "CR", color: colorSlate.primary, imageAssetName: "cursor-mark", preservesOriginalImageColor: true)
        }
        if model.contains("grok") || model.contains("xai") || model.contains("x.ai") || model.contains("x-ai") {
            return ProviderMetadata(label: "Grok", abbreviation: "GK", color: colorSlate.dark, imageAssetName: "grok-mark")
        }
        if model.contains("stepfun") || model.contains("step-3") {
            return ProviderMetadata(label: "StepFun", abbreviation: "ST", color: colorOcean.dark, imageAssetName: "stepfun-mark", preservesOriginalImageColor: true)
        }
        if model.contains("qwen") || model.contains("qwq") {
            return ProviderMetadata(label: "Qwen", abbreviation: "QW", color: colorTeal.primary, imageAssetName: "qwen-mark", preservesOriginalImageColor: true)
        }
        if model.contains("mistral") || model.contains("mixtral") || model.contains("codestral") || model.contains("ministral") {
            return ProviderMetadata(label: "Mistral", abbreviation: "MI", color: colorAmber.primary)
        }
        if model.contains("llama") || model.contains("meta") {
            return ProviderMetadata(label: "Llama", abbreviation: "LL", color: colorViolet.primary)
        }

        let fallbackIndex = abs(modelName.lowercased().unicodeScalars.reduce(0) { ($0 &* 31) &+ Int($1.value) }) % allColorFamilies.count
        let color = allColorFamilies[fallbackIndex].primary
        let abbreviation = String(modelName.prefix(2)).uppercased()
        return ProviderMetadata(label: "Model", abbreviation: abbreviation, color: color)
    }

    init(label: String, abbreviation: String, color: Color, imageAssetName: String? = nil, preservesOriginalImageColor: Bool = false) {
        self.label = label
        self.abbreviation = abbreviation
        self.color = color
        self.imageAssetName = imageAssetName
        self.preservesOriginalImageColor = preservesOriginalImageColor
    }
}

struct ProviderIconBadge: View {
    private let metadata: ProviderMetadata

    init(modelName: String) {
        metadata = ProviderMetadata.forModel(modelName)
    }

    var body: some View {
        badgeBody
            .help(metadata.label)
            .accessibilityLabel(metadata.label)
    }

    @ViewBuilder
    private var badgeBody: some View {
        if let imageAssetName = metadata.imageAssetName {
            BundledIconImage(
                imageAssetName: imageAssetName,
                tint: metadata.preservesOriginalImageColor ? nil : metadata.color,
                padding: 1
            )
            .frame(width: 22, height: 22)
            .background(metadata.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
        } else {
            Text(metadata.abbreviation)
                .font(.system(size: 9, weight: .bold, design: .rounded))
                .foregroundStyle(metadata.color)
                .frame(width: 24, height: 18)
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

    var body: some View {
        sourceIconBadge
            .help(source.displayName)
            .accessibilityLabel(source.displayName)
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

private struct BundledIconImage: View {
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

private struct ModelListLabel: View {
    let models: [String]

    private var modelsText: String {
        models.map(displayModelName).joined(separator: ", ")
    }

    var body: some View {
        if let firstModel = models.first {
            HStack(spacing: 7) {
                ProviderIconBadge(modelName: firstModel)
                Text(modelsText)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(modelsText)
            }
        } else {
            Text("-")
                .foregroundStyle(.secondary)
        }
    }
}

private struct TodayTokenHeroView: View {
    let totalTokens: Int
    let modelCount: Int
    let isRefreshing: Bool

    @State private var displayedValue = 0
    @State private var hasAppeared = false
    @State private var wasRefreshing = false
    @State private var animationTask: Task<Void, Never>?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("TODAY'S TOKENS")
                .font(.caption.weight(.semibold))
                .tracking(1.2)
                .foregroundStyle(.secondary)

            Text(displayedValue.fullTokenText)
                .font(.system(size: 44, weight: .bold, design: .rounded))
                .monospacedDigit()
                .foregroundStyle(
                    LinearGradient(
                        colors: [
                            Color(nsColor: .labelColor),
                            Color.accentColor.opacity(0.85),
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
                .lineLimit(1)
                .minimumScaleFactor(0.45)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentTransition(.numericText())

            Text("\(modelCount) models")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, minHeight: 86, alignment: .leading)
        .padding(.horizontal, 18)
        .padding(.vertical, 14)
        .background(
            LinearGradient(
                colors: [
                    Color.accentColor.opacity(0.10),
                    Color(nsColor: .controlBackgroundColor).opacity(0.55),
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        )
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(Color.accentColor.opacity(0.22), lineWidth: 1)
        )
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

private struct DashboardHeroView: View {
    let totalTokens: Int
    let costText: String
    let isRefreshing: Bool

    @State private var displayedValue = 0
    @State private var hasAppeared = false
    @State private var wasRefreshing = false
    @State private var animationTask: Task<Void, Never>?

    var body: some View {
        VStack(alignment: .center, spacing: 10) {
            Text("TOTAL TOKENS")
                .font(.caption.weight(.semibold))
                .tracking(1.2)
                .foregroundStyle(.secondary)

            Text(displayedValue.fullTokenText)
                .font(.system(size: 48, weight: .bold, design: .rounded))
                .monospacedDigit()
                .foregroundStyle(
                    LinearGradient(
                        colors: [
                            Color(nsColor: .labelColor),
                            Color.accentColor.opacity(0.85),
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
                .lineLimit(1)
                .minimumScaleFactor(0.4)
                .frame(maxWidth: .infinity)
                .contentTransition(.numericText())
                .multilineTextAlignment(.center)

            Text(costText)
                .font(.system(size: 20, weight: .semibold, design: .rounded))
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .minimumScaleFactor(0.6)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
        .padding(.horizontal, 22)
        .padding(.vertical, 20)
        .background(
            LinearGradient(
                colors: [
                    Color.accentColor.opacity(0.10),
                    Color(nsColor: .controlBackgroundColor).opacity(0.55),
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        )
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(Color.accentColor.opacity(0.22), lineWidth: 1)
        )
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
}

private struct DashboardHeatmapView: View {
    let days: [DashboardHeatmapDay]
    let availableWidth: CGFloat

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
                .help("\(day.date.formatted(.dateTime.year().month().day())) — \(day.tokens.fullTokenText) tokens")
        } else {
            RoundedRectangle(cornerRadius: 3, style: .continuous)
                .fill(Color.clear)
                .frame(width: cellSize, height: cellSize)
        }
    }

    private func color(for tokens: Int) -> Color {
        guard tokens > 0, maxTokens > 0 else {
            // Zero-token days: use a visible light gray that contrasts with the card background
            return Color(nsColor: .separatorColor).opacity(0.35)
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
                .font(.system(size: 18, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: 28, height: 28)
                .background(tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 6, style: .continuous))

            VStack(alignment: .leading, spacing: 4) {
                Text(value)
                    .font(.system(size: 21, weight: .semibold, design: .monospaced))
                    .lineLimit(1)
                    .minimumScaleFactor(0.7)
                Text(title.uppercased())
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.secondary)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
            }
        }
        .frame(maxWidth: .infinity, minHeight: 86, alignment: .topLeading)
        .padding(14)
        .background(.background)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
        )
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
                        .fill(Color(nsColor: .controlBackgroundColor))
                    Capsule()
                        .fill(row.source.tintColor)
                        .frame(width: max(geometry.size.width * fillWidthRatio, row.totalTokens > 0 ? 3 : 0))
                }
            }
            .frame(height: 8)

            HStack(spacing: 12) {
                Label(row.cacheReadTokens.tokenText, systemImage: "externaldrive")
                Label(row.cacheShare.percentText, systemImage: "chart.pie")
                Label("\(row.modelCount)", systemImage: "cpu")
                Spacer()
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .padding(10)
        .background(Color(nsColor: .controlBackgroundColor).opacity(0.55))
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
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
            HStack(alignment: .center, spacing: 22) {
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

                TokenMixLegend(rows: legendRows)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
            }
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
                innerRadius: .ratio(0.58),
                angularInset: 1.5
            )
            .cornerRadius(4)
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
                                ("Tokens", row.tokens.tokenText),
                                ("Share", row.percentText),
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
        let innerRadius = outerRadius * 0.58
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
        VStack(alignment: .leading, spacing: 10) {
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
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct TodayModelRowView: View {
    let row: TodayModelUsageRow
    let costText: String

    var body: some View {
        let modelLabel = displayModelName(row.modelName)
        HStack(spacing: 10) {
            HStack(spacing: 8) {
                ProviderIconBadge(modelName: row.modelName)
                Text(modelLabel)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(row.modelName)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            HStack(spacing: 6) {
                UsageSourceIconBadge(source: row.source)
                Text(row.source.label)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .frame(width: 120, alignment: .leading)
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
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .background(Color(nsColor: .controlBackgroundColor).opacity(0.45))
        .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
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
        .background(.background)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
        )
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

private struct BackendLogViewerPanel: View {
    let lines: [String]
    let onClear: () -> Void
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Label("Backend Logs", systemImage: "terminal")
                    .font(.headline)

                Spacer()

                Text("\(lines.count) lines")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Button("Clear") {
                    onClear()
                }
                .controlSize(.small)
                .disabled(lines.isEmpty)

                Button {
                    onClose()
                } label: {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.borderless)
                .controlSize(.small)
                .help("Close logs")
            }

            Divider()

            if lines.isEmpty {
                VStack(spacing: 10) {
                    Image(systemName: "terminal")
                        .font(.system(size: 28))
                        .foregroundStyle(.secondary)
                    Text("No backend logs yet.")
                        .font(.subheadline.weight(.medium))
                    Text("Refresh or change dashboard data to generate backend activity.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, minHeight: 280)
            } else {
                ScrollViewReader { proxy in
                    ScrollView(.vertical, showsIndicators: true) {
                        LazyVStack(alignment: .leading, spacing: 4) {
                            ForEach(Array(lines.enumerated()), id: \.offset) { index, line in
                                Text(line)
                                    .font(.system(.caption, design: .monospaced))
                                    .foregroundStyle(line.contains("[stderr]") ? colorRose.primary : .primary)
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .id(index)
                            }
                        }
                        .padding(10)
                    }
                    .background(Color(nsColor: .textBackgroundColor))
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
                    )
                    .onAppear {
                        scrollToBottom(proxy)
                    }
                    .onChange(of: lines.count) {
                        scrollToBottom(proxy)
                    }
                }
                .frame(minHeight: 360)
            }
        }
        .padding(16)
        .frame(width: 760, height: 520, alignment: .topLeading)
        .background(Color(nsColor: .windowBackgroundColor))
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
        )
        .shadow(color: Color.black.opacity(0.18), radius: 18, y: 10)
        .onTapGesture {}
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

private struct ChartTooltipPanel: View {
    let title: String
    let rows: [(label: String, value: String)]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.caption.weight(.semibold))
                .lineLimit(1)
                .truncationMode(.middle)

            ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                HStack(spacing: 10) {
                    Text(row.label)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    Spacer(minLength: 12)
                    Text(row.value)
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                        .minimumScaleFactor(0.85)
                }
            }
        }
        .font(.caption)
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .frame(width: chartTooltipWidth, alignment: .leading)
        .fixedSize(horizontal: false, vertical: true)
        .background(Color(nsColor: .windowBackgroundColor))
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
        )
        .shadow(color: Color.black.opacity(0.12), radius: 8, y: 4)
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
            .help("Previous models")

            ViewThatFits(in: .horizontal) {
                legendGrid(columnCount: 3, itemWidth: 190)
                legendGrid(columnCount: 2, itemWidth: 190)
                legendGrid(columnCount: 1, itemWidth: 230)
            }
            .frame(maxWidth: .infinity, alignment: .center)

            Text("\(currentPage + 1)/\(pageCount)")
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 42)
                .help("\(totalCount) models")

            Button(action: onNext) {
                Image(systemName: "chevron.right")
                    .frame(width: 18, height: 18)
            }
            .buttonStyle(.borderless)
            .disabled(currentPage >= pageCount - 1)
            .help("Next models")
        }
        .frame(maxWidth: .infinity, minHeight: 48, alignment: .center)
    }

    private func legendGrid(columnCount: Int, itemWidth: CGFloat) -> some View {
        VStack(alignment: .center, spacing: 8) {
            ForEach(Array(legendRows(columnCount: columnCount).enumerated()), id: \.offset) { _, row in
                HStack(spacing: 18) {
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
        return HStack(spacing: 8) {
            Circle()
                .fill(entry.color)
                .frame(width: 9, height: 9)
            LegendBadge(label: entry.label)
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .frame(width: width, alignment: .center)
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

    var body: some View {
        if let source = UsageSource(label: label) {
            UsageSourceIconBadge(source: source)
        } else {
            ProviderIconBadge(modelName: label)
        }
    }
}

private struct LegendPageControls: View {
    let currentPage: Int
    let pageCount: Int
    let totalCount: Int
    let onPrevious: () -> Void
    let onNext: () -> Void

    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            Button(action: onPrevious) {
                Image(systemName: "chevron.left")
                    .frame(width: 18, height: 18)
            }
            .buttonStyle(.borderless)
            .disabled(currentPage == 0)
            .help("Previous models")

            Text("\(currentPage + 1)/\(pageCount)")
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 34)
                .help("\(totalCount) models")

            Button(action: onNext) {
                Image(systemName: "chevron.right")
                    .frame(width: 18, height: 18)
            }
            .buttonStyle(.borderless)
            .disabled(currentPage >= pageCount - 1)
            .help("Next models")
        }
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
            HStack(alignment: .center, spacing: 22) {
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

                ModelCostLegend(
                    slices: legendSlices,
                    currencyController: currencyController
                )
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
            }
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
                innerRadius: .ratio(0.58),
                angularInset: 1.5
            )
            .cornerRadius(4)
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
                                ("Cost", formatCost(slice.cost)),
                                ("Share", slice.percentText),
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
        let innerRadius = outerRadius * 0.58
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
        VStack(alignment: .leading, spacing: 10) {
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
        .frame(maxWidth: .infinity, alignment: .leading)
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
    @Published private(set) var isLoading = false
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
            currencyController: TokenUsageBillingCurrencyController()
        )
    }
}
