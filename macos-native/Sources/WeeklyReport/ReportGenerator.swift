import Foundation

struct GenerationOutput: Equatable {
    var record: ReportRecord
    var content: String
    var durationMS: Int
    var skippedLines: Int
    var skippedFiles: Int
}

@MainActor
struct ReportGenerator {
    var store: JSONStore
    var logCollector: LogCollector
    var llmClient: any LLMCompleting

    init(
        store: JSONStore,
        logCollector: LogCollector = LogCollector(),
        llmClient: any LLMCompleting = DefaultLLMClient()
    ) {
        self.store = store
        self.logCollector = logCollector
        self.llmClient = llmClient
    }

    func runGeneration(
        workspaceIDs: [String],
        templateID: String,
        days: Int,
        providerID: String? = nil
    ) async throws -> GenerationOutput {
        let templates = try loadTemplates()
        guard let template = templates.first(where: { $0.id == templateID }) else {
            throw GenerationError.missingTemplate(templateID)
        }

        let providers = try store.read([LLMProvider].self, from: "llm_providers.json", default: [])
        let provider = try resolveProvider(providers: providers, explicitID: providerID, templateProviderID: template.providerID)

        let allWorkspaces = try store.read([Workspace].self, from: "workspaces.json", default: [])
        let workspaces = allWorkspaces.filter { workspaceIDs.contains($0.id) }
        guard !workspaces.isEmpty else {
            throw GenerationError.noWorkspaceSelected
        }

        let settings = try store.read(AppSettings.self, from: "settings.json", default: .defaults)

        var messages: [LogMessage] = []
        var parseStats = ParseStats()
        for workspace in workspaces {
            do {
                let result = try logCollector.collectMessages(
                    workspace: workspace,
                    days: days,
                    clipChars: settings.promptClipChars
                )
                messages.append(contentsOf: result.messages)
                parseStats.merge(result.stats)
            } catch {
                parseStats.skippedFiles += 1
            }
        }

        let summary = LogAggregator.aggregate(messages, parseStats: parseStats)
        let pastReports = try loadPastReports(limit: settings.pastReportsContext)
        let promptLimits: ReportPromptLimits = provider.kind.isCommandLineProvider ? .commandLine : .unlimited
        let prompt = ReportPromptBuilder.buildPrompt(
            summary: summary,
            template: template,
            pastReports: pastReports,
            limits: promptLimits
        )
        let completion = try await llmClient.complete(provider: provider, prompt: prompt)

        let record = ReportRecord(
            id: "",
            week: "最近 \(days) 天",
            templateID: template.id,
            templateName: template.name,
            providerID: provider.id,
            providerName: provider.name,
            tokensUsed: completion.tokensUsed,
            projectCount: summary.stats.projectCount,
            generatedAt: ISO8601DateFormatter().string(from: Date())
        )
        let saved = try store.saveReport(record: record, content: completion.text)

        return GenerationOutput(
            record: saved,
            content: completion.text,
            durationMS: completion.durationMS,
            skippedLines: summary.stats.skippedLines,
            skippedFiles: summary.stats.skippedFiles
        )
    }

    private func loadTemplates() throws -> [ReportTemplate] {
        let customTemplates = try store.read([ReportTemplate].self, from: "templates.json", default: [])
            .filter { !$0.builtin && !$0.id.hasPrefix("builtin-") }
        return ReportTemplate.builtins + customTemplates
    }

    private func loadPastReports(limit: Int) throws -> [String] {
        guard limit > 0 else { return [] }
        let reports = try store.read([ReportRecord].self, from: "reports/index.json", default: [])
            .sorted { $0.generatedAt > $1.generatedAt }
            .prefix(limit)

        return reports.compactMap { report in
            try? store.readReportBody(id: report.id)
        }
    }

    private func resolveProvider(
        providers: [LLMProvider],
        explicitID: String?,
        templateProviderID: String?
    ) throws -> LLMProvider {
        if let explicitID, !explicitID.isEmpty {
            guard let provider = providers.first(where: { $0.id == explicitID }) else {
                throw GenerationError.missingProvider(explicitID)
            }
            return provider
        }

        if let templateProviderID, !templateProviderID.isEmpty,
           let provider = providers.first(where: { $0.id == templateProviderID }) {
            return provider
        }

        if let provider = providers.first(where: { $0.isDefault }) {
            return provider
        }
        if let provider = providers.first {
            return provider
        }
        throw GenerationError.noProviderConfigured
    }
}

enum GenerationError: LocalizedError, Equatable {
    case missingTemplate(String)
    case missingProvider(String)
    case noWorkspaceSelected
    case noProviderConfigured

    var errorDescription: String? {
        switch self {
        case let .missingTemplate(id):
            "模板不存在：\(id)"
        case let .missingProvider(id):
            "指定的 LLM 源不存在：\(id)"
        case .noWorkspaceSelected:
            "未选中任何工作区"
        case .noProviderConfigured:
            "未配置任何 LLM 源"
        }
    }
}
