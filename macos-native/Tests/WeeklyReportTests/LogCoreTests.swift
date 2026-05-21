import Foundation
import Testing
@testable import WeeklyReport

@Suite("Log core")
struct LogCoreTests {
    @Test
    func clipTextPreservesUnicodeBoundaries() {
        let text = String(repeating: "中", count: 500)
        let output = LogText.clip(text, keeping: 100)
        #expect(output.count == 201)
        #expect(output.allSatisfy { $0 == "中" || $0 == "…" })
    }

    @Test
    func pathBasenameHandlesUnixAndWindowsPaths() {
        #expect(LogText.pathBasename("/Users/me/app") == "app")
        #expect(LogText.pathBasename("C:\\dev\\weekly-report\\") == "weekly-report")
    }

    @Test
    func aggregateGroupsUserPromptsAndDropsAdjacentDuplicates() {
        let now = Date()
        let repeatedPrefix = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        let messages = [
            LogMessage(role: .user, text: "\(repeatedPrefix) first", timestamp: now, project: "weekly-report", tool: .claudeCode, server: "本机"),
            LogMessage(role: .user, text: "\(repeatedPrefix) second", timestamp: now.addingTimeInterval(1), project: "weekly-report", tool: .claudeCode, server: "本机"),
            LogMessage(role: .user, text: "调试 stream API", timestamp: now.addingTimeInterval(2), project: "chat-bot", tool: .codex, server: "本机"),
            LogMessage(role: .assistant, text: "我会先看 scheduler.rs", timestamp: now.addingTimeInterval(3), project: "weekly-report", tool: .claudeCode, server: "本机"),
        ]

        let summary = LogAggregator.aggregate(messages, parseStats: ParseStats(skippedLines: 1, skippedFiles: 2))

        #expect(summary.byProject["weekly-report"] == ["\(repeatedPrefix) first"])
        #expect(summary.byProject["chat-bot"] == ["调试 stream API"])
        #expect(summary.stats.totalPrompts == 2)
        #expect(summary.stats.projectCount == 2)
        #expect(summary.stats.skippedLines == 1)
        #expect(summary.stats.skippedFiles == 2)
        #expect(summary.aiSnippets == ["我会先看 scheduler.rs"])
    }

    @Test
    func aggregateBuildsConversationDigestsWithUserPromptsAndModelExcerpts() throws {
        let now = Date()
        let longOutput = String(repeating: "前", count: 260) + "中间内容" + String(repeating: "后", count: 260)
        let messages = [
            LogMessage(role: .user, text: "实现会话摘要", timestamp: now, project: "weekly-report", tool: .codex, server: "本机", conversationID: "c1"),
            LogMessage(role: .assistant, text: longOutput, timestamp: now.addingTimeInterval(1), project: "weekly-report", tool: .codex, server: "本机", conversationID: "c1"),
            LogMessage(role: .user, text: "补测试", timestamp: now.addingTimeInterval(2), project: "weekly-report", tool: .codex, server: "本机", conversationID: "c1"),
        ]

        let summary = LogAggregator.aggregate(messages)

        let digest = try #require(summary.conversations.first)
        #expect(digest.id == "c1")
        #expect(digest.userPrompts == ["实现会话摘要", "补测试"])
        #expect(digest.modelExcerpt?.contains("…") == true)
        #expect(digest.modelExcerpt?.contains("前") == true)
        #expect(digest.modelExcerpt?.contains("后") == true)
    }
}
