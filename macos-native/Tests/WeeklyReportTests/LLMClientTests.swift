import Foundation
import Testing
@testable import WeeklyReport

@Suite("LLM client")
struct LLMClientTests {
    @Test
    func openAICompatibleRequestMatchesRustShape() throws {
        let request = try LLMRequestBuilder.build(provider: sampleProvider(kind: .openAICompatible), prompt: "你好")
        let body = try requestBody(request)

        #expect(request.url?.absoluteString == "https://api.example.com/v1/chat/completions")
        #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer test-key")
        #expect(body["model"] as? String == "test-model")
        #expect(body["max_tokens"] as? Int == 2048)
        #expect(body["temperature"] as? Double == 0.7)

        let messages = try #require(body["messages"] as? [[String: Any]])
        #expect(messages.first?["role"] as? String == "user")
        #expect(messages.first?["content"] as? String == "你好")
    }

    @Test
    func anthropicRequestUsesMessagesAPIAndXAPIKey() throws {
        let request = try LLMRequestBuilder.build(provider: sampleProvider(kind: .anthropic), prompt: "hi")
        let body = try requestBody(request)

        #expect(request.url?.absoluteString == "https://api.example.com/v1/messages")
        #expect(request.value(forHTTPHeaderField: "anthropic-version") == "2023-06-01")
        #expect(request.value(forHTTPHeaderField: "x-api-key") == "test-key")
        #expect(request.value(forHTTPHeaderField: "Authorization") == nil)
        #expect(body["model"] as? String == "test-model")
    }

    @Test
    func geminiRequestUsesModelPathAndQueryKey() throws {
        let request = try LLMRequestBuilder.build(provider: sampleProvider(kind: .gemini), prompt: "hi")
        let body = try requestBody(request)

        #expect(request.url?.absoluteString == "https://api.example.com/v1beta/models/test-model:generateContent?key=test-key")
        #expect(request.value(forHTTPHeaderField: "Authorization") == nil)

        let config = try #require(body["generationConfig"] as? [String: Any])
        #expect(config["maxOutputTokens"] as? Int == 2048)
        #expect(config["temperature"] as? Double == 0.7)
    }

    @Test
    func openAICompatibleRequestCanIncludeReasoningEffort() throws {
        let provider = sampleProvider(kind: .openAICompatible, reasoningEffort: .high)
        let request = try LLMRequestBuilder.build(provider: provider, prompt: "hi")
        let body = try requestBody(request)

        #expect(body["reasoning_effort"] as? String == "high")
    }

    @Test
    func responseParsersReturnTextAndTokens() throws {
        let openAI = #"{"choices":[{"message":{"content":"OpenAI text"}}],"usage":{"total_tokens":11}}"#
        let anthropic = #"{"content":[{"text":"Claude text"}],"usage":{"input_tokens":5,"output_tokens":7}}"#
        let gemini = #"{"candidates":[{"content":{"parts":[{"text":"Gemini text"}]}}],"usageMetadata":{"totalTokenCount":13}}"#

        #expect(try LLMResponseParser.parse(kind: .openAICompatible, data: Data(openAI.utf8)).text == "OpenAI text")
        #expect(try LLMResponseParser.parse(kind: .anthropic, data: Data(anthropic.utf8)).tokensUsed == 12)
        #expect(try LLMResponseParser.parse(kind: .gemini, data: Data(gemini.utf8)).tokensUsed == 13)
    }

    @Test
    func claudeCodeCommandUsesPrintModeAndParsesJSONResult() async throws {
        let provider = sampleProvider(kind: .claudeCode, model: "sonnet", commandPath: "claude")
        let runner = MockCommandRunner(output: CommandLineOutput(
            stdout: "{\"type\":\"result\",\"result\":\"## 周报\\n完成迁移。\"}",
            stderr: "",
            exitStatus: 0,
            durationMS: 321
        ))
        let client = CommandLineLLMClient(runner: runner)

        let result = try await client.complete(provider: provider, prompt: "生成周报")

        #expect(result.text == "## 周报\n完成迁移。")
        #expect(result.tokensUsed == 0)
        #expect(result.durationMS == 321)
        let invocation = try #require(await runner.lastInvocation())
        #expect(invocation.executable == "claude")
        #expect(invocation.arguments.contains("-p"))
        #expect(invocation.arguments.contains("--output-format"))
        #expect(invocation.arguments.contains("json"))
        #expect(invocation.arguments.contains("--model"))
        #expect(invocation.arguments.contains("sonnet"))
        #expect(invocation.arguments.contains("--effort"))
        #expect(invocation.arguments.contains("medium"))
        #expect(invocation.workingDirectory?.isEmpty == false)
        #expect(invocation.standardInput == "生成周报")
    }

    @Test
    func codexCLICommandUsesExecAndPlainTextOutput() async throws {
        let provider = sampleProvider(kind: .codexCLI, model: "", commandPath: "codex")
        let runner = MockCommandRunner(output: CommandLineOutput(
            stdout: "## 周报\n完成 Codex CLI 接入。\n",
            stderr: "progress",
            exitStatus: 0,
            durationMS: 456
        ))
        let client = CommandLineLLMClient(runner: runner)

        let result = try await client.complete(provider: provider, prompt: "生成周报")

        #expect(result.text == "## 周报\n完成 Codex CLI 接入。")
        let invocation = try #require(await runner.lastInvocation())
        #expect(invocation.executable == "codex")
        #expect(invocation.arguments.first == "exec")
        #expect(invocation.arguments.contains("--sandbox"))
        #expect(invocation.arguments.contains("read-only"))
        #expect(invocation.arguments.contains("model_reasoning_effort=\"medium\""))
        #expect(invocation.arguments.contains("--color"))
        #expect(invocation.arguments.last == "-")
    }

    @Test
    func commandLineFailureSurfacesStderr() async throws {
        let provider = sampleProvider(kind: .codexCLI, model: "", commandPath: "codex")
        let client = CommandLineLLMClient(runner: MockCommandRunner(output: CommandLineOutput(
            stdout: "",
            stderr: "not logged in",
            exitStatus: 1,
            durationMS: 100
        )))

        await #expect(throws: LLMError.commandFailed(command: "codex", status: 1, stderr: "not logged in")) {
            _ = try await client.complete(provider: provider, prompt: "生成周报")
        }
    }

    private func sampleProvider(kind: LLMKind) -> LLMProvider {
        sampleProvider(kind: kind, model: "test-model", commandPath: nil, reasoningEffort: nil)
    }

    private func sampleProvider(kind: LLMKind, reasoningEffort: ReasoningEffort?) -> LLMProvider {
        sampleProvider(kind: kind, model: "test-model", commandPath: nil, reasoningEffort: reasoningEffort)
    }

    private func sampleProvider(
        kind: LLMKind,
        model: String,
        commandPath: String?,
        reasoningEffort: ReasoningEffort? = nil
    ) -> LLMProvider {
        LLMProvider(
            id: "p1",
            name: "Provider",
            kind: kind,
            baseURL: "https://api.example.com/",
            apiKey: "test-key",
            model: model,
            maxTokens: 2048,
            temperature: 0.7,
            isDefault: true,
            extraHeaders: [:],
            commandPath: commandPath,
            reasoningEffort: reasoningEffort
        )
    }

    private func requestBody(_ request: URLRequest) throws -> [String: Any] {
        let data = try #require(request.httpBody)
        return try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
    }
}

private actor MockCommandRunner: CommandLineRunning {
    let output: CommandLineOutput
    private var capturedInvocation: CommandLineInvocation?

    init(output: CommandLineOutput) {
        self.output = output
    }

    func run(_ invocation: CommandLineInvocation) async throws -> CommandLineOutput {
        capturedInvocation = invocation
        return output
    }

    func lastInvocation() -> CommandLineInvocation? {
        capturedInvocation
    }
}
