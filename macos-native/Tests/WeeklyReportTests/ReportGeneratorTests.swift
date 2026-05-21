import Foundation
import Testing
@testable import WeeklyReport

@Suite("Report generator")
struct ReportGeneratorTests {
    @Test
    @MainActor
    func runGenerationCollectsLocalLogsCallsLLMAndPersistsReport() async throws {
        let root = try makeTemporaryDirectory()
        let store = JSONStore(dataDirectory: root.appendingPathComponent("data", isDirectory: true))
        try store.ensureInitialized()

        let claudeRoot = root.appendingPathComponent("claude", isDirectory: true)
        try FileManager.default.createDirectory(at: claudeRoot, withIntermediateDirectories: true)
        try """
        {"display":"实现 Swift 原生日志解析","timestamp":4102444800000,"project":"weekly-report"}
        {"display":"接入 LLM 生成服务","timestamp":4102444801000,"project":"weekly-report"}
        """.write(to: claudeRoot.appendingPathComponent("history.jsonl"), atomically: true, encoding: .utf8)

        let workspace = Workspace(
            id: "w1",
            name: "本机",
            type: .local,
            host: nil,
            user: nil,
            port: nil,
            authMethod: .key,
            sshKey: nil,
            sshPassword: nil,
            claudePath: claudeRoot.path,
            codexPath: nil,
            tools: ["claude-code"]
        )
        let provider = LLMProvider(
            id: "p1",
            name: "Mock",
            kind: .openAICompatible,
            baseURL: "https://api.example.com",
            apiKey: "test",
            model: "mock-model",
            maxTokens: 2048,
            temperature: 0.7,
            isDefault: true,
            extraHeaders: [:]
        )

        try store.write([workspace], to: "workspaces.json")
        try store.write([provider], to: "llm_providers.json", secret: true)
        try store.write(AppSettings(promptClipChars: 200, pastReportsContext: 0), to: "settings.json")

        let generator = ReportGenerator(
            store: store,
            llmClient: MockLLMClient(result: LLMCompletionResult(text: "## 本周 TL;DR\n完成 Swift 原生迁移。", tokensUsed: 42, durationMS: 123))
        )

        let output = try await generator.runGeneration(
            workspaceIDs: ["w1"],
            templateID: "builtin-tech",
            days: 7
        )

        #expect(output.record.id.isEmpty == false)
        #expect(output.record.templateName == "技术周报")
        #expect(output.record.providerName == "Mock")
        #expect(output.record.tokensUsed == 42)
        #expect(output.record.projectCount == 1)
        #expect(output.durationMS == 123)

        let savedReports = try store.read([ReportRecord].self, from: "reports/index.json", default: [])
        #expect(savedReports.map(\.id) == [output.record.id])
        #expect(try store.readReportBody(id: output.record.id) == output.content)
    }

    private func makeTemporaryDirectory() throws -> URL {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("weekly-report-swift-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        return root
    }
}

@MainActor
private struct MockLLMClient: LLMCompleting {
    var result: LLMCompletionResult

    func complete(provider: LLMProvider, prompt: String) async throws -> LLMCompletionResult {
        #expect(provider.name == "Mock")
        #expect(prompt.contains("实现 Swift 原生日志解析"))
        #expect(prompt.contains("<work_logs>"))
        return result
    }
}
