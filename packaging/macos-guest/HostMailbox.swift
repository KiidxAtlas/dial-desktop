import Foundation

/// Everything the session loop reacts to. Posted from the stdin reader, the machine's callbacks and
/// timers; consumed only on the session thread.
enum HostEvent {
    case line(Data)
    case inputClosed
    case lineTooLong
    case machineStarted(String?)
    case machineStopped(String?)
    case guestConnected(GuestChannel)
    case guestConnectFailed(String)
    case retryGuestConnect
}

/// Thread-safe FIFO of `HostEvent`s with a bounded wait.
final class HostMailbox {
    private let condition = NSCondition()
    private var events: [HostEvent] = []

    func post(_ event: HostEvent) {
        condition.lock()
        events.append(event)
        condition.signal()
        condition.unlock()
    }

    /// The next event, or `nil` if none arrives within `timeout` seconds (`nil` timeout waits forever).
    func next(timeout: TimeInterval?) -> HostEvent? {
        condition.lock()
        defer { condition.unlock() }
        let limit = timeout.map { Date().addingTimeInterval(max(0, $0)) }
        while events.isEmpty {
            if let limit {
                if !condition.wait(until: limit) && events.isEmpty {
                    return nil
                }
            } else {
                condition.wait()
            }
        }
        return events.removeFirst()
    }
}

/// Read QMP lines from `descriptor` on a background thread. EOF and oversize lines are posted as
/// events so the session can stop the machine instead of waiting on a closed control channel.
func startStandardInputReader(into mailbox: HostMailbox, descriptor: Int32 = STDIN_FILENO) {
    let thread = Thread {
        var pending: [UInt8] = []
        var chunk = [UInt8](repeating: 0, count: 4096)
        while true {
            let count = read(descriptor, &chunk, chunk.count)
            if count < 0 && errno == EINTR {
                continue
            }
            if count <= 0 {
                break
            }
            pending.append(contentsOf: chunk[0..<count])
            while let newline = pending.firstIndex(of: 0x0A) {
                mailbox.post(.line(Data(pending[0..<newline])))
                pending.removeFirst(newline + 1)
            }
            if pending.count > HostLimits.qmpLineBytes {
                mailbox.post(.lineTooLong)
                return
            }
        }
        if !pending.isEmpty {
            mailbox.post(.line(Data(pending)))
        }
        mailbox.post(.inputClosed)
    }
    thread.start()
}
