import Darwin
import Foundation

/// Framed stdio protocol (docs/ARCHITECTURE.md "Sidecar protocol"). Every frame, in both directions:
/// u32 LE length of the JSON header, the UTF-8 JSON header, then, if the header has `"audio": n`,
/// n × 4 bytes of f32 LE samples (16 kHz mono).
struct Frame {
    var header: [String: Any]
    var audio: [Float]
}

enum FrameIO {
    /// Reads exactly `count` bytes from `fd`. Nil on EOF or error.
    static func readExact(_ fd: Int32, _ count: Int) -> Data? {
        if count == 0 { return Data() }
        var data = Data(count: count)
        var got = 0
        let ok = data.withUnsafeMutableBytes { (raw: UnsafeMutableRawBufferPointer) -> Bool in
            guard let base = raw.baseAddress else { return false }
            while got < count {
                let n = Darwin.read(fd, base + got, count - got)
                if n > 0 { got += n; continue }
                if n < 0, errno == EINTR { continue }
                return false
            }
            return true
        }
        return ok ? data : nil
    }

    /// Next frame from `fd`, nil at EOF. Throws on a malformed header.
    static func read(_ fd: Int32) throws -> Frame? {
        guard let lenData = readExact(fd, 4) else { return nil }
        let len = lenData.withUnsafeBytes { Int(UInt32(littleEndian: $0.loadUnaligned(as: UInt32.self))) }
        guard len > 0, len < 64 << 20, let headerData = readExact(fd, len) else { return nil }
        guard let header = try JSONSerialization.jsonObject(with: headerData) as? [String: Any] else {
            throw HelperError("frame header is not a JSON object")
        }
        var audio: [Float] = []
        let n = (header["audio"] as? NSNumber)?.intValue ?? 0
        if n > 0 {
            guard let bytes = readExact(fd, n * 4) else { return nil }
            audio = [Float](repeating: 0, count: n)
            audio.withUnsafeMutableBytes { dst in bytes.withUnsafeBytes { src in dst.copyMemory(from: src) } }
            if Int(littleEndian: 1) != 1 {   // big-endian host: never on Apple silicon/Intel, kept for correctness
                audio = audio.map { Float(bitPattern: UInt32(littleEndian: $0.bitPattern)) }
            }
        }
        return Frame(header: header, audio: audio)
    }

    static func encode(_ header: [String: Any], audio: [Float] = []) -> Data {
        var h = header
        if !audio.isEmpty { h["audio"] = audio.count }
        let json = (try? JSONSerialization.data(withJSONObject: h)) ?? Data("{}".utf8)
        var out = Data(capacity: 4 + json.count + audio.count * 4)
        var len = UInt32(json.count).littleEndian
        withUnsafeBytes(of: &len) { out.append(contentsOf: $0) }
        out.append(json)
        audio.withUnsafeBytes { out.append(contentsOf: $0) }
        return out
    }
}

/// The only writer of protocol frames. stdout is moved to a private descriptor at startup and fd 1 is pointed at
/// stderr, so nothing printed by us or a library can corrupt the stream. Writes are serialized with a lock.
final class Output {
    static let shared = Output()
    private let lock = NSLock()
    private var fd: Int32 = -1

    /// Call once, before anything else writes.
    func takeStdout() {
        fflush(stdout)
        fd = dup(STDOUT_FILENO)
        dup2(STDERR_FILENO, STDOUT_FILENO)
    }

    func send(_ header: [String: Any], audio: [Float] = []) {
        let data = FrameIO.encode(header, audio: audio)
        lock.lock(); defer { lock.unlock() }
        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            guard let base = raw.baseAddress else { return }
            var off = 0
            while off < raw.count {
                let n = Darwin.write(fd, base + off, raw.count - off)
                if n > 0 { off += n; continue }
                if n < 0, errno == EINTR { continue }
                exit(0)   // parent went away
            }
        }
    }

    func respond(_ id: Any, result: [String: Any]) { send(["id": id, "result": result]) }
    func respond(_ id: Any, error: String) { send(["id": id, "error": error]) }
    func push(_ event: String, _ extra: [String: Any] = [:], audio: [Float] = []) {
        var h = extra
        h["event"] = event
        send(h, audio: audio)
    }
}

/// Logs as a `log` push (the Rust side forwards it to tracing). stderr stays free for crashes and library noise.
func helperLog(_ level: String, _ message: String) {
    Output.shared.push("log", ["level": level, "message": message])
}

struct HelperError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}
