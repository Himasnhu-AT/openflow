"""Inputs for the prompt parity test (see mk_swift_prompt_harness.py). Run from this directory."""
import json

long_heard = "so um the 👨‍👩‍👧 family plan " * 12  # > 200 graphemes, ZWJ emoji clusters
long_before = ("Hi Alex, \"quoted\"\nline two é́ 🇦🇺 " * 8)
long_memory = ("I'm Taylor, I run Zentro (a café ☕️ chain) in Sydney.\n" * 10)

cases = [
    {
        "name": "minimal",
        "tones": {"verbatim": ""},
        "dictionary": [],
        "memory": "",
        "learnedSignals": [],
        "examples": {},
        "raw": "TRANSCRIPT",
        "tone": "casual",
        "context": None,
    },
    {
        "name": "full",
        "tones": {
            "casual": "Casual: lowercase is fine.\nKeep \"quotes\".",
            "professional": "Professional tone — full sentences.",
            "supercasual": "super casual 😎",
            "verbatim": "",
            "Zeta": "capital Z sorts first",
        },
        "dictionary": [
            "JezFlow: jez flow, jazz flow, jazz flo",
            "no wait = no wake; no weight",
            "Wispr Flow (whisper flow, wisper flow)",
            "  Zentro  ",
            "",
            "   ",
            "Acme:",
            "Café Noir : cafe noir",
        ],
        "memory": "  I'm Taylor.\nI run Zentro.  ",
        "learnedSignals": ["I know", "no way"],
        "examples": {
            "casual": [
                ["one", "One."],
                ["two \"q\"", "Two."],
                [long_heard, "short"],
                ["four", long_heard],
                ["five", "Five!"],
            ],
            "professional": [["hi alex", "Hi Alex,"]],
            "missing": [["x", "y"]],
            "supercasual": [],
        },
        "raw": "um hi alex\nsecond line",
        "tone": "professional",
        "context": {"windowTitle": "Re: \"Q3\" numbers — Mail\nextra title text that goes well past sixty characters in length", "before": long_before, "after": "after \"text\"\n" * 20},
    },
    {
        "name": "blank-memory-empty-context",
        "tones": {"casual": "c", "professional": "p"},
        "dictionary": ["Solo"],
        "memory": " \n\t ",
        "learnedSignals": ["scratch it"],
        "examples": {"professional": [["a", "b"], ["c", "d"]]},
        "raw": "",
        "tone": "casual",
        "context": {"windowTitle": "", "before": "", "after": ""},
    },
    {
        "name": "before-only-long-memory",
        "tones": {"casual": "c"},
        "dictionary": ["Term (only alias)"],
        "memory": long_memory,
        "learnedSignals": [],
        "examples": {},
        "raw": "text",
        "tone": "casual",
        "context": {"windowTitle": "", "before": "Hi Alex, about the", "after": ""},
    },
]
json.dump(cases, open('prompt_cases.json', 'w'), ensure_ascii=False, indent=1)
