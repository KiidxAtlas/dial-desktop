import Darwin
import Foundation

/// Limits shared by the stdio QMP subset and the guest wire. The guest limits mirror
/// `Wire` in `Guest.swift` (the companion); they are duplicated here, not imported.
enum HostLimits {
    /// Vsock port the guest companion listens on.
    static let guestPort: UInt32 = 49152
    /// Largest request the companion accepts.
    static let guestRequestBytes = 64 * 1024
    /// Largest screendump reply, status byte included (matches `Wire.maxReplyBytes`).
    static let maxScreendumpBytes = 64 * 1024 * 1024
    /// Largest error message, status byte excluded (matches `Wire.maxErrorBytes`).
    static let maxGuestErrorBytes = 512
    /// Largest stdio QMP request line the helper reads.
    static let qmpLineBytes = 128 * 1024
    static let maxInputEvents = 512
    static let inputEventTypes: Set<String> = ["abs", "btn", "key"]
}

/// A stdio request the helper refuses. The text is sent back to the runner as a QMP error.
struct QMPRefusal: Error {
    let description: String
}

/// The QMP subset the Rust runner uses: `qmp_capabilities`, `screendump` and `input-send-event`.
enum QMPCommand {
    case capabilities
    case screendump(filename: String)
    case inputSendEvent(events: [[String: Any]])
}

/// Parse one QMP request line. Anything outside the subset is refused.
func parseQMPCommand(_ line: Data) throws -> QMPCommand {
    guard let object = try? JSONSerialization.jsonObject(with: line),
        let request = object as? [String: Any],
        let execute = request["execute"] as? String
    else {
        throw QMPRefusal(description: "malformed request")
    }
    let arguments: [String: Any]
    if let raw = request["arguments"] {
        guard let dictionary = raw as? [String: Any] else {
            throw QMPRefusal(description: "arguments must be an object")
        }
        arguments = dictionary
    } else {
        arguments = [:]
    }
    switch execute {
    case "qmp_capabilities":
        return .capabilities
    case "screendump":
        guard let filename = arguments["filename"] as? String, !filename.isEmpty else {
            throw QMPRefusal(description: "screendump needs a filename")
        }
        return .screendump(filename: filename)
    case "input-send-event":
        guard let events = arguments["events"] as? [Any], !events.isEmpty,
            events.count <= HostLimits.maxInputEvents
        else {
            throw QMPRefusal(description: "input-send-event needs between 1 and \(HostLimits.maxInputEvents) events")
        }
        var checked: [[String: Any]] = []
        for event in events {
            guard let object = event as? [String: Any],
                let type = object["type"] as? String,
                HostLimits.inputEventTypes.contains(type)
            else {
                throw QMPRefusal(description: "input event is not one this helper accepts")
            }
            checked.append(object)
        }
        return .inputSendEvent(events: checked)
    default:
        throw QMPRefusal(description: "unsupported command")
    }
}

/// One compact JSON object followed by a newline, as the runner's QMP reader expects.
func jsonLine(_ object: [String: Any]) -> Data {
    var data = (try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]))
        ?? Data(#"{"error":{"class":"GenericError","desc":"internal encoding failure"}}"#.utf8)
    data.append(0x0A)
    return data
}

func qmpReturn() -> Data {
    jsonLine(["return": [String: Any]()])
}

func qmpError(_ description: String) -> Data {
    jsonLine(["error": ["class": "GenericError", "desc": description]])
}

/// The greeting the runner reads before it sends `qmp_capabilities`.
func qmpGreeting() -> Data {
    jsonLine([
        "QMP": [
            "version": ["qemu": ["micro": 0, "minor": 0, "major": 0], "package": "dial-macos-vz"],
            "capabilities": [String](),
        ]
    ])
}

/// Four bytes, big-endian, as the guest wire's length prefix.
func bigEndianLength(_ value: Int) -> Data {
    var big = UInt32(value).bigEndian
    return Data(bytes: &big, count: MemoryLayout<UInt32>.size)
}

/// A guest request frame. The body is built from fixed command names and validated events only;
/// no host path is ever included.
func guestRequestFrame(_ body: [String: Any]) throws -> Data {
    let json = try JSONSerialization.data(withJSONObject: body, options: [.sortedKeys])
    guard json.count > 0, json.count <= HostLimits.guestRequestBytes else {
        throw GuestTransportError(message: "the request for the guest is too large")
    }
    return bigEndianLength(json.count) + json
}

/// True if `data` starts with a binary P6 PPM header (`P6` then whitespace).
func isPPM(_ data: Data) -> Bool {
    let head = [UInt8](data.prefix(3))
    guard head.count == 3, head[0] == 0x50, head[1] == 0x36 else {
        return false
    }
    return [0x0A, 0x20, 0x09, 0x0D].contains(head[2])
}

/// Where a screendump may be written: an absolute `.ppm` path directly inside the run directory.
/// The path comes from the runner's stdio, never from the guest.
func screendumpDestination(filename: String, runDirectory: URL) throws -> URL {
    guard filename.hasPrefix("/"), !filename.contains("\0") else {
        throw QMPRefusal(description: "screendump needs an absolute path")
    }
    let destination = URL(fileURLWithPath: filename).standardizedFileURL
    let parent = destination.deletingLastPathComponent().resolvingSymlinksInPath().path
    guard parent == runDirectory.resolvingSymlinksInPath().path,
        destination.pathExtension.lowercased() == "ppm"
    else {
        throw QMPRefusal(description: "screendump must write a .ppm file directly inside the run directory")
    }
    if let type = fileType(destination.path, follow: false), type != S_IFREG {
        throw QMPRefusal(description: "the screendump destination is not a regular file")
    }
    return destination
}
