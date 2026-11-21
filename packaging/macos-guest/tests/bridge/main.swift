// Both production protocol implementations over a local socketpair; the only substitute is the
// guest screen/input backend. Never asks for macOS permissions and never boots a VM.
// swiftc -swift-version 5 -D DIAL_GUEST_TESTS Guest.swift Host{Arguments,Errors,Guest,Protocol}.swift tests/bridge/main.swift -o "$TMPDIR/bridge-test"
import Darwin
import Foundation

struct Fixture: GuestBackend {
    let pixel: [UInt8]
    func screendump() throws -> [UInt8] { pixel }
    func perform(_ gesture: Gesture) throws {
        guard gesture == .press(modifiers: [], key: "a") else { throw GuestFailure("unexpected gesture") }
    }
}

let image = Array("P6\n1 1\n255\n".utf8) + [255, 128, 0]
var pair: [Int32] = [-1, -1]
precondition(socketpair(AF_UNIX, SOCK_STREAM, 0, &pair) == 0)
let (client, server) = (pair[0], pair[1])
let done = DispatchSemaphore(value: 0)
DispatchQueue.global().async {
    serve(connection: server, backend: Fixture(pixel: image))
    close(server)
    done.signal()
}
let host = GuestLink(channel: GuestChannel(fileDescriptor: client, close: { close(client) }), timeoutSeconds: 2)
let picture = try host.exchange(["execute": "screendump"], kind: .screendump)
precondition(picture == .payload(Data(image)), "frame bytes changed across bridge")
let events = [true, false].map { ["type": "key", "data": ["down": $0, "key": ["type": "qcode", "data": "a"]]] as [String: Any] }
let key = try host.exchange(["execute": "input-send-event", "arguments": ["events": events]], kind: .input)
precondition(key == .payload(Data()), "input not acknowledged")
let bad = try host.exchange(["execute": "input-send-event", "arguments": ["events": events.prefix(1).map { $0 }]], kind: .input)
guard case .refused = bad else { fatalError("incomplete key press was not refused") }
host.close()
precondition(done.wait(timeout: .now() + 2) == .success, "guest loop did not exit on EOF")
print("Host/guest wire: exact frame, accepted key, refused partial key and disconnect passed")
