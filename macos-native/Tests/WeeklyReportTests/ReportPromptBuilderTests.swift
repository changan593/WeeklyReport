import Testing
@testable import WeeklyReport

@Suite("Report prompt")
struct ReportPromptBuilderTests {
    @Test
    func promptContainsStatsLogsSectionsAndPastReports() {
        let summary = WorkSummary(
            byProject: [
                "weekly-report": ["实现 LLM provider 抽象", "把 SQLite 换成 JSON 文件"],
                "chat-bot": ["调试 stream API 的中断问题"],
            ],
            aiSnippets: [],
            conversations: [
                ConversationDigest(
                    id: "c1",
                    project: "weekly-report",
                    tool: "codex",
                    server: "本机",
                    startAt: nil,
                    endAt: nil,
                    userPrompts: ["实现 LLM provider 抽象", "继续完善 SwiftUI"],
                    modelExcerpt: "已完成 Provider 抽象，并补充 SwiftUI 配置页。"
                ),
            ],
            stats: SummaryStats(
                totalPrompts: 3,
                activeDays: 5,
                projectCount: 2,
                mainProject: "weekly-report",
                servers: ["本机"],
                tools: ["claude-code"],
                skippedLines: 0,
                skippedFiles: 0
            )
        )

        let prompt = ReportPromptBuilder.buildPrompt(
            summary: summary,
            template: ReportTemplate.builtins[0],
            pastReports: ["# 上周周报\n\n做了 A。"]
        )

        #expect(prompt.contains("活跃天数：5"))
        #expect(prompt.contains("项目数：2"))
        #expect(prompt.contains("主项目：weekly-report"))
        #expect(prompt.contains("<work_logs>"))
        #expect(prompt.contains("【weekly-report】(2 条指令)"))
        #expect(prompt.contains("<conversation_digests>"))
        #expect(prompt.contains("模型结果摘录：已完成 Provider 抽象"))
        #expect(prompt.contains("本周 TL;DR / 各项目进展 / 技术亮点 / 下周计划"))
        #expect(prompt.contains("<past_report_1>"))
    }

    @Test
    func promptLimitsKeepHighestVolumeProjectsAndMentionOmissions() {
        let summary = WorkSummary(
            byProject: [
                "large": (1...5).map { "large-\($0)" },
                "medium": (1...3).map { "medium-\($0)" },
                "small": ["small-1"],
            ],
            aiSnippets: [],
            conversations: [
                ConversationDigest(
                    id: "large-c1",
                    project: "large",
                    tool: "codex",
                    server: "本机",
                    startAt: nil,
                    endAt: nil,
                    userPrompts: ["large-1", "large-2", "large-3"],
                    modelExcerpt: "完成 large 的主要改动。"
                ),
                ConversationDigest(
                    id: "small-c1",
                    project: "small",
                    tool: "claude-code",
                    server: "本机",
                    startAt: nil,
                    endAt: nil,
                    userPrompts: ["small-1"],
                    modelExcerpt: "完成 small。"
                ),
            ],
            stats: SummaryStats(
                totalPrompts: 9,
                activeDays: 2,
                projectCount: 3,
                mainProject: "large",
                servers: ["本机"],
                tools: ["codex"],
                skippedLines: 0,
                skippedFiles: 0
            )
        )

        let limits = ReportPromptLimits(
            maxProjects: 2,
            maxPromptsPerProject: 2,
            maxTotalPrompts: 3,
            maxConversationDigests: 1,
            maxPromptsPerConversation: 2
        )
        let prompt = ReportPromptBuilder.buildPrompt(
            summary: summary,
            template: ReportTemplate.builtins[0],
            pastReports: [],
            limits: limits
        )

        #expect(ReportPromptBuilder.promptCountAfterApplyingLimits(summary: summary, limits: limits) == 3)
        #expect(prompt.contains("省略 6 条"))
        #expect(prompt.contains("【large】(5 条指令，展示 2 条，省略 3 条)"))
        #expect(prompt.contains("large-1"))
        #expect(prompt.contains("large-2"))
        #expect(prompt.contains("medium-1"))
        #expect(!prompt.contains("【small】"))
        #expect(prompt.contains("已保留 1 个高优先级对话"))
        #expect(prompt.contains("展示 2 条，省略 1 条"))
    }

    @Test
    func promptLimitsClampOversizedTextsAndWholePrompt() {
        let huge = String(repeating: "超", count: 10_000)
        let summary = WorkSummary(
            byProject: ["huge": [huge, huge, huge]],
            aiSnippets: [],
            conversations: [
                ConversationDigest(
                    id: "huge-c1",
                    project: "huge",
                    tool: "codex",
                    server: "本机",
                    startAt: nil,
                    endAt: nil,
                    userPrompts: [huge, huge],
                    modelExcerpt: huge
                ),
            ],
            stats: SummaryStats(
                totalPrompts: 3,
                activeDays: 1,
                projectCount: 1,
                mainProject: "huge",
                servers: ["本机"],
                tools: ["codex"],
                skippedLines: 0,
                skippedFiles: 0
            )
        )
        let limits = ReportPromptLimits(
            maxProjects: 1,
            maxPromptsPerProject: 3,
            maxTotalPrompts: 3,
            maxConversationDigests: 1,
            maxPromptsPerConversation: 2,
            maxCharsPerWorkPrompt: 120,
            maxCharsPerConversationPrompt: 80,
            maxCharsPerModelExcerpt: 80,
            maxPromptChars: 2_000
        )

        let prompt = ReportPromptBuilder.buildPrompt(
            summary: summary,
            template: ReportTemplate.builtins[0],
            pastReports: [],
            limits: limits
        )

        #expect(prompt.count <= 2_000)
        #expect(prompt.contains("…"))
        #expect(!prompt.contains(String(repeating: "超", count: 1_000)))
    }
}
