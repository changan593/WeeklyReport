import Foundation

struct ReportPromptLimits: Equatable {
    var maxProjects: Int? = nil
    var maxPromptsPerProject: Int? = nil
    var maxTotalPrompts: Int? = nil
    var maxConversationDigests: Int? = nil
    var maxPromptsPerConversation: Int? = nil
    var maxCharsPerWorkPrompt: Int? = nil
    var maxCharsPerConversationPrompt: Int? = nil
    var maxCharsPerModelExcerpt: Int? = nil
    var maxPromptChars: Int? = nil

    static let unlimited = ReportPromptLimits(
        maxProjects: nil,
        maxPromptsPerProject: nil,
        maxTotalPrompts: nil,
        maxConversationDigests: nil,
        maxPromptsPerConversation: nil,
        maxCharsPerWorkPrompt: nil,
        maxCharsPerConversationPrompt: nil,
        maxCharsPerModelExcerpt: nil,
        maxPromptChars: nil
    )

    static let commandLine = ReportPromptLimits(
        maxProjects: 12,
        maxPromptsPerProject: 10,
        maxTotalPrompts: 100,
        maxConversationDigests: 24,
        maxPromptsPerConversation: 3,
        maxCharsPerWorkPrompt: 1_500,
        maxCharsPerConversationPrompt: 1_000,
        maxCharsPerModelExcerpt: 1_200,
        maxPromptChars: 650_000
    )
}

enum ReportPromptBuilder {
    static func buildPrompt(
        summary: WorkSummary,
        template: ReportTemplate,
        pastReports: [String],
        limits: ReportPromptLimits = .unlimited
    ) -> String {
        var output = ""
        output += "你是工程师周报助手，请基于以下工作日志生成一份 Markdown 格式的周报。\n\n"
        output += "风格：\(styleLabel(template.style))\n"

        let stats = summary.stats
        output += "活跃天数：\(stats.activeDays) | 项目数：\(stats.projectCount) | 主项目：\(stats.mainProject ?? "无")\n"
        if !stats.servers.isEmpty {
            output += "服务器：\(stats.servers.joined(separator: "、"))\n"
        }
        if !stats.tools.isEmpty {
            output += "工具：\(stats.tools.joined(separator: "、"))\n"
        }
        output += "\n"

        output += "以下是从日志提取的用户工作指令（按项目分组）：\n"
        output += "<work_logs>\n"

        let projects = limitedProjects(summary.byProject, limits: limits)

        let omittedPrompts = summary.stats.totalPrompts - projects.reduce(0) { total, item in
            total + item.prompts.count
        }
        if omittedPrompts > 0 {
            output += "为控制上下文长度，已优先保留近期/高频项目中的 \(summary.stats.totalPrompts - omittedPrompts) 条指令，省略 \(omittedPrompts) 条较低优先级指令。\n"
        }

        if projects.isEmpty {
            output += "（本期未提取到任何用户指令）\n"
        } else {
            for item in projects {
                output += "【\(item.name)】(\(item.originalCount) 条指令"
                if item.omittedCount > 0 {
                    output += "，展示 \(item.prompts.count) 条，省略 \(item.omittedCount) 条"
                }
                output += ")\n"
                for prompt in item.prompts {
                    output += "  · \(singleLine(prompt, maxChars: limits.maxCharsPerWorkPrompt))\n"
                }
                output += "\n"
            }
        }
        output += "</work_logs>\n\n"

        let conversations = limitedConversations(summary.conversations, limits: limits)
        if !conversations.isEmpty {
            let omittedConversations = max(summary.conversations.count - conversations.count, 0)
            output += "以下是按对话整理的执行结果摘要。用户指令保留原意；模型输出仅保留首尾摘录，不包含工具调用过程：\n"
            if omittedConversations > 0 {
                output += "为控制上下文长度，已保留 \(conversations.count) 个高优先级对话，省略 \(omittedConversations) 个对话。\n"
            }
            output += "<conversation_digests>\n"
            for conversation in conversations {
                output += "【\(conversation.project) · \(conversation.tool) · \(conversation.server)】"
                output += "(\(conversation.originalPromptCount) 条指令"
                if conversation.omittedPromptCount > 0 {
                    output += "，展示 \(conversation.userPrompts.count) 条，省略 \(conversation.omittedPromptCount) 条"
                }
                output += ")\n"
                for prompt in conversation.userPrompts {
                    output += "  用户：\(singleLine(prompt, maxChars: limits.maxCharsPerConversationPrompt))\n"
                }
                if let modelExcerpt = conversation.modelExcerpt?.trimmingCharacters(in: .whitespacesAndNewlines),
                   !modelExcerpt.isEmpty {
                    output += "  模型结果摘录：\(singleLine(modelExcerpt, maxChars: limits.maxCharsPerModelExcerpt))\n"
                }
                output += "\n"
            }
            output += "</conversation_digests>\n\n"
        }

        if !pastReports.isEmpty {
            output += "以下是最近的历史周报，请**仅参考其结构和语气**，不要照抄具体内容：\n"
            for (index, report) in pastReports.enumerated() {
                let number = index + 1
                output += "<past_report_\(number)>\n\(report)\n</past_report_\(number)>\n\n"
            }
        }

        if template.sections.isEmpty {
            output += "请输出一份结构清晰的 Markdown 周报。\n\n"
        } else {
            output += "请按以下章节顺序输出 Markdown 周报：\(template.sections.joined(separator: " / "))\n\n"
        }

        output += "要求：\n"
        output += "1. **提炼总结**，不要逐条照抄原始用户指令\n"
        output += "2. 相似指令应**归纳合并**为一句话\n"
        output += "3. 下周计划可基于趋势合理推断，但所有非事实陈述都要标注「（推断）」\n"
        output += "4. 每个章节用 Markdown 二级标题（`##`）开头\n"
        output += "5. 不要包含本指令中提到的元信息（如 `活跃天数`、`<work_logs>` 标签）\n"

        let extra = template.extraPrompt.trimmingCharacters(in: .whitespacesAndNewlines)
        if !extra.isEmpty {
            output += "\n额外要求：\n"
            output += extra
            output += "\n"
        }

        return clampPrompt(output, limits: limits)
    }

    private static func limitedProjects(
        _ byProject: [String: [String]],
        limits: ReportPromptLimits
    ) -> [(name: String, prompts: [String], originalCount: Int, omittedCount: Int)] {
        let sorted = byProject.sorted { lhs, rhs in
            if lhs.value.count != rhs.value.count {
                return lhs.value.count > rhs.value.count
            }
            return lhs.key < rhs.key
        }

        var remainingTotal = limits.maxTotalPrompts ?? Int.max
        let projectLimit = limits.maxProjects ?? Int.max
        let perProjectLimit = limits.maxPromptsPerProject ?? Int.max

        return sorted.prefix(projectLimit).compactMap { name, prompts in
            guard remainingTotal > 0 else { return nil }
            let count = min(prompts.count, perProjectLimit, remainingTotal)
            guard count > 0 else { return nil }
            remainingTotal -= count
            return (
                name: name,
                prompts: Array(prompts.prefix(count)),
                originalCount: prompts.count,
                omittedCount: max(prompts.count - count, 0)
            )
        }
    }

    private static func singleLine(_ text: String, maxChars: Int?) -> String {
        let line = text.replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard let maxChars, line.count > maxChars else {
            return line
        }
        return LogText.clip(line, keeping: max(1, maxChars / 2))
    }

    private static func clampPrompt(_ prompt: String, limits: ReportPromptLimits) -> String {
        guard let maxPromptChars = limits.maxPromptChars, prompt.count > maxPromptChars else {
            return prompt
        }

        let suffix = "\n\n[系统提示：本次输入超过 CLI 长度预算，已截断低优先级上下文。]\n"
        let keep = max(maxPromptChars - suffix.count, 0)
        return String(prompt.prefix(keep)) + suffix
    }

    static func promptCountAfterApplyingLimits(
        summary: WorkSummary,
        limits: ReportPromptLimits
    ) -> Int {
        limitedProjects(summary.byProject, limits: limits).reduce(0) { total, item in
            total + item.prompts.count
        }
    }

    private static func limitedConversations(
        _ conversations: [ConversationDigest],
        limits: ReportPromptLimits
    ) -> [(project: String, tool: String, server: String, userPrompts: [String], originalPromptCount: Int, omittedPromptCount: Int, modelExcerpt: String?)] {
        let sorted = conversations.sorted { lhs, rhs in
            switch (lhs.endAt, rhs.endAt) {
            case let (l?, r?) where l != r:
                return l > r
            case (_?, nil):
                return true
            case (nil, _?):
                return false
            default:
                if lhs.promptCount != rhs.promptCount {
                    return lhs.promptCount > rhs.promptCount
                }
                return lhs.project < rhs.project
            }
        }
        let conversationLimit = limits.maxConversationDigests ?? Int.max
        let promptLimit = limits.maxPromptsPerConversation ?? Int.max
        return sorted.prefix(conversationLimit).map { conversation in
            let shownPrompts = Array(conversation.userPrompts.prefix(promptLimit))
            return (
                project: conversation.project,
                tool: conversation.tool,
                server: conversation.server,
                userPrompts: shownPrompts,
                originalPromptCount: conversation.userPrompts.count,
                omittedPromptCount: max(conversation.userPrompts.count - shownPrompts.count, 0),
                modelExcerpt: conversation.modelExcerpt
            )
        }
    }

    private static func styleLabel(_ style: String) -> String {
        switch style {
        case "tech":
            "技术向 —— 重视代码实现、bug 修复、技术选型"
        case "exec":
            "管理层汇报向 —— 重视业务影响、关键产出、风险与阻塞"
        case "simple":
            "简洁日报向 —— 要点列出即可，不展开细节"
        default:
            "自定义"
        }
    }
}
