import Foundation

public enum PreflightCheckPolicy: String, Equatable, Sendable {
    case blockOnFail
    case warnRequiresAcknowledgement
    case informational
}

public struct MappedPreflightCheck: Equatable, Sendable {
    public var check: PreflightCheckDTO
    public var policy: PreflightCheckPolicy
    public var isKnownContractID: Bool

    public init(check: PreflightCheckDTO, policy: PreflightCheckPolicy, isKnownContractID: Bool) {
        self.check = check
        self.policy = policy
        self.isKnownContractID = isKnownContractID
    }
}

public struct PreflightGatingEvaluation: Equatable, Sendable {
    public var mappedChecks: [MappedPreflightCheck]
    public var blockingFailures: [MappedPreflightCheck]
    public var warningContinuations: [MappedPreflightCheck]
    public var unknownCheckIDs: [String]

    public init(
        mappedChecks: [MappedPreflightCheck],
        blockingFailures: [MappedPreflightCheck],
        warningContinuations: [MappedPreflightCheck],
        unknownCheckIDs: [String]
    ) {
        self.mappedChecks = mappedChecks
        self.blockingFailures = blockingFailures
        self.warningContinuations = warningContinuations
        self.unknownCheckIDs = unknownCheckIDs
    }

    public var requiresWarningAcknowledgement: Bool {
        !warningContinuations.isEmpty
    }

    public func canProceed(acknowledgingWarnings: Bool) -> Bool {
        guard blockingFailures.isEmpty else {
            return false
        }
        guard warningContinuations.isEmpty else {
            return acknowledgingWarnings
        }
        return true
    }
}

public struct PreflightGatingPolicy {
    public static let blockingFailureCheckIDs: Set<String> = [
        "model_path",
        "out_wav",
        "out_jsonl",
        "out_manifest",
        "screen_capture_access",
        "display_availability",
        "microphone_access",
    ]

    public static let warnAcknowledgementCheckIDs: Set<String> = [
        "sample_rate",
        "backend_runtime",
    ]

    public static let knownContractCheckIDs: Set<String> =
        blockingFailureCheckIDs.union(warnAcknowledgementCheckIDs)

    public init() {}

    public static func policy(forCheckID checkID: String) -> PreflightCheckPolicy {
        if blockingFailureCheckIDs.contains(checkID) {
            return .blockOnFail
        }
        if warnAcknowledgementCheckIDs.contains(checkID) {
            return .warnRequiresAcknowledgement
        }
        return .informational
    }

    public func evaluate(_ envelope: PreflightManifestEnvelopeDTO) -> PreflightGatingEvaluation {
        var mappedChecks = [MappedPreflightCheck]()
        var blockingFailures = [MappedPreflightCheck]()
        var warningContinuations = [MappedPreflightCheck]()
        var unknownCheckIDs = [String]()

        for check in envelope.checks {
            let policy = Self.policy(forCheckID: check.id)
            let isKnownContractID = Self.knownContractCheckIDs.contains(check.id)
            let mapped = MappedPreflightCheck(
                check: check,
                policy: policy,
                isKnownContractID: isKnownContractID
            )
            mappedChecks.append(mapped)

            if !isKnownContractID {
                unknownCheckIDs.append(check.id)
            }

            switch policy {
            case .blockOnFail:
                if check.status == .fail {
                    blockingFailures.append(mapped)
                }
            case .warnRequiresAcknowledgement:
                if check.status != .pass {
                    warningContinuations.append(mapped)
                }
            case .informational:
                break
            }
        }

        return PreflightGatingEvaluation(
            mappedChecks: mappedChecks,
            blockingFailures: blockingFailures,
            warningContinuations: warningContinuations,
            unknownCheckIDs: unknownCheckIDs
        )
    }
}
