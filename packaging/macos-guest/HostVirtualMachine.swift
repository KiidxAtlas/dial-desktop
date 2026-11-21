import Foundation
import Virtualization

/// Fixed hardware for the guest. Only these devices are attached; there is no network, audio,
/// serial, clipboard or shared folder device.
enum MachineLayout {
    static let displayWidth = 1280
    static let displayHeight = 800
    static let pixelsPerInch = 80
    static let preferredMemoryBytes: UInt64 = 8 * 1024 * 1024 * 1024
    static let bootTimeoutSeconds: TimeInterval = 600
    static let retryIntervalSeconds: TimeInterval = 1
    static let replyTimeoutSeconds = 30
}

private func readRunFile(_ url: URL, describe: String) throws -> Data {
    do {
        return try Data(contentsOf: url)
    } catch {
        throw HostFailure.configuration("\(describe) cannot be read: \(error.localizedDescription)")
    }
}

/// Build and validate the Virtualization configuration for a run directory. Throws
/// `HostFailure` (exit 3) for missing, unsupported or invalid inputs.
func makeMachineConfiguration(_ request: RunRequest) throws -> VZVirtualMachineConfiguration {
    let directory = request.runDirectory
    func path(_ name: String) -> URL {
        directory.appendingPathComponent(name)
    }

    guard let hardwareModel = VZMacHardwareModel(
        dataRepresentation: try readRunFile(path(RunFileName.hardwareModel), describe: RunFileName.hardwareModel)
    ) else {
        throw HostFailure.configuration("HardwareModel is not a Mac hardware model description")
    }
    guard hardwareModel.isSupported else {
        throw HostFailure.configuration("this Mac does not support the guest's hardware model")
    }
    guard let identifier = VZMacMachineIdentifier(
        dataRepresentation: try readRunFile(path(RunFileName.machineIdentifier), describe: RunFileName.machineIdentifier)
    ) else {
        throw HostFailure.configuration("MachineIdentifier is not a Mac machine identifier")
    }
    let auxiliary = VZMacAuxiliaryStorage(contentsOf: path(RunFileName.auxiliaryStorage))
    let platform = VZMacPlatformConfiguration()
    platform.hardwareModel = hardwareModel
    platform.machineIdentifier = identifier
    platform.auxiliaryStorage = auxiliary

    let configuration = VZVirtualMachineConfiguration()
    configuration.platform = platform
    configuration.bootLoader = VZMacOSBootLoader()
    let halfCPUs = max(2, ProcessInfo.processInfo.activeProcessorCount / 2)
    configuration.cpuCount = min(
        max(halfCPUs, VZVirtualMachineConfiguration.minimumAllowedCPUCount),
        VZVirtualMachineConfiguration.maximumAllowedCPUCount)
    configuration.memorySize = min(
        max(MachineLayout.preferredMemoryBytes, VZVirtualMachineConfiguration.minimumAllowedMemorySize),
        VZVirtualMachineConfiguration.maximumAllowedMemorySize)

    let graphics = VZMacGraphicsDeviceConfiguration()
    graphics.displays = [
        VZMacGraphicsDisplayConfiguration(
            widthInPixels: MachineLayout.displayWidth,
            heightInPixels: MachineLayout.displayHeight,
            pixelsPerInch: MachineLayout.pixelsPerInch)
    ]
    configuration.graphicsDevices = [graphics]

    let disk: VZDiskImageStorageDeviceAttachment
    do {
        disk = try VZDiskImageStorageDeviceAttachment(url: path(RunFileName.disk), readOnly: false)
    } catch {
        throw HostFailure.configuration("Disk.img cannot be attached: \(error.localizedDescription)")
    }
    var storage: [VZStorageDeviceConfiguration] = [VZVirtioBlockDeviceConfiguration(attachment: disk)]
    if let iso = request.projectISO {
        // The ISO is read-only USB mass storage, which needs the xHCI controller (macOS 15+).
        guard #available(macOS 15.0, *) else {
            throw HostFailure.configuration("PROJECT_ISO needs macOS 15 or later on this host")
        }
        let isoAttachment: VZDiskImageStorageDeviceAttachment
        do {
            isoAttachment = try VZDiskImageStorageDeviceAttachment(url: iso, readOnly: true)
        } catch {
            throw HostFailure.configuration("PROJECT_ISO cannot be attached: \(error.localizedDescription)")
        }
        storage.append(VZUSBMassStorageDeviceConfiguration(attachment: isoAttachment))
        configuration.usbControllers = [VZXHCIControllerConfiguration()]
    }
    configuration.storageDevices = storage

    configuration.networkDevices = []
    configuration.audioDevices = []
    if #available(macOS 14.0, *) {
        configuration.keyboards = [VZMacKeyboardConfiguration()]
    } else {
        configuration.keyboards = [VZUSBKeyboardConfiguration()]
    }
    configuration.pointingDevices = [VZUSBScreenCoordinatePointingDeviceConfiguration()]
    configuration.socketDevices = [VZVirtioSocketDeviceConfiguration()]
    configuration.entropyDevices = [VZVirtioEntropyDeviceConfiguration()]

    do {
        try configuration.validate()
    } catch {
        throw HostFailure.configuration("the virtual machine configuration is invalid: \(error.localizedDescription)")
    }
    return configuration
}

/// Virtualization.framework machine. All framework calls run on `queue`; callbacks post to the
/// mailbox, so the session thread never touches framework objects.
final class VirtualMachineHost: NSObject, GuestMachine, VZVirtualMachineDelegate {
    private let queue = DispatchQueue(label: "dial.macos-guest.virtual-machine")
    private let machine: VZVirtualMachine
    private let mailbox: HostMailbox

    init(configuration: VZVirtualMachineConfiguration, mailbox: HostMailbox) {
        machine = VZVirtualMachine(configuration: configuration, queue: queue)
        self.mailbox = mailbox
        super.init()
        // Virtualization requires every VM operation, including the delegate, on the VM's queue.
        queue.sync {
            self.machine.delegate = self
        }
    }

    func start(completion: @escaping (String?) -> Void) {
        queue.async {
            self.machine.start { result in
                switch result {
                case .success:
                    completion(nil)
                case .failure(let error):
                    completion(error.localizedDescription)
                }
            }
        }
    }

    func connectGuest(completion: @escaping (Result<GuestChannel, Error>) -> Void) {
        queue.async {
            guard self.machine.state == .running,
                let device = self.machine.socketDevices.first as? VZVirtioSocketDevice
            else {
                completion(.failure(HostFailure.boot("the virtual machine is not running yet")))
                return
            }
            device.connect(toPort: HostLimits.guestPort) { result in
                switch result {
                case .success(let connection):
                    completion(.success(GuestChannel(
                        fileDescriptor: connection.fileDescriptor,
                        close: { [queue = self.queue] in queue.async { connection.close() } })))
                case .failure(let error):
                    completion(.failure(error))
                }
            }
        }
    }

    func stop(completion: @escaping () -> Void) {
        queue.async {
            guard self.machine.canStop else {
                completion()
                return
            }
            self.machine.stop { _ in
                completion()
            }
        }
    }

    func guestDidStop(_ virtualMachine: VZVirtualMachine) {
        mailbox.post(.machineStopped(nil))
    }

    func virtualMachine(_ virtualMachine: VZVirtualMachine, didStopWithError error: Error) {
        mailbox.post(.machineStopped(error.localizedDescription))
    }
}
