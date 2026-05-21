import Foundation
import AppKit
import Observation

@MainActor
@Observable
final class AppModel {
    private let store: JSONStore

    var selectedPage: AppPage = .workspaces
    var workspaces: [Workspace] = []
    var providers: [LLMProvider] = []
    var templates: [ReportTemplate] = []
    var reports: [ReportRecord] = []
    var schedules: [Schedule] = []
    var settings: AppSettings = .defaults
    var smtpConfig = SMTPConfig.defaults
    var dataDirectory: URL?
    var loadError: String?
    var isLoading = false
    var isGenerating = false
    var notice: AppNotice?

    init(store: JSONStore) {
        self.store = store
    }

    func load() async {
        isLoading = true
        defer { isLoading = false }

        do {
            try store.ensureInitialized()
            dataDirectory = try store.dataDirectory

            workspaces = try store.read([Workspace].self, from: "workspaces.json", default: [])
            if workspaces.isEmpty {
                workspaces = [Self.defaultWorkspace()]
                try store.write(workspaces, to: "workspaces.json")
            }

            providers = try store.read([LLMProvider].self, from: "llm_providers.json", default: [])
                .map { provider in
                    var copy = provider
                    if copy.kind.isCommandLineProvider, copy.reasoningEffort == nil {
                        copy.reasoningEffort = .medium
                    }
                    return copy
                }

            let customTemplates = try store.read([ReportTemplate].self, from: "templates.json", default: [])
                .filter { !$0.builtin && !$0.id.hasPrefix("builtin-") }
            templates = ReportTemplate.builtins + customTemplates

            reports = try store.read([ReportRecord].self, from: "reports/index.json", default: [])
                .sorted { $0.generatedAt > $1.generatedAt }
            schedules = try store.read([Schedule].self, from: "schedules.json", default: [])
            settings = try store.read(AppSettings.self, from: "settings.json", default: .defaults)
            smtpConfig = try store.read(
                SMTPConfig.self,
                from: "smtp.json",
                default: .defaults
            )

            loadError = nil
        } catch {
            loadError = error.localizedDescription
        }
    }

    func openDataDirectory() {
        guard let dataDirectory else { return }
        NSWorkspace.shared.open(dataDirectory)
    }

    @discardableResult
    func saveWorkspace(_ workspace: Workspace) throws -> Workspace {
        var saved = workspace.normalizedForStorage()
        if saved.id.isEmpty {
            saved.id = UUID().uuidString
        }
        guard !saved.name.isEmpty else {
            throw AppConfigError.validation("工作区名称不能为空")
        }

        if let existing = workspaces.firstIndex(where: { $0.id == saved.id }) {
            workspaces[existing] = saved
        } else {
            workspaces.append(saved)
        }
        try store.write(workspaces, to: "workspaces.json")
        return saved
    }

    func deleteWorkspace(id: String) throws {
        let before = workspaces.count
        workspaces.removeAll { $0.id == id }
        guard workspaces.count != before else { return }
        try store.write(workspaces, to: "workspaces.json")
    }

    @discardableResult
    func saveProvider(_ provider: LLMProvider) throws -> LLMProvider {
        var saved = provider.normalizedForStorage()
        if saved.id.isEmpty {
            saved.id = UUID().uuidString
        }
        guard !saved.name.isEmpty else {
            throw AppConfigError.validation("LLM 源名称不能为空")
        }
        if saved.kind.isAPIProvider {
            guard !saved.baseURL.isEmpty else {
                throw AppConfigError.validation("Base URL 不能为空")
            }
            guard !saved.model.isEmpty else {
                throw AppConfigError.validation("模型名称不能为空")
            }
            guard saved.maxTokens > 0 else {
                throw AppConfigError.validation("max tokens 必须大于 0")
            }
        } else if saved.commandExecutable.isEmpty {
            throw AppConfigError.validation("CLI 命令不能为空")
        }

        let wasEmpty = providers.isEmpty
        if wasEmpty {
            saved.isDefault = true
        }
        if saved.isDefault {
            for index in providers.indices where providers[index].id != saved.id {
                providers[index].isDefault = false
            }
        }

        if let existing = providers.firstIndex(where: { $0.id == saved.id }) {
            providers[existing] = saved
        } else {
            providers.append(saved)
        }

        if !providers.isEmpty && !providers.contains(where: \.isDefault) {
            providers[0].isDefault = true
            if providers[0].id == saved.id {
                saved.isDefault = true
            }
        }

        try store.write(providers, to: "llm_providers.json", secret: true)
        return saved
    }

    func deleteProvider(id: String) throws {
        guard let index = providers.firstIndex(where: { $0.id == id }) else { return }
        let wasDefault = providers[index].isDefault
        providers.remove(at: index)
        if wasDefault, providers.indices.contains(providers.startIndex) {
            providers[providers.startIndex].isDefault = true
        }
        try store.write(providers, to: "llm_providers.json", secret: true)
    }

    func setDefaultProvider(id: String) throws {
        guard providers.contains(where: { $0.id == id }) else { return }
        for index in providers.indices {
            providers[index].isDefault = providers[index].id == id
        }
        try store.write(providers, to: "llm_providers.json", secret: true)
    }

    func saveSettings(_ settings: AppSettings) throws {
        guard settings.promptClipChars >= 0 else {
            throw AppConfigError.validation("AI 回复裁剪字符数不能小于 0")
        }
        guard settings.pastReportsContext >= 0 else {
            throw AppConfigError.validation("历史周报参考数量不能小于 0")
        }

        self.settings = settings
        try store.write(settings, to: "settings.json")
    }

    func saveSMTPConfig(_ config: SMTPConfig) throws {
        let saved = config.normalizedForStorage()
        guard saved.port > 0 && saved.port <= 65_535 else {
            throw AppConfigError.validation("SMTP 端口必须在 1 到 65535 之间")
        }

        smtpConfig = saved
        try store.write(saved, to: "smtp.json", secret: true)
    }

    func reportBodyPreview(for report: ReportRecord) -> String {
        do {
            let body = try store.readReportBody(id: report.id)
            return body
        } catch {
            return "无法读取报告正文：\(error.localizedDescription)"
        }
    }

    func generateReportNow() async {
        guard !isGenerating else { return }
        isGenerating = true
        defer { isGenerating = false }

        do {
            let templateID = templates.first?.id ?? "builtin-tech"
            let output = try await ReportGenerator(store: store).runGeneration(
                workspaceIDs: workspaces.map(\.id),
                templateID: templateID,
                days: 7
            )
            await load()
            selectedPage = .reports

            var message = "已生成「\(output.record.templateName)」，tokens \(output.record.tokensUsed)，耗时 \(String(format: "%.1f", Double(output.durationMS) / 1000))s。"
            if output.skippedLines > 0 || output.skippedFiles > 0 {
                message += "\n跳过行：\(output.skippedLines)，跳过文件：\(output.skippedFiles)。"
            }
            notice = AppNotice(title: "生成完成", message: message)
        } catch {
            notice = AppNotice(title: "生成失败", message: error.localizedDescription)
        }
    }

    private static func defaultWorkspace() -> Workspace {
        let home = FileManager.default.homeDirectoryForCurrentUser
        return Workspace(
            id: UUID().uuidString,
            name: "本机",
            type: .local,
            host: nil,
            user: nil,
            port: nil,
            authMethod: .key,
            sshKey: nil,
            sshPassword: nil,
            claudePath: home.appendingPathComponent(".claude").path,
            codexPath: home.appendingPathComponent(".codex").path,
            tools: ["claude-code", "codex"]
        )
    }
}

struct AppNotice: Identifiable {
    let id = UUID()
    var title: String
    var message: String
}

enum AppConfigError: LocalizedError, Equatable {
    case validation(String)

    var errorDescription: String? {
        switch self {
        case let .validation(message):
            message
        }
    }
}

private extension Workspace {
    func normalizedForStorage() -> Workspace {
        var copy = self
        copy.name = copy.name.trimmingCharacters(in: .whitespacesAndNewlines)
        copy.host = copy.host?.nilIfBlank
        copy.user = copy.user?.nilIfBlank
        copy.sshKey = copy.sshKey?.nilIfBlank
        copy.sshPassword = copy.sshPassword?.nilIfBlank
        copy.claudePath = copy.claudePath?.nilIfBlank
        copy.codexPath = copy.codexPath?.nilIfBlank
        copy.tools = copy.tools.filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
        return copy
    }
}

private extension LLMProvider {
    func normalizedForStorage() -> LLMProvider {
        var copy = self
        copy.name = copy.name.trimmingCharacters(in: .whitespacesAndNewlines)
        copy.baseURL = copy.baseURL.trimmingCharacters(in: .whitespacesAndNewlines)
        copy.apiKey = copy.apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        copy.model = copy.model.trimmingCharacters(in: .whitespacesAndNewlines)
        copy.commandPath = copy.commandPath?.nilIfBlank
        if copy.kind.isCommandLineProvider {
            copy.baseURL = ""
            copy.apiKey = ""
            copy.temperature = nil
            copy.maxTokens = 0
        }
        copy.extraHeaders = Dictionary(
            uniqueKeysWithValues: copy.extraHeaders.compactMap { key, value in
                let trimmedKey = key.trimmingCharacters(in: .whitespacesAndNewlines)
                let trimmedValue = value.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !trimmedKey.isEmpty else { return nil }
                return (trimmedKey, trimmedValue)
            }
        )
        return copy
    }
}

private extension SMTPConfig {
    func normalizedForStorage() -> SMTPConfig {
        SMTPConfig(
            host: host.trimmingCharacters(in: .whitespacesAndNewlines),
            port: port,
            username: username.trimmingCharacters(in: .whitespacesAndNewlines),
            password: password.trimmingCharacters(in: .whitespacesAndNewlines),
            fromName: fromName.trimmingCharacters(in: .whitespacesAndNewlines),
            useSSL: useSSL
        )
    }
}

private extension String {
    var nilIfBlank: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
