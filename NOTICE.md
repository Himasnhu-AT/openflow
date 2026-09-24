# Notices

OpenFlow is open source under the BSD 3-Clause License (see LICENSE).

It builds on the following, each under its own license (not redistributed in this package; downloaded by you):

- FluidAudio (Swift, CoreML speech models) — Apache License 2.0 — https://github.com/FluidInference/FluidAudio
- Ollama (default local LLM engine) — MIT License — https://github.com/ollama/ollama
- llama.cpp / llama-server (Homebrew `llama.cpp`) — MIT License — https://github.com/ggml-org/llama.cpp
- transcribe.cpp (bundled as transcribe-server, with ggml) — MIT License — https://github.com/handy-computer/transcribe.cpp
- cpp-httplib (in transcribe-server) — MIT License, Copyright (c) 2024 Yuji Hirose — https://github.com/yhirose/cpp-httplib
- whisper.cpp / whisper-server (optional, when `whisperServerPath` points at Homebrew's `whisper-cpp`) — MIT License — https://github.com/ggml-org/whisper.cpp
- NVIDIA Parakeet TDT 0.6B v2/v3 and Parakeet CTC 110M (CoreML conversions by FluidInference) — CC-BY-4.0 (NVIDIA)
- Qwen3.5 (GGUF builds by Unsloth) — Apache License 2.0 — https://huggingface.co/Qwen
- OpenAI Whisper large v3 turbo (GGUF Q8_0 build by handy-computer) — MIT License — https://github.com/openai/whisper
- SenseVoiceSmall (FunAudioLLM; CoreML conversion by FluidInference) — model license at https://huggingface.co/FunAudioLLM/SenseVoiceSmall
- Silero VAD v6 (CoreML conversion by FluidInference, loaded through FluidAudio) — MIT License — https://github.com/snakers4/silero-vad

Parts of the meeting pipeline are adapted from Meeting-Assistant (https://github.com/Rahulk644/Meeting-Assitant),
MIT License, Copyright (c) 2026 Rahul Khatri: the transcribe-server HTTP wrapper (`sidecar/transcribe-server/transcribe-server.cpp`), the Zoom Accessibility speaker reader
(`sidecar/openflow-helper/Sources/openflow-helper/ZoomSpeakers.swift`) and the interval-overlap speaker
attribution (`crates/openflow-core/src/speakers.rs`). The MIT license requires this copyright notice and
permission notice to accompany those parts:

> Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
> documentation files (the "Software"), to deal in the Software without restriction, including without limitation
> the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and
> to permit persons to whom the Software is furnished to do so, subject to the following conditions: The above
> copyright notice and this permission notice shall be included in all copies or substantial portions of the
> Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED.

Optional cloud processing uses Groq's API under Groq's terms, with your own API key. Nothing is sent unless you turn it on.
