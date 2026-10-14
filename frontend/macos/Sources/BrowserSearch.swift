// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import SwiftUI
import CoreFoundation

@MainActor
final class BrowserSearchPreferences: ObservableObject {
    static let configurationKey = "browser.search.configuration"
    private let defaults: UserDefaults
    @Published private(set) var configuration: BrowserSearchConfiguration
    var provider: BrowserSearchProvider { configuration.provider }
    var endpoint: String { configuration.endpoint }
    var parameter: String { configuration.parameter }
    var configurationError: String? { configuration.configurationError }

    init(defaults: UserDefaults? = nil) {
        let defaults = defaults ?? BrowserAppearance.preferenceStore()
        self.defaults = defaults
        if defaults.object(forKey: Self.configurationKey) == nil {
            configuration = BrowserSearchConfiguration()
        } else if let value = defaults.dictionary(forKey: Self.configurationKey),
                  let version = value["version"] as? NSNumber, CFGetTypeID(version) != CFBooleanGetTypeID(), version.doubleValue == 1,
                  let name = value["provider"] as? String,
                  let provider = BrowserSearchProvider(rawValue: name), BrowserSearchProvider.choices.contains(provider),
                  let endpoint = value["endpoint"] as? String, endpoint.utf8.count <= 2048,
                  let parameter = value["parameter"] as? String, parameter.utf8.count <= 64 {
            configuration = BrowserSearchConfiguration(provider: provider, endpoint: endpoint, parameter: parameter)
        } else {
            configuration = BrowserSearchConfiguration(provider: .unconfigured)
        }
    }
    func setProvider(_ provider: BrowserSearchProvider) {
        guard BrowserSearchProvider.choices.contains(provider) else { return }
        var next = configuration; next.provider = provider; save(next)
    }
    func configureCustom(endpoint: String, parameter: String) throws {
        let next = BrowserSearchConfiguration(provider: .custom,
            endpoint: endpoint.trimmingCharacters(in: .whitespacesAndNewlines),
            parameter: parameter.trimmingCharacters(in: .whitespacesAndNewlines))
        if let error = next.configurationError { throw BrowserAddressFailure(message: error) }
        save(next)
    }
    private func save(_ next: BrowserSearchConfiguration) {
        defaults.set(["version": 1, "provider": next.provider.rawValue, "endpoint": next.endpoint, "parameter": next.parameter] as [String: Any],forKey: Self.configurationKey)
        configuration = next
    }
    func resolve(_ text: String) throws -> BrowserAddressTarget { try configuration.resolve(text) }
}

extension BrowserSearchProvider {
    var title: String {
        switch self {
        case .duckDuckGo: return "DuckDuckGo"
        case .google: return "Google"
        case .bing: return "Bing"
        case .custom: return BrowserStrings.text("Custom")
        case .unconfigured: return BrowserStrings.text("Choose a search engine")
        }
    }
}

struct BrowserSearchSettingsView: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var settings: BrowserSearchPreferences
    @State private var endpoint = ""
    @State private var parameter = "q"
    @State private var error: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Picker(BrowserStrings.text("Search engine"),selection: Binding(get: { settings.provider },set: settings.setProvider)) {
                if settings.provider == .unconfigured { Text(settings.provider.title).tag(BrowserSearchProvider.unconfigured) }
                ForEach(BrowserSearchProvider.choices,id: \.self) { Text($0.title).tag($0) }
            }.accessibilityLabel(BrowserStrings.text("Search engine")).accessibilityIdentifier("search-provider")
            if settings.provider == .custom {
                TextField(BrowserStrings.text("Search URL"),text: $endpoint).textFieldStyle(.roundedBorder).accessibilityIdentifier("search-endpoint")
                HStack {
                    TextField(BrowserStrings.text("Query field"),text: $parameter).textFieldStyle(.roundedBorder).accessibilityIdentifier("search-parameter")
                    Button(BrowserStrings.text("Save custom search")) {
                        do { try settings.configureCustom(endpoint: endpoint,parameter: parameter); error = nil }
                        catch let failure as BrowserAddressFailure { error = failure.message }
                        catch { self.error = "The search settings are invalid." }
                    }.keyboardShortcut(.defaultAction).accessibilityIdentifier("search-save")
                }
            }
            if let error = error ?? settings.configurationError {
                Text(BrowserStrings.text(error)).font(.caption).foregroundStyle(.red).accessibilityIdentifier("search-settings-error")
            }
            Text(BrowserStrings.text("Typing stays in BlueIce. Enter or Go sends the search to your chosen engine.")).font(.caption).foregroundStyle(.secondary)
        }.onAppear { resetDrafts() }
            .onChange(of: settings.provider) { _, _ in resetDrafts() }
            .onChange(of: ObjectIdentifier(settings)) { _, _ in resetDrafts() }
    }
    private func resetDrafts() { endpoint = settings.endpoint; parameter = settings.parameter; error = nil }
}
