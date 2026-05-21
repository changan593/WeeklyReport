import AppKit
import SwiftUI

enum WorkspaceEditorSheet: Identifiable {
    case add
    case edit(Workspace)

    var id: String {
        switch self {
        case .add:
            "add"
        case let .edit(workspace):
            workspace.id
        }
    }

    var workspace: Workspace? {
        switch self {
        case .add:
            nil
        case let .edit(workspace):
            workspace
        }
    }
}

enum ProviderEditorSheet: Identifiable {
    case add
    case edit(LLMProvider)

    var id: String {
        switch self {
        case .add:
            "add"
        case let .edit(provider):
            provider.id
        }
    }

    var provider: LLMProvider? {
        switch self {
        case .add:
            nil
        case let .edit(provider):
            provider
        }
    }
}

struct WorkspaceEditorView: View {
    let model: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var draft: WorkspaceDraft
    @State private var errorMessage: String?

    init(model: AppModel, workspace: Workspace?) {
        self.model = model
        _draft = State(initialValue: WorkspaceDraft(workspace: workspace))
    }

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section("基本") {
                    TextField("名称", text: $draft.name)

                    Picker("类型", selection: $draft.type) {
                        ForEach(WorkspaceKind.allCases, id: \.self) { kind in
                            Text(kind.label).tag(kind)
                        }
                    }
                }

                Section("日志") {
                    Toggle("Claude Code", isOn: $draft.includeClaude)
                        .toggleStyle(.checkbox)
                    if draft.includeClaude {
                        PathField(
                            title: "Claude 路径",
                            path: $draft.claudePath,
                            placeholder: draft.type == .local ? "~/.claude" : "~/.claude",
                            mode: .directory
                        )
                    }

                    Toggle("Codex", isOn: $draft.includeCodex)
                        .toggleStyle(.checkbox)
                    if draft.includeCodex {
                        PathField(
                            title: "Codex 路径",
                            path: $draft.codexPath,
                            placeholder: draft.type == .local ? "~/.codex" : "~/.codex",
                            mode: .directory
                        )
                    }
                }

                if draft.type == .ssh {
                    Section("SSH") {
                        TextField("Host", text: $draft.host)
                        TextField("User", text: $draft.user)
                        TextField("Port", text: $draft.port)

                        Picker("认证", selection: $draft.authMethod) {
                            ForEach(SshAuthMethod.allCases, id: \.self) { method in
                                Text(method.label).tag(method)
                            }
                        }

                        if draft.authMethod == .key {
                            PathField(
                                title: "密钥路径",
                                path: $draft.sshKey,
                                placeholder: "~/.ssh/id_ed25519",
                                mode: .file
                            )
                        } else {
                            SecureField("密码", text: $draft.sshPassword)
                        }
                    }
                }

                if let errorMessage {
                    Section {
                        Text(errorMessage)
                            .foregroundStyle(.red)
                    }
                }
            }
            .formStyle(.grouped)
            .padding(20)

            Divider()
            EditorFooter(
                cancelTitle: "取消",
                saveTitle: "保存",
                onCancel: { dismiss() },
                onSave: save
            )
        }
        .frame(width: 600)
        .frame(minHeight: 460)
        .onChange(of: draft.type) { _, type in
            draft.applyDefaults(for: type)
        }
    }

    private func save() {
        do {
            let workspace = try draft.makeWorkspace()
            try model.saveWorkspace(workspace)
            dismiss()
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}

struct ProviderEditorView: View {
    let model: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var draft: ProviderDraft
    @State private var selectedPresetID: String
    @State private var errorMessage: String?

    init(model: AppModel, provider: LLMProvider?) {
        self.model = model
        let draft = ProviderDraft(provider: provider)
        _draft = State(initialValue: draft)
        _selectedPresetID = State(initialValue: provider == nil ? (LLMProvider.presets.first?.id ?? "custom") : "custom")
    }

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section("预设") {
                    Picker("Provider", selection: $selectedPresetID) {
                        ForEach(LLMProvider.presets) { preset in
                            Text(preset.name).tag(preset.id)
                        }
                    }
                }

                Section("基本") {
                    TextField("名称", text: $draft.name)

                    Picker("协议", selection: $draft.kind) {
                        ForEach(LLMKind.allCases, id: \.self) { kind in
                            Text(kind.label).tag(kind)
                        }
                    }

                    if draft.kind.isCommandLineProvider {
                        TextField("CLI 命令", text: $draft.commandPath)
                            .font(.system(.body, design: .monospaced))
                        ModelField(title: "模型（可选）", kind: draft.kind, model: $draft.model)
                        ReasoningEffortPicker(selection: $draft.reasoningEffort)
                    } else {
                        TextField("Base URL", text: $draft.baseURL)
                        SecureField("API Key", text: $draft.apiKey)
                        ModelField(title: "模型", kind: draft.kind, model: $draft.model)
                        ReasoningEffortPicker(selection: $draft.reasoningEffort)
                        TextField("max tokens", text: $draft.maxTokens)
                        TextField("temperature", text: $draft.temperature)
                    }

                    Toggle("设为默认", isOn: $draft.isDefault)
                        .toggleStyle(.checkbox)
                }

                if let errorMessage {
                    Section {
                        Text(errorMessage)
                            .foregroundStyle(.red)
                    }
                }
            }
            .formStyle(.grouped)
            .padding(20)

            Divider()
            EditorFooter(
                cancelTitle: "取消",
                saveTitle: "保存",
                onCancel: { dismiss() },
                onSave: save
            )
        }
        .frame(width: 600)
        .frame(minHeight: 520)
        .onChange(of: selectedPresetID) { _, presetID in
            guard let preset = LLMProvider.presets.first(where: { $0.id == presetID }) else { return }
            draft.apply(preset: preset)
        }
        .onChange(of: draft.kind) { _, kind in
            draft.applyDefaults(for: kind)
        }
    }

    private func save() {
        do {
            let provider = try draft.makeProvider()
            try model.saveProvider(provider)
            dismiss()
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}

private struct WorkspaceDraft: Equatable {
    var id: String
    var name: String
    var type: WorkspaceKind
    var host: String
    var user: String
    var port: String
    var authMethod: SshAuthMethod
    var sshKey: String
    var sshPassword: String
    var claudePath: String
    var codexPath: String
    var includeClaude: Bool
    var includeCodex: Bool

    init(workspace: Workspace?) {
        if let workspace {
            id = workspace.id
            name = workspace.name
            type = workspace.type
            host = workspace.host ?? ""
            user = workspace.user ?? ""
            port = workspace.port.map(String.init) ?? ""
            authMethod = workspace.authMethod
            sshKey = workspace.sshKey ?? ""
            sshPassword = workspace.sshPassword ?? ""
            claudePath = workspace.claudePath ?? Self.defaultClaudePath(for: workspace.type)
            codexPath = workspace.codexPath ?? Self.defaultCodexPath(for: workspace.type)
            includeClaude = workspace.tools.contains("claude-code")
            includeCodex = workspace.tools.contains("codex")
        } else {
            id = UUID().uuidString
            name = "本机"
            type = .local
            host = ""
            user = ""
            port = "22"
            authMethod = .key
            sshKey = ""
            sshPassword = ""
            claudePath = Self.defaultClaudePath(for: .local)
            codexPath = Self.defaultCodexPath(for: .local)
            includeClaude = true
            includeCodex = true
        }
    }

    mutating func applyDefaults(for type: WorkspaceKind) {
        switch type {
        case .local:
            if claudePath == "~/.claude" {
                claudePath = Self.defaultClaudePath(for: .local)
            }
            if codexPath == "~/.codex" {
                codexPath = Self.defaultCodexPath(for: .local)
            }
        case .ssh:
            if claudePath == Self.defaultClaudePath(for: .local) {
                claudePath = "~/.claude"
            }
            if codexPath == Self.defaultCodexPath(for: .local) {
                codexPath = "~/.codex"
            }
        }
    }

    func makeWorkspace() throws -> Workspace {
        let parsedPort: Int?
        let trimmedPort = port.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmedPort.isEmpty {
            parsedPort = nil
        } else if let value = Int(trimmedPort), value > 0 {
            parsedPort = value
        } else {
            throw AppConfigError.validation("SSH 端口必须是大于 0 的数字")
        }

        var tools: [String] = []
        if includeClaude {
            tools.append("claude-code")
        }
        if includeCodex {
            tools.append("codex")
        }
        guard !tools.isEmpty else {
            throw AppConfigError.validation("至少选择一个日志工具")
        }

        if type == .ssh {
            guard !host.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                throw AppConfigError.validation("SSH Host 不能为空")
            }
            guard !user.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                throw AppConfigError.validation("SSH User 不能为空")
            }
        }

        return Workspace(
            id: id,
            name: name,
            type: type,
            host: type == .ssh ? host.nilIfBlank : nil,
            user: type == .ssh ? user.nilIfBlank : nil,
            port: type == .ssh ? parsedPort : nil,
            authMethod: type == .ssh ? authMethod : .key,
            sshKey: type == .ssh && authMethod == .key ? sshKey.nilIfBlank : nil,
            sshPassword: type == .ssh && authMethod == .password ? sshPassword.nilIfBlank : nil,
            claudePath: includeClaude ? claudePath.nilIfBlank : nil,
            codexPath: includeCodex ? codexPath.nilIfBlank : nil,
            tools: tools
        )
    }

    private static func defaultClaudePath(for type: WorkspaceKind) -> String {
        switch type {
        case .local:
            FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude").path
        case .ssh:
            "~/.claude"
        }
    }

    private static func defaultCodexPath(for type: WorkspaceKind) -> String {
        switch type {
        case .local:
            FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".codex").path
        case .ssh:
            "~/.codex"
        }
    }
}

private struct ProviderDraft: Equatable {
    var id: String
    var name: String
    var kind: LLMKind
    var baseURL: String
    var apiKey: String
    var model: String
    var maxTokens: String
    var temperature: String
    var isDefault: Bool
    var extraHeaders: [String: String]
    var commandPath: String
    var reasoningEffort: ReasoningEffort?

    init(provider: LLMProvider?) {
        if let provider {
            id = provider.id
            name = provider.name
            kind = provider.kind
            baseURL = provider.baseURL
            apiKey = provider.apiKey
            model = provider.model
            maxTokens = String(provider.maxTokens)
            temperature = provider.temperature.map { String($0) } ?? ""
            isDefault = provider.isDefault
            extraHeaders = provider.extraHeaders
            commandPath = provider.commandExecutable
            reasoningEffort = provider.reasoningEffort
        } else if let preset = LLMProvider.presets.first {
            let provider = LLMProvider.makePreset(preset)
            id = provider.id
            name = provider.name
            kind = provider.kind
            baseURL = provider.baseURL
            apiKey = provider.apiKey
            model = provider.model
            maxTokens = String(provider.maxTokens)
            temperature = provider.temperature.map { String($0) } ?? ""
            isDefault = false
            extraHeaders = [:]
            commandPath = provider.commandExecutable
            reasoningEffort = provider.reasoningEffort
        } else {
            id = UUID().uuidString
            name = ""
            kind = .openAICompatible
            baseURL = ""
            apiKey = ""
            model = ""
            maxTokens = "2048"
            temperature = "0.7"
            isDefault = false
            extraHeaders = [:]
            commandPath = ""
            reasoningEffort = nil
        }
    }

    mutating func apply(preset: LLMProviderPreset) {
        name = preset.name
        kind = preset.kind
        baseURL = preset.baseURL
        model = preset.model
        apiKey = preset.apiKey
        maxTokens = String(preset.maxTokens)
        temperature = preset.temperature.map { String($0) } ?? ""
        commandPath = preset.commandPath ?? preset.kind.defaultCommandPath ?? ""
        reasoningEffort = preset.reasoningEffort
    }

    mutating func applyDefaults(for kind: LLMKind) {
        if kind.isCommandLineProvider, commandPath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            commandPath = kind.defaultCommandPath ?? ""
        }
        if kind.isAPIProvider, maxTokens.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || maxTokens == "0" {
            maxTokens = "2048"
        }
    }

    func makeProvider() throws -> LLMProvider {
        let parsedMaxTokens: Int
        if kind.isAPIProvider {
            guard let value = Int(maxTokens.trimmingCharacters(in: .whitespacesAndNewlines)),
                  value > 0
            else {
                throw AppConfigError.validation("max tokens 必须是大于 0 的数字")
            }
            parsedMaxTokens = value
        } else {
            parsedMaxTokens = Int(maxTokens.trimmingCharacters(in: .whitespacesAndNewlines)) ?? 0
        }

        let parsedTemperature: Double?
        let trimmedTemperature = temperature.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmedTemperature.isEmpty {
            parsedTemperature = nil
        } else if let value = Double(trimmedTemperature) {
            parsedTemperature = value
        } else {
            throw AppConfigError.validation("temperature 必须是数字或留空")
        }

        return LLMProvider(
            id: id,
            name: name,
            kind: kind,
            baseURL: kind.isAPIProvider ? baseURL : "",
            apiKey: kind.isAPIProvider ? apiKey : "",
            model: model,
            maxTokens: parsedMaxTokens,
            temperature: kind.isAPIProvider ? parsedTemperature : nil,
            isDefault: isDefault,
            extraHeaders: extraHeaders,
            commandPath: kind.isCommandLineProvider ? commandPath.nilIfBlank : nil,
            reasoningEffort: reasoningEffort
        )
    }
}

private struct ReasoningEffortPicker: View {
    @Binding var selection: ReasoningEffort?

    var body: some View {
        Picker("推理强度", selection: $selection) {
            Text("使用默认").tag(nil as ReasoningEffort?)
            ForEach(ReasoningEffort.allCases, id: \.self) { effort in
                Text(effort.label).tag(Optional(effort))
            }
        }
    }
}

private struct ModelField: View {
    var title: String
    var kind: LLMKind
    @Binding var model: String

    var body: some View {
        LabeledContent(title) {
            HStack(spacing: 8) {
                TextField("", text: $model)
                    .font(.system(.body, design: .monospaced))

                if !kind.modelSuggestions.isEmpty {
                    Menu {
                        ForEach(kind.modelSuggestions, id: \.self) { suggestion in
                            Button(suggestion) {
                                model = suggestion
                            }
                        }
                    } label: {
                        Label("选择模型", systemImage: "list.bullet")
                    }
                    .labelStyle(.iconOnly)
                    .help("选择常用模型")
                }
            }
        }
    }
}

private enum PathMode {
    case directory
    case file
}

private struct PathField: View {
    var title: String
    @Binding var path: String
    var placeholder: String
    var mode: PathMode

    var body: some View {
        LabeledContent(title) {
            HStack(spacing: 8) {
                TextField("", text: $path, prompt: Text(placeholder))
                    .font(.system(.body, design: .monospaced))

                Button {
                    choosePath()
                } label: {
                    Label("选择", systemImage: mode == .directory ? "folder" : "doc")
                }
                .labelStyle(.iconOnly)
                .help("选择")
            }
        }
    }

    private func choosePath() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = mode == .file
        panel.canChooseDirectories = mode == .directory
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = mode == .directory
        if panel.runModal() == .OK, let url = panel.url {
            path = url.path
        }
    }
}

private struct EditorFooter: View {
    var cancelTitle: String
    var saveTitle: String
    var onCancel: () -> Void
    var onSave: () -> Void

    var body: some View {
        HStack {
            Spacer()
            Button(cancelTitle, action: onCancel)
            Button(saveTitle, action: onSave)
                .keyboardShortcut(.defaultAction)
        }
        .padding(16)
        .background(.bar)
    }
}

private extension String {
    var nilIfBlank: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
