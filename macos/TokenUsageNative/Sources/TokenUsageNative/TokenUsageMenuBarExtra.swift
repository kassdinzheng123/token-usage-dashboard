import AppKit
import SwiftUI

private let menuBarMaximumBreakdownRows = 5

private enum MenuBarBreakdownMode: String, CaseIterable, Identifiable {
    case cli
    case model

    var id: String { rawValue }

    var label: String {
        switch self {
        case .cli: "CLI"
        case .model: "Model"
        }
    }

    var title: String {
        switch self {
        case .cli: "By CLI"
        case .model: "By Model"
        }
    }
}

struct TokenUsageMenuBarLabel: View {
    @ObservedObject var store: LiveTokenUsageDashboardStore
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: "chart.bar.xaxis")
            Text("\(todaySummary.tokenText) | \(currencyController.string(fromUSD: todaySummary.totalCostDecimal))")
        }
        .fixedSize()
        .frame(maxHeight: .infinity, alignment: .center)
        .padding(.horizontal, 6)
        .task {
            await store.refreshToday()
        }
    }

    private var todaySummary: TodaySummaryResponse {
        store.todaySummary
    }
}

struct TokenUsageMenuBarExtraView: View {
    @ObservedObject var store: LiveTokenUsageDashboardStore
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController
    @State private var breakdownMode: MenuBarBreakdownMode = .cli
    @State private var isRefreshing = false

    private var todaySummary: TodaySummaryResponse {
        store.todaySummary
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            header

            HStack(spacing: 12) {
                statisticBlock(title: "Tokens", value: todaySummary.tokenText)
                statisticBlock(title: "Cost", value: currencyController.string(fromUSD: todaySummary.totalCostDecimal))
            }

            Divider()

            usageBreakdown

            HStack(spacing: 8) {
                Button {
                    Task { await refreshToday() }
                } label: {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .disabled(isRefreshing)

                Button {
                    MenuBarStatusItemController.shared.openDashboard()
                } label: {
                    Label("Open Dashboard", systemImage: "rectangle.stack")
                }

                Spacer()
            }
        }
        .padding(16)
        .frame(width: 320, height: 420)
        .background(Color(nsColor: .windowBackgroundColor))
        .task {
            await currencyController.refreshExchangeRateIfNeeded()
        }
        .onChange(of: currencyController.selectedCurrency) {
            Task { await currencyController.refreshExchangeRateIfNeeded() }
        }
    }

    private var header: some View {
        HStack {
            VStack(alignment: .leading, spacing: 3) {
                Text("Today")
                    .font(.headline)
                Text(Date().formatted(.dateTime.year().month().day()))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Picker("Currency", selection: $currencyController.selectedCurrency) {
                ForEach(TokenUsageBillingCurrency.allCases) { currency in
                    Text(currency.label).tag(currency)
                }
            }
            .labelsHidden()
            .pickerStyle(.segmented)
            .controlSize(.small)
            .frame(width: 116)

            ZStack {
                ProgressView()
                    .controlSize(.small)
                    .opacity(isRefreshing ? 1 : 0)
            }
            .frame(width: 16, height: 16)
        }
    }

    private var usageBreakdown: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(breakdownMode.title)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)

                Spacer()

                Picker("Breakdown", selection: $breakdownMode) {
                    ForEach(MenuBarBreakdownMode.allCases) { mode in
                        Text(mode.label).tag(mode)
                    }
                }
                .labelsHidden()
                .pickerStyle(.segmented)
                .controlSize(.small)
                .frame(width: 118)
            }

            GeometryReader { geometry in
                let spacing: CGFloat = 6
                let cardHeight = max((geometry.size.height - CGFloat(menuBarMaximumBreakdownRows - 1) * spacing) / CGFloat(menuBarMaximumBreakdownRows), 24)

                ZStack(alignment: .topLeading) {
                    sourceBreakdownRows(cardHeight: cardHeight)
                        .opacity(breakdownMode == .cli ? 1 : 0)
                    modelBreakdownRows(cardHeight: cardHeight)
                        .opacity(breakdownMode == .model ? 1 : 0)
                }
            }
            .frame(maxWidth: .infinity)
        }
    }

    private func sourceBreakdownRows(cardHeight: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            if todaySummary.sourceRows.isEmpty {
                emptyBreakdownText(isRefreshing ? "Loading usage..." : "No usage recorded today")
            } else {
                ForEach(todaySummary.sourceRows.prefix(menuBarMaximumBreakdownRows), id: \.source) { row in
                    sourceBreakdownRow(row)
                        .frame(height: cardHeight, alignment: .leading)
                }
                overflowText(totalCount: todaySummary.sourceRows.count)
            }
        }
    }

    private func modelBreakdownRows(cardHeight: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            let rows = aggregatedModelRows
            if rows.isEmpty {
                emptyBreakdownText(isRefreshing ? "Loading models..." : "No model usage recorded today")
            } else {
                ForEach(rows.prefix(menuBarMaximumBreakdownRows)) { row in
                    modelBreakdownRow(row)
                        .frame(height: cardHeight, alignment: .leading)
                }
                overflowText(totalCount: rows.count)
            }
        }
    }

    private func sourceBreakdownRow(_ row: TodaySourceUsageRow) -> some View {
        HStack(spacing: 10) {
            UsageSourceIconBadge(source: row.source)
            Text(row.source.displayName)
                .lineLimit(1)
            Spacer()
            Text(row.totalTokens.tokenText)
                .font(.system(.caption, design: .monospaced))
            Text(currencyController.string(fromUSD: row.totalCostDecimal))
                .font(.system(.caption, design: .monospaced))
                .frame(width: 72, alignment: .trailing)
        }
        .font(.caption)
    }

    private func modelBreakdownRow(_ row: MenuBarModelUsageRow) -> some View {
        HStack(spacing: 10) {
            ProviderIconBadge(modelName: row.modelName)

            VStack(alignment: .leading, spacing: 1) {
                Text(displayModelName(row.modelName))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(row.modelName)
                Text(row.sourceText)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Text(row.totalTokens.tokenText)
                .font(.system(.caption, design: .monospaced))
            Text(currencyController.string(fromUSD: row.totalCostDecimal))
                .font(.system(.caption, design: .monospaced))
                .frame(width: 72, alignment: .trailing)
        }
        .font(.caption)
    }

    private func emptyBreakdownText(_ text: String) -> some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private func overflowText(totalCount: Int) -> some View {
        if totalCount > menuBarMaximumBreakdownRows {
            Text("+\(totalCount - menuBarMaximumBreakdownRows) more")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .trailing)
        }
    }

    private func statisticBlock(title: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(title.uppercased())
                .font(.caption2.weight(.medium))
                .foregroundStyle(.secondary)
            Text(value)
                .font(.system(size: 20, weight: .semibold, design: .monospaced))
                .lineLimit(1)
                .minimumScaleFactor(0.75)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .appCard()
    }

    private func refreshToday() async {
        guard !isRefreshing else { return }
        isRefreshing = true
        defer { isRefreshing = false }
        await store.refreshToday()
    }

    private var aggregatedModelRows: [MenuBarModelUsageRow] {
        let grouped = todaySummary.modelRows.reduce(into: [String: MenuBarModelUsageAccumulator]()) { totals, row in
            totals[row.modelName, default: MenuBarModelUsageAccumulator()].add(row)
        }

        return grouped
            .map { modelName, totals in
                MenuBarModelUsageRow(
                    modelName: modelName,
                    totalTokens: totals.totalTokens,
                    totalCost: totals.totalCost,
                    sources: totals.sources.sorted { $0.displayName < $1.displayName }
                )
            }
            .sorted {
                if $0.totalTokens == $1.totalTokens {
                    return $0.modelName < $1.modelName
                }
                return $0.totalTokens > $1.totalTokens
            }
    }
}

private struct MenuBarModelUsageAccumulator {
    var totalTokens = 0
    var totalCost = 0.0
    var sources: Set<UsageSource> = []

    mutating func add(_ row: TodayModelUsageRow) {
        totalTokens += row.totalTokens
        totalCost += row.totalCost
        sources.insert(row.source)
    }
}

private struct MenuBarModelUsageRow: Identifiable {
    var id: String { modelName }
    let modelName: String
    let totalTokens: Int
    let totalCost: Double
    let sources: [UsageSource]

    var totalCostDecimal: Decimal {
        Decimal(totalCost)
    }

    var sourceText: String {
        if sources.count == 1, let source = sources.first {
            return source.displayName
        }
        return "\(sources.count) CLI"
    }
}

private extension TodaySummaryResponse {
    var totalCostDecimal: Decimal {
        Decimal(totalCost)
    }

    var tokenText: String {
        totalTokens.tokenText
    }
}

private extension TodaySourceUsageRow {
    var totalCostDecimal: Decimal {
        Decimal(totalCost)
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

// MARK: - NSStatusItem + NSPopover bridge

/// Replaces SwiftUI's `MenuBarExtra(.window)` to avoid the well-known window
/// resize/flicker bug. Uses `NSStatusItem` + `NSPopover` which gives precise
/// control over the popover size and eliminates all scaling on interaction.
@MainActor
final class MenuBarStatusItemController: NSObject, NSPopoverDelegate {
    static let shared = MenuBarStatusItemController()

    private var statusItem: NSStatusItem?
    private var popover: NSPopover?
    private var eventMonitor: Any?
    private var hostingController: NSHostingController<TokenUsageMenuBarExtraView>?
    private weak var labelHostingController: NSHostingController<TokenUsageMenuBarLabel>?

    private let popoverWidth: CGFloat = 320
    private let popoverHeight: CGFloat = 420

    private(set) var store: LiveTokenUsageDashboardStore?
    private(set) var currencyController: TokenUsageBillingCurrencyController?

    private override init() {
        super.init()
    }

    func configure(store: LiveTokenUsageDashboardStore, currencyController: TokenUsageBillingCurrencyController) {
        self.store = store
        self.currencyController = currencyController
        setupStatusItem()
    }

    private func setupStatusItem() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem = item

        let labelView = TokenUsageMenuBarLabel(
            store: store!,
            currencyController: currencyController!
        )
        let labelController = NSHostingController(rootView: labelView)
        labelHostingController = labelController
        item.button?.subviews.removeAll()
        if let button = item.button {
            button.addSubview(labelController.view)
            labelController.view.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                labelController.view.leadingAnchor.constraint(equalTo: button.leadingAnchor),
                labelController.view.trailingAnchor.constraint(equalTo: button.trailingAnchor),
                labelController.view.topAnchor.constraint(equalTo: button.topAnchor),
                labelController.view.bottomAnchor.constraint(equalTo: button.bottomAnchor),
            ])
        }

        item.button?.target = self
        item.button?.action = #selector(togglePopover)
    }

    @objc private func togglePopover() {
        if popover?.isShown == true {
            closePopover()
        } else {
            showPopover()
        }
    }

    private func showPopover() {
        guard let store, let currencyController else { return }

        let contentView = TokenUsageMenuBarExtraView(
            store: store,
            currencyController: currencyController
        )
        let controller = NSHostingController(rootView: contentView)
        hostingController = controller

        let pop = NSPopover()
        pop.behavior = .transient
        pop.animates = false
        pop.delegate = self
        pop.contentViewController = controller
        pop.contentSize = NSSize(width: popoverWidth, height: popoverHeight)
        popover = pop

        controller.preferredContentSize = NSSize(width: popoverWidth, height: popoverHeight)

        pop.appearance = NSApp.effectiveAppearance

        if let button = statusItem?.button {
            pop.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        }

        eventMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] event in
            if self?.popover?.isShown == true {
                self?.closePopover()
            }
        }
    }

    private func closePopover() {
        popover?.performClose(nil)
        if let monitor = eventMonitor {
            NSEvent.removeMonitor(monitor)
            eventMonitor = nil
        }
    }

    func openDashboard() {
        closePopover()
        DispatchQueue.main.async {
            NSApp.activate(ignoringOtherApps: true)
            for window in NSApp.windows where window.title == "Token Usage" {
                window.makeKeyAndOrderFront(nil)
                return
            }
            // If window not found, try opening via NSApp
            NSApp.activate(ignoringOtherApps: true)
        }
    }
}
