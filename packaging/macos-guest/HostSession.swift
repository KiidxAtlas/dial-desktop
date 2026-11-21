import Foundation

/// The virtual machine as the session sees it. The Virtualization implementation lives in
/// `HostVirtualMachine.swift`; tests substitute a fake. Completions may run on any thread.
protocol GuestMachine: AnyObject {
    /// Start the machine. `completion` gets `nil` on success or the error text.
    func start(completion: @escaping (String?) -> Void)
    /// Try one connection to the guest companion port.
    func connectGuest(completion: @escaping (Result<GuestChannel, Error>) -> Void)
    /// Force the machine down. `completion` runs once it is stopped or cannot be stopped.
    func stop(completion: @escaping () -> Void)
}

/// Runs one helper session: greets the runner, boots the machine, waits for the guest companion
/// to accept a connection, then answers QMP requests in order until the runner closes stdin.
///
/// Guest readiness is the companion accepting on vsock port 49152, not a faked QMP reply. The
/// runner's `qmp_capabilities` is therefore answered only after that connection exists.
final class HostSession {
    private let machine: GuestMachine
    private let mailbox: HostMailbox
    private let runDirectory: URL
    private let output: (Data) -> Void
    private let errorOutput: (String) -> Void
    private let bootTimeout: TimeInterval
    private let retryInterval: TimeInterval
    private let replyTimeoutSeconds: Int
    private var guest: GuestLink?
    private var pending: [Data] = []
    private var connectInFlight = false

    private var bootTimeoutMessage: String {
        "the guest companion did not accept a connection on vsock port \(HostLimits.guestPort) within \(Int(bootTimeout)) seconds. "
            + "Sign in to the guest and start the dial companion (dial-guest --serve), then retry"
    }

    private enum Step {
        case next
        case finish(HostFailure?)
    }

    init(
        machine: GuestMachine,
        mailbox: HostMailbox,
        runDirectory: URL,
        output: @escaping (Data) -> Void,
        errorOutput: @escaping (String) -> Void,
        bootTimeout: TimeInterval,
        retryInterval: TimeInterval,
        replyTimeoutSeconds: Int
    ) {
        self.machine = machine
        self.mailbox = mailbox
        self.runDirectory = runDirectory
        self.output = output
        self.errorOutput = errorOutput
        self.bootTimeout = bootTimeout
        self.retryInterval = retryInterval
        self.replyTimeoutSeconds = replyTimeoutSeconds
    }

    /// Run until stdin closes, a failure ends the run, or the boot deadline passes. Returns the
    /// process exit status. The machine is always asked to stop before returning.
    func run() -> Int32 {
        output(qmpGreeting())
        let deadline = Date().addingTimeInterval(bootTimeout)
        machine.start { [mailbox] error in mailbox.post(.machineStarted(error)) }
        requestConnect()
        while true {
            // The boot deadline is absolute and checked on every pass, so a stream of boot-time
            // events (retries, stray lines) cannot keep the wait alive.
            if guest == nil && Date() >= deadline {
                return finish(.boot(bootTimeoutMessage))
            }
            let event = guest == nil
                ? mailbox.next(timeout: max(0, deadline.timeIntervalSinceNow))
                : mailbox.next(timeout: nil)
            guard let event else {
                return finish(.boot(bootTimeoutMessage))
            }
            if case .finish(let failure) = handle(event) {
                return finish(failure)
            }
        }
    }

    private func handle(_ event: HostEvent) -> Step {
        switch event {
        case .inputClosed:
            return .finish(nil)
        case .lineTooLong:
            return .finish(.transport("a QMP request was longer than \(HostLimits.qmpLineBytes) bytes"))
        case .machineStarted(let error):
            if let error {
                return .finish(.boot("could not start the virtual machine: \(error)"))
            }
            return .next
        case .machineStopped(let error):
            let detail = error.map { ": \($0)" } ?? ""
            return .finish(.transport("the virtual machine stopped\(detail)"))
        case .retryGuestConnect:
            requestConnect()
            return .next
        case .guestConnectFailed:
            // Not yet listening is expected while the guest boots; the deadline bounds the wait.
            connectInFlight = false
            scheduleRetry()
            return .next
        case .guestConnected(let channel):
            connectInFlight = false
            guard guest == nil else {
                channel.close()
                return .next
            }
            guest = GuestLink(channel: channel, timeoutSeconds: replyTimeoutSeconds)
            let queued = pending
            pending = []
            for line in queued {
                let step = handleLine(line)
                if case .finish = step {
                    return step
                }
            }
            return .next
        case .line(let line):
            guard guest != nil else {
                pending.append(line)
                return .next
            }
            return handleLine(line)
        }
    }

    private func handleLine(_ line: Data) -> Step {
        if line.allSatisfy({ $0 == 0x20 || $0 == 0x09 || $0 == 0x0D }) {
            return .next
        }
        guard let guest else {
            output(qmpError("the guest is not connected"))
            return .next
        }
        let command: QMPCommand
        do {
            command = try parseQMPCommand(line)
        } catch let refusal as QMPRefusal {
            output(qmpError(refusal.description))
            return .next
        } catch {
            output(qmpError("malformed request"))
            return .next
        }
        switch command {
        case .capabilities:
            output(qmpReturn())
            return .next
        case .screendump(let filename):
            do {
                let destination = try screendumpDestination(filename: filename, runDirectory: runDirectory)
                switch try guest.exchange(["execute": "screendump"], kind: .screendump) {
                case .refused(let message):
                    output(qmpError(message))
                case .payload(let image):
                    try image.write(to: destination, options: .atomic)
                    output(qmpReturn())
                }
                return .next
            } catch let refusal as QMPRefusal {
                output(qmpError(refusal.description))
                return .next
            } catch let transport as GuestTransportError {
                output(qmpError(transport.message))
                return .finish(.transport(transport.message))
            } catch {
                output(qmpError("could not save the screen image: \(error.localizedDescription)"))
                return .next
            }
        case .inputSendEvent(let events):
            do {
                switch try guest.exchange(
                    ["execute": "input-send-event", "arguments": ["events": events]],
                    kind: .input
                ) {
                case .refused(let message):
                    output(qmpError(message))
                case .payload:
                    output(qmpReturn())
                }
                return .next
            } catch let transport as GuestTransportError {
                output(qmpError(transport.message))
                return .finish(.transport(transport.message))
            } catch {
                return .finish(.transport("the guest input channel failed"))
            }
        }
    }

    private func requestConnect() {
        guard !connectInFlight, guest == nil else {
            return
        }
        connectInFlight = true
        machine.connectGuest { [mailbox] result in
            switch result {
            case .success(let channel):
                mailbox.post(.guestConnected(channel))
            case .failure(let error):
                mailbox.post(.guestConnectFailed(String(describing: error)))
            }
        }
    }

    private func scheduleRetry() {
        DispatchQueue.global().asyncAfter(deadline: .now() + retryInterval) { [mailbox] in
            mailbox.post(.retryGuestConnect)
        }
    }

    /// Close the guest channel, stop the machine (bounded), and report any failure on stderr.
    private func finish(_ failure: HostFailure?) -> Int32 {
        guest?.close()
        guest = nil
        let stopped = DispatchSemaphore(value: 0)
        machine.stop { stopped.signal() }
        _ = stopped.wait(timeout: .now() + 15)
        if let failure {
            errorOutput(failure.message)
            return failure.exitCode
        }
        return 0
    }
}
