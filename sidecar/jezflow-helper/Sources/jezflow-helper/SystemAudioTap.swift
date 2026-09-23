import AVFoundation
import CoreAudio

/// 16 kHz mono Float32, non-interleaved (AudioRecorder.targetFormat in the Swift app).
let targetFormat = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 16_000, channels: 1, interleaved: false)!

/// Converts any incoming PCM buffer to 16 kHz mono Float32 samples.
final class Resampler {
    private let converter: AVAudioConverter?
    private let ratio: Double
    init(from format: AVAudioFormat) {
        converter = AVAudioConverter(from: format, to: targetFormat)
        ratio = targetFormat.sampleRate / format.sampleRate
    }
    func convert(_ buffer: AVAudioPCMBuffer) -> [Float] {
        guard let converter, buffer.frameLength > 0 else { return [] }
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 16
        guard let out = AVAudioPCMBuffer(pcmFormat: targetFormat, frameCapacity: capacity) else { return [] }
        var consumed = false
        var error: NSError?
        converter.convert(to: out, error: &error) { _, status in
            if consumed { status.pointee = .noDataNow; return nil }
            consumed = true; status.pointee = .haveData; return buffer
        }
        guard error == nil, let ch = out.floatChannelData else { return [] }
        return Array(UnsafeBufferPointer(start: ch[0], count: Int(out.frameLength)))
    }
}

/// Captures what the Mac is playing (the other side of a call) with a Core Audio process tap. macOS asks for
/// "System Audio Recording" the first time; no virtual audio driver is needed. Ported from MeetingAudio.swift.
/// The IOProc delivers ~150 samples at a time; they are batched to ~100 ms before `onChunk` to keep the
/// per-frame protocol overhead low.
final class SystemAudioTap {
    private var tapID = AudioObjectID(kAudioObjectUnknown)
    private var aggregateID = AudioObjectID(kAudioObjectUnknown)
    private var procID: AudioDeviceIOProcID?
    private var format: AVAudioFormat?
    private var resampler: Resampler?
    var onChunk: (([Float]) -> Void)?
    /// Called after the tap was rebuilt because the default output device changed.
    var onRestart: (() -> Void)?
    private var outputListener: AudioObjectPropertyListenerBlock?
    private var listening = false
    /// Bundle id prefix of the call app (e.g. "us.zoom.xos", "com.google.Chrome"). When set and its audio
    /// processes are found, only their output is tapped, so music or a video playing elsewhere stays out of the
    /// transcript. Empty = everything the Mac plays.
    var appBundleID = ""

    static let batchSamples = 1600   // 100 ms at 16 kHz
    private var pending: [Float] = []
    private let pendingLock = NSLock()
    private let deliver = DispatchQueue(label: "com.jez.flow.helper.tap")

    func start() throws {
        try startTap()
        // When the default output changes (AirPods connect mid-call), the aggregate device keeps following the old
        // output and the far side goes silent: rebuild on change.
        if !listening {
            var addr = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDefaultOutputDevice, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
            let block: AudioObjectPropertyListenerBlock = { [weak self] _, _ in
                guard let self else { return }
                helperLog("info", "Default output changed; rebuilding the system audio tap")
                self.stopTap()
                do { try self.startTap(); self.onRestart?() } catch { helperLog("error", error.localizedDescription) }
            }
            outputListener = block
            AudioObjectAddPropertyListenerBlock(AudioObjectID(kAudioObjectSystemObject), &addr, DispatchQueue.main, block)
            listening = true
        }
    }

    /// Core Audio process objects whose bundle id starts with `prefix` (browsers play audio from helper processes,
    /// whose bundle ids extend the browser's).
    static func audioProcesses(matching prefix: String) -> [AudioObjectID] {
        var addr = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyProcessObjectList, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size) == noErr else { return [] }
        var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
        guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size, &ids) == noErr else { return [] }
        let root = prefix.split(separator: ".").prefix(3).joined(separator: ".")     // com.google.Chrome.helper -> com.google.Chrome
        return ids.filter { id in
            var baddr = AudioObjectPropertyAddress(mSelector: kAudioProcessPropertyBundleID, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
            var value: Unmanaged<CFString>?
            var bsize = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
            guard AudioObjectGetPropertyData(id, &baddr, 0, nil, &bsize, &value) == noErr, let value else { return false }
            let bid = value.takeRetainedValue() as String
            return bid == prefix || bid.hasPrefix(prefix + ".") || (!root.isEmpty && bid.hasPrefix(root))
        }
    }

    private func startTap() throws {
        let processes = appBundleID.isEmpty ? [] : Self.audioProcesses(matching: appBundleID)
        let desc = processes.isEmpty ? CATapDescription(stereoGlobalTapButExcludeProcesses: []) : CATapDescription(stereoMixdownOfProcesses: processes)
        if !processes.isEmpty { helperLog("info", "Tapping \(processes.count) audio process(es) of \(appBundleID)") }
        else if !appBundleID.isEmpty { helperLog("info", "No audio processes found for \(appBundleID); tapping all system audio") }
        desc.uuid = UUID()
        desc.name = "JezFlow meeting"
        desc.isPrivate = true
        desc.muteBehavior = .unmuted
        var tap = AudioObjectID(kAudioObjectUnknown)
        try check(AudioHardwareCreateProcessTap(desc, &tap), "create tap (System Audio Recording permission?)")
        tapID = tap

        let aggDesc: [String: Any] = [
            kAudioAggregateDeviceNameKey: "JezFlow Meeting",
            kAudioAggregateDeviceUIDKey: "com.jez.flow.meeting." + UUID().uuidString,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceIsStackedKey: false,
            kAudioAggregateDeviceTapAutoStartKey: true,
            kAudioAggregateDeviceSubDeviceListKey: [[kAudioSubDeviceUIDKey: try Self.defaultOutputUID()]],
            kAudioAggregateDeviceTapListKey: [[kAudioSubTapDriftCompensationKey: true, kAudioSubTapUIDKey: desc.uuid.uuidString]],
        ]
        var agg = AudioObjectID(kAudioObjectUnknown)
        try check(AudioHardwareCreateAggregateDevice(aggDesc as CFDictionary, &agg), "create aggregate device")
        aggregateID = agg

        var addr = AudioObjectPropertyAddress(mSelector: kAudioTapPropertyFormat, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var asbd = AudioStreamBasicDescription()
        var size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
        try check(AudioObjectGetPropertyData(tapID, &addr, 0, nil, &size, &asbd), "read tap format")
        guard let fmt = AVAudioFormat(streamDescription: &asbd) else { throw Self.error("tap format") }
        format = fmt
        resampler = Resampler(from: fmt)

        try check(AudioDeviceCreateIOProcIDWithBlock(&procID, aggregateID, nil) { [weak self] _, input, _, _, _ in
            self?.handle(input)
        }, "create io proc")
        try check(AudioDeviceStart(aggregateID, procID), "start aggregate device")
        helperLog("info", "System audio tap running at \(Int(fmt.sampleRate)) Hz, \(fmt.channelCount) ch")
    }

    private func handle(_ list: UnsafePointer<AudioBufferList>) {
        guard let format, let buf = AVAudioPCMBuffer(pcmFormat: format, bufferListNoCopy: list, deallocator: nil),
              let samples = resampler?.convert(buf), !samples.isEmpty else { return }
        pendingLock.lock()
        pending.append(contentsOf: samples)
        var batch: [Float] = []
        if pending.count >= Self.batchSamples { batch = pending; pending.removeAll(keepingCapacity: true) }
        pendingLock.unlock()
        if !batch.isEmpty { deliver.async { [weak self] in self?.onChunk?(batch) } }
    }

    private func flushPending() {
        pendingLock.lock()
        let batch = pending
        pending.removeAll()
        pendingLock.unlock()
        if !batch.isEmpty { deliver.async { [weak self] in self?.onChunk?(batch) } }
    }

    func stop() {
        if listening, let block = outputListener {
            var addr = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDefaultOutputDevice, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
            AudioObjectRemovePropertyListenerBlock(AudioObjectID(kAudioObjectSystemObject), &addr, DispatchQueue.main, block)
            listening = false
        }
        stopTap()
        flushPending()
        deliver.sync {}   // every chunk is out before tapStop answers
    }

    private func stopTap() {
        if let procID, aggregateID != kAudioObjectUnknown {
            AudioDeviceStop(aggregateID, procID)
            AudioDeviceDestroyIOProcID(aggregateID, procID)
        }
        procID = nil
        if aggregateID != kAudioObjectUnknown { AudioHardwareDestroyAggregateDevice(aggregateID); aggregateID = kAudioObjectUnknown }
        if tapID != kAudioObjectUnknown { AudioHardwareDestroyProcessTap(tapID); tapID = kAudioObjectUnknown }
    }

    private static func defaultOutputUID() throws -> String {
        var dev = AudioObjectID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        var addr = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDefaultOutputDevice, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size, &dev) == noErr else { throw error("default output device") }
        var uid: Unmanaged<CFString>?
        size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
        addr.mSelector = kAudioDevicePropertyDeviceUID
        guard AudioObjectGetPropertyData(dev, &addr, 0, nil, &size, &uid) == noErr, let uid else { throw error("output device uid") }
        return uid.takeRetainedValue() as String
    }

    private func check(_ status: OSStatus, _ what: String) throws {
        if status != noErr { throw Self.error("\(what) failed (\(status))") }
    }
    private static func error(_ m: String) -> NSError { NSError(domain: "JezFlow", code: 7, userInfo: [NSLocalizedDescriptionKey: "System audio: \(m)"]) }
}
