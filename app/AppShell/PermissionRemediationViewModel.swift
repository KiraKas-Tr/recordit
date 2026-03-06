import Foundation
import AVFoundation
import CoreGraphics

private func defaultNativePermissionStatus(_ permission: RemediablePermission) -> Bool {
    // Keep UI automation deterministic by honoring fixture-only outcomes.
    if ProcessInfo.processInfo.environment["RECORDIT_UI_TEST_MODE"] == "1" {
        return false
    }

    switch permission {
    case .screenRecording:
        return CGPreflightScreenCaptureAccess()
    case .microphone:
        return AVCaptureDevice.authorizationStatus(for: .audio) == .authorized
    }
}

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
    private let nativePermissionStatus: (RemediablePermission) -> Bool

    public init(
        runner: RecorditPreflightRunner = RecorditPreflightRunner(),
        openSystemSettings: @escaping (URL) -> Void = { _ in },
        nativePermissionStatus: ((RemediablePermission) -> Bool)? = nil
    ) {
        self.runner = runner
        self.openSystemSettings = openSystemSettings
        self.nativePermissionStatus = nativePermissionStatus ?? defaultNativePermissionStatus
    }

    public var remediationItems: [PermissionRemediationItem] {
        guard case let .ready(items) = state else {
            return []
        }
        return items
    }

    public var missingPermissions: [RemediablePermission] {
        switch state {
        case .ready(let items):
            return items
                .filter { $0.status == .missing }
                .map(\.permission)
        case .failed:
            // Fail-open for remediation affordances so onboarding never strands users
            // without the direct privacy deep-links.
            return [.screenRecording, .microphone]
        case .idle, .checking:
            return []
        }
    }

    public func runPermissionCheck() {
        recheckPermissions()
    }

    public func recheckPermissions() {
        state = .checking
        do {
            let envelope = try runner.runLivePreflight()
            state = .ready(
                Self.mapPermissionItems(
                    from: envelope,
                    nativePermissionStatus: nativePermissionStatus
                )
            )
        } catch let serviceError as AppServiceError {
            state = .ready(
                Self.nativePermissionFallbackItems(
                    nativePermissionStatus: nativePermissionStatus,
                    preflightFailure: serviceError
                )
            )
        } catch {
            let serviceError = AppServiceError(
                code: .unknown,
                userMessage: "Permission checks could not complete.",
                remediation: "Retry the permission check and inspect preflight diagnostics.",
                debugDetail: String(describing: error)
            )
            state = .ready(
                Self.nativePermissionFallbackItems(
                    nativePermissionStatus: nativePermissionStatus,
                    preflightFailure: serviceError
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
        from envelope: PreflightManifestEnvelopeDTO,
        nativePermissionStatus: ((RemediablePermission) -> Bool)? = nil
    ) -> [PermissionRemediationItem] {
        let resolvedNativePermissionStatus = nativePermissionStatus ?? defaultNativePermissionStatus
        let screenChecks = envelope.checks.filter {
            $0.id == "screen_capture_access" || $0.id == "display_availability"
        }
        let microphoneChecks = envelope.checks.filter { $0.id == "microphone_access" }

        return [
            buildItem(
                permission: .screenRecording,
                checks: screenChecks,
                nativePermissionStatus: resolvedNativePermissionStatus,
                defaultDetail: "Screen Recording access is required to capture system audio.",
                defaultRemediation: "Open System Settings, grant Screen Recording access, then Re-check."
            ),
            buildItem(
                permission: .microphone,
                checks: microphoneChecks,
                nativePermissionStatus: resolvedNativePermissionStatus,
                defaultDetail: "Microphone access is required to capture your voice.",
                defaultRemediation: "Open System Settings, grant Microphone access, then Re-check."
            ),
        ]
    }

    private static func buildItem(
        permission: RemediablePermission,
        checks: [PreflightCheckDTO],
        nativePermissionStatus: (RemediablePermission) -> Bool,
        defaultDetail: String,
        defaultRemediation: String
    ) -> PermissionRemediationItem {
        let allowNativeOverride = ProcessInfo.processInfo.environment["RECORDIT_UI_TEST_MODE"] == "1"
        let nativePermissionGranted = nativePermissionStatus(permission)
        let checkIDs = checks.map(\.id)
        guard !checks.isEmpty else {
            return PermissionRemediationItem(
                permission: permission,
                status: nativePermissionGranted ? .granted : .missing,
                checkIDs: [],
                detail: nativePermissionGranted
                    ? "macOS permission is granted. Run preflight again to refresh diagnostics."
                    : defaultDetail,
                remediation: "Run preflight again and verify permission diagnostics are present."
            )
        }

        if let failing = checks.first(where: { $0.status == .fail }) {
            if nativePermissionGranted, allowNativeOverride {
                return PermissionRemediationItem(
                    permission: permission,
                    status: .granted,
                    checkIDs: checkIDs,
                    detail: "macOS permission is granted. Runtime preflight reported: \(failing.detail)",
                    remediation: failing.remediation ?? defaultRemediation
                )
            }
            return PermissionRemediationItem(
                permission: permission,
                status: .missing,
                checkIDs: checkIDs,
                detail: nativePermissionGranted
                    ? "macOS permission appears granted, but runtime checks still fail: \(failing.detail)"
                    : failing.detail,
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

    private static func nativePermissionFallbackItems(
        nativePermissionStatus: (RemediablePermission) -> Bool,
        preflightFailure: AppServiceError
    ) -> [PermissionRemediationItem] {
        [
            fallbackItem(
                permission: .screenRecording,
                nativePermissionStatus: nativePermissionStatus,
                defaultDetail: "Screen Recording access is required to capture system audio.",
                defaultRemediation: "Open System Settings, grant Screen Recording access, then Re-check.",
                failureDetail: preflightFailure.userMessage
            ),
            fallbackItem(
                permission: .microphone,
                nativePermissionStatus: nativePermissionStatus,
                defaultDetail: "Microphone access is required to capture your voice.",
                defaultRemediation: "Open System Settings, grant Microphone access, then Re-check.",
                failureDetail: preflightFailure.remediation
            ),
        ]
    }

    private static func fallbackItem(
        permission: RemediablePermission,
        nativePermissionStatus: (RemediablePermission) -> Bool,
        defaultDetail: String,
        defaultRemediation: String,
        failureDetail: String
    ) -> PermissionRemediationItem {
        let nativeGranted = nativePermissionStatus(permission)
        if nativeGranted {
            return PermissionRemediationItem(
                permission: permission,
                status: .granted,
                checkIDs: [],
                detail: "macOS permission is granted. Preflight diagnostics unavailable: \(failureDetail)",
                remediation: "You can proceed; rerun checks after runtime diagnostics recover."
            )
        }

        return PermissionRemediationItem(
            permission: permission,
            status: .missing,
            checkIDs: [],
            detail: "\(defaultDetail) Preflight diagnostics unavailable: \(failureDetail)",
            remediation: defaultRemediation
        )
    }
}
