// Guest-only companion for Dial's macOS virtual machine runner.
//
// This file is compiled into a binary that runs INSIDE a macOS guest, started by the user there.
// It never runs on the host: it refuses to serve unless the kernel reports a virtual machine and the
// guest has a guest virtio-socket CID (3 or more). It is not a host helper and it captures or
// injects nothing on the host.
//
// Transport: AF_VSOCK, port 49152, listening on the guest's own CID, accepting peers from the host
// CID (2) only. There is no TCP/UDP listener, no shell, and no filesystem API exposed to the peer.
//
// Wire (both directions): a uint32 big-endian length, then the body.
//   Request body: JSON object {"execute": "screendump"} or
//                 {"execute": "input-send-event", "arguments": {"events": [...]}}
//   Reply to screendump: body = 0x00 followed by a binary P6 PPM of the main display.
//   Reply to input:      body is the single status byte 0x00 on success (length 1).
//   Any error reply:     body = 0x01 followed by a UTF-8 message of at most 512 bytes.
// The event list is the one the host's QEMU-compatible translator emits (`input.rs`): absolute
// x/y pointer events, button down/up, and key down/up by QEMU qcode. Each gesture is fully parsed
// and every CGEvent is built before the first event is posted, so a malformed gesture posts nothing.
//
// Build (guest, Xcode command line tools):
//   swiftc -swift-version 5 -O packaging/macos-guest/Guest.swift -o dial-guest
// Run in the guest only, after granting Screen Recording and Accessibility/event posting to the
// binary in the guest's System Settings: `dial-guest --serve`.

import ScreenCaptureKit
import CoreGraphics
import Foundation

// MARK: - Constants

enum Wire {
    static let port: UInt32 = 49152
    static let hostCID: UInt32 = 2
    static let minGuestCID: UInt32 = 3
    /// AF_VSOCK on macOS (`sys/socket.h`).
    static let addressFamily: Int32 = 40
    /// sockaddr_vm: svm_len(1) svm_family(1) svm_reserved1(2) svm_port(4) svm_cid(4).
    static let sockaddrVMSize: UInt8 = 12
    /// IOCTL_VM_SOCKETS_GET_LOCAL_CID = _IOR('s', 209, uint32_t) (`sys/vsock.h`).
    static let localCIDRequest: UInt = 0x4004_73D1
    static let maxRequestBytes = 64 * 1024
    static let maxReplyBytes = 64 * 1024 * 1024
    static let maxErrorBytes = 512
    static let maxEvents = 16
    static let absMax = 32767
    static let idleTimeoutSeconds = 30
}

// MARK: - Errors

/// A refusal with the message sent back to the host. Messages are shown to the user, so they never
/// contain payload bytes.
struct GuestFailure: Error, Equatable {
    let message: String
    init(_ message: String) { self.message = message }
}

// MARK: - Gestures

enum Button: String, Equatable {
    case left, right, middle
}

/// One event from the translator, in the form the host sends.
enum InputEvent: Equatable {
    case abs(axis: String, value: Int)
    case btn(button: Button, down: Bool)
    case key(qcode: String, down: Bool)
}

/// A validated gesture. Modifiers are qcodes in the order they were pressed.
enum Gesture: Equatable {
    case click(x: Int, y: Int, button: Button)
    case press(modifiers: [String], key: String)
}

enum Request: Equatable {
    case screendump
    case input(Gesture)
}

/// Modifier qcodes the translator may hold around a key (`input.rs` `Modifier::qcode`).
let modifierQcodes: [String] = ["shift", "ctrl", "alt", "meta_l"]

// MARK: - Key table (QEMU qcode to macOS virtual key code, Carbon `kVK_*`)

/// Every qcode the host's `qcode_for` can produce, mapped to the macOS virtual key code.
/// `insert` has no macOS key and is refused rather than mapped to a different key.
let keyCodes: [String: UInt16] = {
    var table: [String: UInt16] = [:]
    let letters: [UInt16] = [
        0x00, 0x0B, 0x08, 0x02, 0x0E, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2E,
        0x2D, 0x1F, 0x23, 0x0C, 0x0F, 0x01, 0x11, 0x20, 0x09, 0x0D, 0x07, 0x10, 0x06,
    ]
    let alphabet = Array("abcdefghijklmnopqrstuvwxyz")
    for (index, code) in letters.enumerated() {
        table[String(alphabet[index])] = code
    }
    let digits: [UInt16] = [0x1D, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19]
    for (digit, code) in digits.enumerated() {
        table["\(digit)"] = code
    }
    let functionKeys: [UInt16] = [
        0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F,
    ]
    for (index, code) in functionKeys.enumerated() {
        table["f\(index + 1)"] = code
    }
    let named: [String: UInt16] = [
        "ret": 0x24, "esc": 0x35, "backspace": 0x33, "tab": 0x30, "spc": 0x31,
        "minus": 0x1B, "equal": 0x18, "bracket_left": 0x21, "bracket_right": 0x1E,
        "backslash": 0x2A, "semicolon": 0x29, "apostrophe": 0x27, "grave_accent": 0x32,
        "comma": 0x2B, "dot": 0x2F, "slash": 0x2C,
        "up": 0x7E, "down": 0x7D, "left": 0x7B, "right": 0x7C,
        "home": 0x73, "end": 0x77, "pgup": 0x74, "pgdn": 0x79, "delete": 0x75,
        "shift": 0x38, "shift_r": 0x3C, "ctrl": 0x3B, "ctrl_r": 0x3E,
        "alt": 0x3A, "alt_r": 0x3D, "meta_l": 0x37, "meta_r": 0x36, "caps_lock": 0x39,
    ]
    table.merge(named) { first, _ in first }
    return table
}()


// MARK: - Request parsing (pure)

private func integer(_ value: Any?) -> Int? {
    guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else {
        return nil
    }
    let double = number.doubleValue
    guard double == double.rounded(), abs(double) < 1e9 else { return nil }
    return number.intValue
}

private func boolean(_ value: Any?) -> Bool? {
    guard let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else {
        return nil
    }
    return number.boolValue
}

private func parseEvent(_ value: Any) throws -> InputEvent {
    let refusal = GuestFailure("input event is malformed")
    guard let object = value as? [String: Any], Set(object.keys) == ["type", "data"],
        let type = object["type"] as? String, let data = object["data"] as? [String: Any]
    else { throw refusal }
    switch type {
    case "abs":
        guard Set(data.keys) == ["axis", "value"], let axis = data["axis"] as? String,
            axis == "x" || axis == "y", let value = integer(data["value"]),
            (0...Wire.absMax).contains(value)
        else { throw refusal }
        return .abs(axis: axis, value: value)
    case "btn":
        guard Set(data.keys) == ["button", "down"], let name = data["button"] as? String,
            let button = Button(rawValue: name), let down = boolean(data["down"])
        else { throw refusal }
        return .btn(button: button, down: down)
    case "key":
        guard Set(data.keys) == ["down", "key"], let down = boolean(data["down"]),
            let key = data["key"] as? [String: Any], Set(key.keys) == ["type", "data"],
            key["type"] as? String == "qcode", let qcode = key["data"] as? String,
            keyCodes[qcode] != nil
        else { throw GuestFailure("unsupported physical key") }
        return .key(qcode: qcode, down: down)
    default:
        throw refusal
    }
}

/// Accept exactly one complete gesture: a click (`abs x`, `abs y`, `btn down`, `btn up`) or a
/// press (modifier downs, key down, key up, modifier ups in reverse).
func parseGesture(_ events: [InputEvent]) throws -> Gesture {
    if case .abs? = events.first {
        guard events.count == 4,
            case .abs(let xAxis, let x) = events[0], xAxis == "x",
            case .abs(let yAxis, let y) = events[1], yAxis == "y",
            case .btn(let button, true) = events[2],
            events[3] == .btn(button: button, down: false)
        else { throw GuestFailure("click must be x, y, button down, button up") }
        return .click(x: x, y: y, button: button)
    }
    let total = events.count
    guard total >= 2, total % 2 == 0, (total - 2) / 2 <= 4 else {
        throw GuestFailure("key gesture has an invalid event count")
    }
    let count = (total - 2) / 2
    var modifiers: [String] = []
    for index in 0..<count {
        guard case .key(let qcode, true) = events[index], modifierQcodes.contains(qcode),
            !modifiers.contains(qcode)
        else { throw GuestFailure("duplicate or unknown key modifier") }
        modifiers.append(qcode)
    }
    guard case .key(let key, true) = events[count], !modifiers.contains(key),
        events[count + 1] == .key(qcode: key, down: false)
    else { throw GuestFailure("key gesture must press and release one key") }
    for (offset, modifier) in modifiers.reversed().enumerated() {
        guard events[count + 2 + offset] == .key(qcode: modifier, down: false) else {
            throw GuestFailure("modifiers must be released in reverse order")
        }
    }
    return .press(modifiers: modifiers, key: key)
}

/// Parse one request body (the JSON object, without its length prefix).
func parseRequest(_ body: [UInt8]) throws -> Request {
    let refusal = GuestFailure("request is not a JSON object")
    guard let object = try? JSONSerialization.jsonObject(with: Data(body)) as? [String: Any]
    else { throw refusal }
    guard Set(object.keys).isSubset(of: ["execute", "arguments"]) else {
        throw GuestFailure("request has an unknown field")
    }
    guard let command = object["execute"] as? String else {
        throw GuestFailure("request has no execute command")
    }
    switch command {
    case "screendump":
        if let arguments = object["arguments"] {
            guard let dictionary = arguments as? [String: Any], dictionary.isEmpty else {
                throw GuestFailure("screendump takes no arguments")
            }
        }
        return .screendump
    case "input-send-event":
        guard let arguments = object["arguments"] as? [String: Any],
            Set(arguments.keys) == ["events"], let raw = arguments["events"] as? [Any],
            !raw.isEmpty, raw.count <= Wire.maxEvents
        else { throw GuestFailure("input-send-event needs one events list") }
        return .input(try parseGesture(try raw.map(parseEvent)))
    default:
        throw GuestFailure("unsupported command")
    }
}

// MARK: - Coordinates and PPM (pure)

/// Map a 0...32767 absolute axis value onto a display extent in points, keeping the result inside
/// the display.
func pointCoordinate(value: Int, origin: Double, extent: Double) -> Double {
    let fraction = Double(value) / Double(Wire.absMax)
    let point = origin + fraction * extent
    return min(max(point, origin), origin + extent - 1)
}

/// Encode 32-bit RGBX pixels (alpha ignored) as a binary P6 PPM.
func encodePPM(width: Int, height: Int, rgbx: [UInt8]) throws -> [UInt8] {
    guard width > 0, height > 0, rgbx.count == width * height * 4 else {
        throw GuestFailure("the guest screen has an invalid size")
    }
    var output = Array("P6\n\(width) \(height)\n255\n".utf8)
    output.reserveCapacity(output.count + width * height * 3)
    for pixel in stride(from: 0, to: rgbx.count, by: 4) {
        output.append(rgbx[pixel])
        output.append(rgbx[pixel + 1])
        output.append(rgbx[pixel + 2])
    }
    return output
}

// MARK: - Framing (pure, over file descriptors)

func bigEndian32(_ value: Int) -> [UInt8] {
    [UInt8((value >> 24) & 0xFF), UInt8((value >> 16) & 0xFF), UInt8((value >> 8) & 0xFF),
        UInt8(value & 0xFF)]
}

func readExact(_ fd: Int32, _ count: Int) -> [UInt8]? {
    var buffer = [UInt8](repeating: 0, count: count)
    var offset = 0
    while offset < count {
        let received = buffer.withUnsafeMutableBytes { raw in
            read(fd, raw.baseAddress! + offset, count - offset)
        }
        if received < 0 {
            if errno == EINTR { continue }
            return nil
        }
        if received == 0 { return nil }
        offset += received
    }
    return buffer
}

func writeAll(_ fd: Int32, _ bytes: [UInt8]) -> Bool {
    var offset = 0
    while offset < bytes.count {
        let sent = bytes.withUnsafeBytes { raw in
            write(fd, raw.baseAddress! + offset, bytes.count - offset)
        }
        if sent < 0 {
            if errno == EINTR { continue }
            return false
        }
        offset += sent
    }
    return true
}

/// The reply body for a failure: status 1 and a bounded message.
func errorBody(_ failure: GuestFailure) -> [UInt8] {
    var message = failure.message
    while message.utf8.count > Wire.maxErrorBytes {
        message.removeLast()
    }
    return [1] + Array(message.utf8)
}

// MARK: - Serving (pure given a backend)

/// The side effects the wire protocol needs. The live backend uses CoreGraphics; tests use a fake.
protocol GuestBackend {
    /// A binary P6 PPM of the screen.
    func screendump() throws -> [UInt8]
    /// Post a fully validated gesture.
    func perform(_ gesture: Gesture) throws
}

/// Serve one connection until the peer closes it or sends a frame that cannot be resynchronised.
/// Each request is validated completely before the backend is called.
func serve(connection fd: Int32, backend: GuestBackend) {
    while true {
        guard let header = readExact(fd, 4) else { return }
        let length = header.reduce(0) { ($0 << 8) | Int($1) }
        guard length > 0, length <= Wire.maxRequestBytes else {
            let message = errorBody(GuestFailure("request is empty or too large"))
            _ = writeAll(fd, bigEndian32(message.count) + message)
            return
        }
        guard let body = readExact(fd, length) else { return }
        let reply: [UInt8]
        do {
            switch try parseRequest(body) {
            case .screendump:
                let ppm = try backend.screendump()
                guard ppm.count + 1 <= Wire.maxReplyBytes else {
                    throw GuestFailure("the guest screen is too large")
                }
                reply = [0] + ppm
            case .input(let gesture):
                try backend.perform(gesture)
                reply = [0]
            }
        } catch let failure as GuestFailure {
            reply = errorBody(failure)
        } catch {
            reply = errorBody(GuestFailure("the guest could not complete the request"))
        }
        guard reply.count <= Wire.maxReplyBytes,
            writeAll(fd, bigEndian32(reply.count) + reply)
        else { return }
    }
}

// MARK: - Live backend (CoreGraphics, guest only)

struct LiveBackend: GuestBackend {
    func screendump() throws -> [UInt8] {
        // Check, never prompt: a missing grant is reported to the host.
        guard CGPreflightScreenCaptureAccess() else {
            throw GuestFailure("Screen Recording is not allowed for the guest companion")
        }
        guard #available(macOS 14.0, *) else {
            throw GuestFailure("the guest companion needs macOS 14 or later")
        }
        let image = try captureMainDisplay()
        let width = image.width
        let height = image.height
        guard width > 0, height > 0, width * height * 3 + 16 <= Wire.maxReplyBytes - 1 else {
            throw GuestFailure("the guest screen is too large")
        }
        var rgbx = [UInt8](repeating: 0, count: width * height * 4)
        let drawn = rgbx.withUnsafeMutableBytes { raw -> Bool in
            guard let context = CGContext(
                data: raw.baseAddress, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
            else { return false }
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
            return true
        }
        guard drawn else { throw GuestFailure("could not read the guest screen pixels") }
        return try encodePPM(width: width, height: height, rgbx: rgbx)
    }

/// Capture the main display with ScreenCaptureKit, waiting at most 15 seconds. The async call is
/// bridged to this synchronous server loop; the result is written once before the semaphore is
/// signalled, so reading it afterwards is safe.
@available(macOS 14.0, *)
private func captureMainDisplay() throws -> CGImage {
    final class Slot: @unchecked Sendable {
        var result: Result<CGImage, Error>?
    }
    let slot = Slot()
    let done = DispatchSemaphore(value: 0)
    let mainDisplay = CGMainDisplayID()
    Task.detached {
        do {
            let content = try await SCShareableContent.excludingDesktopWindows(
                false, onScreenWindowsOnly: true)
            guard let display = content.displays.first(where: { $0.displayID == mainDisplay })
            else {
                throw GuestFailure("the main display is not available to capture")
            }
            let configuration = SCStreamConfiguration()
            configuration.width = CGDisplayPixelsWide(mainDisplay)
            configuration.height = CGDisplayPixelsHigh(mainDisplay)
            configuration.showsCursor = false
            let filter = SCContentFilter(display: display, excludingWindows: [])
            slot.result = .success(
                try await SCScreenshotManager.captureImage(
                    contentFilter: filter, configuration: configuration))
        } catch {
            slot.result = .failure(error)
        }
        done.signal()
    }
    guard done.wait(timeout: .now() + 15) == .success else {
        throw GuestFailure("the guest screen capture timed out")
    }
    switch slot.result {
    case .success(let image)?:
        return image
    case .failure(let error as GuestFailure)?:
        throw error
    case .failure(let error)?:
        throw GuestFailure("could not capture the guest screen: \(error.localizedDescription)")
    case nil:
        throw GuestFailure("could not capture the guest screen")
    }
}

    func perform(_ gesture: Gesture) throws {
        guard CGPreflightPostEventAccess() else {
            throw GuestFailure("event posting is not allowed for the guest companion")
        }
        // Build every event first; post only when all of them exist.
        let events = try cgEvents(for: gesture, bounds: CGDisplayBounds(CGMainDisplayID()))
        for event in events {
            event.post(tap: .cghidEventTap)
        }
    }
}

private func cgEvents(for gesture: Gesture, bounds: CGRect) throws -> [CGEvent] {
    let unavailable = GuestFailure("the guest could not create an input event")
    switch gesture {
    case .click(let x, let y, let button):
        let point = CGPoint(
            x: pointCoordinate(value: x, origin: bounds.minX, extent: bounds.width),
            y: pointCoordinate(value: y, origin: bounds.minY, extent: bounds.height))
        let down: CGEventType
        let up: CGEventType
        let cgButton: CGMouseButton
        switch button {
        case .left: (down, up, cgButton) = (.leftMouseDown, .leftMouseUp, .left)
        case .right: (down, up, cgButton) = (.rightMouseDown, .rightMouseUp, .right)
        case .middle: (down, up, cgButton) = (.otherMouseDown, .otherMouseUp, .center)
        }
        guard let move = CGEvent(
            mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: point,
            mouseButton: .left),
            let press = CGEvent(
                mouseEventSource: nil, mouseType: down, mouseCursorPosition: point,
                mouseButton: cgButton),
            let release = CGEvent(
                mouseEventSource: nil, mouseType: up, mouseCursorPosition: point,
                mouseButton: cgButton)
        else { throw unavailable }
        if button == .middle {
            press.setIntegerValueField(.mouseEventButtonNumber, value: 2)
            release.setIntegerValueField(.mouseEventButtonNumber, value: 2)
        }
        return [move, press, release]
    case .press(let modifiers, let key):
        var held = CGEventFlags()
        var events: [CGEvent] = []
        func emit(_ qcode: String, _ isDown: Bool) throws {
            guard let code = keyCodes[qcode],
                let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: isDown)
            else { throw unavailable }
            event.flags = held
            events.append(event)
        }
        for modifier in modifiers {
            held.insert(modifierFlag(modifier))
            try emit(modifier, true)
        }
        try emit(key, true)
        try emit(key, false)
        for modifier in modifiers.reversed() {
            held.remove(modifierFlag(modifier))
            try emit(modifier, false)
        }
        return events
    }
}

private func modifierFlag(_ qcode: String) -> CGEventFlags {
    switch qcode {
    case "shift": return .maskShift
    case "ctrl": return .maskControl
    case "alt": return .maskAlternate
    default: return .maskCommand
    }
}

// MARK: - Environment and listener (guest only)

/// Refuse anything that is not a guest: the kernel must report a virtual machine, and the vsock
/// local CID must be a guest CID (not the host's 2, nor the reserved 0/1 or the hypervisor's).
/// Returns the local CID.
func verifyGuestEnvironment() throws -> UInt32 {
    var vmm: Int32 = 0
    var size = MemoryLayout<Int32>.size
    guard sysctlbyname("kern.hv_vmm_present", &vmm, &size, nil, 0) == 0, vmm == 1 else {
        throw GuestFailure("refusing to run: this is not a virtual machine guest")
    }
    let fd = socket(afVsock, SOCK_STREAM, 0)
    guard fd >= 0 else {
        throw GuestFailure("refusing to run: AF_VSOCK is not available in this guest")
    }
    defer { close(fd) }
    var cid: UInt32 = 0
    guard ioctl(fd, Wire.localCIDRequest, &cid) == 0 else {
        throw GuestFailure("refusing to run: could not read the guest virtio-socket CID")
    }
    guard cid >= Wire.minGuestCID, cid != UInt32.max else {
        throw GuestFailure("refusing to run: CID \(cid) is not a guest CID")
    }
    return cid
}

private let afVsock = Wire.addressFamily

/// sockaddr_vm in the layout of `sys/vsock.h` (12 bytes).
private func sockaddrVM(cid: UInt32, port: UInt32) -> [UInt8] {
    var raw = [UInt8](repeating: 0, count: Int(Wire.sockaddrVMSize))
    raw[0] = Wire.sockaddrVMSize
    raw[1] = UInt8(Wire.addressFamily)
    raw.withUnsafeMutableBytes { buffer in
        buffer.storeBytes(of: port, toByteOffset: 4, as: UInt32.self)
        buffer.storeBytes(of: cid, toByteOffset: 8, as: UInt32.self)
    }
    return raw
}

private func cidOf(sockaddr raw: [UInt8]) -> UInt32 {
    raw.withUnsafeBytes { $0.loadUnaligned(fromByteOffset: 8, as: UInt32.self) }
}

private func listenOnVsock(cid: UInt32) throws -> Int32 {
    let fd = socket(afVsock, SOCK_STREAM, 0)
    guard fd >= 0 else { throw GuestFailure("could not create the virtio socket") }
    var address = sockaddrVM(cid: cid, port: Wire.port)
    let bound = address.withUnsafeMutableBytes { raw in
        bind(fd, raw.baseAddress!.assumingMemoryBound(to: sockaddr.self),
            socklen_t(Wire.sockaddrVMSize))
    }
    guard bound == 0, listen(fd, 1) == 0 else {
        close(fd)
        throw GuestFailure("could not listen on virtio socket port \(Wire.port)")
    }
    return fd
}

/// Accept one peer, and only from the host CID.
private func acceptHost(_ listener: Int32) -> Int32? {
    var raw = [UInt8](repeating: 0, count: 16)
    var length = socklen_t(raw.count)
    let fd = raw.withUnsafeMutableBytes { buffer in
        accept(listener, buffer.baseAddress!.assumingMemoryBound(to: sockaddr.self), &length)
    }
    guard fd >= 0 else { return nil }
    guard cidOf(sockaddr: raw) == Wire.hostCID else {
        close(fd)
        return nil
    }
    var timeout = timeval(tv_sec: Wire.idleTimeoutSeconds, tv_usec: 0)
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
    var yes: Int32 = 1
    setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &yes, socklen_t(MemoryLayout<Int32>.size))
    return fd
}

#if !DIAL_GUEST_TESTS
@main
enum GuestMain {
    static func main() {
        guard CommandLine.arguments.dropFirst() == ["--serve"] else {
            FileHandle.standardError.write(Data("usage: dial-guest --serve (inside a macOS guest)\n".utf8))
            exit(2)
        }
        // Writes to a closed peer return EPIPE instead of killing the process.
        signal(SIGPIPE, SIG_IGN)
        do {
            let cid = try verifyGuestEnvironment()
            let listener = try listenOnVsock(cid: cid)
            FileHandle.standardError.write(Data("dial-guest listening on vsock port \(Wire.port)\n".utf8))
            while true {
                guard let peer = acceptHost(listener) else { continue }
                serve(connection: peer, backend: LiveBackend())
                close(peer)
            }
        } catch let failure as GuestFailure {
            FileHandle.standardError.write(Data((failure.message + "\n").utf8))
            exit(1)
        } catch {
            exit(1)
        }
    }
}
#endif
