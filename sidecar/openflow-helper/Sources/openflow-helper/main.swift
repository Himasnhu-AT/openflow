import Foundation

// openflow-helper: reads request frames from stdin, answers on stdout (see Protocol.swift). Exits at stdin EOF.
// `--version` prints a version line and exits (sanity check for the bundle).

if CommandLine.arguments.contains("--version") {
    print("openflow-helper 1")
    exit(0)
}

Output.shared.takeStdout()
signal(SIGPIPE, SIG_IGN)

let transcriber = Transcriber()
let senseVoice = SenseVoice()
let vad = Vad()
/// Owned by the main queue (the default-output listener is also delivered there).
var tap: SystemAudioTap?

func handle(_ frame: Frame) async {
    guard let id = frame.header["id"] else { return }   // the helper receives no pushes
    let method = frame.header["method"] as? String ?? ""
    let params = frame.header["params"] as? [String: Any] ?? [:]
    do {
        let result = try await dispatch(method, params, frame.audio)
        Output.shared.respond(id, result: result)
    } catch {
        Output.shared.respond(id, error: error.localizedDescription)
    }
}

func dispatch(_ method: String, _ params: [String: Any], _ audio: [Float]) async throws -> [String: Any] {
    switch method {
    case "ping":
        return [:]
    case "load":
        try await transcriber.load(version: params["version"] as? String ?? "v2")
        return [:]
    case "transcribe":
        let text = try await transcriber.transcribe(audio, rescore: params["rescore"] as? Bool ?? true)
        return ["text": text]
    case "setVocabulary":
        await transcriber.setVocabulary(params["terms"] as? [String] ?? [])
        return [:]
    case "senseVoice":
        return ["text": try await senseVoice.transcribe(audio)]
    case "tapStart":
        let bundle = params["bundle"] as? String ?? ""
        try await onMain {
            tap?.stop()
            let t = SystemAudioTap()
            t.appBundleID = bundle
            t.onChunk = { Output.shared.push("tapAudio", audio: $0) }
            t.onRestart = { Output.shared.push("tapRestarted") }
            do { try t.start() } catch { t.stop(); tap = nil; throw error }
            tap = t
        }
        return [:]
    case "tapStop":
        try await onMain { tap?.stop(); tap = nil }
        return [:]
    case "tapProcesses":
        return ["count": SystemAudioTap.audioProcesses(matching: params["match"] as? String ?? "").count]
    case "appAudio":
        let a = SystemAudioTap.audioActivity(matching: params["match"] as? String ?? "")
        return ["input": a.input, "output": a.output]
    case "vad":
        return ["probabilities": try await vad.probabilities(audio)]
    case "zoomSpeakers":
        let snap = await withCheckedContinuation { (c: CheckedContinuation<ZoomSpeakers.Snapshot?, Never>) in
            DispatchQueue.global(qos: .utility).async { c.resume(returning: ZoomSpeakers.snapshot()) }
        }
        guard let snap else { return ["running": false] }
        var out: [String: Any] = ["running": true, "participants": snap.participants, "activeSource": snap.activeSource]
        if let a = snap.active { out["active"] = a }
        if let me = snap.selfName { out["self"] = me }
        return out
    case "names":
        return ["names": await NameSources.names()]
    default:
        throw HelperError("unknown method \(method)")
    }
}

func onMain(_ body: @escaping () throws -> Void) async throws {
    try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
        DispatchQueue.main.async {
            do { try body(); cont.resume() } catch { cont.resume(throwing: error) }
        }
    }
}

let reader = Thread {
    while true {
        let frame: Frame?
        do { frame = try FrameIO.read(STDIN_FILENO) } catch {
            helperLog("error", "bad frame: \(error.localizedDescription)")
            exit(1)
        }
        guard let frame else {
            // Parent closed stdin: tear the tap down (it holds an aggregate device) and exit.
            DispatchQueue.main.async { tap?.stop(); exit(0) }
            return
        }
        Task { await handle(frame) }
    }
}
reader.start()
dispatchMain()
