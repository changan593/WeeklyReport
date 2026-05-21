import Foundation
import Testing
@testable import WeeklyReport

@Suite("App model configuration")
struct AppModelConfigTests {
    @Test
    @MainActor
    func saveProviderMaintainsSingleDefaultAndPersistsSecretFile() throws {
        let store = JSONStore(dataDirectory: try makeTemporaryDirectory())
        let model = AppModel(store: store)

        let first = try model.saveProvider(makeProvider(name: "A", isDefault: false))
        #expect(first.isDefault)
        #expect(model.providers.count == 1)
        #expect(model.providers[0].isDefault)

        let second = try model.saveProvider(makeProvider(name: "B", isDefault: true))
        #expect(second.isDefault)
        #expect(model.providers.count == 2)
        #expect(model.providers.first { $0.id == first.id }?.isDefault == false)
        #expect(model.providers.first { $0.id == second.id }?.isDefault == true)

        let stored = try store.read([LLMProvider].self, from: "llm_providers.json", default: [])
        #expect(stored.count == 2)
        #expect(stored.first { $0.id == second.id }?.isDefault == true)
    }

    @Test
    @MainActor
    func deleteDefaultProviderReelectsFirstRemainingProvider() throws {
        let store = JSONStore(dataDirectory: try makeTemporaryDirectory())
        let model = AppModel(store: store)

        let first = try model.saveProvider(makeProvider(name: "A", isDefault: false))
        let second = try model.saveProvider(makeProvider(name: "B", isDefault: false))
        let third = try model.saveProvider(makeProvider(name: "C", isDefault: false))

        try model.deleteProvider(id: first.id)

        #expect(model.providers.map(\.id) == [second.id, third.id])
        #expect(model.providers.first?.isDefault == true)
        #expect(model.providers.first?.id == second.id)
    }

    @Test
    @MainActor
    func saveWorkspaceTrimsAndPersistsEditableFields() throws {
        let store = JSONStore(dataDirectory: try makeTemporaryDirectory())
        let model = AppModel(store: store)

        let saved = try model.saveWorkspace(Workspace(
            id: "",
            name: "  本机  ",
            type: .local,
            host: "",
            user: "",
            port: nil,
            authMethod: .key,
            sshKey: "",
            sshPassword: "",
            claudePath: "  ~/.claude  ",
            codexPath: "  ~/.codex  ",
            tools: ["claude-code", "codex"]
        ))

        #expect(saved.id.isEmpty == false)
        #expect(saved.name == "本机")
        #expect(saved.host == nil)
        #expect(saved.claudePath == "~/.claude")

        let stored = try store.read([Workspace].self, from: "workspaces.json", default: [])
        #expect(stored == [saved])
    }

    @Test
    @MainActor
    func saveCommandLineProviderDoesNotRequireAPIFields() throws {
        let store = JSONStore(dataDirectory: try makeTemporaryDirectory())
        let model = AppModel(store: store)

        let saved = try model.saveProvider(LLMProvider(
            id: "",
            name: "  Claude Code 订阅  ",
            kind: .claudeCode,
            baseURL: "",
            apiKey: "should-be-cleared",
            model: "sonnet",
            maxTokens: 0,
            temperature: 0.7,
            isDefault: false,
            extraHeaders: [:],
            commandPath: "  claude  ",
            reasoningEffort: .medium
        ))

        #expect(saved.name == "Claude Code 订阅")
        #expect(saved.baseURL.isEmpty)
        #expect(saved.apiKey.isEmpty)
        #expect(saved.temperature == nil)
        #expect(saved.commandPath == "claude")
        #expect(saved.reasoningEffort == .medium)
        #expect(saved.isDefault)
    }

    @Test
    @MainActor
    func saveSettingsPersistsEditableValues() throws {
        let store = JSONStore(dataDirectory: try makeTemporaryDirectory())
        let model = AppModel(store: store)

        try model.saveSettings(AppSettings(promptClipChars: 320, pastReportsContext: 4))

        #expect(model.settings == AppSettings(promptClipChars: 320, pastReportsContext: 4))

        let stored = try store.read(AppSettings.self, from: "settings.json", default: .defaults)
        #expect(stored == AppSettings(promptClipChars: 320, pastReportsContext: 4))
    }

    @Test
    @MainActor
    func saveSettingsRejectsNegativeValues() throws {
        let store = JSONStore(dataDirectory: try makeTemporaryDirectory())
        let model = AppModel(store: store)
        var didThrow = false

        do {
            try model.saveSettings(AppSettings(promptClipChars: -1, pastReportsContext: 0))
        } catch {
            didThrow = true
        }

        #expect(didThrow)
    }

    @Test
    @MainActor
    func saveSMTPConfigTrimsAndPersistsSecretFile() throws {
        let store = JSONStore(dataDirectory: try makeTemporaryDirectory())
        let model = AppModel(store: store)

        try model.saveSMTPConfig(SMTPConfig(
            host: "  smtp.example.com  ",
            port: 587,
            username: "  me@example.com  ",
            password: "  app-password  ",
            fromName: "  WeeklyReport  ",
            useSSL: false
        ))

        let expected = SMTPConfig(
            host: "smtp.example.com",
            port: 587,
            username: "me@example.com",
            password: "app-password",
            fromName: "WeeklyReport",
            useSSL: false
        )
        #expect(model.smtpConfig == expected)

        let stored = try store.read(SMTPConfig.self, from: "smtp.json", default: .defaults)
        #expect(stored == expected)

        let smtpPath = try store.dataDirectory.appendingPathComponent("smtp.json").path
        let attributes = try FileManager.default.attributesOfItem(atPath: smtpPath)
        #expect((attributes[.posixPermissions] as? NSNumber)?.intValue == 0o600)
    }

    private func makeProvider(name: String, isDefault: Bool) -> LLMProvider {
        LLMProvider(
            id: UUID().uuidString,
            name: name,
            kind: .openAICompatible,
            baseURL: "https://api.example.com",
            apiKey: "sk-test",
            model: "test-model",
            maxTokens: 1024,
            temperature: 0.7,
            isDefault: isDefault,
            extraHeaders: [:]
        )
    }

    private func makeTemporaryDirectory() throws -> URL {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("weekly-report-config-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        return root
    }
}
