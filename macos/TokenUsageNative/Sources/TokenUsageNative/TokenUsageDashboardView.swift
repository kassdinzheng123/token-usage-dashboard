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
    TokenUsageSource.factory.label:  colorOcean.primary,
]

// Model trend chart palette (single-source view): primary shades from each family
private let tokenTrendChartPalette: [Color] = allColorFamilies.map(\.primary)

// Model cost chart palette: primary + light alternation for visual richness
private let modelCostPalette: [Color] = allColorFamilies.flatMap { [$0.primary, $0.light] }

private let chartTooltipWidth = 190.0
private let chartTooltipHeight = 86.0
private let maximumBarWidth: CGFloat = 96.0
private let tokenTrendLegendPageSize = 6
private let modelCostLegendPageSize = 5
private let todayModelPageSize = 10
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
    case factory

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
        case .factory: "Factory Droid"
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

private enum TokenUsageDashboardSection: String, CaseIterable, Identifiable {
    case today
    case dashboard

    var id: String { rawValue }

    var label: String {
        switch self {
        case .today: "Today"
        case .dashboard: "Dashboard"
        }
    }
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
    @State private var hoveredTokenTrendRow: TokenTrendRow?
    @State private var hoveredTokenTrendPoint: CGPoint?
    @State private var hoveredCompositionRow: TokenCompositionRow?
    @State private var hoveredCompositionPoint: CGPoint?
    @State private var modelSearchText = ""
    @State private var isModelFilterExpanded = false
    @State private var tokenTrendLegendPage = 0
    @State private var modelCostLegendPage = 0
    @State private var todayModelPage = 0
    @State private var isViewModeTransitioning = false
    @State private var viewModeTransitionGeneration = 0
    @State private var isLogViewerPresented = false
    @State private var selectedSection: TokenUsageDashboardSection = .today
    @State private var dashboardData: TokenUsageDashboardData

    init(store: Store, currencyController: TokenUsageBillingCurrencyController) {
        self.store = store
        self.currencyController = currencyController
        _dashboardData = State(initialValue: Self.makeDashboardData(from: store))
    }

    public var body: some View {
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
        .task {
            await store.refresh()
        }
        .task {
            await store.refreshToday()
        }
        .task {
            await currencyController.refreshExchangeRateIfNeeded()
        }
        .onChange(of: selectedSection) {
            updateDashboardData()
            Task {
                await refreshSelectedSection()
            }
        }
        .onChange(of: store.records) {
            updateDashboardData()
        }
        .onChange(of: store.selectedSource) {
            updateDashboardData()
            Task { await store.refresh() }
        }
        .onChange(of: store.selectedViewMode) {
            store.updateDateRangeForViewMode()
            updateDashboardData()
            refreshForViewModeChange()
        }
        .onChange(of: store.startDate) {
            updateDashboardData()
            Task { await store.refresh() }
        }
        .onChange(of: store.endDate) {
            updateDashboardData()
            Task { await store.refresh() }
        }
        .onChange(of: store.selectedModels) {
            updateDashboardData()
            tokenTrendLegendPage = 0
        }
        .onChange(of: tokenTrendColorDomain) {
            tokenTrendLegendPage = 0
        }
        .onChange(of: modelCostSlices.map(\.id)) {
            modelCostLegendPage = 0
        }
        .onChange(of: todaySummary.modelRows.map(\.dashboardID)) {
            todayModelPage = 0
        }
        .onChange(of: currencyController.selectedCurrency) {
            Task { await currencyController.refreshExchangeRateIfNeeded() }
        }
    }

    private static func makeDashboardData(from store: Store) -> TokenUsageDashboardData {
        TokenUsageDashboardData.make(
            records: store.records,
            selectedSource: store.selectedSource,
            selectedViewMode: store.selectedViewMode,
            startDate: store.startDate,
            endDate: store.endDate,
            selectedModels: store.selectedModels
        )
    }

    private func updateDashboardData() {
        dashboardData = Self.makeDashboardData(from: store)
        clearChartInteractionState()
    }

    private func clearChartInteractionState() {
        hoveredTokenTrendRow = nil
        hoveredTokenTrendPoint = nil
        hoveredCompositionRow = nil
        hoveredCompositionPoint = nil
    }

    private var dashboardContent: some View {
        ScrollView(.vertical, showsIndicators: true) {
            VStack(alignment: .leading, spacing: 20) {
                header
                sectionSwitcher

                if selectedSection == .today {
                    todayDashboard
                } else if store.records.isEmpty {
                    dashboardLoadingView
                } else {
                    filterBar
                    summaryCards
                    chartSection
                    usageTable
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

    private var sectionSwitcher: some View {
        HStack(spacing: 12) {
            Picker("Section", selection: $selectedSection) {
                ForEach(TokenUsageDashboardSection.allCases) { section in
                    Text(section.label).tag(section)
                }
            }
            .labelsHidden()
            .pickerStyle(.segmented)
            .frame(width: 240)

            if selectedSection == .today {
                Label(Date().formatted(.dateTime.year().month().day()), systemImage: "calendar")
                    .font(.callout)
                    .foregroundStyle(.secondary)

                Spacer()
            } else {
                Text("Source, view, date range, and model filters")
                    .font(.callout)
                    .foregroundStyle(.secondary)

                Spacer()
            }
        }
        .padding(16)
        .background(.background)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
        )
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

        Task { @MainActor in
            withAnimation(.easeInOut(duration: 0.16)) {
                isViewModeTransitioning = true
            }

            async let delay: Void = Self.minimumViewModeTransitionDelay()
            await store.refresh()
            await delay

            guard generation == viewModeTransitionGeneration else { return }

            withAnimation(.easeInOut(duration: 0.16)) {
                isViewModeTransitioning = false
            }
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
                    Task { await refreshSelectedSection(force: true) }
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.bordered)
                .help(selectedSection == .today ? "Refresh today usage" : "Refresh dashboard usage")
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

    private func refreshSelectedSection(force: Bool = false) async {
        switch selectedSection {
        case .today:
            await store.refreshToday(force: force)
        case .dashboard:
            await store.refreshDashboard(force: force)
        }
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

    @ViewBuilder
    private var filterControls: some View {
        if store.selectedViewMode == .daily || store.selectedViewMode == .monthly {
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 16) {
                    sourcePicker
                    viewModePicker
                    Spacer(minLength: 0)
                }

                HStack(spacing: 16) {
                    dateRangePickers
                    Spacer(minLength: 0)
                }
            }
        } else {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 16) {
                    sourcePicker
                    viewModePicker
                    dateRangePickers
                    Spacer(minLength: 0)
                }

                VStack(alignment: .leading, spacing: 10) {
                    HStack(spacing: 16) {
                        sourcePicker
                        viewModePicker
                        Spacer(minLength: 0)
                    }

                    HStack(spacing: 16) {
                        dateRangePickers
                        Spacer(minLength: 0)
                    }
                }
            }
        }
    }

    private var sourcePicker: some View {
        Picker("Source", selection: $store.selectedSource) {
            ForEach(TokenUsageSource.allCases) { source in
                HStack(spacing: 4) {
                    if let imageAssetName = source.imageAssetName {
                        BundledIconImage(imageAssetName: imageAssetName, padding: 1)
                            .frame(width: 16, height: 16)
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

    @ViewBuilder
    private var modelFilter: some View {
        if !availableModels.isEmpty {
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
            LazyVGrid(
                columns: [
                    GridItem(.adaptive(minimum: 172), spacing: 12, alignment: .top),
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
                    title: "Tokens",
                    value: summary.totalTokens.tokenText,
                    subtitle: "\(summary.modelCount) models",
                    systemImage: "number",
                    tint: .green
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

            Grid(horizontalSpacing: 16, verticalSpacing: 16) {
                GridRow {
                    ChartCard(title: "CLI Consumption") {
                        todaySourceBreakdown(summary: summary)
                    }
                    .frame(height: overviewContentHeight + 66, alignment: .top)

                    ChartCard(title: "Token Mix") {
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
        let rowCount = Double(summary.sourceRows.count)
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
                ForEach(summary.sourceRows, id: \.source) { row in
                    TodaySourceRowView(
                        row: row,
                        maxTokens: summary.maxSourceTokens,
                        currencyController: currencyController
                    )
                    .frame(height: todaySourceRowHeight)
                }
            }
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .frame(height: todayOverviewResolvedContentHeight(for: summary), alignment: .topLeading)
        }
    }

    @ViewBuilder
    private func todayTokenMix(summary: TodaySummaryResponse) -> some View {
        let rows = summary.modelTokenRows
        if rows.isEmpty {
            emptyTodayState(text: store.isLoading ? "Loading models..." : "No model usage recorded today")
                .frame(maxWidth: .infinity, alignment: .center)
                .frame(height: todayOverviewResolvedContentHeight(for: summary))
        } else {
            HStack(alignment: .center, spacing: 18) {
                TodayModelTokenDonutChart(rows: rows, totalTokens: summary.totalTokens)
                    .frame(width: 156, height: 156)

                ScrollView(.vertical) {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        ForEach(rows) { row in
                            TodayModelTokenRowView(row: row)
                        }
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .frame(height: todayOverviewResolvedContentHeight(for: summary), alignment: .topLeading)
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
                    TodayModelRowView(row: row, currencyController: currencyController)
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

    private var summaryCards: some View {
        HStack(spacing: 16) {
            SummaryCard(title: "Total Cost", value: currencyController.string(fromUSD: totalCost))
            SummaryCard(title: "Total Tokens", value: totalTokens.tokenText)
            SummaryCard(title: activePeriodLabel, value: activePeriodCount.formatted())
            SummaryCard(title: "Unique Models", value: dashboardData.uniqueModelCount.formatted())
        }
    }

    private var chartSection: some View {
        Grid(horizontalSpacing: 16, verticalSpacing: 16) {
            GridRow {
                ChartCard(title: primaryChartTitle) {
                    tokenTrendChart
                }
                .gridCellColumns(2)
            }

            GridRow {
                ChartCard(title: "Token Composition") {
                    compositionChart
                }

                ChartCard(title: "Model Cost Distribution") {
                    modelCostLegendPager
                } content: {
                    modelCostChart
                }
            }
        }
    }

    private var tokenTrendChart: some View {
        VStack(alignment: .leading, spacing: 10) {
            let rows = tokenTrendRows
            let barWidth = histogramBarWidth(for: rows.map(\.date))

            Chart {
                ForEach(rows) { row in
                    BarMark(
                        x: .value(dateColumnTitle, row.date, unit: store.selectedViewMode == .monthly ? .month : .day),
                        y: .value("Tokens", row.tokens),
                        width: barWidth
                    )
                    .foregroundStyle(by: .value(tokenTrendSeriesLabel, row.series))
                }
            }
            .chartForegroundStyleScale(domain: tokenTrendColorDomain, range: tokenTrendColorRange)
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
                                    hoveredTokenTrendRow = tokenTrendRow(at: point, proxy: proxy, geometry: geometry)
                                    hoveredTokenTrendPoint = hoveredTokenTrendRow == nil ? nil : point
                                case .ended:
                                    hoveredTokenTrendRow = nil
                                    hoveredTokenTrendPoint = nil
                                }
                            }

                        if let row = hoveredTokenTrendRow, let point = hoveredTokenTrendPoint {
                            ChartTooltipPanel(
                                title: row.series,
                                rows: [
                                    (dateColumnTitle, tooltipDateText(for: row.date)),
                                    ("Tokens", row.tokens.tokenText),
                                ]
                            )
                            .position(tooltipPosition(for: point, in: geometry.size))
                            .zIndex(1)
                        }
                    }
                    .allowsHitTesting(true)
                    .animation(nil, value: hoveredTokenTrendRow?.id)
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
            .frame(height: 220)

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
        let barWidth = histogramBarWidth(for: compositionRows.map(\.date))
        let inputRows = compositionRows.filter { $0.kind == .input }
        let cacheReadRows = compositionRows.filter { $0.kind == .cacheRead }
        let outputRows = compositionRows.filter { $0.kind == .output }

        return Chart {
            ForEach(outputRows) { row in
                BarMark(
                    x: .value("Date", row.date, unit: store.selectedViewMode == .monthly ? .month : .day),
                    y: .value("Tokens", row.tokens),
                    width: barWidth
                )
                .foregroundStyle(colorRose.primary)
            }
            ForEach(inputRows) { row in
                BarMark(
                    x: .value("Date", row.date, unit: store.selectedViewMode == .monthly ? .month : .day),
                    y: .value("Tokens", row.tokens),
                    width: barWidth
                )
                .foregroundStyle(colorOcean.primary)
            }
            ForEach(cacheReadRows) { row in
                BarMark(
                    x: .value("Date", row.date, unit: store.selectedViewMode == .monthly ? .month : .day),
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
            GeometryReader { geometry in
                ZStack(alignment: .topLeading) {
                    Rectangle()
                        .fill(.clear)
                        .contentShape(Rectangle())
                        .onContinuousHover { phase in
                            switch phase {
                            case .active(let point):
                                hoveredCompositionRow = compositionRow(at: point, proxy: proxy, geometry: geometry)
                                hoveredCompositionPoint = hoveredCompositionRow == nil ? nil : point
                            case .ended:
                                hoveredCompositionRow = nil
                                hoveredCompositionPoint = nil
                            }
                        }

                    if let hoveredRow = hoveredCompositionRow, let point = hoveredCompositionPoint {
                        let dateKey = periodKey(for: hoveredRow.date)
                        let inputTokens = compositionRows.first { $0.kind == .input && periodKey(for: $0.date) == dateKey }?.tokens ?? 0
                        let cacheReadTokens = compositionRows.first { $0.kind == .cacheRead && periodKey(for: $0.date) == dateKey }?.tokens ?? 0
                        let outputTokens = compositionRows.first { $0.kind == .output && periodKey(for: $0.date) == dateKey }?.tokens ?? 0
                        let totalInput = cacheReadTokens + inputTokens
                        let rawCoverage = totalInput > 0 ? Double(cacheReadTokens) / Double(totalInput) * 100 : 0
                        let cacheCoverage = min(floor(rawCoverage * 10) / 10, 99.9)

                        ChartTooltipPanel(
                            title: "Token Composition",
                            rows: [
                                (dateColumnTitle, tooltipDateText(for: hoveredRow.date)),
                                ("Input", inputTokens.tokenText),
                                ("Cache Read", cacheReadTokens.tokenText),
                                ("Output", outputTokens.tokenText),
                                ("Cache Coverage", String(format: "%.1f%%", cacheCoverage)),
                            ]
                        )
                        .position(tooltipPosition(for: point, in: geometry.size))
                        .zIndex(1)
                    }
                }
                .allowsHitTesting(true)
                .animation(nil, value: hoveredCompositionRow?.id)
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
        .frame(height: 280)
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

    @AxisContentBuilder
    private var xAxisMarks: some AxisContent {
        if store.selectedViewMode == .monthly {
            AxisMarks(values: monthlyXAxisValues) { value in
                AxisGridLine()
                AxisTick()
                AxisValueLabel {
                    if let date = value.as(Date.self) {
                        Text(monthAxisLabel(for: date))
                    }
                }
            }
        } else {
            AxisMarks(values: dailyXAxisValues) { value in
                if let date = value.as(Date.self), isFirstDayOfMonth(date) {
                    AxisGridLine(stroke: StrokeStyle(lineWidth: 0.8))
                    AxisTick(stroke: StrokeStyle(lineWidth: 0.8))
                    AxisValueLabel {
                        Text(monthSeparatorLabel(for: date))
                            .font(.caption.weight(.semibold))
                    }
                } else {
                    AxisGridLine()
                    AxisTick()
                    AxisValueLabel {
                        if let date = value.as(Date.self) {
                            Text(dayOfMonthLabel(for: date))
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var usageTable: some View {
        if store.selectedViewMode == .sessions {
            usageTableWithSessionID
        } else {
            usageTableWithoutSessionID
        }
    }

    @ViewBuilder
    private var usageTableWithSessionID: some View {
        if store.selectedSource == .all {
            usageTableWithSessionIDModelsAfterSource
        } else {
            usageTableWithSessionIDModelsAtEnd
        }
    }

    private var usageTableWithSessionIDModelsAfterSource: some View {
        Table(filteredRecords) {
            TableColumn("Source") { record in
                TokenUsageSourceLabel(source: record.source)
            }
            .width(min: 110, ideal: 130)

            TableColumn("Models") { record in
                ModelListLabel(models: Array(record.modelsUsed.prefix(5)))
            }
            .width(min: 180, ideal: 260)

            TableColumn(dateColumnTitle) { record in
                Text(record.date, format: store.selectedViewMode == .monthly ? .dateTime.year().month() : .dateTime.year().month().day())
            }
            .width(min: 100, ideal: 120)

            TableColumn("Session ID") { record in
                Text(sessionIDText(for: record))
                    .font(.system(.body, design: .monospaced))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(record.sessionID ?? "")
            }
            .width(min: 120, ideal: 150)

            TableColumn("Input") { record in
                Text(record.inputTokens.tokenText)
            }
            .width(min: 80, ideal: 90)

            TableColumn("Output") { record in
                Text(record.outputTokens.tokenText)
            }
            .width(min: 80, ideal: 90)

            TableColumn("Cache Create") { record in
                Text(record.cacheCreationTokens.tokenText)
            }
            .width(min: 100, ideal: 110)

            TableColumn("Cache Read") { record in
                Text(record.cacheReadTokens.tokenText)
            }
            .width(min: 100, ideal: 110)

            TableColumn("Total Tokens") { record in
                Text(record.totalTokens.tokenText)
                    .font(.system(.body, design: .monospaced))
            }
            .width(min: 110, ideal: 120)

            TableColumn("Cost") { record in
                Text(currencyController.string(fromUSD: record.totalCost))
                    .font(.system(.body, design: .monospaced))
            }
            .width(min: 80, ideal: 90)
        }
        .frame(minHeight: 220)
    }

    private var usageTableWithSessionIDModelsAtEnd: some View {
        Table(filteredRecords) {
            TableColumn("Source") { record in
                TokenUsageSourceLabel(source: record.source)
            }
            .width(min: 110, ideal: 130)

            TableColumn(dateColumnTitle) { record in
                Text(record.date, format: store.selectedViewMode == .monthly ? .dateTime.year().month() : .dateTime.year().month().day())
            }
            .width(min: 100, ideal: 120)

            TableColumn("Session ID") { record in
                Text(sessionIDText(for: record))
                    .font(.system(.body, design: .monospaced))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(record.sessionID ?? "")
            }
            .width(min: 120, ideal: 150)

            TableColumn("Input") { record in
                Text(record.inputTokens.tokenText)
            }
            .width(min: 80, ideal: 90)

            TableColumn("Output") { record in
                Text(record.outputTokens.tokenText)
            }
            .width(min: 80, ideal: 90)

            TableColumn("Cache Create") { record in
                Text(record.cacheCreationTokens.tokenText)
            }
            .width(min: 100, ideal: 110)

            TableColumn("Cache Read") { record in
                Text(record.cacheReadTokens.tokenText)
            }
            .width(min: 100, ideal: 110)

            TableColumn("Total Tokens") { record in
                Text(record.totalTokens.tokenText)
                    .font(.system(.body, design: .monospaced))
            }
            .width(min: 110, ideal: 120)

            TableColumn("Cost") { record in
                Text(currencyController.string(fromUSD: record.totalCost))
                    .font(.system(.body, design: .monospaced))
            }
            .width(min: 80, ideal: 90)

            TableColumn("Models") { record in
                ModelListLabel(models: Array(record.modelsUsed.prefix(5)))
            }
            .width(min: 180, ideal: 260)
        }
        .frame(minHeight: 220)
    }

    @ViewBuilder
    private var usageTableWithoutSessionID: some View {
        if store.selectedSource == .all {
            usageTableWithoutSessionIDModelsAfterSource
        } else {
            usageTableWithoutSessionIDModelsAtEnd
        }
    }

    private var usageTableWithoutSessionIDModelsAfterSource: some View {
        Table(filteredRecords) {
            TableColumn("Source") { record in
                TokenUsageSourceLabel(source: record.source)
            }
            .width(min: 110, ideal: 130)

            TableColumn("Models") { record in
                ModelListLabel(models: Array(record.modelsUsed.prefix(5)))
            }
            .width(min: 180, ideal: 260)

            TableColumn(dateColumnTitle) { record in
                Text(record.date, format: store.selectedViewMode == .monthly ? .dateTime.year().month() : .dateTime.year().month().day())
            }
            .width(min: 100, ideal: 120)

            TableColumn("Input") { record in
                Text(record.inputTokens.tokenText)
            }
            .width(min: 80, ideal: 90)

            TableColumn("Output") { record in
                Text(record.outputTokens.tokenText)
            }
            .width(min: 80, ideal: 90)

            TableColumn("Cache Create") { record in
                Text(record.cacheCreationTokens.tokenText)
            }
            .width(min: 100, ideal: 110)

            TableColumn("Cache Read") { record in
                Text(record.cacheReadTokens.tokenText)
            }
            .width(min: 100, ideal: 110)

            TableColumn("Total Tokens") { record in
                Text(record.totalTokens.tokenText)
                    .font(.system(.body, design: .monospaced))
            }
            .width(min: 110, ideal: 120)

            TableColumn("Cost") { record in
                Text(currencyController.string(fromUSD: record.totalCost))
                    .font(.system(.body, design: .monospaced))
            }
            .width(min: 80, ideal: 90)
        }
        .frame(minHeight: 220)
    }

    private var usageTableWithoutSessionIDModelsAtEnd: some View {
        Table(filteredRecords) {
            TableColumn("Source") { record in
                TokenUsageSourceLabel(source: record.source)
            }
            .width(min: 110, ideal: 130)

            TableColumn(dateColumnTitle) { record in
                Text(record.date, format: store.selectedViewMode == .monthly ? .dateTime.year().month() : .dateTime.year().month().day())
            }
            .width(min: 100, ideal: 120)

            TableColumn("Input") { record in
                Text(record.inputTokens.tokenText)
            }
            .width(min: 80, ideal: 90)

            TableColumn("Output") { record in
                Text(record.outputTokens.tokenText)
            }
            .width(min: 80, ideal: 90)

            TableColumn("Cache Create") { record in
                Text(record.cacheCreationTokens.tokenText)
            }
            .width(min: 100, ideal: 110)

            TableColumn("Cache Read") { record in
                Text(record.cacheReadTokens.tokenText)
            }
            .width(min: 100, ideal: 110)

            TableColumn("Total Tokens") { record in
                Text(record.totalTokens.tokenText)
                    .font(.system(.body, design: .monospaced))
            }
            .width(min: 110, ideal: 120)

            TableColumn("Cost") { record in
                Text(currencyController.string(fromUSD: record.totalCost))
                    .font(.system(.body, design: .monospaced))
            }
            .width(min: 80, ideal: 90)

            TableColumn("Models") { record in
                ModelListLabel(models: Array(record.modelsUsed.prefix(5)))
            }
            .width(min: 180, ideal: 260)
        }
        .frame(minHeight: 220)
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
        store.selectedSource == .all ? "Source" : "Model"
    }

    private var tokenTrendColorDomain: [String] {
        dashboardData.tokenTrendColorDomain
    }

    private var tokenTrendColorRange: [Color] {
        if store.selectedSource == .all {
            return tokenTrendColorDomain.map { tokenTrendSourceColors[$0] ?? .blue }
        }

        return tokenTrendColorDomain.enumerated().map { index, _ in
            tokenTrendChartPalette[index % tokenTrendChartPalette.count]
        }
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
        if store.selectedSource == .all {
            return tokenTrendSourceColors[series] ?? .blue
        }

        let index = tokenTrendColorDomain.firstIndex(of: series) ?? 0
        return tokenTrendChartPalette[index % tokenTrendChartPalette.count]
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

    private func isFirstDayOfMonth(_ date: Date) -> Bool {
        Calendar.current.component(.day, from: date) == 1
    }

    private func monthSeparatorLabel(for date: Date) -> String {
        if spansMultipleYears {
            return date.formatted(.dateTime.year().month(.abbreviated))
        }
        return date.formatted(.dateTime.month(.abbreviated))
    }

    private func dayOfMonthLabel(for date: Date) -> String {
        date.formatted(.dateTime.day())
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

    private func periodKey(for record: TokenUsageRecord) -> String {
        switch store.selectedViewMode {
        case .daily, .sessions:
            record.date.formatted(.iso8601.year().month().day())
        case .monthly:
            record.date.formatted(.iso8601.year().month())
        }
    }

    private func histogramBarWidth(for _: [Date]) -> MarkDimension {
        store.selectedViewMode == .monthly ? .fixed(maximumBarWidth) : .automatic
    }

    private func periodKey(for date: Date) -> String {
        switch store.selectedViewMode {
        case .daily, .sessions:
            date.formatted(.iso8601.year().month().day())
        case .monthly:
            date.formatted(.iso8601.year().month())
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

    private func tokenTrendRow(at point: CGPoint, proxy: ChartProxy, geometry: GeometryProxy) -> TokenTrendRow? {
        guard let hit = stackedBarHit(at: point, proxy: proxy, geometry: geometry, dates: tokenTrendRows.map(\.date)) else {
            return nil
        }

        return hitStackedRow(
            date: hit.date,
            tokens: hit.tokens,
            rows: tokenTrendRows,
            rowDate: \.date,
            rowTokens: \.tokens,
            rowOrder: { tokenTrendColorDomain.firstIndex(of: $0.series) ?? Int.max }
        )
    }

    private func compositionRow(at point: CGPoint, proxy: ChartProxy, geometry: GeometryProxy) -> TokenCompositionRow? {
        guard let hit = stackedBarHit(at: point, proxy: proxy, geometry: geometry, dates: compositionRows.map(\.date)) else {
            return nil
        }

        return hitStackedRow(
            date: hit.date,
            tokens: hit.tokens,
            rows: compositionRows,
            rowDate: \.date,
            rowTokens: \.tokens,
            rowOrder: { $0.kind.sortOrder }
        )
    }

    private func stackedBarHit(at point: CGPoint, proxy: ChartProxy, geometry: GeometryProxy, dates: [Date]) -> (date: Date, tokens: Double)? {
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

        let hitDate = periodStart(for: date)
        guard let barDate = representativeDate(for: hitDate, in: dates) else {
            return nil
        }
        let uniqueCount = Set(dates.map { periodKey(for: $0) }).count
        let barCenterX: CGFloat
        let barHalfWidth: CGFloat
        if store.selectedViewMode == .monthly,
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

    private func hitStackedRow<Row>(
        date: Date,
        tokens: Double,
        rows: [Row],
        rowDate: KeyPath<Row, Date>,
        rowTokens: KeyPath<Row, Int>,
        rowOrder: (Row) -> Int
    ) -> Row? {
        let period = periodKey(for: date)
        let periodRows = rows
            .filter { periodKey(for: $0[keyPath: rowDate]) == period && $0[keyPath: rowTokens] > 0 }
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

    private func periodStart(for date: Date) -> Date {
        switch store.selectedViewMode {
        case .daily, .sessions:
            Calendar.current.startOfDay(for: date)
        case .monthly:
            Calendar.current.dateInterval(of: .month, for: date)?.start ?? date
        }
    }

    private func representativeDate(for date: Date, in dates: [Date]) -> Date? {
        let period = periodKey(for: date)
        return dates
            .filter { periodKey(for: $0) == period }
            .min()
    }
}

private struct TokenUsageDashboardData {
    let filteredRecords: [TokenUsageRecord]
    let availableModels: [String]
    let totalCost: Decimal
    let totalTokens: Int
    let activePeriodCount: Int
    let uniqueModelCount: Int
    let modelCostRows: [ModelCostRow]
    let modelCostSlices: [ModelCostSlice]
    let modelCostTotalCost: Decimal
    let tokenTrendRows: [TokenTrendRow]
    let tokenTrendColorDomain: [String]
    let compositionRows: [TokenCompositionRow]
    let monthlyXAxisValues: [Date]
    let dailyXAxisValues: [Date]
    let spansMultipleYears: Bool

    static func make(
        records: [TokenUsageRecord],
        selectedSource: TokenUsageSource,
        selectedViewMode: TokenUsageViewMode,
        startDate: Date,
        endDate: Date,
        selectedModels: Set<String>
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
        let activePeriodCount = Set(filteredRecords.map { periodKey(for: $0.date, viewMode: selectedViewMode) }).count
        let uniqueModelCount = Set(filteredRecords.flatMap(\.modelsUsed)).count
        let modelCostRows = makeModelCostRows(from: filteredRecords)
        let modelCostSlices = makeModelCostSlices(from: modelCostRows)
        let tokenTrendRows = makeTokenTrendRows(
            from: filteredRecords,
            selectedSource: selectedSource,
            selectedModels: selectedModels
        )
        let tokenTrendColorDomain = Array(Set(tokenTrendRows.map(\.series))).sorted()
        let compositionRows = makeCompositionRows(from: filteredRecords, viewMode: selectedViewMode)
        let monthlyXAxisValues = makeMonthlyXAxisValues(rangeStart: rangeStart, rangeEnd: rangeEnd, calendar: calendar)
        let dailyXAxisValues = makeDailyXAxisValues(from: filteredRecords, rangeStart: rangeStart, rangeEnd: rangeEnd, calendar: calendar)
        let spansMultipleYears = Set(monthlyXAxisValues.map { calendar.component(.year, from: $0) }).count > 1

        return TokenUsageDashboardData(
            filteredRecords: filteredRecords,
            availableModels: availableModels,
            totalCost: totalCost,
            totalTokens: totalTokens,
            activePeriodCount: activePeriodCount,
            uniqueModelCount: uniqueModelCount,
            modelCostRows: modelCostRows,
            modelCostSlices: modelCostSlices,
            modelCostTotalCost: modelCostRows.reduce(Decimal.zero) { $0 + $1.cost },
            tokenTrendRows: tokenTrendRows,
            tokenTrendColorDomain: tokenTrendColorDomain,
            compositionRows: compositionRows,
            monthlyXAxisValues: monthlyXAxisValues,
            dailyXAxisValues: dailyXAxisValues,
            spansMultipleYears: spansMultipleYears
        )
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

        return visibleRows.enumerated().map { index, row in
            ModelCostSlice(
                model: row.model,
                cost: row.cost,
                percent: row.cost.doubleValue / total,
                color: allColorFamilies[index % allColorFamilies.count].primary
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
        if model.contains("grok") || model.contains("xai") || model.contains("x.ai") || model.contains("x-ai") {
            return ProviderMetadata(label: "Grok", abbreviation: "GK", color: colorSlate.dark, imageAssetName: "grok-mark")
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

private struct ProviderIconBadge: View {
    let metadata: ProviderMetadata

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
                .background(source.tintColor.opacity(0.12), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
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
                .background(source.tintColor.opacity(0.12), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
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

    var body: some View {
        if let image = IconImageLoader.image(named: imageAssetName) {
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

    static func image(named name: String) -> NSImage? {
        if let cachedImage = imageCache.object(forKey: name as NSString) {
            return cachedImage
        }

        for bundle in fallbackBundles() {
            if let image = image(named: name, in: bundle) {
                imageCache.setObject(image, forKey: name as NSString)
                return image
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
    }
}

private struct TodaySourceRowView: View {
    let row: TodaySourceUsageRow
    let maxTokens: Int
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController

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
                Text(currencyController.string(fromUSD: row.totalCostDecimal))
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

private struct TodayTokenMixDonutChart: View {
    let rows: [TodayTokenRow]
    let totalTokens: Int

    var body: some View {
        ZStack {
            Chart(rows) { row in
                SectorMark(
                    angle: .value("Tokens", row.tokens),
                    innerRadius: .ratio(0.62),
                    angularInset: 1.2
                )
                .cornerRadius(3)
                .foregroundStyle(row.color)
            }
            .chartLegend(.hidden)
            .chartBackground { _ in Color.clear }

            VStack(spacing: 3) {
                Text("Total")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                Text(totalTokens.tokenText)
                    .font(.system(size: 17, weight: .semibold, design: .monospaced))
                    .lineLimit(1)
                    .minimumScaleFactor(0.7)
            }
            .frame(width: 86)
        }
    }
}

private struct TodayModelTokenDonutChart: View {
    let rows: [TodayModelTokenRow]
    let totalTokens: Int

    var body: some View {
        ZStack {
            Chart(rows) { row in
                SectorMark(
                    angle: .value("Tokens", row.tokens),
                    innerRadius: .ratio(0.62),
                    angularInset: 1.2
                )
                .cornerRadius(3)
                .foregroundStyle(row.color)
            }
            .chartLegend(.hidden)
            .chartBackground { _ in Color.clear }

            VStack(spacing: 3) {
                Text("Total")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                Text(totalTokens.tokenText)
                    .font(.system(size: 17, weight: .semibold, design: .monospaced))
                    .lineLimit(1)
                    .minimumScaleFactor(0.7)
            }
            .frame(width: 86)
        }
    }
}

private struct TodayModelTokenRowView: View {
    let row: TodayModelTokenRow

    var body: some View {
        let modelLabel = displayModelName(row.modelName)
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                ProviderIconBadge(modelName: row.modelName)
                Text(modelLabel)
                    .font(.callout.weight(.medium))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(row.modelName)
                Spacer()
                Text(row.tokens.tokenText)
                    .font(.system(.callout, design: .monospaced))
                Text(row.percentText)
                    .font(.system(.callout, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .frame(width: 64, alignment: .trailing)
            }

            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(Color(nsColor: .controlBackgroundColor))
                    Capsule()
                        .fill(row.color)
                        .frame(width: max(geometry.size.width * row.percent, row.tokens > 0 ? 3 : 0))
                }
            }
            .frame(height: 8)
        }
    }
}

private struct TodayTokenMixRowView: View {
    let row: TodayTokenRow
    let totalTokens: Int

    private var fillWidthRatio: Double {
        Double(row.tokens) / Double(max(totalTokens, 1))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                Image(systemName: row.systemImage)
                    .foregroundStyle(row.color)
                    .frame(width: 18)
                Text(row.label)
                    .font(.callout.weight(.medium))
                Spacer()
                Text(row.tokens.tokenText)
                    .font(.system(.callout, design: .monospaced))
                Text(fillWidthRatio.percentText)
                    .font(.system(.callout, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .frame(width: 64, alignment: .trailing)
            }

            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(Color(nsColor: .controlBackgroundColor))
                    Capsule()
                        .fill(row.color)
                        .frame(width: max(geometry.size.width * fillWidthRatio, row.tokens > 0 ? 3 : 0))
                }
            }
            .frame(height: 8)
        }
    }
}

private struct TodayModelRowView: View {
    let row: TodayModelUsageRow
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController

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
            Text(currencyController.string(fromUSD: row.totalCostDecimal))
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
                    ModelCostSectorChart(slices: slices, currencyController: currencyController)
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
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController
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
                                ("Cost", currencyController.string(fromUSD: slice.cost)),
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

    var tokenRows: [TodayTokenRow] {
        [
            TodayTokenRow(label: "Input", tokens: inputTokens, color: .blue, systemImage: "arrow.down.to.line.compact"),
            TodayTokenRow(label: "Output", tokens: outputTokens, color: .green, systemImage: "arrow.up.to.line.compact"),
            TodayTokenRow(label: "Cache Create", tokens: cacheCreationTokens, color: .orange, systemImage: "tray.and.arrow.down"),
            TodayTokenRow(label: "Cache Read", tokens: cacheReadTokens, color: .purple, systemImage: "externaldrive"),
        ]
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
            .enumerated()
            .map { index, row in
                TodayModelTokenRow(
                    modelName: row.modelName,
                    tokens: row.tokens,
                    percent: Double(row.tokens) / Double(total),
                    color: allColorFamilies[index % allColorFamilies.count].primary
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

private struct TodayTokenRow: Identifiable {
    var id: String { label }
    let label: String
    let tokens: Int
    let color: Color
    let systemImage: String
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

private extension Int {
    var tokenText: String {
        if self >= 1_000_000_000 { return String(format: "%.2fB", Double(self) / 1_000_000_000) }
        if self >= 1_000_000 { return String(format: "%.2fM", Double(self) / 1_000_000) }
        if self >= 1_000 { return String(format: "%.1fK", Double(self) / 1_000) }
        return formatted()
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

private func displayModelName(_ modelName: String) -> String {
    if modelName.localizedCaseInsensitiveContains("kiro-claude-opus-4.7") {
        return "kiro-claude-opus-4.7"
    }
    if modelName.hasPrefix("[pi] ") {
        return String(modelName.dropFirst(5))
    }
    return modelName
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
        case .factory: .factory
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
        case .factory: "gearshape.2"
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
        case .factory: "factory-mark"
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
        case .factory: "gearshape.2"
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
        case .factory: "factory-mark"
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
        let sources: [TokenUsageSource] = [.claude, .codex, .opencode, .factory]
        let models = ["claude-sonnet-4", "gpt-5.2-codex", "qwen3-coder", "factory-droid"]

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
