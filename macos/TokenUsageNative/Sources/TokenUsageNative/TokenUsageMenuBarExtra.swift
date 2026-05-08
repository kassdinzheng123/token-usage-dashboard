import SwiftUI

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

            sourceBreakdown

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

    private var sourceBreakdown: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Sources")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)

            if todaySummary.sourceRows.isEmpty {
                Text(store.isLoading ? "Loading usage..." : "No usage recorded today")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                ForEach(todaySummary.sourceRows, id: \.source) { row in
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
            }
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
