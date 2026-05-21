import SwiftUI

struct ContentView: View {
    @Bindable var model: AppModel

    var body: some View {
        NavigationSplitView {
            List(AppPage.allCases, selection: $model.selectedPage) { page in
                Label(page.title, systemImage: page.symbolName)
                    .tag(page)
            }
            .navigationTitle("WeeklyReport")
            .safeAreaInset(edge: .bottom) {
                SidebarFooter(model: model)
            }
        } detail: {
            PageContainer(title: model.selectedPage.title) {
                switch model.selectedPage {
                case .workspaces:
                    WorkspacesPage(model: model)
                case .providers:
                    ProvidersPage(model: model)
                case .templates:
                    TemplatesPage(templates: model.templates)
                case .reports:
                    ReportsPage(model: model)
                case .schedules:
                    SchedulesPage(schedules: model.schedules)
                case .settings:
                    SettingsPage(model: model)
                }
            }
        }
        .alert("加载失败", isPresented: Binding(
            get: { model.loadError != nil },
            set: { if !$0 { model.loadError = nil } }
        )) {
            Button("好") { model.loadError = nil }
        } message: {
            Text(model.loadError ?? "")
        }
        .alert(item: $model.notice) { notice in
            Alert(
                title: Text(notice.title),
                message: Text(notice.message),
                dismissButton: .default(Text("好"))
            )
        }
    }
}

private struct SidebarFooter: View {
    let model: AppModel

    var body: some View {
        VStack(spacing: 10) {
            Button {
                Task { await model.generateReportNow() }
            } label: {
                Label(model.isGenerating ? "正在生成" : "生成周报", systemImage: "sparkles")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .disabled(model.isGenerating || model.isLoading)

            Button {
                Task { await model.load() }
            } label: {
                Label(model.isLoading ? "正在刷新" : "刷新数据", systemImage: "arrow.clockwise")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)
            .disabled(model.isLoading)

            Button {
                model.openDataDirectory()
            } label: {
                Label("打开数据目录", systemImage: "folder")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)
            .disabled(model.dataDirectory == nil)
        }
        .padding()
        .background(.bar)
    }
}

private struct PageContainer<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text(title)
                .font(.largeTitle.weight(.semibold))

            content
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .padding(28)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}

private struct WorkspacesPage: View {
    let model: AppModel
    @State private var activeSheet: WorkspaceEditorSheet?
    @State private var workspaceToDelete: Workspace?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("配置 Claude Code / Codex 日志来源。")
                    .foregroundStyle(.secondary)
                Spacer()
                Button {
                    activeSheet = .add
                } label: {
                    Label("添加工作区", systemImage: "plus")
                }
                .buttonStyle(.borderedProminent)
            }

            if model.workspaces.isEmpty {
                EmptyState(title: "还没有工作区", message: "添加本机工作区后即可从本机日志生成周报。")
            } else {
                Table(model.workspaces) {
                    TableColumn("名称") { workspace in
                        Label(workspace.name, systemImage: workspace.type == .local ? "desktopcomputer" : "server.rack")
                    }
                    TableColumn("类型") { workspace in
                        Text(workspace.type.label)
                    }
                    TableColumn("工具") { workspace in
                        Text(workspace.tools.joined(separator: ", "))
                    }
                    TableColumn("Claude 路径") { workspace in
                        Text(workspace.claudePath ?? "-")
                            .font(.system(.body, design: .monospaced))
                    }
                    TableColumn("Codex 路径") { workspace in
                        Text(workspace.codexPath ?? "-")
                            .font(.system(.body, design: .monospaced))
                    }
                    TableColumn("操作") { workspace in
                        HStack(spacing: 6) {
                            Button {
                                activeSheet = .edit(workspace)
                            } label: {
                                Label("编辑", systemImage: "pencil")
                            }
                            .labelStyle(.iconOnly)
                            .buttonStyle(.borderless)
                            .help("编辑")

                            Button(role: .destructive) {
                                workspaceToDelete = workspace
                            } label: {
                                Label("删除", systemImage: "trash")
                            }
                            .labelStyle(.iconOnly)
                            .buttonStyle(.borderless)
                            .help("删除")
                        }
                    }
                    .width(88)
                }
            }
        }
        .sheet(item: $activeSheet) { sheet in
            WorkspaceEditorView(model: model, workspace: sheet.workspace)
        }
        .confirmationDialog(
            "删除工作区？",
            isPresented: Binding(
                get: { workspaceToDelete != nil },
                set: { if !$0 { workspaceToDelete = nil } }
            ),
            presenting: workspaceToDelete
        ) { workspace in
            Button("删除", role: .destructive) {
                delete(workspace)
            }
        } message: { workspace in
            Text("将删除「\(workspace.name)」的配置，不会删除日志文件。")
        }
    }

    private func delete(_ workspace: Workspace) {
        do {
            try model.deleteWorkspace(id: workspace.id)
        } catch {
            model.notice = AppNotice(title: "删除失败", message: error.localizedDescription)
        }
    }
}

private struct ProvidersPage: View {
    let model: AppModel
    @State private var activeSheet: ProviderEditorSheet?
    @State private var providerToDelete: LLMProvider?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("配置报告生成时使用的模型服务。")
                    .foregroundStyle(.secondary)
                Spacer()
                Button {
                    activeSheet = .add
                } label: {
                    Label("添加 LLM 源", systemImage: "plus")
                }
                .buttonStyle(.borderedProminent)
            }

            if model.providers.isEmpty {
                EmptyState(title: "还没有 LLM 源", message: "添加 Provider 后即可调用模型生成周报。")
            } else {
                Table(model.providers) {
                    TableColumn("名称") { provider in
                        HStack {
                            Text(provider.name)
                            if provider.isDefault {
                                Text("默认")
                                    .font(.caption)
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(.blue.opacity(0.12), in: Capsule())
                            }
                        }
                    }
                    TableColumn("协议") { Text($0.kind.label) }
                    TableColumn("模型") { Text($0.model.isEmpty ? "默认" : $0.model).font(.system(.body, design: .monospaced)) }
                    TableColumn("推理") { Text($0.effectiveReasoningEffort?.label ?? "默认") }
                    TableColumn("端点 / 命令") { Text($0.endpointLabel).font(.system(.body, design: .monospaced)) }
                    TableColumn("操作") { provider in
                        HStack(spacing: 6) {
                            Button {
                                setDefault(provider)
                            } label: {
                                Label("设为默认", systemImage: provider.isDefault ? "checkmark.seal.fill" : "checkmark.seal")
                            }
                            .labelStyle(.iconOnly)
                            .buttonStyle(.borderless)
                            .disabled(provider.isDefault)
                            .help("设为默认")

                            Button {
                                activeSheet = .edit(provider)
                            } label: {
                                Label("编辑", systemImage: "pencil")
                            }
                            .labelStyle(.iconOnly)
                            .buttonStyle(.borderless)
                            .help("编辑")

                            Button(role: .destructive) {
                                providerToDelete = provider
                            } label: {
                                Label("删除", systemImage: "trash")
                            }
                            .labelStyle(.iconOnly)
                            .buttonStyle(.borderless)
                            .help("删除")
                        }
                    }
                    .width(116)
                }
            }
        }
        .sheet(item: $activeSheet) { sheet in
            ProviderEditorView(model: model, provider: sheet.provider)
        }
        .confirmationDialog(
            "删除 LLM 源？",
            isPresented: Binding(
                get: { providerToDelete != nil },
                set: { if !$0 { providerToDelete = nil } }
            ),
            presenting: providerToDelete
        ) { provider in
            Button("删除", role: .destructive) {
                delete(provider)
            }
        } message: { provider in
            Text("将删除「\(provider.name)」的配置。")
        }
    }

    private func setDefault(_ provider: LLMProvider) {
        do {
            try model.setDefaultProvider(id: provider.id)
        } catch {
            model.notice = AppNotice(title: "设置失败", message: error.localizedDescription)
        }
    }

    private func delete(_ provider: LLMProvider) {
        do {
            try model.deleteProvider(id: provider.id)
        } catch {
            model.notice = AppNotice(title: "删除失败", message: error.localizedDescription)
        }
    }
}

private struct TemplatesPage: View {
    let templates: [ReportTemplate]

    var body: some View {
        Table(templates) {
            TableColumn("名称") { template in
                HStack {
                    Text(template.name)
                    if template.builtin {
                        Text("内置")
                            .font(.caption)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(.secondary.opacity(0.12), in: Capsule())
                    }
                }
            }
            TableColumn("风格") { Text($0.style) }
            TableColumn("章节") { Text($0.sections.joined(separator: " / ")) }
        }
    }
}

private struct ReportsPage: View {
    let model: AppModel
    @State private var selectedReportID: ReportRecord.ID?

    private var selectedReport: ReportRecord? {
        model.reports.first { $0.id == selectedReportID }
    }

    var body: some View {
        if model.reports.isEmpty {
            EmptyState(title: "还没有历史周报", message: "生成后的 Markdown 会显示在这里。")
        } else {
            HStack(spacing: 18) {
                Table(model.reports, selection: $selectedReportID) {
                    TableColumn("周期") { Text($0.week) }
                    TableColumn("模板") { Text($0.templateName) }
                    TableColumn("项目数") { Text("\($0.projectCount)") }
                    TableColumn("Tokens") { Text("\($0.tokensUsed)") }
                    TableColumn("时间") { Text($0.generatedAt) }
                }
                .frame(minWidth: 520)

                ScrollView {
                    Text(selectedReport.map(model.reportBodyPreview(for:)) ?? "选择一份报告查看正文。")
                        .font(.system(.body, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding()
                }
                .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
            }
        }
    }
}

private struct SchedulesPage: View {
    let schedules: [Schedule]

    var body: some View {
        if schedules.isEmpty {
            EmptyState(title: "还没有定时任务", message: "原生版本后续会接入后台调度和邮件发送。")
        } else {
            Table(schedules) {
                TableColumn("名称") { Text($0.name) }
                TableColumn("状态") { Text($0.enabled ? "启用" : "停用") }
                TableColumn("Cron") { Text($0.cron).font(.system(.body, design: .monospaced)) }
                TableColumn("收件人") { Text($0.recipients.joined(separator: ", ")) }
                TableColumn("上次状态") { Text($0.lastStatus ?? "-") }
            }
        }
    }
}

private struct SettingsPage: View {
    let model: AppModel
    @State private var settingsDraft: SettingsDraft
    @State private var smtpDraft: SMTPDraft
    @State private var status: SettingsStatus?
    @State private var presetHint: String?

    init(model: AppModel) {
        self.model = model
        _settingsDraft = State(initialValue: SettingsDraft(settings: model.settings))
        _smtpDraft = State(initialValue: SMTPDraft(config: model.smtpConfig))
    }

    var body: some View {
        Form {
            Section("生成设置") {
                LabeledContent("AI 回复裁剪字符数") {
                    TextField("", text: $settingsDraft.promptClipChars)
                        .font(.system(.body, design: .monospaced))
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 120)
                }

                LabeledContent("历史周报参考数量") {
                    TextField("", text: $settingsDraft.pastReportsContext)
                        .font(.system(.body, design: .monospaced))
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 120)
                }

                HStack {
                    Spacer()
                    Button {
                        saveSettings()
                    } label: {
                        Label("保存生成设置", systemImage: "checkmark")
                    }
                    .buttonStyle(.borderedProminent)
                }
            }

            Section("SMTP 邮箱") {
                LabeledContent("快速预设") {
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 116), spacing: 8)],
                        alignment: .leading,
                        spacing: 8
                    ) {
                        ForEach(SMTPPreset.presets) { preset in
                            Button(preset.name) {
                                smtpDraft.apply(preset: preset)
                                presetHint = preset.hint
                                status = nil
                            }
                            .buttonStyle(.bordered)
                            .frame(maxWidth: .infinity)
                        }
                    }
                }

                if let presetHint {
                    Text(presetHint)
                        .foregroundStyle(.secondary)
                }

                LabeledContent("SMTP Host") {
                    TextField("", text: $smtpDraft.host, prompt: Text("smtp.example.com"))
                        .font(.system(.body, design: .monospaced))
                        .textFieldStyle(.roundedBorder)
                }

                LabeledContent("Port") {
                    TextField("", text: $smtpDraft.port, prompt: Text(smtpDraft.useSSL ? "465" : "587"))
                        .font(.system(.body, design: .monospaced))
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 120)
                }

                Picker("加密方式", selection: $smtpDraft.useSSL) {
                    Text("STARTTLS").tag(false)
                    Text("SSL / TLS").tag(true)
                }
                .pickerStyle(.segmented)

                LabeledContent("用户名（邮箱）") {
                    TextField("", text: $smtpDraft.username, prompt: Text("you@example.com"))
                        .font(.system(.body, design: .monospaced))
                        .textFieldStyle(.roundedBorder)
                }

                LabeledContent("密码 / 授权码") {
                    SecureField("", text: $smtpDraft.password)
                        .textFieldStyle(.roundedBorder)
                }

                LabeledContent("发件人显示名") {
                    TextField("", text: $smtpDraft.fromName, prompt: Text("WeeklyReport 周报助手"))
                        .textFieldStyle(.roundedBorder)
                }

                HStack {
                    Spacer()
                    Button {
                        saveSMTPConfig()
                    } label: {
                        Label("保存 SMTP", systemImage: "checkmark")
                    }
                    .buttonStyle(.borderedProminent)
                }
            }

            Section("数据目录") {
                LabeledContent("路径") {
                    Text(model.dataDirectory?.path ?? "-")
                        .font(.system(.body, design: .monospaced))
                        .textSelection(.enabled)
                }
            }

            if let status {
                Section {
                    Text(status.message)
                        .foregroundStyle(status.color)
                }
            }
        }
        .formStyle(.grouped)
        .frame(maxWidth: 760, alignment: .leading)
        .onChange(of: model.settings) { _, settings in
            settingsDraft = SettingsDraft(settings: settings)
        }
        .onChange(of: model.smtpConfig) { _, config in
            smtpDraft = SMTPDraft(config: config)
        }
    }

    private func saveSettings() {
        do {
            let settings = try settingsDraft.makeSettings()
            try model.saveSettings(settings)
            status = .success("已保存生成设置")
        } catch {
            status = .failure(error.localizedDescription)
        }
    }

    private func saveSMTPConfig() {
        do {
            let config = try smtpDraft.makeConfig()
            try model.saveSMTPConfig(config)
            status = .success("已保存 SMTP 配置")
        } catch {
            status = .failure(error.localizedDescription)
        }
    }
}

private enum SettingsStatus {
    case success(String)
    case failure(String)

    var message: String {
        switch self {
        case let .success(message), let .failure(message):
            message
        }
    }

    var color: Color {
        switch self {
        case .success:
            .green
        case .failure:
            .red
        }
    }
}

private struct SettingsDraft: Equatable {
    var promptClipChars: String
    var pastReportsContext: String

    init(settings: AppSettings) {
        promptClipChars = String(settings.promptClipChars)
        pastReportsContext = String(settings.pastReportsContext)
    }

    func makeSettings() throws -> AppSettings {
        guard let parsedPromptClipChars = Int(promptClipChars.trimmingCharacters(in: .whitespacesAndNewlines)) else {
            throw AppConfigError.validation("AI 回复裁剪字符数必须是数字")
        }
        guard let parsedPastReportsContext = Int(pastReportsContext.trimmingCharacters(in: .whitespacesAndNewlines)) else {
            throw AppConfigError.validation("历史周报参考数量必须是数字")
        }

        return AppSettings(
            promptClipChars: parsedPromptClipChars,
            pastReportsContext: parsedPastReportsContext
        )
    }
}

private struct SMTPDraft: Equatable {
    var host: String
    var port: String
    var username: String
    var password: String
    var fromName: String
    var useSSL: Bool

    init(config: SMTPConfig) {
        host = config.host
        port = String(config.port)
        username = config.username
        password = config.password
        fromName = config.fromName
        useSSL = config.useSSL
    }

    mutating func apply(preset: SMTPPreset) {
        host = preset.host
        port = String(preset.port)
        useSSL = preset.useSSL
    }

    func makeConfig() throws -> SMTPConfig {
        let trimmedPort = port.trimmingCharacters(in: .whitespacesAndNewlines)
        let parsedPort: Int
        if trimmedPort.isEmpty {
            parsedPort = useSSL ? 465 : 587
        } else if let value = Int(trimmedPort) {
            parsedPort = value
        } else {
            throw AppConfigError.validation("SMTP 端口必须是数字")
        }

        return SMTPConfig(
            host: host,
            port: parsedPort,
            username: username,
            password: password,
            fromName: fromName,
            useSSL: useSSL
        )
    }
}

private struct SMTPPreset: Identifiable {
    var id: String { name }
    var name: String
    var host: String
    var port: Int
    var useSSL: Bool
    var hint: String

    static let presets = [
        SMTPPreset(
            name: "Gmail",
            host: "smtp.gmail.com",
            port: 465,
            useSSL: true,
            hint: "Gmail 需要开启两步验证并使用应用专用密码。"
        ),
        SMTPPreset(
            name: "Outlook 365",
            host: "smtp.office365.com",
            port: 587,
            useSSL: false,
            hint: "Microsoft 账号需在管理后台开启 SMTP AUTH。"
        ),
        SMTPPreset(
            name: "QQ 邮箱",
            host: "smtp.qq.com",
            port: 465,
            useSSL: true,
            hint: "密码字段填写 QQ 邮箱授权码，不是登录密码。"
        ),
        SMTPPreset(
            name: "163 邮箱",
            host: "smtp.163.com",
            port: 465,
            useSSL: true,
            hint: "密码字段填写网易邮箱客户端授权密码。"
        ),
        SMTPPreset(
            name: "企业微信邮箱",
            host: "smtp.exmail.qq.com",
            port: 465,
            useSSL: true,
            hint: "使用企业邮箱登录密码或专用授权码。"
        ),
    ]
}

private struct EmptyState: View {
    let title: String
    let message: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.headline)
            Text(message)
                .foregroundStyle(.secondary)
        }
        .padding(24)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.quaternary, in: RoundedRectangle(cornerRadius: 10))
    }
}
