import Foundation

@MainActor
public final class ModelSetupViewModel {
    public enum ModelPathKind: String, Equatable, Sendable {
        case file
        case directory
    }

    public struct BackendOption: Equatable, Sendable {
        public var id: String
        public var displayName: String
        public var isSelectable: Bool
        public var requiredModelPathKind: ModelPathKind?
        public var remediation: String

        public init(
            id: String,
            displayName: String,
            isSelectable: Bool,
            requiredModelPathKind: ModelPathKind?,
            remediation: String
        ) {
            self.id = id
            self.displayName = displayName
            self.isSelectable = isSelectable
            self.requiredModelPathKind = requiredModelPathKind
            self.remediation = remediation
        }
    }

    public enum State: Equatable {
        case idle
        case validating
        case ready(ResolvedModelDTO)
        case invalid(AppServiceError)
    }

    public struct ModelDiagnostics: Equatable, Sendable {
        public var asrModel: String
        public var asrModelSource: String
        public var asrModelChecksumSHA256: String?
        public var asrModelChecksumStatus: String

        public init(
            asrModel: String,
            asrModelSource: String,
            asrModelChecksumSHA256: String?,
            asrModelChecksumStatus: String
        ) {
            self.asrModel = asrModel
            self.asrModelSource = asrModelSource
            self.asrModelChecksumSHA256 = asrModelChecksumSHA256
            self.asrModelChecksumStatus = asrModelChecksumStatus
        }
    }

    public static let backendCapabilityMatrix: [BackendOption] = [
        BackendOption(
            id: "whispercpp",
            displayName: "Whisper.cpp",
            isSelectable: true,
            requiredModelPathKind: .file,
            remediation: "Use a model file path such as ggml-tiny.en.bin."
        ),
        BackendOption(
            id: "whisperkit",
            displayName: "WhisperKit",
            isSelectable: true,
            requiredModelPathKind: .directory,
            remediation: "Use a model folder path for WhisperKit."
        ),
        BackendOption(
            id: "moonshine",
            displayName: "Moonshine",
            isSelectable: false,
            requiredModelPathKind: .directory,
            remediation: "Moonshine is not available in this app setup yet. Choose Whisper.cpp or WhisperKit."
        ),
    ]

    public static let selectableBackends: [String] = backendCapabilityMatrix
        .filter(\.isSelectable)
        .map(\.id)

    public static let onboardingAccessibilityElements: [AccessibilityElementDescriptor] = [
        AccessibilityElementDescriptor(
            id: "backend_picker",
            label: "Transcription backend",
            hint: "Choose a supported backend before validating your model path."
        ),
        AccessibilityElementDescriptor(
            id: "model_path_picker",
            label: "Model path",
            hint: "Select a local model file or folder that matches the backend requirements."
        ),
        AccessibilityElementDescriptor(
            id: "validate_selection",
            label: "Validate model setup",
            hint: "Runs model path and backend compatibility checks."
        ),
        AccessibilityElementDescriptor(
            id: "model_diagnostics",
            label: "Model diagnostics",
            hint: "Shows resolved model source and checksum details after validation."
        ),
    ]

    public static let onboardingFocusPlan = KeyboardFocusPlan(
        orderedElementIDs: ["backend_picker", "model_path_picker", "validate_selection", "model_diagnostics"]
    )

    public static let onboardingKeyboardShortcuts: [KeyboardShortcutDescriptor] = [
        KeyboardShortcutDescriptor(
            id: "validate_model",
            key: "return",
            modifiers: ["command"],
            actionSummary: "Validate selected backend and model path."
        ),
    ]

    public private(set) var state: State = .idle
    public private(set) var selectedBackend = "whispercpp"
    public private(set) var selectedModelPath: URL?

    private let modelResolutionService: ModelResolutionService

    public init(modelResolutionService: ModelResolutionService) {
        self.modelResolutionService = modelResolutionService
    }

    public var canStartLiveTranscribe: Bool {
        if case .ready = state {
            return true
        }
        return false
    }

    public var diagnostics: ModelDiagnostics? {
        guard case let .ready(resolved) = state else {
            return nil
        }
        return ModelDiagnostics(
            asrModel: resolved.resolvedPath.path,
            asrModelSource: resolved.source,
            asrModelChecksumSHA256: resolved.checksumSHA256,
            asrModelChecksumStatus: resolved.checksumStatus
        )
    }

    public func chooseBackend(_ backend: String) {
        let normalizedBackend = backend.lowercased()
        guard let option = Self.backendOption(for: normalizedBackend) else {
            state = .invalid(
                AppServiceError(
                    code: .invalidInput,
                    userMessage: "This transcription backend is not supported in setup.",
                    remediation: "Choose Whisper.cpp or WhisperKit.",
                    debugDetail: "backend=\(backend)"
                )
            )
            return
        }

        guard option.isSelectable else {
            state = .invalid(
                AppServiceError(
                    code: .invalidInput,
                    userMessage: "\(option.displayName) is not available in setup yet.",
                    remediation: option.remediation,
                    debugDetail: "backend=\(backend)"
                )
            )
            return
        }

        selectedBackend = normalizedBackend
        validateCurrentSelection()
    }

    public func chooseExistingModelPath(_ path: URL?) {
        selectedModelPath = path?.standardizedFileURL
        validateCurrentSelection()
    }

    public func validateCurrentSelection() {
        state = .validating
        do {
            let backendOption = try validateBackendSupported(selectedBackend)
            try validateSelectedPathKindIfPresent(for: backendOption)
            let resolved = try modelResolutionService.resolveModel(
                ModelResolutionRequest(
                    explicitModelPath: selectedModelPath,
                    backend: selectedBackend
                )
            )
            state = .ready(resolved)
        } catch let serviceError as AppServiceError {
            state = .invalid(serviceError)
        } catch {
            state = .invalid(
                AppServiceError(
                    code: .unknown,
                    userMessage: "Model setup could not be validated.",
                    remediation: "Choose a valid local model path and retry validation.",
                    debugDetail: String(describing: error)
                )
            )
        }
    }

    public var backendOptions: [BackendOption] {
        Self.backendCapabilityMatrix
    }

    private static func backendOption(for backend: String) -> BackendOption? {
        Self.backendCapabilityMatrix.first(where: { $0.id == backend.lowercased() })
    }

    private func validateBackendSupported(_ backend: String) throws -> BackendOption {
        guard let option = Self.backendOption(for: backend) else {
            throw AppServiceError(
                code: .invalidInput,
                userMessage: "This transcription backend is not supported in setup.",
                remediation: "Choose Whisper.cpp or WhisperKit.",
                debugDetail: "backend=\(backend)"
            )
        }

        guard option.isSelectable else {
            throw AppServiceError(
                code: .invalidInput,
                userMessage: "\(option.displayName) is not available in setup yet.",
                remediation: option.remediation,
                debugDetail: "backend=\(backend)"
            )
        }

        return option
    }

    private func validateSelectedPathKindIfPresent(for option: BackendOption) throws {
        guard let requiredKind = option.requiredModelPathKind else {
            return
        }
        guard let selectedModelPath else {
            return
        }

        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: selectedModelPath.path, isDirectory: &isDirectory) else {
            return
        }

        switch requiredKind {
        case .file where isDirectory.boolValue:
            throw AppServiceError(
                code: .invalidInput,
                userMessage: "\(option.displayName) needs a model file, not a folder.",
                remediation: "Pick a model file path for \(option.displayName)."
            )
        case .directory where !isDirectory.boolValue:
            throw AppServiceError(
                code: .invalidInput,
                userMessage: "\(option.displayName) needs a model folder, not a file.",
                remediation: "Pick a model folder path for \(option.displayName)."
            )
        default:
            break
        }
    }
}
