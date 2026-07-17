import SwiftUI
import UniformTypeIdentifiers

struct DailyBriefCard: View {
    let brief: TodayBriefResponse?
    let isLoading: Bool
    let isGenerating: Bool
    let enabledSources: Set<TokenUsageSource>
    let errorMessage: String?
    let onGenerate: () -> Void

    /// Per-CLI ordered project card IDs.
    @State private var projectOrderBySource: [String: [String]] = [:]
    /// Ordered CLI source keys for the board columns.
    @State private var sourceOrder: [String] = []
    @State private var draggingProjectID: String?
    @State private var draggingSourceID: String?
    /// Project cards keep details collapsed by default.
    @State private var expandedProjectIDs: Set<String> = []

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            panelHeader
            hourlyTimeline
            boardContent
                .frame(maxWidth: .infinity, minHeight: 360, alignment: .topLeading)
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
                    Text("Daily Brief")
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

            Button {
                onGenerate()
            } label: {
                if isGenerating {
                    ProgressView()
                        .controlSize(.small)
                } else {
                    Label(
                        brief?.status == "ok" ? "重新生成" : "生成",
                        systemImage: "sparkles"
                    )
                }
            }
            .disabled(isGenerating || isLoading)
            .controlSize(.regular)
        }
    }

    private var statusLine: String {
        if isGenerating && brief == nil {
            return "正在生成今日看板…"
        }
        if let errorMessage, brief == nil {
            return errorMessage
        }
        guard let brief else {
            return "尚未生成今日 Brief。可手动生成，或等待本地 10:00 自动生成。"
        }
        switch brief.status {
        case "ok":
            return brief.boardSummary
        case "error":
            return brief.error ?? "生成失败"
        default:
            return "尚未生成今日 Brief"
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
                .foregroundStyle(.primary)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)

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
                    Text("今日暂无项目卡片。")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, minHeight: 200, alignment: .center)
                } else {
                    ScrollView(.horizontal, showsIndicators: true) {
                        HStack(alignment: .top, spacing: 14) {
                            ForEach(orderedCLIGroups) { group in
                                cliColumn(group)
                                    .opacity(draggingSourceID == group.id ? 0.55 : 1)
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
                        .padding(.bottom, 4)
                    }

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
                Text("尚未生成今日 Brief。")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        } else {
            Text("尚未生成今日 Brief。可手动生成，或等待本地 10:00 自动生成。")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, minHeight: 200, alignment: .center)
        }
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
        .padding(12)
        .frame(width: 260, alignment: .topLeading)
        .background(Color(nsColor: .windowBackgroundColor), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(AppDesign.hairline.opacity(0.45), lineWidth: 1)
        )
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
        .background(
            AppDesign.cardBackground,
            in: RoundedRectangle(cornerRadius: 10, style: .continuous)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .stroke(AppDesign.hairline.opacity(0.45), lineWidth: 1)
        )
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
        withAnimation(.easeInOut(duration: 0.15)) {
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
        withAnimation(.easeInOut(duration: 0.15)) {
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
