#!/usr/bin/env swift
// Puts an uploaded build of Safelight in every TestFlight group of the app
// and submits it for Beta App Review, so internal and external testers run
// the newest build (the external groups get it once Apple approves it).
// Waits for App Store Connect to finish processing the build first.
//
//     scripts/testflight-groups.swift <build number> ["What to Test" text]
//
// Uses an App Store Connect API key, read from the environment or from
// ~/.config/safelight/asc.env (outside the repository):
//
//     ASC_KEY_ID=…        the key's id
//     ASC_ISSUER_ID=…     the issuer id shown above the keys
//     ASC_KEY_PATH=…      optional; default ~/.appstoreconnect/private_keys/AuthKey_<id>.p8
//
// The key never leaves this Mac except to sign a short-lived token.

import CryptoKit
import Foundation

let appID = "6832584559"
let version = "0.1.0"
let defaultNotes = "Please try importing your RAW files (from a card, Files or Photos), editing them and exporting. Send feedback and screenshots from the TestFlight app."

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("testflight: \(message)\n".utf8))
    exit(1)
}

// MARK: Configuration

var env = ProcessInfo.processInfo.environment
let configFile = FileManager.default.homeDirectoryForCurrentUser.appending(path: ".config/safelight/asc.env")
if let text = try? String(contentsOf: configFile, encoding: .utf8) {
    for line in text.split(separator: "\n") where !line.hasPrefix("#") {
        let parts = line.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
        if parts.count == 2, env[parts[0]] == nil { env[parts[0]] = parts[1] }
    }
}
guard let keyID = env["ASC_KEY_ID"], let issuer = env["ASC_ISSUER_ID"] else {
    fail("set ASC_KEY_ID and ASC_ISSUER_ID (or put them in \(configFile.path))")
}
let keyPath = env["ASC_KEY_PATH"].map { ($0 as NSString).expandingTildeInPath }
    ?? FileManager.default.homeDirectoryForCurrentUser.appending(path: ".appstoreconnect/private_keys/AuthKey_\(keyID).p8").path
guard let pem = try? String(contentsOfFile: keyPath, encoding: .utf8),
      let key = try? P256.Signing.PrivateKey(pemRepresentation: pem) else {
    fail("can't read the API key at \(keyPath)")
}

let args = CommandLine.arguments.dropFirst()
guard let build = args.first else { fail("usage: testflight-groups.swift <build number> [what to test]") }
let notes = args.dropFirst().first ?? defaultNotes

// MARK: API

func base64url(_ data: Data) -> String {
    data.base64EncodedString().replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
}

/// A token App Store Connect accepts for 20 minutes.
func token() -> String {
    let header = ["alg": "ES256", "kid": keyID, "typ": "JWT"]
    let now = Int(Date().timeIntervalSince1970)
    let claims: [String: Any] = ["iss": issuer, "iat": now, "exp": now + 1200, "aud": "appstoreconnect-v1"]
    let signing = base64url(try! JSONSerialization.data(withJSONObject: header)) + "."
        + base64url(try! JSONSerialization.data(withJSONObject: claims))
    let signature = try! key.signature(for: Data(signing.utf8))
    return signing + "." + base64url(signature.rawRepresentation)
}

/// Calls the API; returns the status and the JSON body.
func call(_ method: String, _ path: String, _ body: [String: Any]? = nil) async -> (Int, [String: Any]) {
    var request = URLRequest(url: URL(string: "https://api.appstoreconnect.apple.com" + path)!)
    request.httpMethod = method
    request.setValue("Bearer \(token())", forHTTPHeaderField: "Authorization")
    if let body {
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try! JSONSerialization.data(withJSONObject: body)
    }
    guard let (data, response) = try? await URLSession.shared.data(for: request) else { fail("\(method) \(path): no answer") }
    let status = (response as! HTTPURLResponse).statusCode
    let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
    return (status, json)
}

func errors(_ json: [String: Any]) -> String {
    ((json["errors"] as? [[String: Any]]) ?? []).compactMap { $0["detail"] as? String ?? $0["title"] as? String }.joined(separator: "; ")
}

// MARK: The build

print("Waiting for build \(build) of \(version) to finish processing…")
var buildID: String?
for _ in 0..<90 {
    let (status, json) = await call("GET", "/v1/builds?filter[app]=\(appID)&filter[version]=\(build)&filter[preReleaseVersion.version]=\(version)&fields[builds]=processingState")
    guard status == 200 else { fail("builds: \(status) \(errors(json))") }
    if let found = (json["data"] as? [[String: Any]])?.first,
       let state = (found["attributes"] as? [String: Any])?["processingState"] as? String {
        if state == "VALID" { buildID = found["id"] as? String; break }
        if state == "FAILED" || state == "INVALID" { fail("build \(build) is \(state)") }
    }
    try await Task.sleep(for: .seconds(20))
}
guard let buildID else { fail("build \(build) didn't finish processing within 30 minutes") }

// What to Test, which external testers see and Beta App Review needs.
let (_, localizations) = await call("GET", "/v1/builds/\(buildID)/betaBuildLocalizations")
if ((localizations["data"] as? [Any]) ?? []).isEmpty {
    let (status, json) = await call("POST", "/v1/betaBuildLocalizations", ["data": [
        "type": "betaBuildLocalizations",
        "attributes": ["locale": "en-US", "whatsNew": notes],
        "relationships": ["build": ["data": ["type": "builds", "id": buildID]]],
    ]])
    if status >= 300 { print("What to Test not set: \(status) \(errors(json))") }
}

// Every group of the app.
let (groupStatus, groupJSON) = await call("GET", "/v1/apps/\(appID)/betaGroups?limit=50")
guard groupStatus == 200, let groups = groupJSON["data"] as? [[String: Any]] else {
    fail("groups: \(groupStatus) \(errors(groupJSON))")
}
var external = false
for group in groups {
    guard let id = group["id"] as? String, let attributes = group["attributes"] as? [String: Any] else { continue }
    let name = attributes["name"] as? String ?? id
    if attributes["isInternalGroup"] as? Bool == false { external = true }
    let (status, json) = await call("POST", "/v1/betaGroups/\(id)/relationships/builds", ["data": [["type": "builds", "id": buildID]]])
    print(status < 300 ? "Added to \(name)" : "\(name): \(status) \(errors(json))")
}

// External groups get it once Beta App Review approves it.
if external {
    let (status, json) = await call("POST", "/v1/betaAppReviewSubmissions", ["data": [
        "type": "betaAppReviewSubmissions",
        "relationships": ["build": ["data": ["type": "builds", "id": buildID]]],
    ]])
    switch status {
    case 200..<300: print("Submitted for Beta App Review")
    case 409: print("Already submitted for Beta App Review: \(errors(json))")
    default: fail("Beta App Review: \(status) \(errors(json))")
    }
}
