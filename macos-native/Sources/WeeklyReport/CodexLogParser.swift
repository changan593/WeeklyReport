import Foundation

enum CodexLogParser {
    static func parseRolloutLine(
        _ line: String,
        server: String,
        project: String,
        clipChars: Int,
        conversationID: String = ""
    ) -> [LogMessage] {
        guard let object = JSONLine.object(line),
              object["type"] as? String == "response_item",
              let payload = object["payload"] as? [String: Any]
        else {
            return []
        }

        let timestamp = JSONLine.timestamp(object["timestamp"])
        guard payload["type"] as? String == "message" else {
            return []
        }

        return parseMessage(payload, server: server, project: project, timestamp: timestamp, clipChars: clipChars, conversationID: conversationID)
    }

    static func inferRolloutProject(from contents: String, fallbackName: String) -> String {
        for line in contents.lines {
            guard let object = JSONLine.object(String(line)),
                  object["type"] as? String == "session_meta"
            else {
                continue
            }

            if let payload = object["payload"] as? [String: Any],
               let cwd = payload["cwd"] as? String {
                return LogText.pathBasename(cwd)
            }
            if let cwd = object["cwd"] as? String {
                return LogText.pathBasename(cwd)
            }
        }
        return fallbackName
    }

    private static func parseMessage(
        _ payload: [String: Any],
        server: String,
        project: String,
        timestamp: Date?,
        clipChars: Int,
        conversationID: String
    ) -> [LogMessage] {
        guard let blocks = payload["content"] as? [[String: Any]] else {
            return []
        }

        var parts: [String] = []
        var hasUserText = false
        var hasAssistantText = false

        for block in blocks {
            guard let type = block["type"] as? String,
                  let rawText = block["text"] as? String
            else {
                continue
            }

            let text = rawText.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !text.isEmpty else { continue }

            switch type {
            case "input_text":
                parts.append(text)
                hasUserText = true
            case "output_text":
                parts.append(text)
                hasAssistantText = true
            default:
                continue
            }
        }

        guard !parts.isEmpty else { return [] }

        let roleString = payload["role"] as? String
        let role: LogRole = if roleString == "user" || (roleString == nil && hasUserText && !hasAssistantText) {
            .user
        } else {
            .assistant
        }
        let combined = parts.joined(separator: "\n")
        let text = role == .user ? combined : LogText.clip(combined, keeping: clipChars)

        return [
            LogMessage(
                role: role,
                text: text,
                timestamp: timestamp,
                project: project,
                tool: .codex,
                server: server,
                conversationID: conversationID
            ),
        ]
    }
}

private extension String {
    var lines: [Substring] {
        split(whereSeparator: \.isNewline)
    }
}
