import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("accessibility_smoke failed: \(message)\n", stderr)
        exit(1)
    }
}

private func assertCatalog(
    name: String,
    elements: [AccessibilityElementDescriptor],
    focusPlan: KeyboardFocusPlan,
    shortcuts: [KeyboardShortcutDescriptor]
) {
    check(!elements.isEmpty, "\(name) should expose at least one accessibility element")

    let ids = elements.map(\.id)
    check(Set(ids).count == ids.count, "\(name) accessibility IDs must be unique")
    check(
        focusPlan.orderedElementIDs == ids,
        "\(name) focus order should deterministically match declared accessibility elements"
    )

    for element in elements {
        check(!element.label.isEmpty, "\(name) element \(element.id) must include a label")
        check(!element.hint.isEmpty, "\(name) element \(element.id) must include a hint")
    }

    check(!shortcuts.isEmpty, "\(name) should expose keyboard shortcuts")
    for shortcut in shortcuts {
        check(!shortcut.key.isEmpty, "\(name) shortcut \(shortcut.id) must include a key")
        check(!shortcut.actionSummary.isEmpty, "\(name) shortcut \(shortcut.id) must include an action summary")
    }
}

@MainActor
private func runSmoke() {
    assertCatalog(
        name: "ModelSetupViewModel",
        elements: ModelSetupViewModel.onboardingAccessibilityElements,
        focusPlan: ModelSetupViewModel.onboardingFocusPlan,
        shortcuts: ModelSetupViewModel.onboardingKeyboardShortcuts
    )

    assertCatalog(
        name: "PreflightViewModel",
        elements: PreflightViewModel.accessibilityElements,
        focusPlan: PreflightViewModel.focusPlan,
        shortcuts: PreflightViewModel.keyboardShortcuts
    )

    assertCatalog(
        name: "RuntimeViewModel",
        elements: RuntimeViewModel.accessibilityElements,
        focusPlan: RuntimeViewModel.focusPlan,
        shortcuts: RuntimeViewModel.keyboardShortcuts
    )

    assertCatalog(
        name: "SessionDetailViewModel",
        elements: SessionDetailViewModel.accessibilityElements,
        focusPlan: SessionDetailViewModel.focusPlan,
        shortcuts: SessionDetailViewModel.keyboardShortcuts
    )

    assertCatalog(
        name: "SessionPlaybackViewModel",
        elements: SessionPlaybackViewModel.accessibilityElements,
        focusPlan: SessionPlaybackViewModel.focusPlan,
        shortcuts: SessionPlaybackViewModel.keyboardShortcuts
    )

    assertCatalog(
        name: "SessionListViewModel",
        elements: SessionListViewModel.accessibilityElements,
        focusPlan: SessionListViewModel.focusPlan,
        shortcuts: SessionListViewModel.keyboardShortcuts
    )

    assertCatalog(
        name: "SessionExportAccessibilityCatalog",
        elements: SessionExportAccessibilityCatalog.elements,
        focusPlan: SessionExportAccessibilityCatalog.focusPlan,
        shortcuts: SessionExportAccessibilityCatalog.keyboardShortcuts
    )
}

@main
struct AccessibilitySmokeMain {
    @MainActor
    static func main() {
        runSmoke()
        print("accessibility_smoke: PASS")
    }
}
