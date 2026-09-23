import FluidAudio
import Foundation

/// Parakeet TDT (NVIDIA) on CoreML via FluidAudio, ported from the Swift app's Transcriber. Live re-decode and
/// the dictionary/extra-name merge moved to Rust (`openflow_engines::live`); this actor only loads, decodes and boosts.
actor Transcriber {
    private var manager: AsrManager?
    private var loadedVersion: String?
    private var ctcModels: CtcModels?
    private var boosting: VocabularyBoostingSession?
    private var boostedTerms: [String] = []

    func load(version: String) async throws {
        if manager != nil, loadedVersion == version { return }
        let v: AsrModelVersion = version == "v3" ? .v3 : .v2
        let models = try await AsrModels.downloadAndLoad(version: v)
        let m = AsrManager(config: .default)
        try await m.loadModels(models)
        manager = m
        loadedVersion = version
        helperLog("info", "Parakeet \(version) loaded")
    }

    /// `terms` are already merged, trimmed and filtered to >= 4 characters by the caller. Empty = boosting off.
    /// No-op when unchanged. The similarity bar is high: with the default it rewrote "kind regards" as
    /// "Zentro regards".
    func setVocabulary(_ terms: [String]) async {
        let cleaned = terms.map { $0.trimmingCharacters(in: .whitespaces) }.filter { $0.count >= 4 }
        guard cleaned != boostedTerms else { return }
        boostedTerms = cleaned
        boosting = nil
        guard !cleaned.isEmpty else { return }
        do {
            if ctcModels == nil { ctcModels = try await CtcModels.downloadAndLoad() }
            let context = CustomVocabularyContext(terms: cleaned.map { CustomVocabularyTerm(text: $0) }, minSimilarity: 0.75, minTermLength: 4)
            // The acoustic "rescue" pass over-fires on small dictionaries (it turned "hey so" into "OpenFlow");
            // FluidAudio recommends it off for short keyword lists. String-similarity boosting still runs.
            boosting = try await VocabularyBoostingSession(vocabulary: context, ctcModels: ctcModels!,
                                                           config: .init(spotterRescueEnabled: false))
            helperLog("info", "Vocabulary boosting on for \(cleaned.count) terms")
        } catch {
            helperLog("error", "Vocabulary boosting unavailable: \(error.localizedDescription)")
        }
    }

    /// One decode with a fresh decoder state. `rescore` applies vocabulary boosting when a vocabulary is set.
    func transcribe(_ samples: [Float], rescore: Bool) async throws -> String {
        guard let manager else { throw HelperError("Speech model still loading") }
        var state = try TdtDecoderState(decoderLayers: await manager.decoderLayerCount)
        let result = try await manager.transcribe(samples, decoderState: &state)
        var text = result.text
        if rescore, let boosting, let timings = result.tokenTimings,
           let out = await boosting.rescore(text: text, tokenTimings: timings, audioSamples: samples), out.wasModified {
            helperLog("info", "Vocabulary: \(text) -> \(out.text)")
            text = out.text
        }
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

/// SenseVoice (multilingual fallback for meetings), loaded lazily on first use.
actor SenseVoice {
    private var sense: SenseVoiceManager?

    func transcribe(_ samples: [Float]) async throws -> String {
        if sense == nil { sense = try await SenseVoiceManager.load() }
        return try await sense!.transcribe(audio: samples).trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
