import Foundation

struct JSONStore {
    let fileManager: FileManager
    let appDirectoryName: String
    let overrideDataDirectory: URL?

    init(
        fileManager: FileManager = .default,
        appDirectoryName: String = "WeeklyReport",
        dataDirectory: URL? = nil
    ) {
        self.fileManager = fileManager
        self.appDirectoryName = appDirectoryName
        self.overrideDataDirectory = dataDirectory
    }

    var dataDirectory: URL {
        get throws {
            if let overrideDataDirectory {
                return overrideDataDirectory
            }
            let base = try fileManager.url(
                for: .applicationSupportDirectory,
                in: .userDomainMask,
                appropriateFor: nil,
                create: true
            )
            return base.appendingPathComponent(appDirectoryName, isDirectory: true)
        }
    }

    func ensureInitialized() throws {
        let root = try dataDirectory
        try fileManager.createDirectory(at: root, withIntermediateDirectories: true)
        try fileManager.createDirectory(
            at: root.appendingPathComponent("reports", isDirectory: true),
            withIntermediateDirectories: true
        )
    }

    func read<T: Decodable>(_ type: T.Type, from relativePath: String, default defaultValue: T) throws -> T {
        let url = try dataDirectory.appendingPathComponent(relativePath)
        guard fileManager.fileExists(atPath: url.path) else {
            return defaultValue
        }
        let data = try Data(contentsOf: url)
        return try JSONDecoder.weeklyReport.decode(T.self, from: data)
    }

    func write<T: Encodable>(_ value: T, to relativePath: String, secret: Bool = false) throws {
        let url = try dataDirectory.appendingPathComponent(relativePath)
        let parent = url.deletingLastPathComponent()
        try fileManager.createDirectory(at: parent, withIntermediateDirectories: true)

        let data = try JSONEncoder.weeklyReport.encode(value)
        let temporaryURL = url.deletingLastPathComponent()
            .appendingPathComponent(url.lastPathComponent + ".tmp")
        try data.write(to: temporaryURL, options: [.atomic])

        if fileManager.fileExists(atPath: url.path) {
            _ = try fileManager.replaceItemAt(url, withItemAt: temporaryURL)
        } else {
            try fileManager.moveItem(at: temporaryURL, to: url)
        }

        if secret {
            try? fileManager.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        }
    }

    func readReportBody(id: String) throws -> String {
        let url = try dataDirectory
            .appendingPathComponent("reports", isDirectory: true)
            .appendingPathComponent("\(id).md")
        return try String(contentsOf: url, encoding: .utf8)
    }

    func saveReport(record: ReportRecord, content: String) throws -> ReportRecord {
        var saved = record
        if saved.id.isEmpty {
            saved.id = UUID().uuidString
        }

        let reportURL = try dataDirectory
            .appendingPathComponent("reports", isDirectory: true)
            .appendingPathComponent("\(saved.id).md")
        try fileManager.createDirectory(
            at: reportURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try Data(content.utf8).write(to: reportURL, options: [.atomic])

        var index = try read([ReportRecord].self, from: "reports/index.json", default: [])
        if let existing = index.firstIndex(where: { $0.id == saved.id }) {
            index[existing] = saved
        } else {
            index.append(saved)
        }
        try write(index, to: "reports/index.json")
        return saved
    }
}

extension JSONDecoder {
    static let weeklyReport: JSONDecoder = {
        JSONDecoder()
    }()
}

extension JSONEncoder {
    static let weeklyReport: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        return encoder
    }()
}
