import ApplicationServices
import AppKit
import Foundation

/// Who is talking in the Zoom desktop app, read through Accessibility (ported from Meeting-Assistant's
/// meeting-state.swift, MIT). Zoom draws its meeting UI in an embedded Chromium layer that Accessibility only sees
/// after `AXManualAccessibility` is set on the app. Three shapes carry names:
///   "Talking: rajat prakash"                          AXValue in the floating video window; still there while you share
///   "Rahul Khatri, Computer audio muted, Video off"   a video tile's description
///   "View Rahul Khatri's profile"                     a tile's profile button
/// The first tile of the floating video window follows the active speaker in speaker view.
enum ZoomSpeakers {
    static let bundleID = "us.zoom.xos"

    struct Snapshot {
        var active: String?
        /// "label" (Talking:, authoritative) or "tile" (floating window's first tile).
        var activeSource = ""
        var selfName: String?
        var participants: [String] = []
    }

    static func snapshot() -> Snapshot? {
        guard AXIsProcessTrusted(),
              let app = NSRunningApplication.runningApplications(withBundleIdentifier: bundleID).first else { return nil }
        let appEl = AXUIElementCreateApplication(app.processIdentifier)
        AXUIElementSetAttributeValue(appEl, "AXManualAccessibility" as CFString, kCFBooleanTrue)
        var s = Snapshot()
        var visited = 0
        func walk(_ el: AXUIElement, floating: Bool, depth: Int) {
            visited += 1
            if depth > 30 || visited > 4000 { return }
            let title = string(el, kAXTitleAttribute) ?? ""
            let inFloat = floating || title.lowercased().contains("floating video")
            if let d = string(el, kAXDescriptionAttribute) {
                let fromTile = nameFromTile(d), fromProfile = nameFromProfile(d)
                if let n = fromTile ?? fromProfile {
                    if !s.participants.contains(n) { s.participants.append(n) }
                    if fromTile != nil && describesSelf(d) { s.selfName = n }
                    if inFloat && s.active == nil { s.active = n; s.activeSource = "tile" }
                }
            }
            if let v = string(el, kAXValueAttribute), let n = nameFromTalking(v) {
                s.active = n
                s.activeSource = "label"
                if !s.participants.contains(n) { s.participants.append(n) }
            }
            var kids: CFTypeRef?
            if AXUIElementCopyAttributeValue(el, kAXChildrenAttribute as CFString, &kids) == .success, let list = kids as? [AXUIElement] {
                for k in list { walk(k, floating: inFloat, depth: depth + 1) }
            }
        }
        var windows: CFTypeRef?
        guard AXUIElementCopyAttributeValue(appEl, kAXWindowsAttribute as CFString, &windows) == .success,
              let list = windows as? [AXUIElement] else { return s }
        for w in list { walk(w, floating: false, depth: 0) }
        return s
    }

    private static func string(_ el: AXUIElement, _ attr: String) -> String? {
        var v: CFTypeRef?
        guard AXUIElementCopyAttributeValue(el, attr as CFString, &v) == .success else { return nil }
        return v as? String
    }

    /// 2–40 characters, 1–5 words, and no Zoom interface word (whole words: "Sharen" and "Mike" survive).
    static func validName(_ s: String) -> Bool {
        let n = s.trimmingCharacters(in: .whitespaces)
        guard n.count >= 2, n.count <= 40, n.rangeOfCharacter(from: .letters) != nil,
              n.split(separator: " ").count <= 5 else { return false }
        let tokens = Set(n.lowercased().split { !$0.isLetter }.map(String.init))
        let ui: Set<String> = ["zoom", "audio", "video", "mute", "unmute", "muted", "share", "sharing", "settings", "setting",
                               "microphone", "mic", "speaker", "camera", "waiting", "participants", "participant", "reactions",
                               "reaction", "profile", "record", "recording", "screen", "gallery"]
        return tokens.isDisjoint(with: ui)
    }

    /// "Rahul Khatri (Host, me)" -> "Rahul Khatri"
    static func canon(_ s: String) -> String {
        (s.firstIndex(of: "(").map { String(s[..<$0]) } ?? s).trimmingCharacters(in: .whitespaces)
    }

    static func nameFromTile(_ d: String) -> String? {
        let low = d.lowercased()
        guard d.contains(","), low.contains("audio") || low.contains("video") || low.contains("muted") else { return nil }
        let n = canon(d.components(separatedBy: ",")[0])
        return validName(n) ? n : nil
    }

    static func nameFromProfile(_ d: String) -> String? {
        guard d.hasPrefix("View ") else { return nil }
        var n = String(d.dropFirst(5))
        for suffix in ["'s profile", "\u{2019}s profile", " profile"] {
            if let r = n.range(of: suffix) { n = String(n[..<r.lowerBound]); break }
        }
        n = canon(n)
        return validName(n) ? n : nil
    }

    static func nameFromTalking(_ v: String) -> String? {
        guard v.lowercased().hasPrefix("talking:") else { return nil }
        let n = canon(String(v.dropFirst("talking:".count)))
        return validName(n) ? n.capitalized : nil
    }

    static func describesSelf(_ d: String) -> Bool {
        let l = d.lowercased()
        guard let open = l.firstIndex(of: "("), let close = l[l.index(after: open)...].firstIndex(of: ")") else { return false }
        return l[l.index(after: open)..<close].split { !$0.isLetter }.contains("me")
    }
}
