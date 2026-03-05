import Foundation

public struct AccessibilityElementDescriptor: Equatable, Sendable {
    public var id: String
    public var label: String
    public var hint: String
    public var value: String?

    public init(id: String, label: String, hint: String, value: String? = nil) {
        self.id = id
        self.label = label
        self.hint = hint
        self.value = value
    }
}

public struct KeyboardShortcutDescriptor: Equatable, Sendable {
    public var id: String
    public var key: String
    public var modifiers: [String]
    public var actionSummary: String

    public init(id: String, key: String, modifiers: [String], actionSummary: String) {
        self.id = id
        self.key = key
        self.modifiers = modifiers
        self.actionSummary = actionSummary
    }
}

public struct KeyboardFocusPlan: Equatable, Sendable {
    public var orderedElementIDs: [String]

    public init(orderedElementIDs: [String]) {
        self.orderedElementIDs = orderedElementIDs
    }
}
