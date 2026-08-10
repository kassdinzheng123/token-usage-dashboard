import SwiftUI

/// Daily page: pick a bound project and a date, generate (or load) the daily
/// work report for that project, and manage project bindings. The report
/// aggregates every supported CLI's sessions under the project path and
/// merges the project's git commits of that day into a two-part narrative.
struct DailyReportPageView<Store: TokenUsageDashboardProviding>: View {
    @ObservedObject var store: Store

    @State private var selectedProject: String = ""
    @State private var selectedDate: Date = Date()
    @State private var isManagingProjects = false

    private let calendar = Calendar.current

    private var dateString: String {
        let formatter = DateFormatter()
        formatter.calendar = calendar
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = calendar.timeZone
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter.string(from: selectedDate)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            header

            if store.projects.isEmpty {
                emptyProjectsState
            } else {
                reportContent
            }
        }
        .onAppear {
            Task { await store.loadProjects() }
        }
        .onChange(of: store.projects) { _, projects in
            if selectedProject.isEmpty, let first = projects.first {
                selectedProject = first.name
                Task { await store.loadDailyReport(project: first.name, date: dateString) }
            }
        }
        .onChange(of: selectedProject) { _, project in
            guard !project.isEmpty else { return }
            Task { await store.loadDailyReport(project: project, date: dateString) }
        }
        .onChange(of: selectedDate) { _, _ in
            guard !selectedProject.isEmpty else { return }
            Task { await store.loadDailyReport(project: selectedProject, date: dateString) }
        }
        .sheet(isPresented: $isManagingProjects) {
            ProjectBindingEditor(store: store)
        }
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 12) {
            Picker("项目", selection: $selectedProject) {
                ForEach(store.projects) { project in
                    Text(project.name).tag(project.name)
                }
            }
            .labelsHidden()
            .frame(width: 200)

            DatePicker(
                "日期",
                selection: $selectedDate,
                displayedComponents: .date
            )
            .labelsHidden()
            .frame(width: 140)

            Button {
                Task {
                    await store.generateDailyReport(
                        project: selectedProject,
                        date: dateString
                    )
                }
            } label: {
                if store.isGeneratingDailyReport {
                    ProgressView()
                        .controlSize(.small)
                } else {
                    Label("生成日报", systemImage: "wand.and.stars")
                }
            }
            .disabled(
                selectedProject.isEmpty || store.isGeneratingDailyReport
            )

            Spacer(minLength: 0)

            Button("管理项目") {
                isManagingProjects = true
            }
        }
    }

    // MARK: - States

    private var emptyProjectsState: some View {
        VStack(spacing: 12) {
            Image(systemName: "folder.badge.plus")
                .font(.system(size: 32))
                .foregroundStyle(.secondary)
            Text("还没有绑定项目")
                .font(.headline)
            Text("绑定「名称 ↔ 本地路径」后，即可聚合该目录下所有 CLI 的会话并读取 git 提交，生成一日工作纪要。")
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 420)
            Button("添加项目绑定") {
                isManagingProjects = true
            }
            .buttonStyle(.borderedProminent)
        }
        .frame(maxWidth: .infinity, minHeight: 320)
    }

    private var emptyReportState: some View {
        VStack(spacing: 12) {
            Image(systemName: "doc.text")
                .font(.system(size: 32))
                .foregroundStyle(.secondary)
            Text("尚未生成日报")
                .font(.headline)
            Text("点击「生成日报」，结合当日 CLI 会话与 git 提交生成工作纪要。")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, minHeight: 320)
    }

    private func errorBox(_ message: String) -> some View {
        Label(message, systemImage: "exclamationmark.triangle")
            .font(.callout)
            .foregroundStyle(.red)
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.red.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
    }

    // MARK: - Report

    @ViewBuilder
    private var reportContent: some View {
        if let report = store.dailyReport {
            if report.status == "error" && report.overview.isEmpty {
                errorBox(report.error ?? "生成失败")
            } else {
                VStack(alignment: .leading, spacing: 16) {
                    if let error = report.error {
                        errorBox(error)
                    }
                    statsRow(report)
                    overviewCard(report)
                    workItemsList(report)
                }
            }
        } else if store.isGeneratingDailyReport {
            ProgressView("正在聚合会话与 git 提交，生成日报…")
                .frame(maxWidth: .infinity, minHeight: 320)
        } else if let error = store.dailyReportError {
            errorBox(error)
        } else {
            emptyReportState
        }
    }

    private func statsRow(_ report: DailyReport) -> some View {
        HStack(spacing: 8) {
            statChip("\(report.sessionCount) 个会话")
            statChip("\(report.commitCount) 次提交")
            statChip("\(report.tokenTotal.formatted()) tokens")
            statChip(report.coverageLabel)
        }
        .font(.caption)
    }

    private func statChip(_ text: String) -> some View {
        Text(text)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(.quaternary.opacity(0.5), in: Capsule())
    }

    private func overviewCard(_ report: DailyReport) -> some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                Text("今日概览")
                    .font(.headline)
                Text(report.overview)
                    .font(.callout)
                    .lineSpacing(4)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func workItemsList(_ report: DailyReport) -> some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 12) {
                Text("完成的工作")
                    .font(.headline)
                if report.workItems.isEmpty {
                    Text("未能提炼出具体事项。")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(report.workItems) { item in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(item.title)
                                .font(.subheadline.weight(.semibold))
                            if !item.detail.isEmpty {
                                Text(item.detail)
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                                    .lineSpacing(3)
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.vertical, 2)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// Sheet for managing project bindings: upsert by name + path, delete rows.
private struct ProjectBindingEditor<Store: TokenUsageDashboardProviding>: View {
    @ObservedObject var store: Store
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var path = ""
    @State private var isSaving = false

    private var canSave: Bool {
        !name.trimmingCharacters(in: .whitespaces).isEmpty
            && !path.trimmingCharacters(in: .whitespaces).isEmpty
            && !isSaving
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("项目绑定")
                .font(.headline)
            Text("名称用于展示与选择；路径用于聚合该目录下所有 CLI 的会话，并读取其中的 git 提交。")
                .font(.caption)
                .foregroundStyle(.secondary)

            Form {
                TextField("名称（如 token-usage）", text: $name)
                TextField("本地路径（如 ~/CodeSpace/token-usage）", text: $path)
            }
            .formStyle(.grouped)

            if let error = store.dailyReportError {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.red)
            }

            HStack {
                Spacer()
                Button("关闭") {
                    dismiss()
                }
                .keyboardShortcut(.cancelAction)
                Button("添加 / 更新") {
                    isSaving = true
                    Task {
                        let ok = await store.addProject(name: name, path: path)
                        isSaving = false
                        if ok {
                            name = ""
                            path = ""
                        }
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(!canSave)
            }

            if !store.projects.isEmpty {
                Divider()
                Text("已绑定")
                    .font(.subheadline.weight(.medium))
                ScrollView {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(store.projects) { project in
                            HStack(spacing: 8) {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(project.name)
                                        .font(.callout.weight(.medium))
                                    Text(project.path)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                }
                                Spacer()
                                Button(role: .destructive) {
                                    Task { await store.removeProject(name: project.name) }
                                } label: {
                                    Image(systemName: "trash")
                                }
                                .buttonStyle(.borderless)
                                .help("删除绑定")
                            }
                            .padding(8)
                            .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 8))
                        }
                    }
                }
                .frame(maxHeight: 200)
            }
        }
        .padding(20)
        .frame(width: 440)
    }
}
