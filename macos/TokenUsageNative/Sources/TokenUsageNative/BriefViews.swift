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
        let cellShape = RoundedRectangle(cornerRadius: 9, style: .continuous)
        let fillColor = intensity > 0
            ? Color.accentColor.opacity(intensity * 0.55)
            : Color.primary.opacity(isFuture ? 0.015 : 0.04)

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
            .background(fillColor, in: cellShape)
            .overlay(
                cellShape.stroke(isToday ? Color.accentColor : Color.clear, lineWidth: 1.5)
            )
            .contentShape(cellShape)
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

/// Vertical timeline of months, newest first, grouped by year. The rail node
/// size and tint encode each month's token volume relative to the busiest
/// month; tapping a row opens its month view.
private struct BriefAllView: View {
    let months: [BriefMonthEntry]
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController
    let onSelectMonth: (String) -> Void

    private enum Item: Identifiable {
        case year(String)
        case month(BriefMonthEntry)

        var id: String {
            switch self {
            case .year(let year): "year-\(year)"
            case .month(let month): month.id
            }
        }
    }

    private var maxTokens: Int {
        max(months.map(\.totalTokens).max() ?? 0, 1)
    }

    private var items: [Item] {
        var result: [Item] = []
        var currentYear = ""
        for month in months.sorted(by: { $0.month > $1.month }) {
            let year = String(month.month.prefix(4))
            if year != currentYear {
                currentYear = year
                result.append(.year(year))
            }
            result.append(.month(month))
        }
        return result
    }

    var body: some View {
        if months.isEmpty {
            Text("暂无历史数据。")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, minHeight: 200)
        } else {
            let items = self.items
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
                    let showsTopLine = index > 0
                    let showsBottomLine = index < items.count - 1
                    switch item {
                    case .year(let year):
                        BriefTimelineYearRow(
                            year: year,
                            showsTopLine: showsTopLine,
                            showsBottomLine: showsBottomLine
                        )
                    case .month(let month):
                        BriefTimelineMonthRow(
                            month: month,
                            maxTokens: maxTokens,
                            currencyController: currencyController,
                            showsTopLine: showsTopLine,
                            showsBottomLine: showsBottomLine,
                            onSelect: { onSelectMonth(month.month) }
                        )
                    }
                }
            }
            .padding(.vertical, 8)
            .appCard()
        }
    }
}

/// The vertical rail: a continuous 2pt spine with a node centered on the row.
private struct BriefTimelineRail: View {
    let nodeSize: CGFloat
    let nodeOpacity: Double
    let isCurrent: Bool
    let showsTopLine: Bool
    let showsBottomLine: Bool

    var body: some View {
        VStack(spacing: 0) {
            spine.opacity(showsTopLine ? 1 : 0)
            ZStack {
                if isCurrent {
                    Circle()
                        .strokeBorder(Color.accentColor.opacity(0.3), lineWidth: 3)
                        .frame(width: nodeSize + 9, height: nodeSize + 9)
                }
                Circle()
                    .fill(Color.accentColor.opacity(nodeOpacity))
                    .frame(width: nodeSize, height: nodeSize)
            }
            .frame(width: 26, height: max(nodeSize + 9, 20))
            spine.opacity(showsBottomLine ? 1 : 0)
        }
        .frame(width: 26)
    }

    private var spine: some View {
        RoundedRectangle(cornerRadius: 1, style: .continuous)
            .fill(Color.accentColor.opacity(0.16))
            .frame(width: 2)
            .frame(maxHeight: .infinity)
    }
}

/// Year separator on the timeline.
private struct BriefTimelineYearRow: View {
    let year: String
    let showsTopLine: Bool
    let showsBottomLine: Bool

    var body: some View {
        HStack(spacing: 14) {
            BriefTimelineRail(
                nodeSize: 6,
                nodeOpacity: 0.35,
                isCurrent: false,
                showsTopLine: showsTopLine,
                showsBottomLine: showsBottomLine
            )
            Text(year)
                .font(.title3.weight(.bold))
                .foregroundStyle(.secondary)
            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
    }
}

/// One month on the timeline: node, label, stats, usage bar, and the CLI
/// source icons (SVG marks) that were active that month.
private struct BriefTimelineMonthRow: View {
    let month: BriefMonthEntry
    let maxTokens: Int
    @ObservedObject var currencyController: TokenUsageBillingCurrencyController
    let showsTopLine: Bool
    let showsBottomLine: Bool
    let onSelect: () -> Void

    @State private var isHovering = false

    private var isCurrent: Bool {
        month.month == BriefDateHelper.currentMonthString
    }

    private var fraction: Double {
        min(1, max(0.04, Double(month.totalTokens) / Double(maxTokens)))
    }

    private var nodeSize: CGFloat {
        9 + 6 * CGFloat(sqrt(fraction))
    }

    private var knownSources: [UsageSource] {
        month.sources.compactMap(UsageSource.init(rawValue:))
    }

    var body: some View {
        Button(action: onSelect) {
            HStack(alignment: .top, spacing: 14) {
                BriefTimelineRail(
                    nodeSize: nodeSize,
                    nodeOpacity: 0.4 + 0.6 * fraction,
                    isCurrent: isCurrent,
                    showsTopLine: showsTopLine,
                    showsBottomLine: showsBottomLine
                )

                VStack(alignment: .leading, spacing: 7) {
                    header
                    stats
                    usageBar
                    footer
                }
                .padding(.vertical, 10)

                Spacer(minLength: 0)

                Image(systemName: "chevron.right")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.tertiary)
                    .padding(.top, 14)
            }
            .padding(.trailing, 12)
            .background(
                Color.primary.opacity(isHovering ? 0.045 : 0),
                in: RoundedRectangle(cornerRadius: 12, style: .continuous)
            )
            .contentShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        }
        .buttonStyle(.plain)
        .onHover { isHovering = $0 }
        .help("查看 \(BriefDateHelper.monthLabel(month.month)) 的月视图")
    }

    private var header: some View {
        HStack(spacing: 8) {
            Text(BriefDateHelper.shortMonthLabel(month.month))
                .font(.headline)
            if isCurrent {
                Text("本月")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(Color.accentColor)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.accentColor.opacity(0.12), in: Capsule())
            }
            Spacer(minLength: 0)
            Text(month.totalTokens.briefDayTokenText)
                .font(.system(.subheadline, design: .monospaced).weight(.semibold))
            Text(currencyController.string(fromUSD: Decimal(month.totalCost)))
                .font(.system(.subheadline, design: .monospaced))
                .foregroundStyle(.secondary)
        }
    }

    private var stats: some View {
        HStack(spacing: 12) {
            Label("\(month.activeDays) 活跃天", systemImage: "calendar")
            Label("\(month.projects) 项目", systemImage: "folder")
            Label("\(month.sessions) 次会话", systemImage: "terminal")
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .imageScale(.small)
    }

    private var usageBar: some View {
        GeometryReader { proxy in
            ZStack(alignment: .leading) {
                Capsule()
                    .fill(Color.primary.opacity(0.06))
                Capsule()
                    .fill(
                        LinearGradient(
                            colors: [Color.accentColor.opacity(0.55), Color.accentColor],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .frame(width: max(4, proxy.size.width * fraction))
            }
        }
        .frame(height: 4)
    }

    @ViewBuilder
    private var footer: some View {
        if !knownSources.isEmpty || !month.topProjects.isEmpty {
            HStack(spacing: 6) {
                ForEach(knownSources) { source in
                    UsageSourceIconBadge(source: source, size: 18)
                }
                if !month.topProjects.isEmpty {
                    Text(month.topProjects.joined(separator: "  ·  "))
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
            }
        }
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

    /// Short month label without the year ("7月"), used in the timeline where
    /// a year header already provides the year context.
    static func shortMonthLabel(_ month: String) -> String {
        guard let value = monthDayFormatter.date(from: "\(month)-01") else { return month }
        return shortMonthLabelFormatter.string(from: value)
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

    private static let shortMonthLabelFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "zh_CN")
        formatter.dateFormat = "M月"
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
