// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Combine
import Foundation
import SwiftUI

enum BrowserLanguage: String, CaseIterable, Sendable {
    case system, english = "en", traditionalChinese = "zh-Hant"
    var title: String {
        switch self {
        case .system: return BrowserStrings.text("Follow macOS")
        case .english: return "English"
        case .traditionalChinese: return "繁體中文"
        }
    }
}

private final class BrowserLocalizationResources: NSObject {}

enum BrowserStrings {
    static let preferenceKey = "browser.interfaceLanguage"
    static let changed = Notification.Name("cc.blueice.interface-language.changed")
    // Owner startup configuration also reaches the launcher's private panel.
    // Language changes do not travel through page or permission-decision IPC.
    static func preferenceDomain(arguments: [String] = ProcessInfo.processInfo.arguments,
                                 environment: [String: String] = ProcessInfo.processInfo.environment) -> String {
        if let i = arguments.firstIndex(of: "--preferences-domain"), arguments.indices.contains(i + 1), validDomain(arguments[i + 1]) {
            return arguments[i + 1]
        }
        if let domain = environment["BLUEICE_PREFERENCES_DOMAIN"], validDomain(domain) { return domain }
        return "cc.blueice.BlueIce"
    }
    private static func validDomain(_ value: String) -> Bool {
        !value.isEmpty && value.utf8.count <= 255 && !value.unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) }
    }
    static func preferences() -> UserDefaults { UserDefaults(suiteName: preferenceDomain()) ?? .standard }
    static var systemLanguages: [String] {
        // An application-specific AppleLanguages override must not change what
        // "Follow macOS" means while this process is still running.
        (UserDefaults.standard.persistentDomain(forName: UserDefaults.globalDomain)?["AppleLanguages"] as? [String]) ?? Locale.preferredLanguages
    }
    static func resolved(_ language: BrowserLanguage, preferred: [String] = systemLanguages) -> String {
        if language != .system { return language.rawValue }
        return Bundle.preferredLocalizations(from: ["en", "zh-Hant"], forPreferences: preferred).first ?? "en"
    }
    static var currentLanguage: String {
        resolved(initialLanguage())
    }
    static func initialLanguage(defaults: UserDefaults? = nil) -> BrowserLanguage {
        let value = (defaults ?? preferences()).string(forKey: preferenceKey)
        if let value { return BrowserLanguage(rawValue: value) ?? .system }
        return BrowserLanguage(rawValue: ProcessInfo.processInfo.environment["BLUEICE_INTERFACE_LANGUAGE"] ?? "") ?? .system
    }
    static func prepareApplicationLanguage() {
        let arguments = ProcessInfo.processInfo.arguments
        if let index = arguments.firstIndex(of: "--interface-language"), arguments.indices.contains(index + 1),
           let language = BrowserLanguage(rawValue: arguments[index + 1]) {
            preferences().set(language.rawValue, forKey: preferenceKey)
            preferences().synchronize()
        }
        // Keep the standard AppKit menus in the same startup language. This
        // touches only this application's domain, never NSGlobalDomain.
        UserDefaults.standard.removeObject(forKey: "AppleLanguages")
        let language = initialLanguage()
        if language != .system { UserDefaults.standard.set([language.rawValue], forKey: "AppleLanguages") }
    }
    static var resourceBundle: Bundle { Bundle(for: BrowserLocalizationResources.self) }
    static func text(_ key: String, language: String? = nil, bundle: Bundle? = nil) -> String {
        let language = language ?? currentLanguage
        let safeLanguage = ["en", "zh-Hant"].contains(language) ? language : "en"
        guard let path = (bundle ?? resourceBundle).path(forResource: safeLanguage, ofType: "lproj"),
              let localized = Bundle(path: path) else { return key }
        return localized.localizedString(forKey: key, value: key, table: "Localizable")
    }
    static func format(_ key: String, _ arguments: CVarArg...) -> String {
        String(format: text(key), arguments: arguments)
    }
}

@MainActor
final class BrowserLocalization: ObservableObject {
    static let shared: BrowserLocalization = {
        BrowserStrings.prepareApplicationLanguage()
        return BrowserLocalization()
    }()
    @Published private(set) var language: BrowserLanguage
    private let defaults: UserDefaults
    private let domain: String
    private var observation: AnyCancellable?
    init(defaults: UserDefaults? = nil, domain: String? = nil, observeChanges: Bool = true) {
        self.defaults = defaults ?? BrowserStrings.preferences()
        self.domain = domain ?? BrowserStrings.preferenceDomain()
        language = BrowserStrings.initialLanguage(defaults: self.defaults)
        if observeChanges {
            observation = DistributedNotificationCenter.default()
                .publisher(for: BrowserStrings.changed, object: self.domain as NSString)
                .sink { [weak self] _ in Task { @MainActor in self?.refresh() } }
        }
    }
    var locale: Locale { Locale(identifier: BrowserStrings.resolved(language)) }
    func refresh() {
        defaults.synchronize()
        let next = BrowserLanguage(rawValue: defaults.string(forKey: BrowserStrings.preferenceKey) ?? "") ?? .system
        if language != next { language = next }
    }
    func setLanguage(_ value: BrowserLanguage) {
        guard value != language else { return }
        defaults.set(value.rawValue, forKey: BrowserStrings.preferenceKey)
        defaults.synchronize()
        language = value
        DistributedNotificationCenter.default().postNotificationName(BrowserStrings.changed, object: domain,
                                                                    userInfo: nil, deliverImmediately: true)
    }
}

struct BrowserLanguageSettingsView: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Picker(BrowserStrings.text("Interface language"), selection: Binding(get: { localization.language }, set: localization.setLanguage)) {
                ForEach(BrowserLanguage.allCases, id: \.self) { Text($0.title).tag($0) }
            }.accessibilityLabel(BrowserStrings.text("Interface language")).accessibilityIdentifier("interface-language")
            Text(BrowserStrings.text("Browser controls update immediately. Standard macOS menus and dialogs use the startup language; restart BlueIce to update them. Page text and translation settings stay separate."))
                .font(.caption).foregroundStyle(.secondary)
        }.padding([.horizontal, .top], 24).frame(width: 460)
    }
}
