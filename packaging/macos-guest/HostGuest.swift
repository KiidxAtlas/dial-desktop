import Darwin
import Foundation

/// A failure of the guest channel itself. The session treats it as fatal for the run.
struct GuestTransportError: Error {
    let message: String
}

/// An open connection to the guest companion. `fileDescriptor` stays valid until `close()`.
struct GuestChannel {
    let fileDescriptor: Int32
    let close: () -> Void
}

enum GuestReplyKind {
    case screendump
    case input

    /// Largest payload this kind of reply may carry after the status byte.
    var maxPayload: Int {
        switch self {
        case .screendump: return HostLimits.maxScreendumpBytes
        case .input: return 0
        }
    }
}

enum GuestOutcome: Equatable {
    /// Success. Screendumps carry the P6 PPM; input replies carry nothing.
    case payload(Data)
    /// The guest refused the request with a readable message.
    case refused(String)
}

/// One request/reply exchange at a time over a guest channel. Every read and write waits with
/// `poll` and gives up after `timeoutSeconds`, so a silent guest cannot hang the helper.
final class GuestLink {
    private let fileDescriptor: Int32
    private let timeout: TimeInterval
    private let onClose: () -> Void
    private var closed = false
    /// Wall-clock limit for the exchange in progress. Set once per exchange, not per read, so a
    /// guest that trickles bytes cannot extend it.
    private var exchangeDeadline = Date.distantFuture

    init(channel: GuestChannel, timeoutSeconds: Int) {
        fileDescriptor = channel.fileDescriptor
        timeout = TimeInterval(timeoutSeconds)
        onClose = channel.close
    }

    func close() {
        guard !closed else {
            return
        }
        closed = true
        onClose()
    }

    /// Send one request and read its reply. Protocol violations throw `GuestTransportError`;
    /// a guest-reported error comes back as `.refused` with its text.
    func exchange(_ request: [String: Any], kind: GuestReplyKind) throws -> GuestOutcome {
        guard !closed else {
            throw GuestTransportError(message: "the guest channel is closed")
        }
        exchangeDeadline = Date().addingTimeInterval(timeout)
        try writeAll(try guestRequestFrame(request))
        let length = Int(try readUInt32())
        guard length > 0 else {
            throw GuestTransportError(message: "the guest reply has no status byte")
        }
        guard length <= 1 + HostLimits.maxScreendumpBytes else {
            throw GuestTransportError(message: "the guest reply is too large")
        }
        let status = try readExact(1)[0]
        let bodyLength = length - 1
        switch status {
        case 0:
            if kind == .input && bodyLength == 0 { return .payload(Data()) }
            guard kind == .screendump, bodyLength <= kind.maxPayload else {
                throw GuestTransportError(message: "the guest sent an unexpected payload")
            }
            let body = Data(try readExact(bodyLength))
            guard isPPM(body) else {
                throw GuestTransportError(message: "the guest screen image is not a P6 PPM")
            }
            return .payload(body)
        case 1:
            guard bodyLength <= HostLimits.maxGuestErrorBytes else {
                throw GuestTransportError(message: "the guest error is too large")
            }
            let body = Data(try readExact(bodyLength))
            guard !body.isEmpty, let text = String(data: body, encoding: .utf8),
                text.unicodeScalars.allSatisfy({ $0.value >= 0x20 && $0.value != 0x7F })
            else {
                return .refused("the guest sent an unreadable error")
            }
            return .refused(text)
        default:
            throw GuestTransportError(message: "the guest sent an unknown reply status")
        }
    }

    private func readUInt32() throws -> UInt32 {
        let bytes = try readExact(4)
        return bytes.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
    }

    private func readExact(_ count: Int) throws -> [UInt8] {
        var buffer = [UInt8](repeating: 0, count: count)
        var offset = 0
        while offset < count {
            try waitForDescriptor(events: Int16(POLLIN))
            let received = buffer.withUnsafeMutableBytes { raw in
                read(fileDescriptor, raw.baseAddress! + offset, count - offset)
            }
            if received > 0 {
                offset += received
            } else if received == 0 {
                throw GuestTransportError(message: "the guest closed its channel")
            } else if errno != EINTR {
                throw GuestTransportError(message: "could not read from the guest: \(String(cString: strerror(errno)))")
            }
        }
        return buffer
    }

    private func writeAll(_ data: Data) throws {
        let bytes = [UInt8](data)
        var offset = 0
        while offset < bytes.count {
            try waitForDescriptor(events: Int16(POLLOUT))
            let sent = bytes.withUnsafeBytes { raw in
                write(fileDescriptor, raw.baseAddress! + offset, bytes.count - offset)
            }
            if sent > 0 {
                offset += sent
            } else if sent < 0 && errno != EINTR {
                throw GuestTransportError(message: "could not reach the guest: \(String(cString: strerror(errno)))")
            }
        }
    }

    private func waitForDescriptor(events: Int16) throws {
        var descriptor = pollfd(fd: fileDescriptor, events: events, revents: 0)
        while true {
            let remaining = exchangeDeadline.timeIntervalSinceNow
            guard remaining > 0 else {
                throw GuestTransportError(message: "the guest did not answer in time")
            }
            let milliseconds = Int32(min(remaining * 1000, Double(Int32.max)).rounded(.up))
            let ready = poll(&descriptor, 1, milliseconds)
            if ready > 0 {
                return
            }
            if ready == 0 {
                throw GuestTransportError(message: "the guest did not answer in time")
            }
            if errno != EINTR {
                throw GuestTransportError(message: "could not wait for the guest: \(String(cString: strerror(errno)))")
            }
        }
    }
}
