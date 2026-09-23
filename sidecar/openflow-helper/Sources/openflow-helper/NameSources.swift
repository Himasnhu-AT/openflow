import Contacts
import EventKit
import Foundation

/// Names from Contacts (people and companies) and the next 48 h of calendar events (titles, attendees), ported
/// from NameSources.swift. Used only to bias speech recognition, never stored.
enum NameSources {
    static func names() async -> [String] {
        var out: [String] = []
        let contacts = CNContactStore()
        if (try? await contacts.requestAccess(for: .contacts)) == true {
            let keys = [CNContactGivenNameKey, CNContactFamilyNameKey, CNContactOrganizationNameKey, CNContactNicknameKey] as [CNKeyDescriptor]
            let req = CNContactFetchRequest(keysToFetch: keys)
            try? contacts.enumerateContacts(with: req) { c, _ in
                for n in [c.givenName, c.familyName, c.nickname, c.organizationName, [c.givenName, c.familyName].filter { !$0.isEmpty }.joined(separator: " ")] {
                    let t = n.trimmingCharacters(in: .whitespaces)
                    if t.count >= 3, t.first?.isLetter == true { out.append(t) }
                }
            }
        }
        let events = EKEventStore()
        if (try? await events.requestFullAccessToEvents()) == true {
            let start = Calendar.current.startOfDay(for: Date())
            let pred = events.predicateForEvents(withStart: start, end: start.addingTimeInterval(86_400 * 2), calendars: nil)
            for e in events.events(matching: pred) {
                if let t = e.title { out.append(contentsOf: contextNames(t)) }
                for a in e.attendees ?? [] { if let n = a.name, n.count >= 3 { out.append(n) } }
            }
        }
        var seen = Set<String>()
        return out.filter { seen.insert($0.lowercased()).inserted }
    }

    // Copied from TextRules.swift (contextNames + commonCapitalised) so the helper has no dependency on the Rust
    // text rules; keep in sync with openflow_core::text_rules::context_names.
    private static let commonCapitalised: Set<String> = [
        "i", "i'm", "i'll", "i've", "i'd", "the", "a", "an", "and", "but", "or", "so", "hi", "hey", "hello", "dear", "thanks", "thank", "cheers",
        "kind", "best", "regards", "yes", "no", "ok", "okay", "yeah", "also", "please", "sorry", "just", "well", "oh", "wow", "haha", "lol",
        "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "january", "february", "march", "april", "may", "june",
        "july", "august", "september", "october", "november", "december", "today", "tomorrow", "yesterday", "morning", "afternoon", "evening",
        "this", "that", "these", "those", "there", "here", "what", "when", "where", "who", "why", "how", "it", "it's", "we", "we're", "you",
        "he", "she", "they", "my", "your", "our", "his", "her", "their", "if", "in", "on", "at", "to", "for", "of", "with", "from", "by",
        "not", "can", "could", "would", "should", "will", "do", "does", "did", "is", "are", "was", "were", "be", "been", "have", "has", "had",
        "let's", "let", "hope", "see", "send", "get", "got", "one", "two", "three", "first", "second", "third", "new", "re", "fw", "fwd",
        "window", "untitled", "all", "notes", "messages", "mail", "chrome", "safari", "google", "search", "inbox", "sent", "draft",
    ]

    /// Capitalised words and phrases in `text` that look like names, in order of appearance ("Wispr Flow", "Zentro").
    static func contextNames(_ text: String, limit: Int = 15) -> [String] {
        let words = text.split { $0.isWhitespace }.map { $0.trimmingCharacters(in: CharacterSet.punctuationCharacters.subtracting(CharacterSet(charactersIn: "'’-"))) }
        let lower = Set(words.filter { $0.first?.isLowercase == true }.map { $0.lowercased() })
        func isName(_ w: String) -> Bool {
            guard w.count >= 3, let f = w.first, f.isUppercase, w.dropFirst().contains(where: { $0.isLetter }),
                  !commonCapitalised.contains(w.lowercased()), !lower.contains(w.lowercased()) else { return false }
            return true
        }
        var out: [String] = []
        var i = 0
        while i < words.count {
            guard isName(words[i]) else { i += 1; continue }
            var phrase = [words[i]]
            var j = i + 1
            while j < words.count, phrase.count < 3, isName(words[j]) { phrase.append(words[j]); j += 1 }
            let name = phrase.joined(separator: " ")
            if !out.contains(where: { $0.lowercased() == name.lowercased() }) { out.append(name) }
            i = j
        }
        return Array(out.prefix(limit))
    }
}
