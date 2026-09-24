import FluidAudio
import Foundation

/// Silero VAD (Core ML, via FluidAudio) as a gate before speech recognition: an utterance the energy segmenter let
/// through, in which Silero hears no voice at all (a door, typing, music, line noise), is never sent to a
/// recogniser that would invent words for it. Gate, not compaction: the clip itself is never cut, so onsets
/// and pauses stay intact (Meeting-Assistant's benchmark: compacting hurt every non-Whisper model).
actor Vad {
    private var manager: VadManager?
    private var failed = false

    /// Per 256 ms chunk speech probabilities. Throws when the model can't be loaded; callers fail open.
    func probabilities(_ samples: [Float]) async throws -> [Float] {
        if manager == nil {
            if failed { throw HelperError("VAD model unavailable") }
            do {
                manager = try await VadManager(config: VadConfig(defaultThreshold: 0.5))
                helperLog("info", "Silero VAD loaded")
            } catch {
                failed = true
                throw error
            }
        }
        return try await manager!.process(samples).map(\.probability)
    }
}
