import Foundation

enum ClaudeLogParser {
    static func parseHistoryLine(_ line: String, server: String) -> LogMessage? {
        guard let object = JSONLine.object(line),
              let rawText = object["display"] as? String
        else {
            return nil
        }

        let text = rawText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return nil }

        let project = (object["project"] as? String).map(LogText.pathBasename) ?? "Claude Code"
        return LogMessage(
            role: .user,
            text: text,
            timestamp: JSONLine.timestamp(object["timestamp"]),
            project: project,
            tool: .claudeCode,
            server: server,
            conversationID: (object["sessionId"] as? String) ?? "claude-history-\(project)"
        )
    }

    static func parseSessionLine(
        _ line: String,
        server: String,
        project: String,
        clipChars: Int,
        conversationID: String = ""
    ) -> [LogMessage] {
        guard let object = JSONLine.object(line) else {
            return []
        }

        let timestamp = JSONLine.timestamp(object["timestamp"])
        let type = object["type"] as? String

        switch type {
        case "user":
            return parseUserLine(object, server: server, project: project, timestamp: timestamp, conversationID: conversationID)
        case "assistant":
            return parseAssistantLine(object, server: server, project: project, timestamp: timestamp, clipChars: clipChars, conversationID: conversationID)
        default:
            return []
        }
    }

    static func inferSessionProject(from contents: String, fallbackName: String) -> String {
        for line in contents.lines {
            if let object = JSONLine.object(String(line)),
               let cwd = object["cwd"] as? String {
                return LogText.pathBasename(cwd)
            }
        }
        return fallbackName
    }

    private static func parseUserLine(
        _ object: [String: Any],
        server: String,
        project: String,
        timestamp: Date?,
        conversationID: String
    ) -> [LogMessage] {
        if object["toolUseResult"] != nil {
            return []
        }
        if (object["isMeta"] as? Bool) == true {
            return []
        }
        guard let message = object["message"] as? [String: Any],
              let rawText = message["content"] as? String
        else {
            return []
        }
        let text = rawText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return [] }

        return [
            LogMessage(
                role: .user,
                text: text,
                timestamp: timestamp,
                project: project,
                tool: .claudeCode,
                server: server,
                conversationID: resolvedConversationID(object: object, fallback: conversationID)
            ),
        ]
    }

    private static func parseAssistantLine(
        _ object: [String: Any],
        server: String,
        project: String,
        timestamp: Date?,
        clipChars: Int,
        conversationID: String
    ) -> [LogMessage] {
        guard let message = object["message"] as? [String: Any],
              let blocks = message["content"] as? [[String: Any]]
        else {
            return []
        }

        let parts = blocks.compactMap { block -> String? in
            guard block["type"] as? String == "text",
                  let rawText = block["text"] as? String
            else {
                return nil
            }
            let text = rawText.trimmingCharacters(in: .whitespacesAndNewlines)
            return text.isEmpty ? nil : text
        }

        guard !parts.isEmpty else { return [] }
        return [
            LogMessage(
                role: .assistant,
                text: LogText.clip(parts.joined(separator: "\n"), keeping: clipChars),
                timestamp: timestamp,
                project: project,
                tool: .claudeCode,
                server: server,
                conversationID: resolvedConversationID(object: object, fallback: conversationID)
            ),
        ]
    }

    private static func resolvedConversationID(object: [String: Any], fallback: String) -> String {
        if let sessionID = object["sessionId"] as? String, !sessionID.isEmpty {
            return sessionID
        }
        return fallback
    }
}

private extension String {
    var lines: [Substring] {
        split(whereSeparator: \.isNewline)
    }
}
