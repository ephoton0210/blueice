// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

enum BrowserSearchProvider: String, CaseIterable {
    case duckDuckGo = "duckduckgo", google, bing, custom, unconfigured
    static let choices: [Self] = [.duckDuckGo, .google, .bing, .custom]
}

struct BrowserAddressTarget: Equatable {
    let url: String
    let isSearch: Bool
}

struct BrowserAddressFailure: Error {
    let message: String
}

struct BrowserSearchConfiguration {
    var provider: BrowserSearchProvider = .duckDuckGo
    var endpoint = ""
    var parameter = "q"

    var configurationError: String? {
        do { _ = try searchURL("BlueIce"); return nil }
        catch let error as BrowserAddressFailure { return error.message }
        catch { return "The search settings are invalid." }
    }

    func resolve(_ input: String) throws -> BrowserAddressTarget {
        let text = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { throw BrowserAddressFailure(message: "Enter an address or search text.") }
        guard !text.unicodeScalars.contains(where: { $0.properties.generalCategory == .control || CharacterSet.newlines.contains($0) }) else {
            throw BrowserAddressFailure(message: "Enter a single-line address or search text within the length limit.")
        }
        if text.hasPrefix("?") {
            return BrowserAddressTarget(url: try searchURL(String(text.dropFirst()).trimmingCharacters(in: .whitespaces)), isSearch: true)
        }
        if text.hasPrefix("//") {
            return BrowserAddressTarget(url: "https:" + text, isSearch: false)
        }
        if text.hasPrefix("/") {
            return BrowserAddressTarget(url: URL(fileURLWithPath: text).absoluteString, isSearch: false)
        }
        if text.hasPrefix("~/") {
            throw BrowserAddressFailure(message: "Use a complete file: URL for a local file address.")
        }
        if !text.contains("://"), text.rangeOfCharacter(from: .whitespacesAndNewlines) == nil {
            let candidate = "https://" + text
            if let parts = URLComponents(string: candidate), parts.url != nil, let host = parts.host,
               parts.user == nil, parts.password == nil,
               host.contains(".") || host.lowercased() == "localhost" || host.contains(":") {
                return BrowserAddressTarget(url: candidate, isSearch: false)
            }
        }
        // An explicit scheme stays a navigation, including schemes core will
        // refuse. Never turn a failed URL into a second external search.
        if let colon = text.firstIndex(of: ":") {
            let scheme = text[..<colon]
            if scheme.first?.isASCII == true, scheme.first?.isLetter == true,
               scheme.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || "+-.".contains($0)) }) {
                return BrowserAddressTarget(url: text, isSearch: false)
            }
        }
        return BrowserAddressTarget(url: try searchURL(text), isSearch: true)
    }

    private func searchURL(_ query: String) throws -> String {
        guard !query.isEmpty, query.utf8.count <= 2048 else {
            throw BrowserAddressFailure(message: "Enter search text within the 2048-byte limit.")
        }
        let address: String
        let field: String
        switch provider {
        case .duckDuckGo: address = "https://duckduckgo.com/"; field = "q"
        case .google: address = "https://www.google.com/search"; field = "q"
        case .bing: address = "https://www.bing.com/search"; field = "q"
        case .custom: address = endpoint; field = parameter
        case .unconfigured: throw BrowserAddressFailure(message: "Choose a search engine in Settings before searching.")
        }
        guard address.utf8.count <= 2048, let original = URLComponents(string: address),
              ["http", "https"].contains(original.scheme?.lowercased() ?? ""),
              original.host?.isEmpty == false, original.user == nil, original.password == nil,
              original.fragment == nil, original.url != nil,
              !field.isEmpty, field.utf8.count <= 64,
              !field.unicodeScalars.contains(where: { $0.properties.generalCategory == .control || CharacterSet.newlines.contains($0) }),
              field == field.trimmingCharacters(in: .whitespacesAndNewlines) else {
            throw BrowserAddressFailure(message: "Use a valid HTTP(S) search URL without credentials or a fragment, and a query field.")
        }
        var parts = original
        // URLQueryItem leaves '+' literal. Form-style search endpoints decode
        // it as a space, so use RFC 3986 unreserved ASCII for every query item.
        let allowed = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
        func encode(_ text: String) -> String { text.addingPercentEncoding(withAllowedCharacters: allowed)! }
        // Preserve fixed fields verbatim, including form-style '+' spaces and
        // valueless flags. Decode only field names to remove duplicate queries.
        let fixed = (parts.percentEncodedQuery?.split(separator: "&", omittingEmptySubsequences: false) ?? []).filter { pair in
            let name = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false).first ?? ""
            return String(name).replacingOccurrences(of: "+", with: " ").removingPercentEncoding != field
        }
        parts.percentEncodedQuery = (fixed.map(String.init) + [encode(field) + "=" + encode(query)]).joined(separator: "&")
        guard let url = parts.url?.absoluteString, url.utf8.count <= 8192 else {
            throw BrowserAddressFailure(message: "The search URL exceeds the navigation length limit.")
        }
        return url
    }
}
