import Foundation

enum LogRole: String, Equatable {
    case user
    case assistant
}

enum CodingTool: String, Equatable, Comparable {
    case claudeCode = "claude-code"
    case codex

    static func < (lhs: CodingTool, rhs: CodingTool) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

struct LogMessage: Equatable {
    var role: LogRole
    var text: String
    var timestamp: Date?
    var project: String
    var tool: CodingTool
    var server: String
    var conversationID: String = ""
}

struct ParseStats: Equatable {
    var skippedLines = 0
    var skippedFiles = 0

    mutating func merge(_ other: ParseStats) {
        skippedLines += other.skippedLines
        skippedFiles += other.skippedFiles
    }
}

struct SummaryStats: Equatable {
    var totalPrompts: Int = 0
    var activeDays: Int = 0
    var projectCount: Int = 0
    var mainProject: String?
    var servers: [String] = []
    var tools: [String] = []
    var skippedLines: Int = 0
    var skippedFiles: Int = 0
}

struct WorkSummary: Equatable {
    var byProject: [String: [String]] = [:]
    var aiSnippets: [String] = []
    var conversations: [ConversationDigest] = []
    var stats = SummaryStats()
}

struct ConversationDigest: Equatable, Identifiable {
    var id: String
    var project: String
    var tool: String
    var server: String
    var startAt: Date?
    var endAt: Date?
    var userPrompts: [String]
    var modelExcerpt: String?

    var promptCount: Int {
        userPrompts.count
    }
}

enum LogText {
    static func clip(_ text: String, keeping count: Int) -> String {
        guard count > 0 else { return "" }
        let chars = Array(text)
        guard chars.count > count * 2 else { return text }
        return String(chars.prefix(count)) + "…" + String(chars.suffix(count))
    }

    static func pathBasename(_ path: String) -> String {
        let trimmed = path.trimmingCharacters(in: CharacterSet(charactersIn: "/\\"))
        let split = trimmed.split(whereSeparator: { $0 == "/" || $0 == "\\" })
        return split.last.map(String.init) ?? path
    }

    static func prefixMatches(_ lhs: String, _ rhs: String, count: Int) -> Bool {
        String(lhs.prefix(count)) == String(rhs.prefix(count))
    }
}

enum JSONLine {
    static func object(_ line: String) -> [String: Any]? {
        guard let data = line.trimmingCharacters(in: .whitespacesAndNewlines).data(using: .utf8),
              !data.isEmpty,
              let value = try? JSONSerialization.jsonObject(with: data),
              let object = value as? [String: Any]
        else {
            return nil
        }
        return object
    }

    static func isValidJSON(_ line: String) -> Bool {
        guard let data = line.trimmingCharacters(in: .whitespacesAndNewlines).data(using: .utf8),
              !data.isEmpty
        else {
            return false
        }
        return (try? JSONSerialization.jsonObject(with: data)) != nil
    }

    static func timestamp(_ value: Any?) -> Date? {
        if let string = value as? String {
            let fractional = ISO8601DateFormatter()
            fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            if let date = fractional.date(from: string) {
                return date
            }

            let formatter = ISO8601DateFormatter()
            formatter.formatOptions = [.withInternetDateTime]
            return formatter.date(from: string)
        }

        if let number = value as? NSNumber {
            let raw = number.doubleValue
            let milliseconds = raw < 1_000_000_000_000 ? raw * 1000 : raw
            return Date(timeIntervalSince1970: milliseconds / 1000)
        }

        return nil
    }
}

enum LogAggregator {
    static let aiSnippetLimit = 10

    static func aggregate(_ messages: [LogMessage], parseStats: ParseStats = ParseStats()) -> WorkSummary {
        let sorted = messages.sorted { lhs, rhs in
            switch (lhs.timestamp, rhs.timestamp) {
            case let (l?, r?) where l != r:
                return l < r
            case (nil, _?):
                return true
            case (_?, nil):
                return false
            default:
                return lhs.project < rhs.project
            }
        }

        var byProject: [String: [String]] = [:]
        var lastTextByProject: [String: String] = [:]
        var servers = Set<String>()
        var tools = Set<String>()
        var activeDays = Set<String>()
        var assistantPool: [(Date?, String)] = []
        var conversationBuckets: [String: ConversationBucket] = [:]
        var lastUserTextByConversation: [String: String] = [:]
        var totalPrompts = 0

        let dayFormatter = DateFormatter()
        dayFormatter.calendar = .current
        dayFormatter.locale = Locale(identifier: "en_US_POSIX")
        dayFormatter.dateFormat = "yyyy-MM-dd"

        for message in sorted {
            servers.insert(message.server)
            tools.insert(message.tool.rawValue)
            if let timestamp = message.timestamp {
                activeDays.insert(dayFormatter.string(from: timestamp))
            }

            switch message.role {
            case .user:
                if let previous = lastTextByProject[message.project],
                   LogText.prefixMatches(previous, message.text, count: 30) {
                    continue
                }
                lastTextByProject[message.project] = message.text
                byProject[message.project, default: []].append(message.text)
                totalPrompts += 1

                let key = conversationKey(for: message)
                if let previous = lastUserTextByConversation[key],
                   LogText.prefixMatches(previous, message.text, count: 30) {
                    break
                }
                lastUserTextByConversation[key] = message.text
                conversationBuckets[key, default: ConversationBucket(message: message)]
                    .appendUser(message)
            case .assistant:
                let trimmed = message.text.trimmingCharacters(in: .whitespacesAndNewlines)
                if !trimmed.isEmpty {
                    assistantPool.append((message.timestamp, trimmed))
                    let key = conversationKey(for: message)
                    conversationBuckets[key, default: ConversationBucket(message: message)]
                        .appendAssistant(message)
                }
            }
        }

        assistantPool.sort { lhs, rhs in
            switch (lhs.0, rhs.0) {
            case let (l?, r?):
                return l > r
            case (_?, nil):
                return true
            case (nil, _?):
                return false
            case (nil, nil):
                return lhs.1 < rhs.1
            }
        }

        let mainProject = byProject.max { lhs, rhs in
            lhs.value.count < rhs.value.count
        }?.key
        let conversations = conversationBuckets.values
            .map { $0.digest() }
            .filter { !$0.userPrompts.isEmpty || ($0.modelExcerpt?.isEmpty == false) }
            .sorted { lhs, rhs in
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

        return WorkSummary(
            byProject: byProject,
            aiSnippets: assistantPool.prefix(aiSnippetLimit).map(\.1),
            conversations: conversations,
            stats: SummaryStats(
                totalPrompts: totalPrompts,
                activeDays: activeDays.count,
                projectCount: byProject.count,
                mainProject: mainProject,
                servers: servers.sorted(),
                tools: tools.sorted(),
                skippedLines: parseStats.skippedLines,
                skippedFiles: parseStats.skippedFiles
            )
        )
    }

    private static func conversationKey(for message: LogMessage) -> String {
        let id = message.conversationID.trimmingCharacters(in: .whitespacesAndNewlines)
        let conversationID = id.isEmpty ? "fallback-\(message.project)" : id
        return "\(message.server)|\(message.tool.rawValue)|\(message.project)|\(conversationID)"
    }
}

private struct ConversationBucket {
    var id: String
    var project: String
    var tool: String
    var server: String
    var startAt: Date?
    var endAt: Date?
    var userPrompts: [String] = []
    var assistantOutputs: [String] = []

    init(message: LogMessage) {
        let rawID = message.conversationID.trimmingCharacters(in: .whitespacesAndNewlines)
        id = rawID.isEmpty ? "\(message.server)-\(message.tool.rawValue)-\(message.project)" : rawID
        project = message.project
        tool = message.tool.rawValue
        server = message.server
        startAt = message.timestamp
        endAt = message.timestamp
    }

    mutating func appendUser(_ message: LogMessage) {
        updateDates(message.timestamp)
        userPrompts.append(message.text)
    }

    mutating func appendAssistant(_ message: LogMessage) {
        updateDates(message.timestamp)
        let trimmed = message.text.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            assistantOutputs.append(trimmed)
        }
    }

    func digest() -> ConversationDigest {
        ConversationDigest(
            id: id,
            project: project,
            tool: tool,
            server: server,
            startAt: startAt,
            endAt: endAt,
            userPrompts: userPrompts,
            modelExcerpt: modelExcerpt()
        )
    }

    private mutating func updateDates(_ timestamp: Date?) {
        guard let timestamp else { return }
        if startAt.map({ timestamp < $0 }) ?? true {
            startAt = timestamp
        }
        if endAt.map({ timestamp > $0 }) ?? true {
            endAt = timestamp
        }
    }

    private func modelExcerpt() -> String? {
        let joined = assistantOutputs.joined(separator: "\n")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !joined.isEmpty else { return nil }
        return LogText.clip(joined, keeping: 200)
    }
}
