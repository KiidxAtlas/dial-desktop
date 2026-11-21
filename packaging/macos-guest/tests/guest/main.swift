// Guest-only host-runnable checks for the pure parts of Guest.swift: request and gesture validation,
// the key table, coordinate mapping, PPM encoding, and the framed serve loop over a socketpair with a
// fake backend. Nothing here touches the VM, vsock, Screen Recording or event posting.
//
// Run from packaging/macos-guest:
//   swiftc -swift-version 5 -D DIAL_GUEST_TESTS Guest.swift tests/guest/main.swift -o /tmp/dial-guest-tests
//   /tmp/dial-guest-tests
// (tests/run.sh builds the host helper suite in tests/main.swift; it is separate from this file.)

import Foundation

#if canImport(Darwin)
import Darwin
#endif

var checks = 0
var failures: [String] = []

func check(_ condition: @autoclosure () -> Bool, _ label: String) {
    checks += 1
    if !condition() {
        failures.append(label)
    }
}

func jsonBody(_ object: Any) -> [UInt8] {
    Array((try? JSONSerialization.data(withJSONObject: object)) ?? Data())
}

func keyEvent(_ qcode: String, down: Bool) -> [String: Any] {
    ["type": "key", "data": ["down": down, "key": ["type": "qcode", "data": qcode]]]
}

func clickEvents(x: Int, y: Int, button: String) -> [Any] {
    [
        ["type": "abs", "data": ["axis": "x", "value": x]],
        ["type": "abs", "data": ["axis": "y", "value": y]],
        ["type": "btn", "data": ["button": button, "down": true]],
        ["type": "btn", "data": ["button": button, "down": false]],
    ]
}

/// The event order `input.rs` produces: modifier downs, key down and up, modifier ups in reverse.
func pressEvents(key: String, modifiers: [String]) -> [Any] {
    var events: [Any] = modifiers.map { keyEvent($0, down: true) }
    events.append(keyEvent(key, down: true))
    events.append(keyEvent(key, down: false))
    events += modifiers.reversed().map { keyEvent($0, down: false) }
    return events
}

func inputBody(_ events: [Any]) -> [UInt8] {
    jsonBody(["execute": "input-send-event", "arguments": ["events": events]])
}

func frame(_ body: [UInt8]) -> [UInt8] {
    bigEndian32(body.count) + body
}

// MARK: - Request parsing

struct RequestCase {
    let name: String
    let body: [UInt8]
    let expected: Request?
}

let requestCases: [RequestCase] = [
    RequestCase(name: "screendump", body: jsonBody(["execute": "screendump"]), expected: .screendump),
    RequestCase(
        name: "screendump with empty arguments",
        body: jsonBody(["execute": "screendump", "arguments": [String: Any]()]),
        expected: .screendump),
    RequestCase(
        name: "click at the far corner",
        body: inputBody(clickEvents(x: 32767, y: 0, button: "left")),
        expected: .input(.click(x: 32767, y: 0, button: .left))),
    RequestCase(
        name: "right click",
        body: inputBody(clickEvents(x: 1, y: 2, button: "right")),
        expected: .input(.click(x: 1, y: 2, button: .right))),
    RequestCase(
        name: "middle click",
        body: inputBody(clickEvents(x: 1, y: 2, button: "middle")),
        expected: .input(.click(x: 1, y: 2, button: .middle))),
    RequestCase(
        name: "plain key",
        body: inputBody(pressEvents(key: "a", modifiers: [])),
        expected: .input(.press(modifiers: [], key: "a"))),
    RequestCase(
        name: "right shift with left shift held",
        body: inputBody(pressEvents(key: "shift_r", modifiers: ["shift"])),
        expected: .input(.press(modifiers: ["shift"], key: "shift_r"))),
    RequestCase(
        name: "shift on its own",
        body: inputBody(pressEvents(key: "shift", modifiers: [])),
        expected: .input(.press(modifiers: [], key: "shift"))),
    RequestCase(
        name: "all four modifiers",
        body: inputBody(pressEvents(key: "f12", modifiers: ["shift", "ctrl", "alt", "meta_l"])),
        expected: .input(.press(modifiers: ["shift", "ctrl", "alt", "meta_l"], key: "f12"))),
    RequestCase(name: "unknown command", body: jsonBody(["execute": "query-status"]), expected: nil),
    RequestCase(
        name: "screendump with arguments",
        body: jsonBody(["execute": "screendump", "arguments": ["filename": "/tmp/x"]]),
        expected: nil),
    RequestCase(
        name: "unknown top-level field",
        body: jsonBody(["execute": "screendump", "id": 1]),
        expected: nil),
    RequestCase(name: "not JSON", body: Array("screendump".utf8), expected: nil),
    RequestCase(name: "JSON array", body: jsonBody(["screendump"]), expected: nil),
    RequestCase(
        name: "pointer value above the axis range",
        body: inputBody(clickEvents(x: 32768, y: 0, button: "left")),
        expected: nil),
    RequestCase(
        name: "pointer value is a boolean",
        body: inputBody([
            ["type": "abs", "data": ["axis": "x", "value": true]],
            ["type": "abs", "data": ["axis": "y", "value": 0]],
            ["type": "btn", "data": ["button": "left", "down": true]],
            ["type": "btn", "data": ["button": "left", "down": false]],
        ]),
        expected: nil),
    RequestCase(
        name: "pointer value is fractional",
        body: inputBody([
            ["type": "abs", "data": ["axis": "x", "value": 1.5]],
            ["type": "abs", "data": ["axis": "y", "value": 0]],
            ["type": "btn", "data": ["button": "left", "down": true]],
            ["type": "btn", "data": ["button": "left", "down": false]],
        ]),
        expected: nil),
    RequestCase(
        name: "unknown axis",
        body: inputBody([
            ["type": "abs", "data": ["axis": "z", "value": 1]],
            ["type": "abs", "data": ["axis": "y", "value": 1]],
            ["type": "btn", "data": ["button": "left", "down": true]],
            ["type": "btn", "data": ["button": "left", "down": false]],
        ]),
        expected: nil),
    RequestCase(
        name: "unknown button",
        body: inputBody(clickEvents(x: 1, y: 1, button: "back")),
        expected: nil),
    RequestCase(
        name: "release of a different button",
        body: inputBody([
            ["type": "abs", "data": ["axis": "x", "value": 1]],
            ["type": "abs", "data": ["axis": "y", "value": 1]],
            ["type": "btn", "data": ["button": "left", "down": true]],
            ["type": "btn", "data": ["button": "right", "down": false]],
        ]),
        expected: nil),
    RequestCase(
        name: "click without release",
        body: inputBody(Array(clickEvents(x: 1, y: 1, button: "left").prefix(3))),
        expected: nil),
    RequestCase(
        name: "unsupported physical key",
        body: inputBody(pressEvents(key: "insert", modifiers: [])),
        expected: nil),
    RequestCase(
        name: "duplicate modifier",
        body: inputBody(pressEvents(key: "a", modifiers: ["shift", "shift"])),
        expected: nil),
    RequestCase(
        name: "unknown modifier",
        body: inputBody(pressEvents(key: "a", modifiers: ["shift_r"])),
        expected: nil),
    RequestCase(
        name: "key equal to a held modifier",
        body: inputBody(pressEvents(key: "shift", modifiers: ["shift"])),
        expected: nil),
    RequestCase(
        name: "modifiers released out of order",
        body: inputBody([
            keyEvent("shift", down: true), keyEvent("ctrl", down: true),
            keyEvent("a", down: true), keyEvent("a", down: false),
            keyEvent("shift", down: false), keyEvent("ctrl", down: false),
        ]),
        expected: nil),
    RequestCase(
        name: "key never released",
        body: inputBody([keyEvent("a", down: true), keyEvent("b", down: false)]),
        expected: nil),
    RequestCase(
        name: "empty events list",
        body: inputBody([]),
        expected: nil),
    RequestCase(
        name: "odd event count",
        body: inputBody([keyEvent("a", down: true), keyEvent("a", down: false), keyEvent("b", down: true)]),
        expected: nil),
    RequestCase(
        name: "more events than the limit",
        body: inputBody(Array(repeating: keyEvent("a", down: true), count: 17)),
        expected: nil),
    RequestCase(
        name: "extra field in an event",
        body: inputBody([
            ["type": "key", "extra": 1, "data": ["down": true, "key": ["type": "qcode", "data": "a"]]],
        ]),
        expected: nil),
    RequestCase(
        name: "unknown event type",
        body: inputBody([["type": "wheel", "data": [String: Any]()]]),
        expected: nil),
]

for testCase in requestCases {
    do {
        let parsed = try parseRequest(testCase.body)
        check(testCase.expected == parsed, "request: \(testCase.name) parsed as \(parsed)")
    } catch {
        check(testCase.expected == nil, "request: \(testCase.name) was refused: \(error)")
    }
}

// MARK: - Key table

let hostQcodes: [String] =
    (0..<26).map { String(UnicodeScalar(UInt8(ascii: "a") + UInt8($0))) }
    + (0..<10).map { String($0) }
    + (1...12).map { "f\($0)" }
    + [
        "ret", "esc", "backspace", "tab", "spc", "minus", "equal", "bracket_left",
        "bracket_right", "backslash", "semicolon", "apostrophe", "grave_accent", "comma",
        "dot", "slash", "up", "down", "left", "right", "home", "end", "pgup", "pgdn", "delete",
        "shift", "shift_r", "ctrl", "ctrl_r", "alt", "alt_r", "meta_l", "meta_r", "caps_lock",
    ]
for qcode in hostQcodes {
    check(keyCodes[qcode] != nil, "key table: \(qcode) has a macOS key code")
}
check(keyCodes["insert"] == nil, "key table: insert is refused")
let knownKeyCodes: [(String, UInt16)] = [
    ("a", 0x00), ("0", 0x1D), ("1", 0x12), ("f1", 0x7A), ("ret", 0x24), ("meta_l", 0x37), ("shift_r", 0x3C),
]
for (qcode, code) in knownKeyCodes {
    check(keyCodes[qcode] == code, "key table: \(qcode) is 0x\(String(code, radix: 16))")
}

// MARK: - Coordinates and PPM

func near(_ a: Double, _ b: Double) -> Bool { abs(a - b) < 0.5 }
check(pointCoordinate(value: 0, origin: 10, extent: 1000) == 10, "coordinate: zero is the origin")
check(pointCoordinate(value: 32767, origin: 10, extent: 1000) == 1009, "coordinate: full range stays inside the display")
check(near(pointCoordinate(value: 16384, origin: 0, extent: 1440), 720), "coordinate: midpoint")

do {
    let ppm = try encodePPM(width: 2, height: 1, rgbx: [1, 2, 3, 0, 4, 5, 6, 255])
    check(ppm == Array("P6\n2 1\n255\n".utf8) + [1, 2, 3, 4, 5, 6], "ppm: header and RGB bytes, alpha dropped")
} catch {
    check(false, "ppm: 2x1 encodes: \(error)")
}
check((try? encodePPM(width: 2, height: 1, rgbx: [1, 2, 3])) == nil, "ppm: short pixel buffer is refused")

// MARK: - Serving over a socketpair with a fake backend

final class FakeBackend: GuestBackend {
    var performed: [Gesture] = []
    var screenFailure: GuestFailure?

    func screendump() throws -> [UInt8] {
        if let screenFailure { throw screenFailure }
        return Array("P6\n1 1\n255\n".utf8) + [9, 9, 9]
    }

    func perform(_ gesture: Gesture) throws {
        performed.append(gesture)
    }
}

func readReply(_ fd: Int32) -> [UInt8]? {
    guard let header = readExact(fd, 4) else { return nil }
    let length = header.reduce(0) { ($0 << 8) | Int($1) }
    return readExact(fd, length)
}

func runSession(_ requests: [[UInt8]], backend: FakeBackend) -> [[UInt8]] {
    var fds: [Int32] = [0, 0]
    guard socketpair(AF_UNIX, SOCK_STREAM, 0, &fds) == 0 else {
        check(false, "socketpair")
        return []
    }
    for request in requests {
        _ = writeAll(fds[0], request)
    }
    shutdown(fds[0], SHUT_WR)
    // Close the server end once it returns, so the client reads EOF after the last reply.
    serve(connection: fds[1], backend: backend)
    close(fds[1])
    var replies: [[UInt8]] = []
    while let reply = readReply(fds[0]) {
        replies.append(reply)
    }
    close(fds[0])
    return replies
}

let validClick = frame(inputBody(clickEvents(x: 5, y: 6, button: "left")))
let invalidPress = frame(inputBody(pressEvents(key: "insert", modifiers: [])))

do {
    let backend = FakeBackend()
    let replies = runSession(
        [frame(jsonBody(["execute": "screendump"])), validClick, invalidPress, validClick], backend: backend)
    check(replies.count == 4, "serve: one reply per request (got \(replies.count))")
    check(replies.count > 0 && replies[0] == [0] + Array("P6\n1 1\n255\n".utf8) + [9, 9, 9],
        "serve: screendump reply is status 0 plus PPM")
    check(replies.count > 1 && replies[1] == [0], "serve: input success reply is the one-byte body [0]")
    check(replies.count > 1 && frame(replies[1]) == [0, 0, 0, 1, 0],
        "serve: input success golden frame is [0,0,0,1,0]")
    check(replies.count > 2 && replies[2].first == 1, "serve: refused key gesture gets an error reply")
    check(replies.count > 3 && replies[3] == [0], "serve: connection keeps serving after a refusal")
    check(backend.performed == [.click(x: 5, y: 6, button: .left), .click(x: 5, y: 6, button: .left)],
        "serve: only validated gestures reach the backend")
}

do {
    let backend = FakeBackend()
    backend.screenFailure = GuestFailure("Screen Recording is not allowed for the guest companion")
    let replies = runSession([frame(jsonBody(["execute": "screendump"]))], backend: backend)
    check(replies.count == 1 && replies[0].first == 1, "serve: backend failure is an error reply")
    check(replies.count == 1 && String(decoding: replies[0].dropFirst(), as: UTF8.self).hasPrefix("Screen Recording"),
        "serve: error reply carries the message")
}

do {
    let backend = FakeBackend()
    let oversized = bigEndian32(Wire.maxRequestBytes + 1)
    let replies = runSession([oversized, validClick], backend: backend)
    check(replies.count == 1 && replies[0].first == 1, "serve: oversized frame is refused")
    check(backend.performed.isEmpty, "serve: nothing is posted after an oversized frame")
}

do {
    let backend = FakeBackend()
    let replies = runSession([bigEndian32(0)], backend: backend)
    check(replies.count == 1 && replies[0].first == 1, "serve: empty frame is refused")
}

let longMessage = GuestFailure(String(repeating: "é", count: 900))
check(errorBody(longMessage).count <= 1 + Wire.maxErrorBytes, "error body is bounded")

// MARK: - Result

print("guest pure checks: \(checks) run, \(failures.count) failed")
for failure in failures {
    print("FAIL: \(failure)")
}
exit(failures.isEmpty ? 0 : 1)
