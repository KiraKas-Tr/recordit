import Foundation

public enum RemediablePermission: String, Equatable, Sendable {
    case screenRecording = "screen_recording"
    case microphone
}

public enum PermissionReadiness: String, Equatable, Sendable {
    case granted
    case missing
}

public struct PermissionRemediationItem: Equatable, Sendable {
    public var permission: RemediablePermission
    public var status: PermissionReadiness
    public var checkIDs: [String]
    public var detail: String
    public var remediation: String

    public init(
        permission: RemediablePermission,
        status: PermissionReadiness,
        checkIDs: [String],
        detail: String,
        remediation: String
    ) {
        self.permission = permission
        self.status = status
        self.checkIDs = checkIDs
        self.detail = detail
        self.remediation = remediation
    }
}

@MainActor
public final class PermissionRemediationViewModel {
    public enum State: Equatable {
        case idle
        case checking
        case ready([PermissionRemediationItem])
        case failed(AppServiceError)
    }

    public static let screenRecordingRestartAdvisory =
        "You may need to quit and reopen Recordit after changing Screen Recording access."

    public private(set) var state: State = .idle
    public private(set) var shouldShowScreenRecordingRestartAdvisory = false
    public private(set) var lastOpenedSettingsURL: URL?

    private let runner: RecorditPreflightRunner
    private let openSystemSettings: (URL) -> Void

    public init(
        runner: RecorditPreflightRunner = RecorditPreflightRunner(),
        openSystemSettings: @escaping (URL) -> Void = { _ in }
    ) {
        self.runner = runner
        self.openSystemSettings = openSystemSettings
    }

    public var remediationItems: [PermissionRemediationItem] {
        guard case let .ready(items) = state else {
            return []
        }
        return items
    }

    public var missingPermissions: [RemediablePermission] {
        remediationItems
            .filter { $0.status == .missing }
            .map(\.permission)
    }

    public func runPermissionCheck() {
        recheckPermissions()
    }

    public func recheckPermissions() {
        state = .checking
        do {
            let envelope = try runner.runLivePreflight()
            state = .ready(Self.mapPermissionItems(from: envelope))
        } catch let serviceError as AppServiceError {
            state = .failed(serviceError)
        } catch {
            state = .failed(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Permission checks could not complete.",
                    remediation: "Retry the permission check and inspect preflight diagnostics.",
                    debugDetail: String(describing: error)
                )
            )
        }
    }

    @discardableResult
    public func openSettings(for permission: RemediablePermission) -> Bool {
        guard let url = Self.settingsURL(for: permission) else {
            return false
        }
        openSystemSettings(url)
        lastOpenedSettingsURL = url
        if permission == .screenRecording {
            shouldShowScreenRecordingRestartAdvisory = true
        }
        return true
    }

    public func dismissScreenRecordingRestartAdvisory() {
        shouldShowScreenRecordingRestartAdvisory = false
    }

    public static func settingsURL(for permission: RemediablePermission) -> URL? {
        switch permission {
        case .screenRecording:
            return URL(
                string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            )
        case .microphone:
            return URL(
                string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            )
        }
    }

    public static func mapPermissionItems(
        from envelope: PreflightManifestEnvelopeDTO
    ) -> [PermissionRemediationItem] {
        let screenChecks = envelope.checks.filter {
            $0.id == "screen_capture_access" || $0.id == "display_availability"
        }
        let microphoneChecks = envelope.checks.filter { $0.id == "microphone_access" }

        return [
            buildItem(
                permission: .screenRecording,
                checks: screenChecks,
                defaultDetail: "Screen Recording access is required to capture system audio.",
                defaultRemediation: "Open System Settings, grant Screen Recording access, then Re-check."
            ),
            buildItem(
                permission: .microphone,
                checks: microphoneChecks,
                defaultDetail: "Microphone access is required to capture your voice.",
                defaultRemediation: "Open System Settings, grant Microphone access, then Re-check."
            ),
        ]
    }

    private static func buildItem(
        permission: RemediablePermission,
        checks: [PreflightCheckDTO],
        defaultDetail: String,
        defaultRemediation: String
    ) -> PermissionRemediationItem {
        let checkIDs = checks.map(\.id)
        guard !checks.isEmpty else {
            return PermissionRemediationItem(
                permission: permission,
                status: .missing,
                checkIDs: [],
                detail: defaultDetail,
                remediation: "Run preflight again and verify permission diagnostics are present."
            )
        }

        if let failing = checks.first(where: { $0.status == .fail }) {
            return PermissionRemediationItem(
                permission: permission,
                status: .missing,
                checkIDs: checkIDs,
                detail: failing.detail,
                remediation: failing.remediation ?? defaultRemediation
            )
        }

        let representative = checks[0]
        return PermissionRemediationItem(
            permission: permission,
            status: .granted,
            checkIDs: checkIDs,
            detail: representative.detail,
            remediation: representative.remediation ?? defaultRemediation
        )
    }
}
