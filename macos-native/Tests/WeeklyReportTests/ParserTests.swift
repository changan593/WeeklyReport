import Testing
@testable import WeeklyReport

@Suite("JSONL parsers")
struct ParserTests {
    private let claudeHistory = """
    {"display":"帮我把 SQLite 换成 JSON","pastedContents":{},"timestamp":1747476225000,"project":"-Users-me-weekly-report"}
    {"display":"修一下 schedule 的 cron 解析","pastedContents":{},"timestamp":1747476500000,"project":"-Users-me-weekly-report"}
    {"display":"调试 stream API 的中断问题","pastedContents":{},"timestamp":1747477000000,"project":"-Users-me-chat-bot"}
    {"display":"   ","pastedContents":{},"timestamp":1747477500000,"project":"-Users-me-x"}
    {"display":"   写一份 README","pastedContents":{},"timestamp":1747478000000,"project":"-Users-me-weekly-report"}
    """

    private let claudeSessionToolResult = """
    {"type":"user","timestamp":"2026-05-17T11:00:00Z","uuid":"u1","sessionId":"s3","cwd":"/Users/me/weekly-report","message":{"role":"user","content":"运行测试看看"}}
    {"type":"assistant","timestamp":"2026-05-17T11:00:05Z","uuid":"a1","sessionId":"s3","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu1","name":"Bash","input":{"command":"cargo test"}}]}}
    {"type":"user","timestamp":"2026-05-17T11:00:30Z","uuid":"u2","parentUuid":"a1","sessionId":"s3","toolUseResult":"test result: ok. 37 passed","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu1","content":[{"type":"text","text":"test result: ok. 37 passed"}]}]}}
    {"type":"user","timestamp":"2026-05-17T11:01:00Z","uuid":"u3","sessionId":"s3","isMeta":true,"message":{"role":"user","content":"系统注入的元消息"}}
    """

    private let codexRollout = """
    {"timestamp":"2026-05-17T09:00:00Z","type":"session_meta","payload":{"id":"thread-uuid","timestamp":"2026-05-17T09:00:00Z","cwd":"/Users/me/weekly-report","originator":"codex","cli_version":"0.50.0"}}
    {"timestamp":"2026-05-17T09:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"重构这个模块"}]}}
    {"timestamp":"2026-05-17T09:00:15Z","type":"response_item","payload":{"type":"reasoning","summary":[{"text":"我先看一下结构"}]}}
    {"timestamp":"2026-05-17T09:00:20Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"好的，我会先扫描目录结构，找出待重构的关键文件。"}]}}
    {"timestamp":"2026-05-17T09:00:25Z","type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\\"command\\":[\\"ls\\",\\"-la\\"]}"}}
    {"timestamp":"2026-05-17T09:02:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"加测试"}]}}
    """

    @Test
    func claudeHistoryParsesNonEmptyDisplayPrompts() {
        let messages = claudeHistory.split(whereSeparator: \.isNewline)
            .compactMap { ClaudeLogParser.parseHistoryLine(String($0), server: "本机") }

        #expect(messages.count == 4)
        #expect(messages.allSatisfy { $0.role == .user })
        #expect(messages[0].text == "帮我把 SQLite 换成 JSON")
        #expect(messages[0].project == "-Users-me-weekly-report")
    }

    @Test
    func claudeSessionDropsToolResultAndMetaUserLines() {
        let messages = claudeSessionToolResult.split(whereSeparator: \.isNewline)
            .flatMap {
                ClaudeLogParser.parseSessionLine(
                    String($0),
                    server: "本机",
                    project: "weekly-report",
                    clipChars: 200,
                    conversationID: "fixture-session"
                )
            }

        let userMessages = messages.filter { $0.role == .user }
        #expect(userMessages.map(\.text) == ["运行测试看看"])
        #expect(!messages.contains { $0.text.contains("test result") })
        #expect(userMessages.allSatisfy { $0.conversationID == "s3" })
    }

    @Test
    func codexRolloutParsesMessagesAndDropsReasoningToolCalls() {
        let project = CodexLogParser.inferRolloutProject(from: codexRollout, fallbackName: "fallback")
        let messages = codexRollout.split(whereSeparator: \.isNewline)
            .flatMap {
                CodexLogParser.parseRolloutLine(
                    String($0),
                    server: "本机",
                    project: project,
                    clipChars: 200,
                    conversationID: "rollout-fixture"
                )
            }

        #expect(project == "weekly-report")
        #expect(messages.map(\.text) == ["重构这个模块", "好的，我会先扫描目录结构，找出待重构的关键文件。", "加测试"])
        #expect(messages.map(\.role) == [.user, .assistant, .user])
        #expect(messages.allSatisfy { $0.tool == .codex })
        #expect(messages.allSatisfy { $0.conversationID == "rollout-fixture" })
    }
}
