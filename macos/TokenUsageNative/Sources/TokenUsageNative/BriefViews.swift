import SwiftUI

/// Brief page with a day / month / all progression: the all view drills into
/// a month, the month view drills into a day.
struct BriefPageView<Store: TokenUsageDashboardProviding>: View {
    @ObservedObject var store: Store
    @ObservedObject var preferences: TokenUsagePreferencesController
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController

    @State private var scope: BriefScope = .day
    @State private var focusedDate: String = BriefDateHelper.todayString
    @State private var focusedMonth: String = BriefDateHelper.currentMonthString

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            scopeHeader

            switch scope {
            case .day:
                dayContent
            case .month:
                monthContent
            case .all:
                allContent
            }
        }
        .onAppear {
            Task { await store.loadBrief(for: focusedDate) }
        }
    }

    // MARK: - Header

    private var scopeHeader: some View {
        HStack(spacing: 12) {
            Picker("Brief scope", selection: $scope) {
                ForEach(BriefScope.allCases) { scope in
                    Text(scope.label).tag(scope)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 220)

            Spacer(minLength: 0)

            switch scope {
            case .day:
                dayNavigation
            case .month:
                monthNavigation
            case .all:
                EmptyView()
            }
        }
    }

    private var dayNavigation: some View {
        HStack(spacing: 8) {
            Button {
                focusedDate = BriefDateHelper.date(focusedDate, addingDays: -1)
                Task { await store.loadBrief(for: focusedDate) }
            } label: {
                Image(systemName: "chevron.left")
            }
            .controlSize(.small)

            Text(BriefDateHelper.dayLabel(focusedDate))
                .font(.subheadline.weight(.medium))
                .frame(minWidth: 110)

            Button {
                focusedDate = BriefDateHelper.date(focusedDate, addingDays: 1)
                Task { await store.loadBrief(for: focusedDate) }
            } label: {
                Image(systemName: "chevron.right")
            }
            .controlSize(.small)
            .disabled(focusedDate >= BriefDateHelper.todayString)

            if focusedDate != BriefDateHelper.todayString {
                Button("回到今天") {
                    focusedDate = BriefDateHelper.todayString
                }
                .controlSize(.small)
            }
        }
    }

    private var monthNavigation: some View {
        HStack(spacing: 8) {
            Button {
                focusedMonth = BriefDateHelper.month(focusedMonth, addingMonths: -1)
            } label: {
                Image(systemName: "chevron.left")
            }
            .controlSize(.small)

            Text(BriefDateHelper.monthLabel(focusedMonth))
                .font(.subheadline.weight(.medium))
                .frame(minWidth: 110)

            Button {
                focusedMonth = BriefDateHelper.month(focusedMonth, addingMonths: 1)
            } label: {
                Image(systemName: "chevron.right")
            }
            .controlSize(.small)
            .disabled(focusedMonth >= BriefDateHelper.currentMonthString)

            if focusedMonth != BriefDateHelper.currentMonthString {
                Button("回到本月") {
                    focusedMonth = BriefDateHelper.currentMonthString
                }
                .controlSize(.small)
            }
        }
    }

    // MARK: - Day

    private var dayContent: some View {
        DailyBriefCard(
            date: focusedDate,
            brief: briefForFocusedDate,
            isLoading: store.isLoading,
            isGenerating: store.isGeneratingBrief,
            enabledSources: preferences.enabledSources,
            errorMessage: store.briefErrorMessage,
            onRegenerate: { mode in
                Task { await store.generateBrief(for: focusedDate, mode: mode) }
            }
        )
        .id(focusedDate)
    }

    private var briefForFocusedDate: TodayBriefResponse? {
        if focusedDate == BriefDateHelper.todayString, let todayBrief = store.todayBrief {
            return todayBrief
        }
        return store.briefCache[focusedDate]
    }

    // MARK: - Month

    private var monthContent: some View {
        BriefMonthView(
            month: focusedMonth,
            days: store.briefDays,
            currencyController: currencyController,
            onSelectDate: { date in
                focusedDate = date
                Task { await store.loadBrief(for: date) }
                withAnimation(.easeInOut(duration: 0.18)) {
                    scope = .day
                }
            }
        )
        .task(id: focusedMonth) {
            await store.loadBriefDays(month: focusedMonth)
        }
    }

    // MARK: - All

    private var allContent: some View {
        BriefAllView(
            months: store.briefMonths,
            currencyController: currencyController,
            onSelectMonth: { month in
                focusedMonth = month
                withAnimation(.easeInOut(duration: 0.18)) {
                    scope = .month
                }
            }
        )
        .task {
            await store.loadBriefMonths()
        }
    }
}

private enum BriefScope: String, CaseIterable, Identifiable {
    case day
    case month
    case all

    var id: String { rawValue }

    var label: String {
        switch self {
        case .day: "日"
        case .month: "月"
        case .all: "全部"
        }
    }
}

// MARK: - Month view

/// Month calendar of day cards. Color intensity encodes how many projects the
/// day touched (from its brief); tapping a day opens the day view.
private struct BriefMonthView: View {
    let month: String
    let days: [BriefDayEntry]
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController
    let onSelectDate: (String) -> Void

    private let columns = Array(repeating: GridItem(.flexible(), spacing: 8), count: 7)

    private var entriesByDate: [String: BriefDayEntry] {
        Dictionary(uniqueKeysWithValues: days.map { ($0.date, $0) })
    }

    private var maxProjects: Int {
        max(days.compactMap(\.projects).max() ?? 0, 1)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 0) {
                ForEach(Array(BriefDateHelper.weekdaySymbols.enumerated()), id: \.offset) { _, symbol in
                    Text(symbol)
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity)
                }
            }

            LazyVGrid(columns: columns, spacing: 8) {
                ForEach(Array(BriefDateHelper.calendarCells(for: month).enumerated()), id: \.offset) { _, cell in
                    if let date = cell {
                        dayCell(date: date, entry: entriesByDate[date])
                    } else {
                        Color.clear
                            .frame(minHeight: 86)
                    }
                }
            }

            HStack(spacing: 6) {
                Text("项目")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                Text("少")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                HStack(spacing: 2) {
                    ForEach([0.15, 0.35, 0.6, 0.9], id: \.self) { intensity in
                        RoundedRectangle(cornerRadius: 2, style: .continuous)
                            .fill(Color.accentColor.opacity(intensity))
                            .frame(width: 12, height: 10)
                    }
                }
                Text("多")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(16)
        .appCard()
    }

    private func dayCell(date: String, entry: BriefDayEntry?) -> some View {
        let projects = entry?.projects
        let intensity = projects.map { max(0.12, sqrt(Double($0) / Double(maxProjects))) } ?? 0
        let isToday = date == BriefDateHelper.todayString
        let isFuture = date > BriefDateHelper.todayString

        return Button {
            onSelectDate(date)
        } label: {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(BriefDateHelper.dayOfMonth(date))
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(isToday ? Color.accentColor : .primary)
                    Spacer(minLength: 0)
                    if let projects, projects > 0 {
                        Text("\(projects) 项目")
                            .font(.system(size: 9, weight: .semibold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 2)
                            .background(Color.accentColor.opacity(0.85), in: Capsule())
                    }
                }

                Spacer(minLength: 0)

                if let entry, entry.totalTokens > 0 {
                    Text(entry.totalTokens.briefDayTokenText)
                        .font(.system(size: 10, weight: .medium, design: .monospaced))
                        .foregroundStyle(.secondary)
                }
                if let topProject = entry?.topProjects.first {
                    Text(topProject)
                        .font(.system(size: 9))
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                } else if entry != nil, entry?.hasBrief == false {
                    Text("未生成 Brief")
                        .font(.system(size: 9))
                        .foregroundStyle(.quaternary)
                }
            }
            .padding(8)
            .frame(maxWidth: .infinity, minHeight: 86, alignment: .topLeading)
            .background {
                if intensity > 0 {
                    RoundedRectangle(cornerRadius: 9, style: .continuous)
                        .fill(Color.accentColor.opacity(intensity * 0.55))
                } else {
                    RoundedRectangle(cornerRadius: 9, style: .continuous)
                        .fill(Color.primary.opacity(isFuture ? 0.015 : 0.04))
                }
            }
            .overlay(
                RoundedRectangle(cornerRadius: 9, style: .continuous)
                    .stroke(isToday ? Color.accentColor : Color.clear, lineWidth: 1.5)
            )
            .contentShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
        }
        .buttonStyle(.plain)
        .help(dayTooltip(date: date, entry: entry))
    }

    private func dayTooltip(date: String, entry: BriefDayEntry?) -> String {
        var lines = [date]
        if let entry {
            lines.append("\(entry.sessions) sessions · \(entry.totalTokens.briefDayTokenText) tokens")
            if let projects = entry.projects {
                lines.append("\(projects) 个项目")
            }
            if !entry.topProjects.isEmpty {
                lines.append(entry.topProjects.joined(separator: ", "))
            }
            if let summary = entry.briefSummary, !summary.isEmpty {
                lines.append(summary)
            }
        } else {
            lines.append("无用量记录")
        }
        return lines.joined(separator: "\n")
    }
}

// MARK: - All view

/// Left-to-right timeline of month cards. Tapping a month opens its month view.
private struct BriefAllView: View {
    let months: [BriefMonthEntry]
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController
    let onSelectMonth: (String) -> Void

    var body: some View {
        if months.isEmpty {
            Text("暂无历史数据。")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, minHeight: 200)
        } else {
            ScrollViewReader { proxy in
                ScrollView(.horizontal, showsIndicators: true) {
                    HStack(alignment: .top, spacing: 14) {
                        ForEach(months) { month in
                            monthCard(month)
                                .id(month.id)
                        }
                    }
                    .padding(.vertical, 4)
                }
                .onAppear {
                    if let last = months.last {
                        proxy.scrollTo(last.id, anchor: .trailing)
                    }
                }
            }
        }
    }

    private func monthCard(_ month: BriefMonthEntry) -> some View {
        Button {
            onSelectMonth(month.month)
        } label: {
            VStack(alignment: .leading, spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(BriefDateHelper.monthLabel(month.month))
                        .font(.headline)
                    Text(month.month)
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(.tertiary)
                }

                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(month.totalTokens.briefDayTokenText)
                            .font(.system(.subheadline, design: .monospaced).weight(.semibold))
                        Text("tokens")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    VStack(alignment: .leading, spacing: 2) {
                        Text(currencyController.string(fromUSD: Decimal(month.totalCost)))
                            .font(.system(.subheadline, design: .monospaced).weight(.semibold))
                        Text("cost")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }

                Text("\(month.activeDays) 活跃天 · \(month.projects) 项目")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                if !month.topProjects.isEmpty {
                    VStack(alignment: .leading, spacing: 3) {
                        ForEach(month.topProjects, id: \.self) { project in
                            HStack(spacing: 5) {
                                Circle()
                                    .fill(Color.accentColor.opacity(0.7))
                                    .frame(width: 4, height: 4)
                                Text(project)
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                                    .truncationMode(.tail)
                            }
                        }
                    }
                }

                Spacer(minLength: 0)
            }
            .padding(14)
            .frame(width: 220)
            .frame(minHeight: 180, alignment: .topLeading)
            .contentShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        }
        .buttonStyle(.plain)
        .appGlassCard(cornerRadius: 12)
    }
}

// MARK: - Date helpers

enum BriefDateHelper {
    static var todayString: String {
        dayFormatter.string(from: Date())
    }

    static var currentMonthString: String {
        monthFormatter.string(from: Date())
    }

    static var weekdaySymbols: [String] {
        // Week starts on Monday, matching the activity heatmap.
        ["一", "二", "三", "四", "五", "六", "日"]
    }

    /// Calendar cells for a month: nil placeholders pad the first week so day
    /// 1 lands on its weekday column.
    static func calendarCells(for month: String) -> [String?] {
        guard let first = monthDayFormatter.date(from: "\(month)-01") else {
            return []
        }
        let calendar = mondayCalendar
        guard let range = calendar.range(of: .day, in: .month, for: first) else {
            return []
        }
        // weekday: 1 = Sunday ... 7 = Saturday; Monday-first offset.
        let weekday = calendar.component(.weekday, from: first)
        let leadingBlanks = (weekday + 5) % 7

        var cells: [String?] = Array(repeating: nil, count: leadingBlanks)
        for day in range {
            cells.append("\(month)-\(String(format: "%02d", day))")
        }
        return cells
    }

    static func date(_ date: String, addingDays days: Int) -> String {
        guard let value = dayFormatter.date(from: date),
              let shifted = mondayCalendar.date(byAdding: .day, value: days, to: value)
        else { return date }
        return dayFormatter.string(from: shifted)
    }

    static func month(_ month: String, addingMonths months: Int) -> String {
        guard let value = monthDayFormatter.date(from: "\(month)-01"),
              let shifted = mondayCalendar.date(byAdding: .month, value: months, to: value)
        else { return month }
        return monthFormatter.string(from: shifted)
    }

    static func dayOfMonth(_ date: String) -> String {
        String(date.suffix(2))
    }

    static func dayLabel(_ date: String) -> String {
        guard let value = dayFormatter.date(from: date) else { return date }
        return dayLabelFormatter.string(from: value)
    }

    static func monthLabel(_ month: String) -> String {
        guard let value = monthDayFormatter.date(from: "\(month)-01") else { return month }
        return monthLabelFormatter.string(from: value)
    }

    private static let mondayCalendar: Calendar = {
        var calendar = Calendar(identifier: .gregorian)
        calendar.firstWeekday = 2
        return calendar
    }()

    private static let dayFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter
    }()

    private static let monthDayFormatter = dayFormatter

    private static let monthFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM"
        return formatter
    }()

    private static let dayLabelFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "zh_CN")
        formatter.dateFormat = "M月d日 EEEE"
        return formatter
    }()

    private static let monthLabelFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "zh_CN")
        formatter.dateFormat = "yyyy年M月"
        return formatter
    }()
}

private extension Int {
    var briefDayTokenText: String {
        if self >= 1_000_000_000 { return String(format: "%.2fB", Double(self) / 1_000_000_000) }
        if self >= 1_000_000 { return String(format: "%.2fM", Double(self) / 1_000_000) }
        if self >= 1_000 { return String(format: "%.1fK", Double(self) / 1_000) }
        return formatted()
    }
}
