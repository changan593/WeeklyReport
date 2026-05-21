import Foundation

enum AppPage: String, CaseIterable, Identifiable {
    case workspaces
    case providers
    case templates
    case reports
    case schedules
    case settings

    var id: String { rawValue }

    var title: String {
        switch self {
        case .workspaces: "工作区"
        case .providers: "LLM 源"
        case .templates: "周报模板"
        case .reports: "历史周报"
        case .schedules: "定时任务"
        case .settings: "设置"
        }
    }

    var symbolName: String {
        switch self {
        case .workspaces: "folder"
        case .providers: "sparkles"
        case .templates: "doc.text"
        case .reports: "tray.full"
        case .schedules: "calendar.badge.clock"
        case .settings: "gearshape"
        }
    }
}

enum WorkspaceKind: String, Codable, CaseIterable {
    case local
    case ssh

    var label: String {
        switch self {
        case .local: "本机"
        case .ssh: "SSH"
        }
    }
}

enum SshAuthMethod: String, Codable, CaseIterable {
    case key
    case password

    var label: String {
        switch self {
        case .key: "密钥"
        case .password: "密码"
        }
    }
}

struct Workspace: Codable, Identifiable, Hashable {
    var id: String
    var name: String
    var type: WorkspaceKind
    var host: String?
    var user: String?
    var port: Int?
    var authMethod: SshAuthMethod
    var sshKey: String?
    var sshPassword: String?
    var claudePath: String?
    var codexPath: String?
    var tools: [String]

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case type
        case host
        case user
        case port
        case authMethod = "auth_method"
        case sshKey = "ssh_key"
        case sshPassword = "ssh_password"
        case claudePath = "claude_path"
        case codexPath = "codex_path"
        case tools
    }
}

enum LLMKind: String, Codable, CaseIterable {
    case openAICompatible = "OpenAiCompatible"
    case anthropic = "Anthropic"
    case gemini = "Gemini"
    case claudeCode = "ClaudeCode"
    case codexCLI = "CodexCLI"

    var label: String {
        switch self {
        case .openAICompatible: "OpenAI 兼容"
        case .anthropic: "Anthropic"
        case .gemini: "Gemini"
        case .claudeCode: "Claude Code"
        case .codexCLI: "Codex CLI"
        }
    }

    var isAPIProvider: Bool {
        switch self {
        case .openAICompatible, .anthropic, .gemini:
            true
        case .claudeCode, .codexCLI:
            false
        }
    }

    var isCommandLineProvider: Bool {
        !isAPIProvider
    }

    var defaultCommandPath: String? {
        switch self {
        case .claudeCode:
            "claude"
        case .codexCLI:
            "codex"
        case .openAICompatible, .anthropic, .gemini:
            nil
        }
    }

    var modelSuggestions: [String] {
        switch self {
        case .openAICompatible:
            ["gpt-5.5", "gpt-5.4", "gpt-4o-mini", "deepseek-chat", "qwen-plus", "moonshot-v1-8k"]
        case .anthropic:
            ["claude-sonnet-4-20250514", "claude-opus-4-1"]
        case .gemini:
            ["gemini-1.5-pro", "gemini-1.5-flash"]
        case .claudeCode:
            ["sonnet", "opus"]
        case .codexCLI:
            ["gpt-5.5", "gpt-5.4", "gpt-5.4-mini"]
        }
    }
}

enum ReasoningEffort: String, Codable, CaseIterable {
    case low
    case medium
    case high
    case xhigh

    var label: String {
        switch self {
        case .low: "低"
        case .medium: "中"
        case .high: "高"
        case .xhigh: "极高"
        }
    }
}

struct LLMProvider: Codable, Identifiable, Hashable {
    var id: String
    var name: String
    var kind: LLMKind
    var baseURL: String
    var apiKey: String
    var model: String
    var maxTokens: Int
    var temperature: Double?
    var isDefault: Bool
    var extraHeaders: [String: String]
    var commandPath: String?
    var reasoningEffort: ReasoningEffort?

    init(
        id: String,
        name: String,
        kind: LLMKind,
        baseURL: String,
        apiKey: String,
        model: String,
        maxTokens: Int,
        temperature: Double?,
        isDefault: Bool,
        extraHeaders: [String: String],
        commandPath: String? = nil,
        reasoningEffort: ReasoningEffort? = nil
    ) {
        self.id = id
        self.name = name
        self.kind = kind
        self.baseURL = baseURL
        self.apiKey = apiKey
        self.model = model
        self.maxTokens = maxTokens
        self.temperature = temperature
        self.isDefault = isDefault
        self.extraHeaders = extraHeaders
        self.commandPath = commandPath
        self.reasoningEffort = reasoningEffort
    }

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case kind
        case baseURL = "base_url"
        case apiKey = "api_key"
        case model
        case maxTokens = "max_tokens"
        case temperature
        case isDefault = "is_default"
        case extraHeaders = "extra_headers"
        case commandPath = "command_path"
        case reasoningEffort = "reasoning_effort"
    }
}

struct LLMProviderPreset: Identifiable, Hashable {
    var id: String
    var name: String
    var kind: LLMKind
    var baseURL: String
    var model: String
    var apiKey: String
    var maxTokens: Int
    var temperature: Double?
    var isDefault: Bool
    var commandPath: String? = nil
    var reasoningEffort: ReasoningEffort? = nil
}

struct ReportTemplate: Codable, Identifiable, Hashable {
    var id: String
    var name: String
    var style: String
    var sections: [String]
    var providerID: String?
    var extraPrompt: String
    var builtin: Bool

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case style
        case sections
        case providerID = "provider_id"
        case extraPrompt = "extra_prompt"
        case builtin
    }
}

struct ReportRecord: Codable, Identifiable, Hashable {
    var id: String
    var week: String
    var templateID: String
    var templateName: String
    var providerID: String?
    var providerName: String?
    var tokensUsed: Int
    var projectCount: Int
    var generatedAt: String

    enum CodingKeys: String, CodingKey {
        case id
        case week
        case templateID = "template_id"
        case templateName = "template_name"
        case providerID = "provider_id"
        case providerName = "provider_name"
        case tokensUsed = "tokens_used"
        case projectCount = "project_count"
        case generatedAt = "generated_at"
    }
}

struct Schedule: Codable, Identifiable, Hashable {
    var id: String
    var name: String
    var cron: String
    var enabled: Bool
    var workspaceIDs: [String]
    var templateID: String
    var providerID: String?
    var days: Int
    var recipients: [String]
    var cc: [String]
    var subjectTemplate: String
    var lastRun: String?
    var lastStatus: String?
    var nextRun: String?

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case cron
        case enabled
        case workspaceIDs = "workspace_ids"
        case templateID = "template_id"
        case providerID = "provider_id"
        case days
        case recipients
        case cc
        case subjectTemplate = "subject_tpl"
        case lastRun = "last_run"
        case lastStatus = "last_status"
        case nextRun = "next_run"
    }
}

struct SMTPConfig: Codable, Hashable {
    var host: String
    var port: Int
    var username: String
    var password: String
    var fromName: String
    var useSSL: Bool

    enum CodingKeys: String, CodingKey {
        case host
        case port
        case username
        case password
        case fromName = "from_name"
        case useSSL = "use_ssl"
    }
}

extension SMTPConfig {
    static let defaults = SMTPConfig(
        host: "",
        port: 587,
        username: "",
        password: "",
        fromName: "",
        useSSL: false
    )
}

struct AppSettings: Codable, Hashable {
    var promptClipChars: Int
    var pastReportsContext: Int

    enum CodingKeys: String, CodingKey {
        case promptClipChars = "prompt_clip_chars"
        case pastReportsContext = "past_reports_context"
    }
}

extension AppSettings {
    static let defaults = AppSettings(promptClipChars: 200, pastReportsContext: 2)
}

extension LLMProvider {
    static let presets: [LLMProviderPreset] = [
        LLMProviderPreset(
            id: "anthropic",
            name: "Anthropic Claude",
            kind: .anthropic,
            baseURL: "https://api.anthropic.com",
            model: "claude-sonnet-4-20250514",
            apiKey: "",
            maxTokens: 2048,
            temperature: nil,
            isDefault: true
        ),
        LLMProviderPreset(
            id: "claude-code",
            name: "Claude Code 订阅",
            kind: .claudeCode,
            baseURL: "",
            model: "sonnet",
            apiKey: "",
            maxTokens: 0,
            temperature: nil,
            isDefault: false,
            commandPath: "claude",
            reasoningEffort: .medium
        ),
        LLMProviderPreset(
            id: "openai",
            name: "OpenAI GPT",
            kind: .openAICompatible,
            baseURL: "https://api.openai.com",
            model: "gpt-4o-mini",
            apiKey: "",
            maxTokens: 2048,
            temperature: 0.7,
            isDefault: false
        ),
        LLMProviderPreset(
            id: "codex-cli",
            name: "Codex CLI 订阅",
            kind: .codexCLI,
            baseURL: "",
            model: "",
            apiKey: "",
            maxTokens: 0,
            temperature: nil,
            isDefault: false,
            commandPath: "codex",
            reasoningEffort: .medium
        ),
        LLMProviderPreset(
            id: "deepseek",
            name: "DeepSeek",
            kind: .openAICompatible,
            baseURL: "https://api.deepseek.com",
            model: "deepseek-chat",
            apiKey: "",
            maxTokens: 2048,
            temperature: 0.7,
            isDefault: false
        ),
        LLMProviderPreset(
            id: "openrouter",
            name: "OpenRouter",
            kind: .openAICompatible,
            baseURL: "https://openrouter.ai/api",
            model: "anthropic/claude-sonnet-4",
            apiKey: "",
            maxTokens: 2048,
            temperature: 0.7,
            isDefault: false
        ),
        LLMProviderPreset(
            id: "moonshot",
            name: "Kimi (月之暗面)",
            kind: .openAICompatible,
            baseURL: "https://api.moonshot.cn",
            model: "moonshot-v1-8k",
            apiKey: "",
            maxTokens: 2048,
            temperature: 0.7,
            isDefault: false
        ),
        LLMProviderPreset(
            id: "qwen",
            name: "通义千问 (Qwen)",
            kind: .openAICompatible,
            baseURL: "https://dashscope.aliyuncs.com/compatible-mode",
            model: "qwen-plus",
            apiKey: "",
            maxTokens: 2048,
            temperature: 0.7,
            isDefault: false
        ),
        LLMProviderPreset(
            id: "gemini",
            name: "Gemini",
            kind: .gemini,
            baseURL: "https://generativelanguage.googleapis.com",
            model: "gemini-1.5-pro",
            apiKey: "",
            maxTokens: 2048,
            temperature: 0.7,
            isDefault: false
        ),
        LLMProviderPreset(
            id: "ollama",
            name: "本地 Ollama",
            kind: .openAICompatible,
            baseURL: "http://localhost:11434",
            model: "qwen2.5:7b",
            apiKey: "ollama",
            maxTokens: 2048,
            temperature: 0.7,
            isDefault: false
        ),
        LLMProviderPreset(
            id: "local-openai",
            name: "本地 vLLM / LM Studio",
            kind: .openAICompatible,
            baseURL: "http://localhost:8000",
            model: "Qwen/Qwen2.5-7B-Instruct",
            apiKey: "EMPTY",
            maxTokens: 2048,
            temperature: 0.7,
            isDefault: false
        ),
        LLMProviderPreset(
            id: "custom",
            name: "自定义 (OpenAI 兼容)",
            kind: .openAICompatible,
            baseURL: "",
            model: "",
            apiKey: "",
            maxTokens: 2048,
            temperature: 0.7,
            isDefault: false
        ),
    ]

    static func makePreset(_ preset: LLMProviderPreset) -> LLMProvider {
        LLMProvider(
            id: UUID().uuidString,
            name: preset.name,
            kind: preset.kind,
            baseURL: preset.baseURL,
            apiKey: preset.apiKey,
            model: preset.model,
            maxTokens: preset.maxTokens,
            temperature: preset.temperature,
            isDefault: preset.isDefault,
            extraHeaders: [:],
            commandPath: preset.commandPath,
            reasoningEffort: preset.reasoningEffort
        )
    }

    var endpointLabel: String {
        if kind.isCommandLineProvider {
            return commandExecutable
        }
        return baseURL
    }

    var commandExecutable: String {
        let trimmed = commandPath?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return trimmed.isEmpty ? (kind.defaultCommandPath ?? "") : trimmed
    }

    var effectiveReasoningEffort: ReasoningEffort? {
        if let reasoningEffort {
            return reasoningEffort
        }
        return kind.isCommandLineProvider ? .medium : nil
    }
}

extension ReportTemplate {
    static let builtins: [ReportTemplate] = [
        ReportTemplate(
            id: "builtin-tech",
            name: "技术周报",
            style: "tech",
            sections: ["本周 TL;DR", "各项目进展", "技术亮点", "下周计划"],
            providerID: nil,
            extraPrompt: "",
            builtin: true
        ),
        ReportTemplate(
            id: "builtin-exec",
            name: "管理层汇报",
            style: "exec",
            sections: ["执行摘要", "关键进展", "风险阻塞", "下周重点"],
            providerID: nil,
            extraPrompt: "",
            builtin: true
        ),
        ReportTemplate(
            id: "builtin-simple",
            name: "简洁日报",
            style: "simple",
            sections: ["做了啥", "问题", "下周"],
            providerID: nil,
            extraPrompt: "",
            builtin: true
        ),
    ]
}
