"""Builds a Swift harness from the real Polisher.swift / TextRules.swift source (mechanically, by line ranges),
so golden prompt output comes from the Swift code itself rather than a hand copy.

Regenerate the goldens:
  python3 mk_prompt_cases.py
  python3 mk_swift_prompt_harness.py /tmp/harness.swift && swiftc -O -o /tmp/harness /tmp/harness.swift
  /tmp/harness prompt_cases.json prompt_golden.json
"""
import re
import sys

D = '/Users/himanshu/codes/JezFlow/dep/Sources/JezFlow/'
pol = open(D + 'Polisher.swift').read().split('\n')
tr = open(D + 'TextRules.swift').read().split('\n')


def func_block(lines, sig):
    i = next(k for k, l in enumerate(lines) if sig in l)
    indent = len(lines[i]) - len(lines[i].lstrip())
    j = i + 1
    while lines[j] != ' ' * indent + '}':
        j += 1
    return '\n'.join(lines[i:j + 1])


user_msg = func_block(pol, 'func userMessage(')
sys_prompt = func_block(pol, 'func systemPrompt(')
parse_dict = func_block(tr, 'static func parseDictionary(')
notes_i = next(k for k, l in enumerate(pol) if 'private static let notesFormat = """' in l)
j = notes_i + 1
while pol[j] != '    """':
    j += 1
notes_fmt = '\n'.join(pol[notes_i:j + 1]).replace('private static let', 'static let')
who = next(l for l in pol if 'let who = config.memory' in l).strip()
chunk = next(l for l in pol if 'let chunkSystem =' in l).strip()
merge = next(l for l in pol if 'let mergeSystem =' in l).strip()
title_line = next(l for l in pol if 'complete(system: "Give these meeting notes' in l)
title = re.search(r'complete\(system: (".*?"), user: summary', title_line).group(1)

harness = f'''
import Foundation

struct Config {{ var learnedSignals: [String] = []; var memory = "" }}
struct FieldContext {{
    var windowTitle = ""; var before = ""; var after = ""
    var isEmpty: Bool {{ windowTitle.isEmpty && before.isEmpty && after.isEmpty }}
}}
enum TextRules {{
{parse_dict}
}}
final class P {{
    let config: Config
    init(config: Config) {{ self.config = config }}
{user_msg}

{sys_prompt}

{notes_fmt}
    func chunkSystem() -> String {{
        {who}
        {chunk}
        return chunkSystem
    }}
    func mergeSystem() -> String {{
        {merge}
        return mergeSystem
    }}
    static let titleSystem = {title}
}}

struct Case: Decodable {{
    var name: String
    var tones: [String: String]
    var dictionary: [String]
    var memory: String
    var learnedSignals: [String]
    var examples: [String: [[String]]]
    var raw: String
    var tone: String
    var context: [String: String]?
}}

let cases = try! JSONDecoder().decode([Case].self, from: FileManager.default.contents(atPath: CommandLine.arguments[1])!)
var out: [[String: String]] = []
for c in cases {{
    let p = P(config: Config(learnedSignals: c.learnedSignals, memory: c.memory))
    let ex = c.examples.mapValues {{ $0.map {{ ($0[0], $0[1]) }} }}
    let ctx = c.context.map {{ FieldContext(windowTitle: $0["windowTitle"] ?? "", before: $0["before"] ?? "", after: $0["after"] ?? "") }}
    out.append([
        "name": c.name,
        "system": p.systemPrompt(tones: c.tones, dictionary: c.dictionary, memory: c.memory, examples: ex),
        "user": p.userMessage(c.raw, tone: c.tone, context: ctx),
        "chunkSystem": p.chunkSystem(),
        "mergeSystem": p.mergeSystem(),
        "titleSystem": P.titleSystem,
    ])
}}
let data = try! JSONSerialization.data(withJSONObject: out, options: [.prettyPrinted, .sortedKeys])
FileManager.default.createFile(atPath: CommandLine.arguments[2], contents: data)
'''
open(sys.argv[1], 'w').write(harness)
