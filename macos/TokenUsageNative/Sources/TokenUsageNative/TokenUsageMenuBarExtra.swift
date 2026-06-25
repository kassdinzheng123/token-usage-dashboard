import SwiftUI

private let menuBarMaximumBreakdownRows = 6

private enum MenuBarBreakdownMode: String, CaseIterable, Identifiable {
    case cli
    case model

    var id: String { rawValue }

    var label: String {
        switch self {
        case .cli:
            "CLI"
        case .model:
            "Model"
        }
    }

    var title: String {
        switch self {
        case .cli:
            "By CLI"
        case .model:
            "By Model"
        }
    }
}

struct TokenUsageMenuBarLabel: View {
    @ObservedObject var store: LiveTokenUsageDashboardStore

    var body: some View {
        Label(todaySummary.tokenText, systemImage: "chart.bar.xaxis")
            .task {
                await refreshToday()
            }
    }

    private var todaySummary: TodaySummaryResponse {
        store.todaySummary
    }

    private func refreshToday() async {
        await store.refreshToday()
    }
}

struct TokenUsageMenuBarExtraView: View {
    @ObservedObject var store: LiveTokenUsageDashboardStore
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController
    @Environment(\.openWindow) private var openWindow
    @State private var breakdownMode: MenuBarBreakdownMode = .cli

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
                .disabled(store.isLoading)

                Button {
                    openWindow(id: "dashboard")
                } label: {
                    Label("Open Dashboard", systemImage: "rectangle.stack")
                }
            }
        }
        .padding(16)
        .frame(width: 320)
        .task {
            await refreshToday()
        }
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

            if store.isLoading {
                ProgressView()
                    .controlSize(.small)
            }
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

            switch breakdownMode {
            case .cli:
                sourceBreakdownRows
            case .model:
                modelBreakdownRows
            }
        }
    }

    private var sourceBreakdownRows: some View {
        VStack(alignment: .leading, spacing: 8) {
            if todaySummary.sourceRows.isEmpty {
                emptyBreakdownText(store.isLoading ? "Loading usage..." : "No usage recorded today")
            } else {
                ForEach(todaySummary.sourceRows.prefix(menuBarMaximumBreakdownRows), id: \.source) { row in
                    sourceBreakdownRow(row)
                }
                overflowText(totalCount: todaySummary.sourceRows.count)
            }
        }
    }

    private var modelBreakdownRows: some View {
        VStack(alignment: .leading, spacing: 8) {
            let rows = aggregatedModelRows
            if rows.isEmpty {
                emptyBreakdownText(store.isLoading ? "Loading models..." : "No model usage recorded today")
            } else {
                ForEach(rows.prefix(menuBarMaximumBreakdownRows)) { row in
                    modelBreakdownRow(row)
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
        .background(Color(nsColor: .controlBackgroundColor))
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
    }

    private func refreshToday() async {
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
