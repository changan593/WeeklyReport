import Foundation

struct LLMCompletionResult: Equatable {
    var text: String
    var tokensUsed: Int
    var durationMS: Int
}

@MainActor
protocol LLMCompleting {
    func complete(provider: LLMProvider, prompt: String) async throws -> LLMCompletionResult
}

struct DefaultLLMClient: LLMCompleting {
    var apiClient = URLSessionLLMClient()
    var commandLineClient = CommandLineLLMClient()

    func complete(provider: LLMProvider, prompt: String) async throws -> LLMCompletionResult {
        if provider.kind.isCommandLineProvider {
            return try await commandLineClient.complete(provider: provider, prompt: prompt)
        }
        return try await apiClient.complete(provider: provider, prompt: prompt)
    }
}

struct URLSessionLLMClient: LLMCompleting {
    var session: URLSession = .shared

    func complete(provider: LLMProvider, prompt: String) async throws -> LLMCompletionResult {
        let request = try LLMRequestBuilder.build(provider: provider, prompt: prompt)
        let started = Date()
        let (data, response) = try await session.data(for: request)
        let durationMS = Int(Date().timeIntervalSince(started) * 1000)

        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(status) else {
            let body = String(data: data, encoding: .utf8) ?? ""
            throw LLMError.httpStatus(status, bodyPreview: String(body.prefix(500)))
        }

        var result = try LLMResponseParser.parse(kind: provider.kind, data: data)
        result.durationMS = durationMS
        return result
    }
}

enum LLMError: LocalizedError, Equatable {
    case invalidBaseURL(String)
    case invalidURL(String)
    case invalidResponse(String)
    case httpStatus(Int, bodyPreview: String)
    case unsupportedProviderKind(LLMKind)
    case commandFailed(command: String, status: Int32, stderr: String)
    case commandOutputEmpty(command: String)

    var errorDescription: String? {
        switch self {
        case let .invalidBaseURL(url):
            "LLM base_url 无效：\(url)"
        case let .invalidURL(url):
            "LLM 请求 URL 无效：\(url)"
        case let .invalidResponse(message):
            message
        case let .httpStatus(status, bodyPreview):
            "LLM API 返回 \(status): \(bodyPreview)"
        case let .unsupportedProviderKind(kind):
            "当前调用路径不支持 \(kind.label)"
        case let .commandFailed(command, status, stderr):
            "CLI 调用失败：\(command) 退出码 \(status)。\(stderr)"
        case let .commandOutputEmpty(command):
            "CLI 调用没有输出：\(command)"
        }
    }
}

enum LLMRequestBuilder {
    static func build(provider: LLMProvider, prompt: String) throws -> URLRequest {
        switch provider.kind {
        case .openAICompatible:
            try buildOpenAICompatible(provider: provider, prompt: prompt)
        case .anthropic:
            try buildAnthropic(provider: provider, prompt: prompt)
        case .gemini:
            try buildGemini(provider: provider, prompt: prompt)
        case .claudeCode, .codexCLI:
            throw LLMError.unsupportedProviderKind(provider.kind)
        }
    }

    private static func buildOpenAICompatible(provider: LLMProvider, prompt: String) throws -> URLRequest {
        let url = try url(baseURL: provider.baseURL, appending: "/v1/chat/completions")
        var body: [String: Any] = [
            "model": provider.model,
            "messages": [["role": "user", "content": prompt]],
            "max_tokens": provider.maxTokens,
        ]
        if let temperature = provider.temperature {
            body["temperature"] = temperature
        }
        if let reasoningEffort = provider.reasoningEffort {
            body["reasoning_effort"] = reasoningEffort.rawValue
        }

        var headers = defaultHeaders(provider: provider)
        if !provider.apiKey.isEmpty {
            headers["Authorization"] = "Bearer \(provider.apiKey)"
        }
        return try request(url: url, headers: headers, body: body)
    }

    private static func buildAnthropic(provider: LLMProvider, prompt: String) throws -> URLRequest {
        let url = try url(baseURL: provider.baseURL, appending: "/v1/messages")
        var body: [String: Any] = [
            "model": provider.model,
            "max_tokens": provider.maxTokens,
            "messages": [["role": "user", "content": prompt]],
        ]
        if let temperature = provider.temperature {
            body["temperature"] = temperature
        }

        var headers = defaultHeaders(provider: provider)
        headers["anthropic-version"] = "2023-06-01"
        if !provider.apiKey.isEmpty {
            headers["x-api-key"] = provider.apiKey
        }
        return try request(url: url, headers: headers, body: body)
    }

    private static func buildGemini(provider: LLMProvider, prompt: String) throws -> URLRequest {
        let base = try url(baseURL: provider.baseURL, appending: "/v1beta/models/\(provider.model):generateContent")
        var components = URLComponents(url: base, resolvingAgainstBaseURL: false)
        if !provider.apiKey.isEmpty {
            components?.queryItems = [URLQueryItem(name: "key", value: provider.apiKey)]
        }
        guard let url = components?.url else {
            throw LLMError.invalidURL(base.absoluteString)
        }

        var generationConfig: [String: Any] = ["maxOutputTokens": provider.maxTokens]
        if let temperature = provider.temperature {
            generationConfig["temperature"] = temperature
        }
        let body: [String: Any] = [
            "contents": [["parts": [["text": prompt]]]],
            "generationConfig": generationConfig,
        ]

        return try request(url: url, headers: defaultHeaders(provider: provider), body: body)
    }

    private static func defaultHeaders(provider: LLMProvider) -> [String: String] {
        var headers = ["Content-Type": "application/json"]
        for (key, value) in provider.extraHeaders {
            headers[key] = value
        }
        return headers
    }

    private static func url(baseURL: String, appending path: String) throws -> URL {
        let trimmed = baseURL.trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard !trimmed.isEmpty else {
            throw LLMError.invalidBaseURL(baseURL)
        }
        guard let url = URL(string: trimmed + path) else {
            throw LLMError.invalidURL(trimmed + path)
        }
        return url
    }

    private static func request(url: URL, headers: [String: String], body: [String: Any]) throws -> URLRequest {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        for (key, value) in headers {
            request.setValue(value, forHTTPHeaderField: key)
        }
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        return request
    }
}

enum LLMResponseParser {
    static func parse(kind: LLMKind, data: Data) throws -> LLMCompletionResult {
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw LLMError.invalidResponse("响应 JSON 不是对象")
        }

        let parsed: (String, Int) = switch kind {
        case .openAICompatible:
            try parseOpenAICompatible(object)
        case .anthropic:
            try parseAnthropic(object)
        case .gemini:
            try parseGemini(object)
        case .claudeCode, .codexCLI:
            throw LLMError.unsupportedProviderKind(kind)
        }

        return LLMCompletionResult(text: parsed.0, tokensUsed: parsed.1, durationMS: 0)
    }

    private static func parseOpenAICompatible(_ object: [String: Any]) throws -> (String, Int) {
        guard let choices = object["choices"] as? [[String: Any]],
              let message = choices.first?["message"] as? [String: Any],
              let text = message["content"] as? String
        else {
            throw LLMError.invalidResponse("OpenAI 响应缺少 choices[0].message.content")
        }
        let usage = object["usage"] as? [String: Any]
        let tokens = (usage?["total_tokens"] as? NSNumber)?.intValue ?? 0
        return (text, tokens)
    }

    private static func parseAnthropic(_ object: [String: Any]) throws -> (String, Int) {
        guard let content = object["content"] as? [[String: Any]],
              let text = content.first?["text"] as? String
        else {
            throw LLMError.invalidResponse("Anthropic 响应缺少 content[0].text")
        }
        let usage = object["usage"] as? [String: Any]
        let input = (usage?["input_tokens"] as? NSNumber)?.intValue ?? 0
        let output = (usage?["output_tokens"] as? NSNumber)?.intValue ?? 0
        return (text, input + output)
    }

    private static func parseGemini(_ object: [String: Any]) throws -> (String, Int) {
        guard let candidates = object["candidates"] as? [[String: Any]],
              let content = candidates.first?["content"] as? [String: Any],
              let parts = content["parts"] as? [[String: Any]],
              let text = parts.first?["text"] as? String
        else {
            throw LLMError.invalidResponse("Gemini 响应缺少 candidates[0].content.parts[0].text")
        }
        let usage = object["usageMetadata"] as? [String: Any]
        let tokens = (usage?["totalTokenCount"] as? NSNumber)?.intValue ?? 0
        return (text, tokens)
    }
}

struct CommandLineInvocation: Equatable, Sendable {
    var executable: String
    var arguments: [String]
    var standardInput: String
    var workingDirectory: String?
}

struct CommandLineOutput: Equatable, Sendable {
    var stdout: String
    var stderr: String
    var exitStatus: Int32
    var durationMS: Int
}

protocol CommandLineRunning: Sendable {
    func run(_ invocation: CommandLineInvocation) async throws -> CommandLineOutput
}

struct CommandLineLLMClient: LLMCompleting {
    var runner: any CommandLineRunning = ProcessCommandRunner()

    func complete(provider: LLMProvider, prompt: String) async throws -> LLMCompletionResult {
        let invocation = try LLMCommandBuilder.build(provider: provider, prompt: prompt)
        let output = try await runner.run(invocation)
        guard output.exitStatus == 0 else {
            throw LLMError.commandFailed(
                command: invocation.executable,
                status: output.exitStatus,
                stderr: output.stderr.trimmedForError
            )
        }

        let text = try LLMCommandResponseParser.parse(provider: provider, output: output)
        return LLMCompletionResult(text: text, tokensUsed: 0, durationMS: output.durationMS)
    }
}

enum LLMCommandBuilder {
    static func build(provider: LLMProvider, prompt: String) throws -> CommandLineInvocation {
        switch provider.kind {
        case .claudeCode:
            return claudeCode(provider: provider, prompt: prompt)
        case .codexCLI:
            return codexCLI(provider: provider, prompt: prompt)
        case .openAICompatible, .anthropic, .gemini:
            throw LLMError.unsupportedProviderKind(provider.kind)
        }
    }

    private static func claudeCode(provider: LLMProvider, prompt: String) -> CommandLineInvocation {
        var arguments = [
            "-p",
            "--output-format", "json",
            "--input-format", "text",
            "--no-session-persistence",
            "--tools", "",
        ]
        let model = provider.model.trimmingCharacters(in: .whitespacesAndNewlines)
        if !model.isEmpty {
            arguments.append(contentsOf: ["--model", model])
        }
        if let effort = provider.effectiveReasoningEffort {
            arguments.append(contentsOf: ["--effort", effort.rawValue])
        }
        return CommandLineInvocation(
            executable: provider.commandExecutable,
            arguments: arguments,
            standardInput: prompt,
            workingDirectory: FileManager.default.homeDirectoryForCurrentUser.path
        )
    }

    private static func codexCLI(provider: LLMProvider, prompt: String) -> CommandLineInvocation {
        var arguments = [
            "exec",
            "-c", "disable_response_storage=true",
            "--ephemeral",
            "--sandbox", "read-only",
            "--skip-git-repo-check",
            "--color", "never",
        ]
        let model = provider.model.trimmingCharacters(in: .whitespacesAndNewlines)
        if !model.isEmpty {
            arguments.append(contentsOf: ["--model", model])
        }
        if let effort = provider.effectiveReasoningEffort {
            arguments.append(contentsOf: ["-c", "model_reasoning_effort=\"\(effort.rawValue)\""])
        }
        arguments.append("-")
        return CommandLineInvocation(
            executable: provider.commandExecutable,
            arguments: arguments,
            standardInput: prompt,
            workingDirectory: FileManager.default.homeDirectoryForCurrentUser.path
        )
    }
}

enum LLMCommandResponseParser {
    static func parse(provider: LLMProvider, output: CommandLineOutput) throws -> String {
        switch provider.kind {
        case .claudeCode:
            return try parseClaudeCode(output.stdout, command: provider.commandExecutable)
        case .codexCLI:
            return try parsePlainText(output.stdout, command: provider.commandExecutable)
        case .openAICompatible, .anthropic, .gemini:
            throw LLMError.unsupportedProviderKind(provider.kind)
        }
    }

    private static func parseClaudeCode(_ stdout: String, command: String) throws -> String {
        let trimmed = stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            throw LLMError.commandOutputEmpty(command: command)
        }
        guard let data = trimmed.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return trimmed
        }
        if let result = object["result"] as? String, !result.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return result
        }
        if let text = object["text"] as? String, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return text
        }
        if let message = object["message"] as? [String: Any],
           let content = message["content"] as? [[String: Any]] {
            let text = content.compactMap { $0["text"] as? String }.joined(separator: "\n")
            if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                return text
            }
        }
        throw LLMError.invalidResponse("Claude Code CLI JSON 输出缺少 result")
    }

    private static func parsePlainText(_ stdout: String, command: String) throws -> String {
        let trimmed = stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            throw LLMError.commandOutputEmpty(command: command)
        }
        return trimmed
    }
}

struct ProcessCommandRunner: CommandLineRunning {
    func run(_ invocation: CommandLineInvocation) async throws -> CommandLineOutput {
        try await Task.detached(priority: .userInitiated) {
            try runProcess(invocation)
        }.value
    }
}

private func runProcess(_ invocation: CommandLineInvocation) throws -> CommandLineOutput {
    let fileManager = FileManager.default
    let directory = fileManager.temporaryDirectory
        .appendingPathComponent("weeklyreport-cli-\(UUID().uuidString)", isDirectory: true)
    try fileManager.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? fileManager.removeItem(at: directory) }

    let stdinURL = directory.appendingPathComponent("stdin.txt")
    let stdoutURL = directory.appendingPathComponent("stdout.txt")
    let stderrURL = directory.appendingPathComponent("stderr.txt")
    try Data(invocation.standardInput.utf8).write(to: stdinURL)
    try Data().write(to: stdoutURL)
    try Data().write(to: stderrURL)

    let stdinHandle = try FileHandle(forReadingFrom: stdinURL)
    let stdoutHandle = try FileHandle(forWritingTo: stdoutURL)
    let stderrHandle = try FileHandle(forWritingTo: stderrURL)
    defer {
        try? stdinHandle.close()
        try? stdoutHandle.close()
        try? stderrHandle.close()
    }

    let process = Process()
    if invocation.executable.contains("/") {
        process.executableURL = URL(fileURLWithPath: invocation.executable)
        process.arguments = invocation.arguments
    } else {
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = [invocation.executable] + invocation.arguments
    }
    process.standardInput = stdinHandle
    process.standardOutput = stdoutHandle
    process.standardError = stderrHandle
    process.environment = commandLineEnvironment()
    if let workingDirectory = invocation.workingDirectory {
        process.currentDirectoryURL = URL(fileURLWithPath: workingDirectory, isDirectory: true)
    }

    let started = Date()
    try process.run()
    process.waitUntilExit()
    let durationMS = Int(Date().timeIntervalSince(started) * 1000)

    try? stdoutHandle.close()
    try? stderrHandle.close()
    let stdout = (try? String(contentsOf: stdoutURL, encoding: .utf8)) ?? ""
    let stderr = (try? String(contentsOf: stderrURL, encoding: .utf8)) ?? ""
    return CommandLineOutput(
        stdout: stdout,
        stderr: stderr,
        exitStatus: process.terminationStatus,
        durationMS: durationMS
    )
}

private func commandLineEnvironment() -> [String: String] {
    var environment = ProcessInfo.processInfo.environment
    let home = FileManager.default.homeDirectoryForCurrentUser.path
    let fallbackPath = [
        "\(home)/.npm-global/bin",
        "\(home)/.local/bin",
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ].joined(separator: ":")
    if let path = environment["PATH"], !path.isEmpty {
        environment["PATH"] = "\(path):\(fallbackPath)"
    } else {
        environment["PATH"] = fallbackPath
    }
    return environment
}

private extension String {
    var trimmedForError: String {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.count <= 1200 {
            return trimmed
        }
        return "\(String(trimmed.prefix(450)))\n...\n\(String(trimmed.suffix(750)))"
    }
}
