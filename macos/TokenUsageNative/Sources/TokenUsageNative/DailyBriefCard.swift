import SwiftUI
import UniformTypeIdentifiers

struct DailyBriefCard: View {
    /// The date (YYYY-MM-DD) this card presents.
    let date: String
    let brief: TodayBriefResponse?
    let isLoading: Bool
    let isGenerating: Bool
    let enabledSources: Set<TokenUsageSource>
    let errorMessage: String?
    let onRegenerate: (BriefRegenerateMode) -> Void

    /// Per-CLI ordered project card IDs.
    @State private var projectOrderBySource: [String: [String]] = [:]
    /// Ordered CLI source keys for the board columns.
    @State private var sourceOrder: [String] = []
    @State private var draggingProjectID: String?
    @State private var draggingSourceID: String?
    /// Project cards keep details collapsed by default.
    @State private var expandedProjectIDs: Set<String> = []
    @State private var isHourPickerPresented = false
    @State private var isSourcePickerPresented = false
    @State private var selectedHours: Set<Int> = []
    @State private var selectedSources: Set<String> = []

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            panelHeader
            // Timeline and kanban board share one neutral grouped surface.
            VStack(alignment: .leading, spacing: 14) {
                hourlyTimeline
                boardContent
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .appGroupedSurface(cornerRadius: 14)
        }
        .padding(16)
        .appCard()
        .onChange(of: brief?.date) { _, _ in
            syncOrderFromBrief()
            expandedProjectIDs = []
        }
        .onChange(of: brief?.contentFingerprint) { _, _ in
            syncOrderFromBrief()
            expandedProjectIDs = []
        }
        .onAppear {
            syncOrderFromBrief()
        }
    }

    private var panelHeader: some View {
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text("Brief")
                        .font(.title3.weight(.semibold))
                    if let brief, brief.status == "ok" {
                        Text("\(orderedCLIGroups.count) CLI · \(totalProjectCount) 项目")
                            .font(.caption2.weight(.medium))
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 7)
                            .padding(.vertical, 3)
                            .background(Color.primary.opacity(0.05), in: Capsule())
                    }
                }

                Text(statusLine)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }

            Spacer(minLength: 0)

            if let brief, brief.status == "ok", sourcesChanged(for: brief) {
                Text("筛选已变")
                    .font(.caption2)
                    .foregroundStyle(AppPalette.semanticWarning)
            }

            if isGenerating {
                ProgressView()
                    .controlSize(.small)
            }

            HStack(spacing: 8) {
                Button {
                    onRegenerate(.full)
                } label: {
                    Label(
                        brief?.status == "ok" ? "重新生成" : "生成",
                        systemImage: "sparkles"
                    )
                }
                .disabled(isGenerating || isLoading)
                .controlSize(.regular)

                Button {
                    selectedHours = []
                    isHourPickerPresented = true
                } label: {
                    Label("按小时", systemImage: "clock")
                }
                .disabled(isGenerating || isLoading || brief == nil)
                .controlSize(.regular)
                .popover(isPresented: $isHourPickerPresented, arrowEdge: .bottom) {
                    hourPickerPopover
                }

                Button {
                    selectedSources = Set(brief?.enabledSources ?? enabledBriefSources.map(\.rawValue))
                    isSourcePickerPresented = true
                } label: {
                    Label("按 CLI", systemImage: "terminal")
                }
                .disabled(isGenerating || isLoading || enabledBriefSources.isEmpty)
                .controlSize(.regular)
                .popover(isPresented: $isSourcePickerPresented, arrowEdge: .bottom) {
                    sourcePickerPopover
                }
            }
        }
    }

    /// Brief-capable CLIs enabled in settings, sorted for a stable order.
    private var enabledBriefSources: [TokenUsageSource] {
        TokenUsageSource.allCases.filter {
            $0 != .all
                && enabledSources.contains($0)
                && TokenUsagePreferencesController.briefSupportedSources.contains($0)
        }
    }

    private var hourPickerPopover: some View {
        let hoursWithEntries = Set(brief?.hours?.map(\.hour) ?? [])
        let unresolvedHours = Set(
            (brief?.hours ?? [])
                .filter { $0.headline == Self.usageOnlyHeadline }
                .map(\.hour)
        )
        return VStack(alignment: .leading, spacing: 12) {
            Text("按小时重新生成")
                .font(.headline)
            Text("仅重新生成选中的小时，其余小时保留现有摘要。")
                .font(.caption)
                .foregroundStyle(.secondary)

            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 6), count: 6), spacing: 6) {
                ForEach(0..<24, id: \.self) { hour in
                    hourChip(hour, hasEntry: hoursWithEntries.contains(hour))
                }
            }

            HStack(spacing: 10) {
                if !unresolvedHours.isEmpty {
                    Button("选中 \(unresolvedHours.count) 个未解析小时") {
                        selectedHours = unresolvedHours
                    }
                    .font(.caption)
                    .buttonStyle(.plain)
                    .foregroundStyle(Color.accentColor)
                }
                Spacer()
                Button("生成") {
                    isHourPickerPresented = false
                    onRegenerate(.hours(selectedHours.sorted()))
                }
                .disabled(selectedHours.isEmpty)
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
            }
        }
        .padding(14)
        .frame(width: 320)
    }

    private func hourChip(_ hour: Int, hasEntry: Bool) -> some View {
        let isSelected = selectedHours.contains(hour)
        return Button {
            if isSelected {
                selectedHours.remove(hour)
            } else {
                selectedHours.insert(hour)
            }
        } label: {
            Text(String(format: "%02d:00", hour))
                .font(.system(.caption2, design: .monospaced))
                .frame(maxWidth: .infinity)
                .padding(.vertical, 5)
                .background(
                    isSelected ? Color.accentColor.opacity(0.85) : Color.primary.opacity(hasEntry ? 0.08 : 0.03),
                    in: RoundedRectangle(cornerRadius: 6, style: .continuous)
                )
                .foregroundStyle(isSelected ? Color.white : Color.primary.opacity(hasEntry ? 0.85 : 0.35))
        }
        .buttonStyle(.plain)
    }

    private var sourcePickerPopover: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("按 CLI 重新生成")
                .font(.headline)
            Text("仅重新生成选中 CLI 的项目卡片，其他 CLI 保留现有卡片。")
                .font(.caption)
                .foregroundStyle(.secondary)

            VStack(alignment: .leading, spacing: 4) {
                ForEach(enabledBriefSources) { source in
                    let isSelected = selectedSources.contains(source.rawValue)
                    Button {
                        if isSelected {
                            selectedSources.remove(source.rawValue)
                        } else {
                            selectedSources.insert(source.rawValue)
                        }
                    } label: {
                        HStack(spacing: 8) {
                            Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                                .foregroundStyle(isSelected ? Color.accentColor : Color.secondary)
                            if let usageSource = UsageSource(rawValue: source.rawValue) {
                                UsageSourceIconBadge(source: usageSource, size: 18)
                            }
                            Text(source.label)
                                .font(.subheadline)
                            Spacer()
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }

            HStack {
                Spacer()
                Button("生成") {
                    isSourcePickerPresented = false
                    onRegenerate(.sources(selectedSources.sorted()))
                }
                .disabled(selectedSources.isEmpty)
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
            }
        }
        .padding(14)
        .frame(width: 280)
    }

    /// Headline the backend uses for hours that only carry usage records.
    private static let usageOnlyHeadline = "仅有用量记录，无对话内容"

    private var isToday: Bool {
        date == Self.todayString
    }

    static var todayString: String {
        let formatter = DateFormatter()
        formatter.calendar = .current
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter.string(from: Date())
    }

    private var statusLine: String {
        if isGenerating && brief == nil {
            return "正在生成看板…"
        }
        if let errorMessage, brief == nil {
            return errorMessage
        }
        guard let brief else {
            return isToday
                ? "尚未生成今日 Brief。可手动生成，或等待北京时间 8:00 自动初始化。"
                : "该日尚未生成 Brief，可手动生成。"
        }
        switch brief.status {
        case "ok":
            return brief.boardSummary
        case "error":
            return brief.error ?? "生成失败"
        default:
            return isToday ? "尚未生成今日 Brief" : "该日尚未生成 Brief"
        }
    }

    @ViewBuilder
    private var hourlyTimeline: some View {
        if let hours = brief?.hours, !hours.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Text("Hour by Hour")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)

                VStack(alignment: .leading, spacing: 4) {
                    ForEach(hours.sorted { $0.hour < $1.hour }) { item in
                        hourlyRow(item)
                    }
                }
            }
        }
    }

    private func hourlyRow(_ item: HourlyBriefItem) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(Self.hourLabel(item.hour))
                .font(.system(.caption, design: .monospaced))
                .foregroundStyle(.secondary)
                .frame(width: 40, alignment: .leading)

            Text(item.headline)
                .font(.subheadline)
                .foregroundStyle(item.headline == Self.usageOnlyHeadline ? .tertiary : .primary)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)

            if item.headline == Self.usageOnlyHeadline {
                Image(systemName: "exclamationmark.circle")
                    .font(.caption2)
                    .foregroundStyle(AppPalette.semanticWarning)
                    .help("该小时只有用量记录，可用「按小时」重新生成解析")
            }

            Spacer(minLength: 8)

            Text(item.tokens.briefTokenText)
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .frame(minHeight: 24)
    }

    private static func hourLabel(_ hour: Int) -> String {
        switch hour {
        case 0: "12AM"
        case 12: "12PM"
        case 1...11: "\(hour)AM"
        default: "\(hour - 12)PM"
        }
    }

    @ViewBuilder
    private var boardContent: some View {
        if isGenerating && brief == nil {
            ProgressView("正在生成项目卡片…")
                .frame(maxWidth: .infinity, minHeight: 280, alignment: .center)
        } else if let errorMessage, brief == nil {
            Text(errorMessage)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, minHeight: 200, alignment: .center)
        } else if let brief {
            switch brief.status {
            case "ok":
                if orderedCLIGroups.isEmpty {
                    Text("暂无项目卡片。")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, minHeight: 200, alignment: .center)
                } else {
                    ScrollView(.horizontal, showsIndicators: true) {
                        boardColumns
                    }
                    .fixedSize(horizontal: false, vertical: true)

                    if let error = brief.error, !error.isEmpty {
                        Text(error)
                            .font(.caption2)
                            .foregroundStyle(AppPalette.semanticWarning)
                    }
                }
            case "error":
                VStack(alignment: .leading, spacing: 6) {
                    Text("生成失败")
                        .font(.subheadline.weight(.medium))
                    Text(brief.error ?? "未知错误")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, minHeight: 200, alignment: .leading)
            default:
                Text(isToday ? "尚未生成今日 Brief。" : "该日尚未生成 Brief。")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        } else {
            Text(
                isToday
                    ? "尚未生成今日 Brief。可手动生成，或等待北京时间 8:00 自动初始化。"
                    : "该日尚未生成 Brief，可手动生成。"
            )
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, minHeight: 200, alignment: .center)
        }
    }

    /// The kanban columns remain transparent inside the board's shared
    /// grouped surface, while each project card keeps an opaque background.
    private var boardColumns: some View {
        HStack(alignment: .top, spacing: 14) {
            ForEach(orderedCLIGroups) { group in
                cliColumn(group)
                    .opacity(draggingSourceID == group.id ? 0.55 : 1)
                    .animation(.easeOut(duration: 0.15), value: draggingSourceID)
                    .onDrag {
                        draggingSourceID = group.id
                        return NSItemProvider(object: group.id as NSString)
                    }
                    .onDrop(
                        of: [UTType.text],
                        delegate: BriefSourceDropDelegate(
                            targetSource: group.id,
                            sourceOrder: $sourceOrder,
                            draggingSourceID: $draggingSourceID,
                            onReorder: persistOrder
                        )
                    )
            }
        }
        // Keep the surface edge and horizontal scrollbar clear of the cards.
        .padding(.top, 2)
        .padding(.horizontal, 2)
        .padding(.bottom, 8)
    }

    private func cliColumn(_ group: BriefCLIGroup) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                if let source = UsageSource(rawValue: group.id) {
                    UsageSourceIconBadge(source: source)
                }
                Text(sourceLabel(for: group.id))
                    .font(.subheadline.weight(.semibold))
                Text("\(group.projects.count)")
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.primary.opacity(0.06), in: Capsule())
                Spacer(minLength: 0)
                Image(systemName: "line.3.horizontal")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                    .help("拖拽调整 CLI 顺序")
            }

            VStack(alignment: .leading, spacing: 10) {
                ForEach(group.projects) { card in
                    projectKanbanCard(card)
                        .opacity(draggingProjectID == card.id ? 0.5 : 1)
                        .animation(.easeOut(duration: 0.15), value: draggingProjectID)
                        .onDrag {
                            draggingProjectID = card.id
                            return NSItemProvider(object: card.id as NSString)
                        }
                        .onDrop(
                            of: [UTType.text],
                            delegate: BriefProjectDropDelegate(
                                source: group.id,
                                targetProjectID: card.id,
                                projectOrderBySource: $projectOrderBySource,
                                draggingProjectID: $draggingProjectID,
                                onReorder: persistOrder
                            )
                        )
                }
            }
        }
        .frame(width: 260, alignment: .topLeading)
    }

    private func projectKanbanCard(_ card: TodayBriefCardItem) -> some View {
        let isDetailExpanded = expandedProjectIDs.contains(card.id)

        return VStack(alignment: .leading, spacing: 8) {
            Button {
                withAnimation(.easeInOut(duration: 0.16)) {
                    if isDetailExpanded {
                        expandedProjectIDs.remove(card.id)
                    } else {
                        expandedProjectIDs.insert(card.id)
                    }
                }
            } label: {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 6) {
                        Image(systemName: isDetailExpanded ? "chevron.down" : "chevron.right")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.secondary)
                            .frame(width: 8)

                        Text(card.project)
                            .font(.caption.weight(.semibold))
                            .lineLimit(1)
                            .foregroundStyle(.primary)

                        Spacer(minLength: 0)

                        Image(systemName: "line.3.horizontal")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    }

                    Text(card.headline)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.primary)
                        .multilineTextAlignment(.leading)
                        .fixedSize(horizontal: false, vertical: true)

                    HStack(spacing: 8) {
                        Text("\(card.sessionCount) sessions")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                        if card.coverage != "full" {
                            Text(card.coverage)
                                .font(.caption2)
                                .foregroundStyle(AppPalette.semanticWarning)
                        }
                        Spacer(minLength: 0)
                        Text(isDetailExpanded ? "收起" : "详情")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if isDetailExpanded {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(card.bullets.prefix(5).enumerated()), id: \.offset) { _, bullet in
                        HStack(alignment: .top, spacing: 6) {
                            Circle()
                                .fill(Color.secondary.opacity(0.55))
                                .frame(width: 4, height: 4)
                                .padding(.top, 5)
                            Text(bullet)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
                .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .padding(11)
        .frame(maxWidth: .infinity, alignment: .topLeading)
        .appCard()
    }

    private var orderedCLIGroups: [BriefCLIGroup] {
        guard let brief else { return [] }
        let cards = brief.boardCards
        let grouped = Dictionary(grouping: cards, by: \.source)

        var sources = sourceOrder.filter { grouped[$0] != nil }
        for source in grouped.keys.sorted() where !sources.contains(source) {
            sources.append(source)
        }

        return sources.compactMap { source in
            guard let projects = grouped[source] else { return nil }
            let byID = Dictionary(uniqueKeysWithValues: projects.map { ($0.id, $0) })
            let saved = projectOrderBySource[source] ?? []
            var ordered: [TodayBriefCardItem] = []
            for id in saved {
                if let card = byID[id] {
                    ordered.append(card)
                }
            }
            for card in projects where !saved.contains(card.id) {
                ordered.append(card)
            }
            return BriefCLIGroup(id: source, projects: ordered)
        }
    }

    private var totalProjectCount: Int {
        orderedCLIGroups.reduce(0) { $0 + $1.projects.count }
    }

    private func syncOrderFromBrief() {
        guard let brief else {
            sourceOrder = []
            projectOrderBySource = [:]
            return
        }
        let cards = brief.boardCards
        let grouped = Dictionary(grouping: cards, by: \.source)
        let defaultSources = grouped.keys.sorted()

        if let savedSources = loadSavedSourceOrder(for: brief.date) {
            sourceOrder = savedSources.filter { grouped[$0] != nil }
                + defaultSources.filter { !savedSources.contains($0) }
        } else {
            sourceOrder = defaultSources
        }

        var projectOrders: [String: [String]] = [:]
        let savedProjects = loadSavedProjectOrder(for: brief.date)
        for (source, projects) in grouped {
            let ids = projects.map(\.id)
            if let saved = savedProjects[source] {
                projectOrders[source] = saved.filter { ids.contains($0) } + ids.filter { !saved.contains($0) }
            } else {
                projectOrders[source] = ids
            }
        }
        projectOrderBySource = projectOrders
    }

    private func persistOrder() {
        guard let date = brief?.date else { return }
        UserDefaults.standard.set(sourceOrder, forKey: sourceOrderKey(for: date))
        if let data = try? JSONEncoder().encode(projectOrderBySource) {
            UserDefaults.standard.set(data, forKey: projectOrderKey(for: date))
        }
    }

    private func loadSavedSourceOrder(for date: String) -> [String]? {
        UserDefaults.standard.stringArray(forKey: sourceOrderKey(for: date))
    }

    private func loadSavedProjectOrder(for date: String) -> [String: [String]] {
        guard let data = UserDefaults.standard.data(forKey: projectOrderKey(for: date)),
              let decoded = try? JSONDecoder().decode([String: [String]].self, from: data)
        else {
            return [:]
        }
        return decoded
    }

    private func sourceOrderKey(for date: String) -> String {
        "TokenUsage.briefSourceOrder.\(date)"
    }

    private func projectOrderKey(for date: String) -> String {
        "TokenUsage.briefProjectOrder.\(date)"
    }

    private func sourcesChanged(for brief: TodayBriefResponse) -> Bool {
        let briefSources = Set(brief.enabledSources.compactMap(TokenUsageSource.init(rawValue:)))
        let current = Set(
            enabledSources.filter { TokenUsagePreferencesController.briefSupportedSources.contains($0) }
        )
        return briefSources != current
    }

    private func sourceLabel(for raw: String) -> String {
        TokenUsageSource(rawValue: raw)?.label ?? raw
    }

}

private struct BriefCLIGroup: Identifiable {
    let id: String
    let projects: [TodayBriefCardItem]
}

private struct BriefSourceDropDelegate: DropDelegate {
    let targetSource: String
    @Binding var sourceOrder: [String]
    @Binding var draggingSourceID: String?
    let onReorder: () -> Void

    func dropEntered(info: DropInfo) {
        guard let fromID = draggingSourceID,
              fromID != targetSource,
              let from = sourceOrder.firstIndex(of: fromID),
              let to = sourceOrder.firstIndex(of: targetSource)
        else { return }
        withAnimation(.spring(duration: 0.35, bounce: 0.2)) {
            sourceOrder.move(fromOffsets: IndexSet(integer: from), toOffset: to > from ? to + 1 : to)
        }
    }

    func performDrop(info: DropInfo) -> Bool {
        draggingSourceID = nil
        onReorder()
        return true
    }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        DropProposal(operation: .move)
    }
}

private struct BriefProjectDropDelegate: DropDelegate {
    let source: String
    let targetProjectID: String
    @Binding var projectOrderBySource: [String: [String]]
    @Binding var draggingProjectID: String?
    let onReorder: () -> Void

    func dropEntered(info: DropInfo) {
        guard let draggingID = draggingProjectID,
              draggingID != targetProjectID
        else { return }
        var order = projectOrderBySource[source] ?? []
        guard let from = order.firstIndex(of: draggingID),
              let to = order.firstIndex(of: targetProjectID)
        else { return }
        withAnimation(.spring(duration: 0.35, bounce: 0.2)) {
            order.move(fromOffsets: IndexSet(integer: from), toOffset: to > from ? to + 1 : to)
            projectOrderBySource[source] = order
        }
    }

    func performDrop(info: DropInfo) -> Bool {
        draggingProjectID = nil
        onReorder()
        return true
    }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        DropProposal(operation: .move)
    }
}

/// Compact token count for the Hour by Hour rows — same format as the
/// dashboard's file-private `Int.tokenText`.
private extension Int {
    var briefTokenText: String {
        if self >= 1_000_000_000 { return String(format: "%.2fB", Double(self) / 1_000_000_000) }
        if self >= 1_000_000 { return String(format: "%.2fM", Double(self) / 1_000_000) }
        if self >= 1_000 { return String(format: "%.1fK", Double(self) / 1_000) }
        return formatted()
    }
}
