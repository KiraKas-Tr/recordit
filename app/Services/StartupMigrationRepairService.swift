import Foundation

public struct StartupMigrationRepairReport: Equatable, Sendable {
    public var indexPath: URL
    public var startedAt: Date
    public var completedAt: Date
    public var timeBudgetSeconds: TimeInterval
    public var didExceedTimeBudget: Bool
    public var sessionCountScanned: Int
    public var staleIndexEntryCount: Int
    public var missingIndexEntryCount: Int
    public var legacyImportCount: Int
    public var truncatedSessionCount: Int
    public var queryableAfterRepair: Bool
    public var failureMessages: [String]

    public init(
        indexPath: URL,
        startedAt: Date,
        completedAt: Date,
        timeBudgetSeconds: TimeInterval,
        didExceedTimeBudget: Bool,
        sessionCountScanned: Int,
        staleIndexEntryCount: Int,
        missingIndexEntryCount: Int,
        legacyImportCount: Int,
        truncatedSessionCount: Int,
        queryableAfterRepair: Bool,
        failureMessages: [String]
    ) {
        self.indexPath = indexPath
        self.startedAt = startedAt
        self.completedAt = completedAt
        self.timeBudgetSeconds = timeBudgetSeconds
        self.didExceedTimeBudget = didExceedTimeBudget
        self.sessionCountScanned = sessionCountScanned
        self.staleIndexEntryCount = staleIndexEntryCount
        self.missingIndexEntryCount = missingIndexEntryCount
        self.legacyImportCount = legacyImportCount
        self.truncatedSessionCount = truncatedSessionCount
        self.queryableAfterRepair = queryableAfterRepair
        self.failureMessages = failureMessages
    }
}

public protocol StartupMigrationRepairing {
    func runRepair() -> StartupMigrationRepairReport
}

public struct StartupMigrationRepairService: StartupMigrationRepairing {
    public typealias SessionsRootProvider = @Sendable () throws -> URL
    public typealias Clock = @Sendable () -> Date
    public typealias Logger = @Sendable (String) -> Void

    private struct PersistedIndexFile: Codable {
        var schemaVersion: String
        var generatedAtUTC: String
        var entries: [PersistedIndexEntry]
    }

    private struct PersistedIndexEntry: Codable {
        var sessionID: String
        var rootPath: String
        var status: String
        var mode: String
        var ingestSource: String
        var startedAtUTC: String
    }

    private static let indexDirectoryName = ".recordit"
    private static let indexFileName = "session-library-index.json"

    private let sessionLibraryService: any SessionLibraryService
    private let sessionsRootProvider: SessionsRootProvider
    private let timeBudgetSeconds: TimeInterval
    private let maxPersistedEntries: Int
    private let clock: Clock
    private let logger: Logger

    public init(
        sessionLibraryService: any SessionLibraryService,
        sessionsRootProvider: @escaping SessionsRootProvider = {
            try StartupMigrationRepairService.defaultSessionsRoot()
        },
        timeBudgetSeconds: TimeInterval = 1.0,
        maxPersistedEntries: Int = 2_000,
        clock: @escaping Clock = { Date() },
        logger: @escaping Logger = { message in
            guard let data = "\(message)\n".data(using: .utf8) else {
                return
            }
            FileHandle.standardError.write(data)
        }
    ) {
        self.sessionLibraryService = sessionLibraryService
        self.sessionsRootProvider = sessionsRootProvider
        self.timeBudgetSeconds = max(0.1, timeBudgetSeconds)
        self.maxPersistedEntries = max(1, maxPersistedEntries)
        self.clock = clock
        self.logger = logger
    }

    public func runRepair() -> StartupMigrationRepairReport {
        let startedAt = clock()
        let deadline = startedAt.addingTimeInterval(timeBudgetSeconds)
        var failures = [String]()

        let sessionsRootURL: URL
        do {
            sessionsRootURL = try sessionsRootProvider().standardizedFileURL
        } catch {
            let fallbackIndex = FileManager.default.temporaryDirectory
                .appendingPathComponent(Self.indexFileName)
            let message = "resolve sessions root failed: \(String(describing: error))"
            failures.append(message)
            logger("[startup-repair] \(message)")
            return StartupMigrationRepairReport(
                indexPath: fallbackIndex,
                startedAt: startedAt,
                completedAt: clock(),
                timeBudgetSeconds: timeBudgetSeconds,
                didExceedTimeBudget: false,
                sessionCountScanned: 0,
                staleIndexEntryCount: 0,
                missingIndexEntryCount: 0,
                legacyImportCount: 0,
                truncatedSessionCount: 0,
                queryableAfterRepair: false,
                failureMessages: failures
            )
        }

        let indexURL = Self.indexURL(for: sessionsRootURL)
        let existingEntries = loadPersistedEntries(at: indexURL, failures: &failures)

        var didExceedTimeBudget = isPastDeadline(deadline)
        var scannedSessions = [SessionSummaryDTO]()
        if !didExceedTimeBudget {
            do {
                scannedSessions = try sessionLibraryService.listSessions(query: SessionQuery())
            } catch {
                failures.append("session scan failed: \(String(describing: error))")
                logger("[startup-repair] session scan failed: \(String(describing: error))")
            }
        }
        didExceedTimeBudget = didExceedTimeBudget || isPastDeadline(deadline)

        let persistedEntries = scannedSessions.prefix(maxPersistedEntries).map {
            Self.persistedEntry(from: $0)
        }
        let truncatedCount = max(0, scannedSessions.count - persistedEntries.count)

        let existingPaths = Set(existingEntries.map(\.rootPath))
        let repairedPaths = Set(persistedEntries.map(\.rootPath))
        let staleCount = existingPaths.subtracting(repairedPaths).count
        let missingCount = repairedPaths.subtracting(existingPaths).count
        let legacyImportCount = persistedEntries.filter {
            $0.ingestSource == SessionIngestSource.legacyFlatImport.rawValue
        }.count

        if !didExceedTimeBudget {
            do {
                try writePersistedEntries(
                    persistedEntries,
                    to: indexURL,
                    generatedAt: clock()
                )
            } catch {
                failures.append("index write failed: \(String(describing: error))")
                logger("[startup-repair] index write failed: \(String(describing: error))")
            }
        } else {
            failures.append("time budget exceeded before index write")
            logger("[startup-repair] time budget exceeded before index write")
        }

        let queryableAfterRepair: Bool
        do {
            _ = try sessionLibraryService.listSessions(query: SessionQuery())
            queryableAfterRepair = true
        } catch {
            queryableAfterRepair = false
            failures.append("post-repair query failed: \(String(describing: error))")
            logger("[startup-repair] post-repair query failed: \(String(describing: error))")
        }

        let report = StartupMigrationRepairReport(
            indexPath: indexURL,
            startedAt: startedAt,
            completedAt: clock(),
            timeBudgetSeconds: timeBudgetSeconds,
            didExceedTimeBudget: didExceedTimeBudget,
            sessionCountScanned: scannedSessions.count,
            staleIndexEntryCount: staleCount,
            missingIndexEntryCount: missingCount,
            legacyImportCount: legacyImportCount,
            truncatedSessionCount: truncatedCount,
            queryableAfterRepair: queryableAfterRepair,
            failureMessages: failures
        )

        logger(
            "[startup-repair] root=\(sessionsRootURL.path) scanned=\(report.sessionCountScanned) " +
            "stale=\(report.staleIndexEntryCount) missing=\(report.missingIndexEntryCount) " +
            "legacy=\(report.legacyImportCount) failures=\(report.failureMessages.count)"
        )
        return report
    }

    private static func indexURL(for sessionsRoot: URL) -> URL {
        sessionsRoot
            .appendingPathComponent(indexDirectoryName, isDirectory: true)
            .appendingPathComponent(indexFileName, isDirectory: false)
            .standardizedFileURL
    }

    public static func defaultSessionsRoot() throws -> URL {
        let env = ProcessInfo.processInfo.environment
        let dataRootURL: URL
        if let override = env["RECORDIT_CONTAINER_DATA_ROOT"]?.trimmingCharacters(in: .whitespacesAndNewlines),
           !override.isEmpty {
            let overrideURL = URL(fileURLWithPath: override)
            guard overrideURL.path.hasPrefix("/") else {
                throw AppServiceError(
                    code: .invalidInput,
                    userMessage: "Storage root override is invalid.",
                    remediation: "Set RECORDIT_CONTAINER_DATA_ROOT to an absolute path."
                )
            }
            dataRootURL = overrideURL
        } else {
            guard let home = env["HOME"], !home.isEmpty else {
                throw AppServiceError(
                    code: .ioFailure,
                    userMessage: "Could not resolve the app storage root.",
                    remediation: "Ensure HOME is available, then retry."
                )
            }
            dataRootURL = URL(fileURLWithPath: home)
                .appendingPathComponent("Library", isDirectory: true)
                .appendingPathComponent("Containers", isDirectory: true)
                .appendingPathComponent("com.recordit.sequoiatranscribe", isDirectory: true)
                .appendingPathComponent("Data", isDirectory: true)
        }

        return dataRootURL
            .appendingPathComponent("artifacts", isDirectory: true)
            .appendingPathComponent("packaged-beta", isDirectory: true)
            .appendingPathComponent("sessions", isDirectory: true)
    }

    private func loadPersistedEntries(
        at indexURL: URL,
        failures: inout [String]
    ) -> [PersistedIndexEntry] {
        let fileManager = FileManager.default
        guard fileManager.fileExists(atPath: indexURL.path) else {
            return []
        }
        do {
            let handle = try FileHandle(forReadingFrom: indexURL)
            defer { try? handle.close() }
            let data = try handle.readToEnd() ?? Data()
            let payload = try JSONDecoder().decode(PersistedIndexFile.self, from: data)
            return payload.entries
        } catch {
            failures.append("existing index decode failed: \(String(describing: error))")
            logger("[startup-repair] existing index decode failed: \(String(describing: error))")
            return []
        }
    }

    private func writePersistedEntries(
        _ entries: [PersistedIndexEntry],
        to indexURL: URL,
        generatedAt: Date
    ) throws {
        let fileManager = FileManager.default
        let parentURL = indexURL.deletingLastPathComponent()
        try fileManager.createDirectory(
            at: parentURL,
            withIntermediateDirectories: true,
            attributes: nil
        )

        let payload = PersistedIndexFile(
            schemaVersion: "1",
            generatedAtUTC: Self.iso8601.string(from: generatedAt),
            entries: entries
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let data = try encoder.encode(payload)
        try data.write(to: indexURL, options: .atomic)
    }

    private func isPastDeadline(_ deadline: Date) -> Bool {
        clock() >= deadline
    }

    private static func persistedEntry(from session: SessionSummaryDTO) -> PersistedIndexEntry {
        PersistedIndexEntry(
            sessionID: session.sessionID,
            rootPath: session.rootPath.standardizedFileURL.path,
            status: session.status.rawValue,
            mode: session.mode.rawValue,
            ingestSource: session.ingestSource.rawValue,
            startedAtUTC: iso8601.string(from: session.startedAt)
        )
    }

    private static var iso8601: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        return formatter
    }()
}
