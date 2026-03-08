import Foundation

public protocol OnboardingCompletionStore: AnyObject {
    func isOnboardingComplete() -> Bool
    func markOnboardingComplete()
    func resetOnboardingCompletion()
}

public final class UserDefaultsOnboardingCompletionStore: OnboardingCompletionStore {
    public static let defaultKey = "recordit.onboarding.completed"

    private let userDefaults: UserDefaults
    private let key: String

    public init(
        userDefaults: UserDefaults = .standard,
        key: String = UserDefaultsOnboardingCompletionStore.defaultKey
    ) {
        self.userDefaults = userDefaults
        self.key = key
    }

    public func isOnboardingComplete() -> Bool {
        userDefaults.bool(forKey: key)
    }

    public func markOnboardingComplete() {
        userDefaults.set(true, forKey: key)
    }

    public func resetOnboardingCompletion() {
        userDefaults.set(false, forKey: key)
    }
}
