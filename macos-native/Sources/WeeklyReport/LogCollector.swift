import Foundation

struct LogCollector {
    var fileManager: FileManager = .default

    func collectMessages(
        workspace: Workspace,
        days: Int,
        clipChars: Int
    ) throws -> (messages: [LogMessage], stats: ParseStats) {
        guard workspace.type == .local else {
            throw NSError(
                domain: "WeeklyReport.LogCollector",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Swift 原生版本暂未接入 SSH 同步"]
            )
        }

        let since = Date().addingTimeInterval(TimeInterval(-max(days, 0) * 24 * 60 * 60))
        var messages: [LogMessage] = []
        var stats = ParseStats()

        if workspace.tools.contains("claude-code") {
            let root = URL(fileURLWithPath: expandTilde(workspace.claudePath ?? "~/.claude"))
            let result = collectClaude(root: root, server: workspace.name, since: since, clipChars: clipChars)
            messages.append(contentsOf: result.messages)
            stats.merge(result.stats)
        }

        if workspace.tools.contains("codex") {
            let root = URL(fileURLWithPath: expandTilde(workspace.codexPath ?? "~/.codex"))
            let result = collectCodex(root: root, server: workspace.name, since: since, clipChars: clipChars)
            messages.append(contentsOf: result.messages)
            stats.merge(result.stats)
        }

        return (messages, stats)
    }

    private func collectClaude(
        root: URL,
        server: String,
        since: Date,
        clipChars: Int
    ) -> (messages: [LogMessage], stats: ParseStats) {
        var output: [LogMessage] = []
        var stats = ParseStats()

        let history = root.appendingPathComponent("history.jsonl")
        if isFile(history), modifiedAfter(history, since: since) {
            let parsed = parseClaudeHistoryFile(history, server: server, since: since)
            output.append(contentsOf: parsed.messages)
            stats.merge(parsed.stats)
        }

        let projects = root.appendingPathComponent("projects", isDirectory: true)
        for file in jsonlFiles(under: projects) {
            guard modifiedAfter(file, since: since) else { continue }
            let parsed = parseClaudeSessionFile(file, server: server, since: since, clipChars: clipChars)
            output.append(contentsOf: parsed.messages)
            stats.merge(parsed.stats)
        }

        return (output, stats)
    }

    private func collectCodex(
        root: URL,
        server: String,
        since: Date,
        clipChars: Int
    ) -> (messages: [LogMessage], stats: ParseStats) {
        let sessions = root.appendingPathComponent("sessions", isDirectory: true)
        var output: [LogMessage] = []
        var stats = ParseStats()

        for file in jsonlFiles(under: sessions) {
            guard file.lastPathComponent.hasPrefix("rollout-"),
                  modifiedAfter(file, since: since)
            else {
                continue
            }
            let parsed = parseCodexRolloutFile(file, server: server, since: since, clipChars: clipChars)
            output.append(contentsOf: parsed.messages)
            stats.merge(parsed.stats)
        }

        return (output, stats)
    }

    private func parseClaudeHistoryFile(
        _ file: URL,
        server: String,
        since: Date
    ) -> (messages: [LogMessage], stats: ParseStats) {
        do {
            let contents = try String(contentsOf: file, encoding: .utf8)
            var stats = ParseStats()
            let messages = contents.lines.compactMap { line -> LogMessage? in
                let text = String(line)
                if let message = ClaudeLogParser.parseHistoryLine(text, server: server) {
                    if message.timestamp.map({ $0 >= since }) ?? true {
                        return message
                    }
                    return nil
                }
                if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                   !JSONLine.isValidJSON(text) {
                    stats.skippedLines += 1
                }
                return nil
            }
            return (messages, stats)
        } catch {
            return ([], ParseStats(skippedFiles: 1))
        }
    }

    private func parseClaudeSessionFile(
        _ file: URL,
        server: String,
        since: Date,
        clipChars: Int
    ) -> (messages: [LogMessage], stats: ParseStats) {
        do {
            let contents = try String(contentsOf: file, encoding: .utf8)
            let project = ClaudeLogParser.inferSessionProject(
                from: contents,
                fallbackName: file.deletingPathExtension().lastPathComponent
            )
            let conversationID = file.deletingPathExtension().lastPathComponent
            var stats = ParseStats()
            let messages = contents.lines.flatMap { line -> [LogMessage] in
                let text = String(line)
                guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return [] }
                guard JSONLine.isValidJSON(text) else {
                    stats.skippedLines += 1
                    return []
                }
                return ClaudeLogParser.parseSessionLine(
                    text,
                    server: server,
                    project: project,
                    clipChars: clipChars,
                    conversationID: conversationID
                )
                    .filter { $0.timestamp.map { $0 >= since } ?? true }
            }
            return (messages, stats)
        } catch {
            return ([], ParseStats(skippedFiles: 1))
        }
    }

    private func parseCodexRolloutFile(
        _ file: URL,
        server: String,
        since: Date,
        clipChars: Int
    ) -> (messages: [LogMessage], stats: ParseStats) {
        do {
            let contents = try String(contentsOf: file, encoding: .utf8)
            let project = CodexLogParser.inferRolloutProject(
                from: contents,
                fallbackName: file.deletingPathExtension().lastPathComponent
            )
            let conversationID = file.deletingPathExtension().lastPathComponent
            var stats = ParseStats()
            let messages = contents.lines.flatMap { line -> [LogMessage] in
                let text = String(line)
                guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return [] }
                guard JSONLine.isValidJSON(text) else {
                    stats.skippedLines += 1
                    return []
                }
                return CodexLogParser.parseRolloutLine(
                    text,
                    server: server,
                    project: project,
                    clipChars: clipChars,
                    conversationID: conversationID
                )
                    .filter { $0.timestamp.map { $0 >= since } ?? true }
            }
            return (messages, stats)
        } catch {
            return ([], ParseStats(skippedFiles: 1))
        }
    }

    private func jsonlFiles(under root: URL) -> [URL] {
        guard let enumerator = fileManager.enumerator(
            at: root,
            includingPropertiesForKeys: [.isRegularFileKey, .contentModificationDateKey],
            options: [.skipsHiddenFiles]
        ) else {
            return []
        }

        return enumerator.compactMap { item in
            guard let url = item as? URL,
                  url.pathExtension == "jsonl",
                  isFile(url)
            else {
                return nil
            }
            return url
        }
    }

    private func isFile(_ url: URL) -> Bool {
        (try? url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true
    }

    private func modifiedAfter(_ url: URL, since: Date) -> Bool {
        guard let date = try? url.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate else {
            return false
        }
        return date >= since
    }

    private func expandTilde(_ path: String) -> String {
        if path == "~" {
            return fileManager.homeDirectoryForCurrentUser.path
        }
        if path.hasPrefix("~/") {
            return fileManager.homeDirectoryForCurrentUser
                .appendingPathComponent(String(path.dropFirst(2)))
                .path
        }
        return path
    }
}

private extension String {
    var lines: [Substring] {
        split(whereSeparator: \.isNewline)
    }
}
