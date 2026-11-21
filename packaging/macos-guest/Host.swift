import Darwin
import Foundation

/// `dial-macos-guest-host run RUN_DIR [PROJECT_ISO]`: boots one macOS guest from RUN_DIR and serves
/// the runner's stdio QMP subset (`qmp_capabilities`, `screendump`, `input-send-event`) by
/// relaying to the guest companion. Host screen capture and host input are never used.
///
/// Stdout carries QMP replies only; diagnostics go to stderr. Stdin closing stops the machine.
@main
enum DialMacGuestHost {
    static func main() {
        signal(SIGPIPE, SIG_IGN)
        do {
            let request = try parseRunArguments(Array(CommandLine.arguments.dropFirst()))
            let mailbox = HostMailbox()
            let machine = VirtualMachineHost(
                configuration: try makeMachineConfiguration(request),
                mailbox: mailbox)
            startStandardInputReader(into: mailbox)
            let session = HostSession(
                machine: machine,
                mailbox: mailbox,
                runDirectory: request.runDirectory,
                output: writeStandardOutput,
                errorOutput: writeStandardError,
                bootTimeout: MachineLayout.bootTimeoutSeconds,
                retryInterval: MachineLayout.retryIntervalSeconds,
                replyTimeoutSeconds: MachineLayout.replyTimeoutSeconds)
            exit(session.run())
        } catch let failure as HostFailure {
            writeStandardError(failure.message)
            exit(failure.exitCode)
        } catch {
            writeStandardError("unexpected failure: \(error.localizedDescription)")
            exit(5)
        }
    }
}
