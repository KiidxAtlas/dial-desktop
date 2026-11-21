import Darwin
import Foundation

// Host helper tests. run.sh compiles these with the helper's session, protocol, guest-transport and
// argument files, plus the companion's Guest.swift with its @main removed. The companion's own
// `serve(connection:backend:)` therefore answers the guest-side checks; nothing here copies its wire.
// No Virtualization framework, no real guest, no network.

final class Harness {
    private(set) var passed = 0
    private(set) var failures: [String] = []

    func check(_ condition: @autoclosure () -> Bool, _ name: String) {
        if condition() {
            passed += 1
        } else {
            failures.append(name)
            print("FAIL \(name)")
        }
    }
}

let harness = Harness()
// As in the helper entry point: a write to a closed peer must fail, not kill the test process.
signal(SIGPIPE, SIG_IGN)
let scratch = FileManager.default.temporaryDirectory
    .appendingPathComponent("dial-macos-host-tests-\(getpid())", isDirectory: true)
try? FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)

// MARK: - Fixtures

func makeRunDirectory(_ name: String, omit: String? = nil) -> URL {
    let directory = scratch.appendingPathComponent(name, isDirectory: true)
    try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    for file in RunFileName.required where file != omit {
        FileManager.default.createFile(atPath: directory.appendingPathComponent(file).path, contents: Data("x".utf8))
    }
    return directory
}

func makeFile(_ name: String) -> URL {
    let url = scratch.appendingPathComponent(name)
    FileManager.default.createFile(atPath: url.path, contents: Data("iso".utf8))
    return url
}

let ppm = Data("P6\n2 1\n255\n".utf8) + Data([1, 2, 3, 4, 5, 6])

func socketPair() -> (Int32, Int32) {
    var fds: [Int32] = [0, 0]
    precondition(socketpair(AF_UNIX, SOCK_STREAM, 0, &fds) == 0, "socketpair failed")
    return (fds[0], fds[1])
}

func readBytes(_ fd: Int32, _ count: Int) -> [UInt8]? {
    var buffer = [UInt8](repeating: 0, count: count)
    var offset = 0
    while offset < count {
        let received = buffer.withUnsafeMutableBytes { raw in
            read(fd, raw.baseAddress! + offset, count - offset)
        }
        if received > 0 {
            offset += received
        } else if received < 0 && errno == EINTR {
            continue
        } else {
            return nil
        }
    }
    return buffer
}

/// Read one length-prefixed request frame from the helper side.
func readRequestFrame(_ fd: Int32) -> Bool {
    guard let header = readBytes(fd, 4) else { return false }
    let length = header.reduce(0) { ($0 << 8) | Int($1) }
    return readBytes(fd, length) != nil
}

func sendBytes(_ fd: Int32, _ bytes: [UInt8]) {
    bytes.withUnsafeBytes { raw in
        var offset = 0
        while offset < bytes.count {
            let sent = write(fd, raw.baseAddress! + offset, bytes.count - offset)
            if sent <= 0 { return }
            offset += sent
        }
    }
}

func lengthPrefix(_ value: UInt32) -> [UInt8] {
    [UInt8(value >> 24), UInt8((value >> 16) & 0xFF), UInt8((value >> 8) & 0xFF), UInt8(value & 0xFF)]
}

/// A fake companion: runs `handler` on the far end of a socket pair, then closes it.
func startServer(_ handler: @escaping (Int32) -> Void) -> (client: Int32, done: DispatchSemaphore) {
    let (client, server) = socketPair()
    let done = DispatchSemaphore(value: 0)
    Thread {
        handler(server)
        close(server)
        done.signal()
    }.start()
    return (client, done)
}

final class FakeBackend: GuestBackend {
    private let lock = NSLock()
    private(set) var screendumps = 0
    private(set) var gestures: [Gesture] = []
    var refuseGesture: String?

    func screendump() throws -> [UInt8] {
        lock.lock()
        screendumps += 1
        lock.unlock()
        return [UInt8](ppm)
    }

    func perform(_ gesture: Gesture) throws {
        lock.lock()
        gestures.append(gesture)
        lock.unlock()
        if let refuseGesture {
            throw GuestFailure(refuseGesture)
        }
    }
}

// MARK: - Arguments

func testArguments(_ harness: Harness) {
    let run = makeRunDirectory("args-run")
    let iso = makeFile("project.iso")
    let text = makeFile("project.txt")
    let missingDisk = makeRunDirectory("args-missing-disk", omit: RunFileName.disk)
    let missingIso = scratch.appendingPathComponent("missing.iso").path
    let cases: [(name: String, arguments: [String], exitCode: Int32?)] = [
        ("run directory only", ["run", run.path], nil),
        ("run directory and iso", ["run", run.path, iso.path], nil),
        ("legacy helper prefix refused", ["helper", "run", run.path], 2),
        ("missing subcommand", [run.path], 2),
        ("extra argument refused", ["run", run.path, iso.path, "extra"], 2),
        ("iso must have .iso extension", ["run", run.path, text.path], 2),
        ("missing iso refused", ["run", run.path, missingIso], 3),
        ("missing run directory refused", ["run", scratch.appendingPathComponent("nope").path], 3),
        ("missing Disk.img refused", ["run", missingDisk.path], 3),
    ]
    for item in cases {
        do {
            let request = try parseRunArguments(item.arguments)
            harness.check(item.exitCode == nil, "arguments accepted: \(item.name)")
            if item.exitCode == nil {
                harness.check(
                    request.runDirectory == URL(fileURLWithPath: item.arguments[1]).standardizedFileURL,
                    "run directory kept: \(item.name)")
                harness.check(request.projectISO == (item.arguments.count == 3 ? iso.standardizedFileURL : nil),
                    "iso kept: \(item.name)")
            }
        } catch let failure as HostFailure {
            harness.check(item.exitCode == failure.exitCode, "exit code for \(item.name): \(failure.message)")
        } catch {
            harness.check(false, "unexpected error for \(item.name)")
        }
    }
}

// MARK: - QMP and paths

func testQMP(_ harness: Harness) {
    func kind(_ line: String) -> String {
        do {
            switch try parseQMPCommand(Data(line.utf8)) {
            case .capabilities: return "capabilities"
            case .screendump(let filename): return "screendump:\(filename)"
            case .inputSendEvent(let events): return "input:\(events.count)"
            }
        } catch let refusal as QMPRefusal {
            return "refused:\(refusal.description)"
        } catch {
            return "other"
        }
    }
    let click = #"{"type":"abs","data":{"axis":"x","value":1}}"#
    let events = (1...513).map { _ in click }.joined(separator: ",")
    let table: [(line: String, expected: String)] = [
        (#"{"execute":"qmp_capabilities"}"#, "capabilities"),
        (#"{"execute":"qmp_capabilities","arguments":{}}"#, "capabilities"),
        (#"{"execute":"screendump","arguments":{"filename":"/r/screen.ppm"}}"#, "screendump:/r/screen.ppm"),
        (#"{"execute":"input-send-event","arguments":{"events":[\#(click)]}}"#, "input:1"),
        (#"{"execute":"query-status"}"#, "refused:unsupported command"),
        (#"{"execute":"qmp_capabilities","arguments":"x"}"#, "refused:arguments must be an object"),
        (#"{"execute":"screendump","arguments":{}}"#, "refused:screendump needs a filename"),
        (#"{"execute":"input-send-event","arguments":{"events":[]}}"#,
            "refused:input-send-event needs between 1 and 512 events"),
        (#"{"execute":"input-send-event","arguments":{"events":[\#(events)]}}"#,
            "refused:input-send-event needs between 1 and 512 events"),
        (#"{"execute":"input-send-event","arguments":{"events":[{"type":"shell","data":{}}]}}"#,
            "refused:input event is not one this helper accepts"),
        ("not json", "refused:malformed request"),
    ]
    for item in table {
        harness.check(kind(item.line) == item.expected, "qmp \(item.line.prefix(60)): got \(kind(item.line))")
    }
    harness.check(String(data: qmpGreeting(), encoding: .utf8)?.contains("\"QMP\"") == true, "greeting names QMP")
    harness.check(qmpReturn().last == 0x0A && qmpError("x").last == 0x0A, "replies end with newline")
}

func testScreendumpPaths(_ harness: Harness) {
    let run = makeRunDirectory("paths-run")
    let table: [(filename: String, accepted: Bool)] = [
        (run.appendingPathComponent("screen.ppm").path, true),
        (run.appendingPathComponent("SCREEN.PPM").path, true),
        (scratch.appendingPathComponent("outside.ppm").path, false),
        (run.appendingPathComponent("../outside.ppm").path, false),
        (run.appendingPathComponent("sub/screen.ppm").path, false),
        (run.appendingPathComponent("screen.png").path, false),
        ("screen.ppm", false),
        (run.path + "/bad\0.ppm", false),
    ]
    for item in table {
        do {
            _ = try screendumpDestination(filename: item.filename, runDirectory: run)
            harness.check(item.accepted, "destination accepted: \(item.filename)")
        } catch {
            harness.check(!item.accepted, "destination refused: \(item.filename)")
        }
    }
}

func testFramesAndPPM(_ harness: Harness) {
    let table: [(Data, Bool)] = [
        (Data("P6\n2 1\n255\n".utf8), true),
        (Data("P6 2 1 255 ".utf8), true),
        (Data("P5\n2 1\n255\n".utf8), false),
        (Data("P6".utf8), false),
        (Data(), false),
    ]
    for (data, expected) in table {
        harness.check(isPPM(data) == expected, "isPPM \(String(decoding: data, as: UTF8.self))")
    }
    harness.check(bigEndianLength(0x0102_0304) == Data([1, 2, 3, 4]), "length prefix is big-endian")
    if let frame = try? guestRequestFrame(["execute": "screendump"]) {
        let body = frame.dropFirst(4)
        harness.check(frame.prefix(4) == bigEndianLength(body.count), "request length prefix matches body")
        harness.check(String(decoding: body, as: UTF8.self) == #"{"execute":"screendump"}"#, "request body is the fixed command")
        harness.check(!String(decoding: body, as: UTF8.self).contains("filename"), "request never carries a filename")
    } else {
        harness.check(false, "screendump request frame encodes")
    }
}

// MARK: - Guest exchange

/// Run one exchange against `handler` on the far end and report what the link returned.
enum ExchangeOutcome {
    case payload(Data)
    case refused(String)
    case transport(String)
}

func exchange(
    _ request: [String: Any],
    kind: GuestReplyKind,
    timeoutSeconds: Int = 5,
    handler: @escaping (Int32) -> Void
) -> ExchangeOutcome {
    let (client, done) = startServer(handler)
    let link = GuestLink(
        channel: GuestChannel(fileDescriptor: client, close: { close(client) }),
        timeoutSeconds: timeoutSeconds)
    defer {
        link.close()
        _ = done.wait(timeout: .now() + 5)
    }
    do {
        switch try link.exchange(request, kind: kind) {
        case .payload(let data): return .payload(data)
        case .refused(let message): return .refused(message)
        }
    } catch let transport as GuestTransportError {
        return .transport(transport.message)
    } catch {
        return .transport("unexpected \(error)")
    }
}

let screendumpRequest: [String: Any] = ["execute": "screendump"]
let clickRequest: [String: Any] = [
    "execute": "input-send-event",
    "arguments": ["events": [
        ["type": "abs", "data": ["axis": "x", "value": 100]],
        ["type": "abs", "data": ["axis": "y", "value": 100]],
        ["type": "btn", "data": ["down": true, "button": "left"]],
        ["type": "btn", "data": ["down": false, "button": "left"]],
    ]],
]

/// Exchanges against the companion's real `serve`, so the agreement is with its code.
func testCompanionAgreement(_ harness: Harness) {
    let backend = FakeBackend()
    let companion: (Int32) -> Void = { fd in serve(connection: fd, backend: backend) }

    if case .payload(let image) = exchange(screendumpRequest, kind: .screendump, handler: companion) {
        harness.check(image == ppm, "companion screendump returns the PPM")
    } else {
        harness.check(false, "companion screendump succeeds")
    }

    if case .payload(let body) = exchange(clickRequest, kind: .input, handler: companion) {
        harness.check(body.isEmpty, "companion input success has an empty body")
    } else {
        harness.check(false, "companion input click succeeds")
    }
    harness.check(backend.gestures.count == 1, "companion performed the click once")

    backend.refuseGesture = "the guest refused this gesture"
    let refused = exchange(clickRequest, kind: .input, handler: companion)
    if case .refused(let message) = refused {
        harness.check(message == "the guest refused this gesture", "companion error text reaches the host")
    } else {
        harness.check(false, "companion gesture refusal is reported as refused")
    }
    backend.refuseGesture = nil

    let malformed: [String: Any] = [
        "execute": "input-send-event",
        "arguments": ["events": [["type": "abs", "data": ["axis": "z", "value": 1]]]],
    ]
    if case .refused = exchange(malformed, kind: .input, handler: companion) {
        harness.check(true, "companion refuses malformed events")
    } else {
        harness.check(false, "companion refuses malformed events")
    }
}

/// Replies a raw peer can send. The companion never sends the malformed ones; the host must refuse them.
func testRawReplies(_ harness: Harness) {
    let controlError: [UInt8] = [1] + Array("bad\u{7}text".utf8)
    let longError: [UInt8] = [1] + [UInt8](repeating: 0x61, count: 513)
    let table: [(name: String, kind: GuestReplyKind, reply: [UInt8], expected: String)] = [
        ("screendump without a status is refused", .screendump, lengthPrefix(0), "transport:the guest reply has no status byte"),
        ("input without a status is refused", .input, lengthPrefix(0), "transport:the guest reply has no status byte"),
        ("status 1 message is refused as text", .screendump, lengthPrefix(5) + [1] + Array("boom".utf8), "refused:boom"),
        ("control character in error is unreadable", .screendump, lengthPrefix(UInt32(controlError.count)) + controlError, "refused:the guest sent an unreadable error"),
        ("oversized error is a transport failure", .screendump, lengthPrefix(UInt32(longError.count)) + longError, "transport:the guest error is too large"),
        ("unknown status byte", .screendump, lengthPrefix(2) + [7, 0], "transport:the guest sent an unknown reply status"),
        ("screendump payload must be a PPM", .screendump, lengthPrefix(5) + [0] + Array("P5 x".utf8), "transport:the guest screen image is not a P6 PPM"),
        ("input reply with a payload is refused", .input, lengthPrefix(2) + [0, 0x41], "transport:the guest sent an unexpected payload"),
        ("length beyond the cap is refused", .screendump, lengthPrefix(0xFFFF_FFFF), "transport:the guest reply is too large"),
    ]
    for item in table {
        let outcome = exchange(screendumpRequest.merging(["execute": item.kind == .input ? "input-send-event" : "screendump"]) { $1 }, kind: item.kind) { fd in
            _ = readRequestFrame(fd)
            sendBytes(fd, item.reply)
        }
        let got: String
        switch outcome {
        case .payload(let data): got = "payload:\(data.count)"
        case .refused(let message): got = "refused:\(message)"
        case .transport(let message): got = "transport:\(message)"
        }
        harness.check(got == item.expected, "\(item.name): got \(got)")
    }

    let closed = exchange(screendumpRequest, kind: .screendump) { fd in _ = readRequestFrame(fd) }
    if case .transport(let message) = closed {
        harness.check(message.contains("closed its channel"), "peer close before reply is a transport failure")
    } else {
        harness.check(false, "peer close before reply is a transport failure")
    }

    let truncated = exchange(screendumpRequest, kind: .screendump) { fd in
        _ = readRequestFrame(fd)
        sendBytes(fd, lengthPrefix(100) + [0] + Array(ppm.prefix(10)))
    }
    if case .transport = truncated {
        harness.check(true, "truncated reply is a transport failure")
    } else {
        harness.check(false, "truncated reply is a transport failure")
    }
}

/// The wall-clock limit covers the whole exchange: a peer trickling bytes cannot extend it.
func testExchangeDeadline(_ harness: Harness) {
    let started = Date()
    let outcome = exchange(screendumpRequest, kind: .screendump, timeoutSeconds: 1) { fd in
        _ = readRequestFrame(fd)
        for byte in [UInt8(0), 0, 0, 5] {
            Thread.sleep(forTimeInterval: 0.3)
            sendBytes(fd, [byte])
        }
    }
    let elapsed = Date().timeIntervalSince(started)
    if case .transport(let message) = outcome {
        harness.check(message == "the guest did not answer in time", "trickling peer times out: \(message)")
    } else {
        harness.check(false, "trickling peer times out")
    }
    harness.check(elapsed < 2.0, "deadline is wall-clock, took \(String(format: "%.2f", elapsed))s")
}

// MARK: - Session

final class FakeMachine: GuestMachine {
    let backend = FakeBackend()
    var startError: String?
    var connects: Bool
    /// Replaces the companion for each connection; nil runs the real `serve`.
    var serverHandler: ((Int32) -> Void)?
    private let lock = NSLock()
    private(set) var stopCount = 0
    private(set) var connectAttempts = 0

    init(connects: Bool) {
        self.connects = connects
    }

    func start(completion: @escaping (String?) -> Void) {
        completion(startError)
    }

    func connectGuest(completion: @escaping (Result<GuestChannel, Error>) -> Void) {
        lock.lock()
        connectAttempts += 1
        lock.unlock()
        guard connects else {
            completion(.failure(FakeConnectError()))
            return
        }
        let (client, server) = socketPair()
        let handler = serverHandler ?? { [backend] fd in serve(connection: fd, backend: backend) }
        Thread {
            handler(server)
            close(server)
        }.start()
        completion(.success(GuestChannel(fileDescriptor: client, close: { close(client) })))
    }

    func stop(completion: @escaping () -> Void) {
        lock.lock()
        stopCount += 1
        lock.unlock()
        completion()
    }
}

struct FakeConnectError: Error, CustomStringConvertible {
    var description: String { "connection refused" }
}

struct SessionResult {
    let exitCode: Int32?
    let lines: [[String: Any]]
    let errors: [String]
}

/// Run a session on its own thread. `events` are queued before the run starts; `keepPosting`
/// keeps feeding boot-time events until the session returns.
func runSession(
    _ machine: FakeMachine,
    runDirectory: URL,
    bootTimeout: TimeInterval = 5,
    events: [HostEvent],
    keepPosting: Bool = false,
    closeAfter: TimeInterval? = nil
) -> SessionResult {
    let mailbox = HostMailbox()
    for event in events {
        mailbox.post(event)
    }
    // EOF after a delay, so the guest has connected and queued requests have been answered first.
    if let closeAfter {
        Thread {
            Thread.sleep(forTimeInterval: closeAfter)
            mailbox.post(.inputClosed)
        }.start()
    }
    let output = NSMutableData()
    let errors = NSMutableArray()
    let lock = NSLock()
    let session = HostSession(
        machine: machine,
        mailbox: mailbox,
        runDirectory: runDirectory,
        output: { data in lock.lock(); output.append(data); lock.unlock() },
        errorOutput: { message in lock.lock(); errors.add(message); lock.unlock() },
        bootTimeout: bootTimeout,
        retryInterval: 0.02,
        replyTimeoutSeconds: 5)
    let finished = DispatchSemaphore(value: 0)
    var exitCode: Int32?
    let stopFeeding = NSLock()
    var feeding = keepPosting
    Thread {
        exitCode = session.run()
        stopFeeding.lock(); feeding = false; stopFeeding.unlock()
        finished.signal()
    }.start()
    if keepPosting {
        Thread {
            while true {
                stopFeeding.lock(); let go = feeding; stopFeeding.unlock()
                guard go else { return }
                mailbox.post(.retryGuestConnect)
                Thread.sleep(forTimeInterval: 0.001)
            }
        }.start()
    }
    let completed = finished.wait(timeout: .now() + 15) == .success
    lock.lock()
    let text = String(decoding: output as Data, as: UTF8.self)
    let errorTexts = errors.map { $0 as? String ?? "" }
    lock.unlock()
    let lines = text.split(separator: "\n").compactMap { line -> [String: Any]? in
        (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any]
    }
    return SessionResult(exitCode: completed ? exitCode : nil, lines: lines, errors: errorTexts)
}

func returnsOnly(_ lines: [[String: Any]]) -> Int {
    lines.filter { $0["return"] != nil }.count
}

func errorTexts(_ lines: [[String: Any]]) -> [String] {
    lines.compactMap { (($0["error"] as? [String: Any])?["desc"]) as? String }
}

func testSession(_ harness: Harness) {
    let run = makeRunDirectory("session-run")
    let screen = run.appendingPathComponent("screen.ppm").path
    let click = clickRequest

    // Happy path: queued requests are held until the companion accepts, then answered in order.
    do {
        let machine = FakeMachine(connects: true)
        let request = try! JSONSerialization.data(withJSONObject: ["execute": "screendump", "arguments": ["filename": screen]])
        let clickLine = try! JSONSerialization.data(withJSONObject: click)
        let result = runSession(machine, runDirectory: run, events: [
            .line(Data(#"{"execute":"qmp_capabilities"}"#.utf8)),
            .line(request),
            .line(clickLine),
        ], closeAfter: 0.5)
        harness.check(result.exitCode == 0, "happy path exits 0, got \(String(describing: result.exitCode))")
        harness.check(result.lines.first?["QMP"] != nil, "greeting is first")
        harness.check(returnsOnly(result.lines) == 3, "three successful replies: \(returnsOnly(result.lines))")
        harness.check((try? Data(contentsOf: URL(fileURLWithPath: screen))) == ppm, "screendump file holds the guest PPM")
        harness.check(machine.backend.screendumps == 1 && machine.backend.gestures.count == 1, "guest saw one screendump and one click")
        harness.check(machine.stopCount == 1, "machine stopped once")
    }

    // EOF while the guest is still booting: nothing is answered and the machine is stopped.
    do {
        let machine = FakeMachine(connects: false)
        let result = runSession(machine, runDirectory: run, events: [
            .line(Data(#"{"execute":"qmp_capabilities"}"#.utf8)),
            .inputClosed,
        ])
        harness.check(result.exitCode == 0, "EOF during pending boot exits 0")
        harness.check(result.lines.count == 1 && result.lines.first?["QMP"] != nil, "only the greeting before EOF")
        harness.check(machine.stopCount == 1, "EOF during boot stops the machine")
    }

    // Boot deadline, with boot-time events arriving continuously: the deadline still ends the wait.
    do {
        let machine = FakeMachine(connects: false)
        let started = Date()
        let result = runSession(machine, runDirectory: run, bootTimeout: 0.3, events: [], keepPosting: true)
        let elapsed = Date().timeIntervalSince(started)
        harness.check(result.exitCode == 4, "boot timeout exits 4, got \(String(describing: result.exitCode))")
        harness.check(result.errors.first?.contains("dial-guest --serve") == true, "boot message names the companion")
        harness.check(result.errors.first?.contains("vsock port 49152") == true, "boot message names the port")
        harness.check(elapsed < 3, "boot deadline held under event load: \(String(format: "%.2f", elapsed))s")
        harness.check(machine.stopCount == 1, "boot timeout stops the machine")
    }

    // The machine refuses to start (for example a missing virtualization entitlement).
    do {
        let machine = FakeMachine(connects: true)
        machine.startError = "The process doesn't have the com.apple.security.virtualization entitlement."
        let result = runSession(machine, runDirectory: run, events: [.machineStarted(machine.startError)])
        harness.check(result.exitCode == 4, "start failure exits 4")
        harness.check(result.errors.first?.hasPrefix("could not start the virtual machine") == true, "start failure is reported")
        harness.check(machine.stopCount == 1, "start failure stops the machine")
    }

    // Refusals that never reach the guest.
    do {
        let machine = FakeMachine(connects: true)
        let outside = scratch.appendingPathComponent("outside.ppm").path
        let refused = try! JSONSerialization.data(withJSONObject: ["execute": "screendump", "arguments": ["filename": outside]])
        let unknown = Data(#"{"execute":"query-status"}"#.utf8)
        let badEvent = try! JSONSerialization.data(withJSONObject: ["execute": "input-send-event", "arguments": ["events": [["type": "shell", "data": [:]]]]])
        let result = runSession(machine, runDirectory: run, events: [
            .line(Data(#"{"execute":"qmp_capabilities"}"#.utf8)),
            .line(refused), .line(unknown), .line(badEvent),
        ], closeAfter: 0.5)
        let messages = errorTexts(result.lines)
        harness.check(result.exitCode == 0, "refusals do not end the run")
        harness.check(messages.count == 3, "three refusals answered: \(messages)")
        harness.check(machine.backend.screendumps == 0 && machine.backend.gestures.isEmpty, "refused requests never reach the guest")
        harness.check(!FileManager.default.fileExists(atPath: outside), "no file written outside the run directory")
    }

    // Guest hangs up mid-session: a transport failure ends the run with exit 5.
    do {
        let machine = FakeMachine(connects: true)
        machine.serverHandler = { fd in _ = readRequestFrame(fd) }
        let request = try! JSONSerialization.data(withJSONObject: ["execute": "screendump", "arguments": ["filename": screen]])
        let result = runSession(machine, runDirectory: run, events: [
            .line(Data(#"{"execute":"qmp_capabilities"}"#.utf8)),
            .line(request),
        ])
        harness.check(result.exitCode == 5, "guest hang-up exits 5, got \(String(describing: result.exitCode))")
        harness.check(errorTexts(result.lines).contains("the guest closed its channel"), "hang-up is reported to the runner")
        harness.check(machine.stopCount == 1, "transport failure stops the machine")
    }
}

// MARK: - Run

testArguments(harness)
testQMP(harness)
testScreendumpPaths(harness)
testFramesAndPPM(harness)
testCompanionAgreement(harness)
testRawReplies(harness)
testExchangeDeadline(harness)
testSession(harness)

try? FileManager.default.removeItem(at: scratch)
print("host tests: \(harness.passed) passed, \(harness.failures.count) failed")
exit(harness.failures.isEmpty ? 0 : 1)
