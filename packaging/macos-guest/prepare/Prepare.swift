// Prepare.swift: optional, offline base preparation for the macOS guest.
//
// This is a standalone developer utility. It is not part of the dial build and not a
// repository prerequisite. It installs macOS from a user-supplied, locally stored IPSW into
// a NEW private folder, then shows Setup Assistant in a VZVirtualMachineView window so the
// user completes it inside the VM.
//
// Guarantees (see README.md for the full list):
//   * no guest network: the VM has no network device. Nothing is downloaded unless
//     --download-latest is given, which fetches Apple's restore image for this Mac once
//   * no host sharing: no directory shares, no clipboard/spice, no vsock, no serial console
//   * the destination must not exist; it is created with mode 0700 and never overwritten
//   * consent is required on an interactive terminal before anything is written, unless the
//     caller (Dial's App panel) already collected it and passes --consent-given
//   * the only files written are Disk.img, HardwareModel, MachineIdentifier, AuxiliaryStorage
//   * on cancel or failure only those four files this run created are removed, then their folder
//   * no project or host app is ever executed
//
// Usage:
//   Prepare --ipsw PATH --dest NEW_DIR --disk-gib N [--cpus N] [--memory-gib N] [--tools-iso PATH]
//           [--download-latest] [--consent-given]
//
// Exit codes: 0 success, 2 refused (nothing created), 1 failure (partial prep removed),
// 130 cancelled by the user (partial prep removed).

import AppKit
import Foundation
import Virtualization

// MARK: - Errors and output

enum OfflinePrepError: Error {
    case usage(String)
    case refused(String)
    case failed(String)
}

enum OfflinePrepOutput {
    static func say(_ text: String) {
        FileHandle.standardOutput.write(Data((text + "\n").utf8))
    }

    static func warn(_ text: String) {
        FileHandle.standardError.write(Data(("prepare: " + text + "\n").utf8))
    }
}

// MARK: - Artifacts

/// The only files this utility writes. The importer takes exactly these four.
enum OfflinePrepArtifact: String, CaseIterable {
    case disk = "Disk.img"
    case hardwareModel = "HardwareModel"
    case machineIdentifier = "MachineIdentifier"
    case auxiliaryStorage = "AuxiliaryStorage"
}

// MARK: - Arguments

struct OfflinePrepOptions {
    var ipswPath: String
    var destPath: String
    var diskGiB: Int
    var cpus: Int?
    var memoryGiB: Int?
    var toolsISOPath: String?
    /// Download Apple's latest supported restore image to `ipswPath` first, when it is not there.
    var downloadLatest = false
    /// The caller showed the plan and collected consent itself.
    var consentGiven = false
}

enum OfflinePrepArgs {
    static let usage = """
        usage: Prepare --ipsw PATH --dest NEW_DIR --disk-gib N [--cpus N] [--memory-gib N] [--tools-iso PATH]
          --ipsw       local, user-owned restore image (.ipsw); downloaded only with --download-latest
          --dest       a folder that does NOT exist yet; its parent must be yours and not group/world-writable
          --disk-gib   virtual disk size in GiB (64...4096); required, no default
          --cpus       optional vCPU count (defaults to the restore image minimum, up to 4)
          --memory-gib optional memory in GiB (defaults to 8, or the restore image minimum)
          --tools-iso  optional read-only .iso/.cdr with the guest companion, attached over USB
          --download-latest  download Apple's latest restore image for this Mac to --ipsw if it is missing
          --consent-given    the caller already collected consent (no terminal prompt)
        """

    static let knownFlags: Set<String> = ["--ipsw", "--dest", "--disk-gib", "--cpus", "--memory-gib", "--tools-iso"]
    static let switches: Set<String> = ["--download-latest", "--consent-given"]

    static func parse(_ argv: [String]) throws -> OfflinePrepOptions {
        var values: [String: String] = [:]
        var index = 0
        while index < argv.count {
            let flag = argv[index]
            if switches.contains(flag) {
                guard values[flag] == nil else {
                    throw OfflinePrepError.usage("\(flag) given more than once")
                }
                values[flag] = "1"
                index += 1
                continue
            }
            guard knownFlags.contains(flag) else {
                throw OfflinePrepError.usage("unknown argument '\(flag)'")
            }
            guard index + 1 < argv.count else {
                throw OfflinePrepError.usage("\(flag) needs a value")
            }
            let value = argv[index + 1]
            guard !value.hasPrefix("--") else {
                throw OfflinePrepError.usage("\(flag) needs a value, found '\(value)'")
            }
            guard values[flag] == nil else {
                throw OfflinePrepError.usage("\(flag) given more than once")
            }
            values[flag] = value
            index += 2
        }

        for required in ["--ipsw", "--dest", "--disk-gib"] where values[required] == nil {
            throw OfflinePrepError.usage("missing required \(required)")
        }

        guard let diskText = values["--disk-gib"], let diskGiB = Int(diskText),
              (64...4096).contains(diskGiB) else {
            throw OfflinePrepError.refused("--disk-gib must be an integer from 64 to 4096 (explicit choice, no default)")
        }
        let cpus = try values["--cpus"].map { try positiveInt($0, flag: "--cpus", max: 1024) }
        let memory = try values["--memory-gib"].map { try positiveInt($0, flag: "--memory-gib", max: 1024) }

        return OfflinePrepOptions(
            ipswPath: values["--ipsw"]!,
            destPath: values["--dest"]!,
            diskGiB: diskGiB,
            cpus: cpus,
            memoryGiB: memory,
            toolsISOPath: values["--tools-iso"],
            downloadLatest: values["--download-latest"] != nil,
            consentGiven: values["--consent-given"] != nil
        )
    }

    private static func positiveInt(_ text: String, flag: String, max: Int) throws -> Int {
        guard let number = Int(text), (1...max).contains(number) else {
            throw OfflinePrepError.refused("\(flag) must be an integer from 1 to \(max)")
        }
        return number
    }
}

// MARK: - Path and input validation (no writes happen here)

struct OfflinePrepPaths {
    let ipsw: URL
    let dest: URL
    let toolsISO: URL?

    func file(_ artifact: OfflinePrepArtifact) -> URL {
        dest.appendingPathComponent(artifact.rawValue)
    }
}

enum OfflinePrepValidate {
    static func absoluteURL(_ path: String) -> URL {
        URL(fileURLWithPath: (path as NSString).expandingTildeInPath).standardizedFileURL
    }

    static func lstatInfo(_ path: String) -> stat? {
        var info = stat()
        return lstat(path, &info) == 0 ? info : nil
    }

    static func isRegularFile(_ info: stat) -> Bool {
        (info.st_mode & S_IFMT) == S_IFREG
    }

    static func realPath(_ path: String) -> String? {
        guard let resolved = realpath(path, nil) else { return nil }
        defer { free(resolved) }
        return String(cString: resolved)
    }

    static func validate(_ options: OfflinePrepOptions) throws -> OfflinePrepPaths {
        let ipsw = absoluteURL(options.ipswPath)
        try validateIPSW(ipsw)

        let dest = absoluteURL(options.destPath)
        try validateDestination(dest)

        var iso: URL?
        if let isoPath = options.toolsISOPath {
            let url = absoluteURL(isoPath)
            try validateToolsISO(url)
            guard url.path != ipsw.path else {
                throw OfflinePrepError.refused("--tools-iso and --ipsw must be different files")
            }
            iso = url
        }
        return OfflinePrepPaths(ipsw: ipsw, dest: dest, toolsISO: iso)
    }

    private static func validateIPSW(_ url: URL) throws {
        guard url.pathExtension.lowercased() == "ipsw" else {
            throw OfflinePrepError.refused("--ipsw must be a .ipsw file")
        }
        guard let info = lstatInfo(url.path) else {
            throw OfflinePrepError.refused("--ipsw does not exist: \(url.path)")
        }
        guard isRegularFile(info) else {
            throw OfflinePrepError.refused("--ipsw must be a regular file, not a symlink, folder or device")
        }
        guard info.st_size >= 1 << 30 else {
            throw OfflinePrepError.refused("--ipsw is implausibly small (under 1 GiB)")
        }
        guard access(url.path, R_OK) == 0 else {
            throw OfflinePrepError.refused("--ipsw is not readable by this user")
        }
    }

    private static func validateToolsISO(_ url: URL) throws {
        let ext = url.pathExtension.lowercased()
        guard ext == "iso" || ext == "cdr" else {
            throw OfflinePrepError.refused("--tools-iso must be a .iso or .cdr file")
        }
        guard let info = lstatInfo(url.path), isRegularFile(info) else {
            throw OfflinePrepError.refused("--tools-iso must exist as a regular file, not a symlink")
        }
        guard info.st_size > 0, info.st_size <= 32 << 30 else {
            throw OfflinePrepError.refused("--tools-iso must be non-empty and at most 32 GiB")
        }
        guard access(url.path, R_OK) == 0 else {
            throw OfflinePrepError.refused("--tools-iso is not readable by this user")
        }
    }

    private static func validateDestination(_ dest: URL) throws {
        let parent = dest.deletingLastPathComponent()
        guard dest.lastPathComponent != "", dest.lastPathComponent != "/",
              dest.lastPathComponent != ".", dest.lastPathComponent != ".." else {
            throw OfflinePrepError.refused("--dest must name a new folder")
        }
        guard let parentInfo = lstatInfo(parent.path), (parentInfo.st_mode & S_IFMT) == S_IFDIR else {
            throw OfflinePrepError.refused("the parent of --dest must exist as a real folder (not a symlink)")
        }
        guard realPath(parent.path) == parent.path else {
            throw OfflinePrepError.refused("give --dest by its real path; the parent path contains a symlink")
        }
        guard parentInfo.st_uid == getuid() else {
            throw OfflinePrepError.refused("the parent of --dest must be owned by you")
        }
        guard (parentInfo.st_mode & 0o022) == 0 else {
            throw OfflinePrepError.refused("the parent of --dest must not be group- or world-writable")
        }
        var probe = stat()
        guard lstat(dest.path, &probe) != 0, errno == ENOENT else {
            throw OfflinePrepError.refused("--dest already exists; choose a new folder name")
        }
    }

}

// MARK: - Host and resource checks

enum OfflinePrepHost {
    static func requireSupportedHost() throws {
        #if !arch(arm64)
        throw OfflinePrepError.refused("this utility needs an Apple silicon Mac")
        #else
        guard VZVirtualMachine.isSupported else {
            throw OfflinePrepError.refused("Virtualization is not supported on this Mac (check VM host limits)")
        }
        #endif
    }

    static func requireConsent(_ plan: String, phrase: String, given: Bool) throws {
        if given {
            OfflinePrepOutput.say(plan)
            return
        }
        guard isatty(STDIN_FILENO) == 1 else {
            throw OfflinePrepError.refused("consent needs an interactive terminal; nothing was created")
        }
        OfflinePrepOutput.say(plan)
        OfflinePrepOutput.say("\nType exactly the following line to consent, or anything else to cancel:")
        OfflinePrepOutput.say(phrase)
        guard let line = readLine(), line == phrase else {
            throw OfflinePrepError.refused("consent not given; nothing was created")
        }
    }
}

// MARK: - Restore image download (only with --download-latest)

/// Downloads Apple's latest restore image for this Mac to a destination that does not exist yet.
/// The file appears only when the download completed; a failed download leaves nothing behind.
final class OfflinePrepDownloader: NSObject, URLSessionDownloadDelegate, @unchecked Sendable {
    private let destination: URL
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Void, Error>?
    private var lastPercent = -1

    init(destination: URL) {
        self.destination = destination
    }

    static func latestURL() async throws -> URL {
        let image: VZMacOSRestoreImage = try await withCheckedThrowingContinuation { continuation in
            VZMacOSRestoreImage.fetchLatestSupported { continuation.resume(with: $0) }
        }
        let version = image.operatingSystemVersion
        OfflinePrepOutput.say("Downloading macOS \(version.majorVersion).\(version.minorVersion) from Apple.")
        return image.url
    }

    func download(_ url: URL) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            lock.withLock { self.continuation = continuation }
            let session = URLSession(configuration: .default, delegate: self, delegateQueue: nil)
            session.downloadTask(with: url).resume()
            session.finishTasksAndInvalidate()
        }
    }

    private func finish(_ result: Result<Void, Error>) {
        let pending = lock.withLock { () -> CheckedContinuation<Void, Error>? in
            defer { continuation = nil }
            return continuation
        }
        pending?.resume(with: result)
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didWriteData _: Int64,
                    totalBytesWritten written: Int64, totalBytesExpectedToWrite total: Int64) {
        guard total > 0 else { return }
        let percent = Int(written * 100 / total)
        guard percent != lastPercent else { return }
        lastPercent = percent
        OfflinePrepOutput.say("download progress: \(percent)%")
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didFinishDownloadingTo location: URL) {
        if let http = downloadTask.response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            finish(.failure(OfflinePrepError.failed("the download failed with HTTP \(http.statusCode)")))
            return
        }
        // The temporary file is removed when this method returns, so it is moved here.
        do {
            try FileManager.default.moveItem(at: location, to: destination)
            finish(.success(()))
        } catch {
            finish(.failure(OfflinePrepError.failed("could not keep the download: \(error.localizedDescription)")))
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        if let error {
            finish(.failure(OfflinePrepError.failed("the download failed: \(error.localizedDescription)")))
        }
    }
}

// MARK: - Session (runs on the main actor; owns the VM, window and cleanup)

@MainActor
final class OfflinePrepSession: NSObject, NSWindowDelegate, VZVirtualMachineDelegate {
    private let options: OfflinePrepOptions
    private let paths: OfflinePrepPaths

    private var createdDirectory = false
    private var createdArtifacts: Set<OfflinePrepArtifact> = []
    private var finished = false
    /// True only after the installer finished and this object became the setup VM's delegate.
    private var setupStarted = false

    private var vm: VZVirtualMachine?
    private var installer: VZMacOSInstaller?
    private var window: NSWindow?
    private var progressTimer: Timer?
    private var signalSources: [DispatchSourceSignal] = []

    init(options: OfflinePrepOptions, paths: OfflinePrepPaths) {
        self.options = options
        self.paths = paths
    }

    func start() async {
        do {
            let image = try await loadRestoreImage(paths.ipsw)
            // dial-guest uses ScreenCaptureKit's SCScreenshotManager, which needs guest macOS 14.
            guard image.operatingSystemVersion.majorVersion >= 14 else {
                throw OfflinePrepError.refused("the restore image is macOS \(image.operatingSystemVersion.majorVersion); the guest companion needs macOS 14 or later")
            }
            guard let requirements = image.mostFeaturefulSupportedConfiguration,
                  requirements.hardwareModel.isSupported else {
                throw OfflinePrepError.refused("this Mac cannot install this restore image (no supported hardware model)")
            }
            let cpus = try resolveCPUs(requirements)
            let memoryBytes = try resolveMemory(requirements)

            try OfflinePrepHost.requireConsent(
                plan(cpus: cpus, memoryBytes: memoryBytes),
                phrase: "yes, prepare the offline macOS base in this new folder",
                given: options.consentGiven
            )

            try makePrivateDirectory()
            try writeArtifacts(requirements: requirements)
            let machine = try buildMachine(cpus: cpus, memoryBytes: memoryBytes)
            try installSettingUp(machine: machine)
        } catch let error as OfflinePrepError {
            switch error {
            case .usage(let message), .refused(let message):
                abort(message, code: 2)
            case .failed(let message):
                abort(message, code: 1)
            }
        } catch {
            abort("unexpected error: \(error)", code: 1)
        }
    }

    // MARK: Plan and resources

    private func plan(cpus: Int, memoryBytes: UInt64) -> String {
        """

        Offline macOS base preparation
          restore image : \(paths.ipsw.path) (read only, used locally)
          new folder    : \(paths.dest.path) (mode 0700, must not exist; created now)
          writes        : Disk.img (\(options.diskGiB) GiB sparse), HardwareModel, MachineIdentifier, AuxiliaryStorage
          VM            : \(cpus) vCPU, \(memoryBytes >> 30) GiB memory
          tools ISO     : \(paths.toolsISO?.path ?? "none") (attached read-only over USB)
          excluded      : network, host folder sharing, clipboard, vsock, serial console, audio
          you will      : install macOS, then complete Setup Assistant inside the VM window
          cancel        : closing the window or Ctrl-C deletes only the files this run created
          finish        : shut macOS down from the guest's Apple menu; the folder is then final
        """
    }

    private func resolveCPUs(_ requirements: VZMacOSConfigurationRequirements) throws -> Int {
        let minimum = Int(requirements.minimumSupportedCPUCount)
        let fallback = max(minimum, min(4, ProcessInfo.processInfo.activeProcessorCount))
        let cpus = options.cpus ?? fallback
        guard cpus >= minimum,
              cpus >= Int(VZVirtualMachineConfiguration.minimumAllowedCPUCount),
              cpus <= Int(VZVirtualMachineConfiguration.maximumAllowedCPUCount) else {
            throw OfflinePrepError.refused("--cpus \(cpus) is outside what this restore image and host allow (minimum \(minimum))")
        }
        return cpus
    }

    private func resolveMemory(_ requirements: VZMacOSConfigurationRequirements) throws -> UInt64 {
        let minimum = requirements.minimumSupportedMemorySize
        let bytes = options.memoryGiB.map { UInt64($0) << 30 } ?? max(minimum, 8 << 30)
        guard bytes >= minimum,
              bytes >= VZVirtualMachineConfiguration.minimumAllowedMemorySize,
              bytes <= VZVirtualMachineConfiguration.maximumAllowedMemorySize,
              bytes < ProcessInfo.processInfo.physicalMemory else {
            throw OfflinePrepError.refused("--memory-gib is outside what this restore image and host allow")
        }
        return bytes
    }

    // MARK: Files

    private func loadRestoreImage(_ url: URL) async throws -> VZMacOSRestoreImage {
        // Local file only. The remote fetchLatestSupported API is deliberately not used.
        try await withCheckedThrowingContinuation { continuation in
            VZMacOSRestoreImage.load(from: url) { result in
                continuation.resume(with: result)
            }
        }
    }

    private func makePrivateDirectory() throws {
        // mkdir fails if anything already exists at the path, so a race cannot reuse a folder.
        guard mkdir(paths.dest.path, 0o700) == 0 else {
            throw OfflinePrepError.refused("could not create \(paths.dest.path) (errno \(errno)); nothing was created")
        }
        createdDirectory = true
    }

    private func writeArtifacts(requirements: VZMacOSConfigurationRequirements) throws {
        let hardwareModel = requirements.hardwareModel
        try writeNew(.hardwareModel, hardwareModel.dataRepresentation)

        let identifier = VZMacMachineIdentifier()
        try writeNew(.machineIdentifier, identifier.dataRepresentation)

        try createSparseDisk()
        // Creating aux storage without the allow-overwrite option refuses an existing file.
        createdArtifacts.insert(.auxiliaryStorage)
        _ = try VZMacAuxiliaryStorage(
            creatingStorageAt: paths.file(.auxiliaryStorage),
            hardwareModel: hardwareModel,
            options: []
        )
    }

    private func writeNew(_ artifact: OfflinePrepArtifact, _ data: Data) throws {
        // O_EXCL: only a file this call actually creates is recorded for cleanup.
        let fd = open(paths.file(artifact).path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
        guard fd >= 0 else {
            throw OfflinePrepError.failed("could not create \(artifact.rawValue) (errno \(errno))")
        }
        createdArtifacts.insert(artifact)
        defer { close(fd) }
        try data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) throws -> Void in
            guard let base = raw.baseAddress else { return }
            var offset = 0
            while offset < raw.count {
                let written = write(fd, base + offset, raw.count - offset)
                guard written > 0 else {
                    throw OfflinePrepError.failed("could not write \(artifact.rawValue) (errno \(errno))")
                }
                offset += written
            }
        }
        guard fsync(fd) == 0 else {
            throw OfflinePrepError.failed("could not sync \(artifact.rawValue)")
        }
    }

    private func createSparseDisk() throws {
        let fd = open(paths.file(.disk).path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
        guard fd >= 0 else {
            throw OfflinePrepError.failed("could not create Disk.img (errno \(errno))")
        }
        createdArtifacts.insert(.disk)
        defer { close(fd) }
        guard ftruncate(fd, off_t(options.diskGiB) << 30) == 0, fsync(fd) == 0 else {
            throw OfflinePrepError.failed("could not size Disk.img to \(options.diskGiB) GiB")
        }
    }

    // MARK: VM

    private func buildMachine(cpus: Int, memoryBytes: UInt64) throws -> VZVirtualMachine {
        let hardwareModel = try readHardwareModel()
        let identifier = try readMachineIdentifier()
        let auxiliary = VZMacAuxiliaryStorage(url: paths.file(.auxiliaryStorage))

        let platform = VZMacPlatformConfiguration()
        platform.hardwareModel = hardwareModel
        platform.machineIdentifier = identifier
        platform.auxiliaryStorage = auxiliary

        let configuration = VZVirtualMachineConfiguration()
        configuration.platform = platform
        configuration.bootLoader = VZMacOSBootLoader()
        configuration.cpuCount = cpus
        configuration.memorySize = memoryBytes

        let disk = try VZDiskImageStorageDeviceAttachment(url: paths.file(.disk), readOnly: false)
        configuration.storageDevices = [VZVirtioBlockDeviceConfiguration(attachment: disk)]

        let graphics = VZMacGraphicsDeviceConfiguration()
        // Same display as the runtime (packaging/macos-guest/HostVirtualMachine.swift MachineLayout), so
        // the Setup Assistant layout matches the layout the base boots with later.
        graphics.displays = [VZMacGraphicsDisplayConfiguration(widthInPixels: 1280, heightInPixels: 800, pixelsPerInch: 80)]
        configuration.graphicsDevices = [graphics]
        configuration.keyboards = [VZMacKeyboardConfiguration()]
        configuration.pointingDevices = [VZUSBScreenCoordinatePointingDeviceConfiguration()]
        configuration.entropyDevices = [VZVirtioEntropyDeviceConfiguration()]

        // Explicitly empty: no network, no host sharing, no sockets, no serial, no balloon.
        configuration.networkDevices = []
        configuration.directorySharingDevices = []
        // No vsock here on purpose: the guest companion's vsock listener is attached only by the runtime.
        configuration.socketDevices = []
        configuration.serialPorts = []
        configuration.audioDevices = []
        configuration.memoryBalloonDevices = []

        if let isoURL = paths.toolsISO {
            let isoAttachment = try VZDiskImageStorageDeviceAttachment(url: isoURL, readOnly: true)
            let controller = VZXHCIControllerConfiguration()
            controller.usbDevices = [VZUSBMassStorageDeviceConfiguration(attachment: isoAttachment)]
            configuration.usbControllers = [controller]
        }

        try configuration.validate()
        return VZVirtualMachine(configuration: configuration, queue: .main)
    }

    private func readHardwareModel() throws -> VZMacHardwareModel {
        let data = try Data(contentsOf: paths.file(.hardwareModel))
        guard let model = VZMacHardwareModel(dataRepresentation: data), model.isSupported else {
            throw OfflinePrepError.failed("HardwareModel is not supported on this host")
        }
        return model
    }

    private func readMachineIdentifier() throws -> VZMacMachineIdentifier {
        let data = try Data(contentsOf: paths.file(.machineIdentifier))
        guard let identifier = VZMacMachineIdentifier(dataRepresentation: data) else {
            throw OfflinePrepError.failed("MachineIdentifier could not be read back")
        }
        return identifier
    }

    private func installSettingUp(machine: VZVirtualMachine) throws {
        vm = machine

        let installer = VZMacOSInstaller(virtualMachine: machine, restoringFromImageAt: paths.ipsw)
        self.installer = installer

        showWindow(for: machine)
        installSignalHandlers()

        progressTimer = Timer.scheduledTimer(withTimeInterval: 15, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.reportProgress() }
        }

        OfflinePrepOutput.say("Installing macOS from the local restore image. This takes a while.")
        installer.install { [weak self] result in
            Task { @MainActor in self?.installFinished(result) }
        }
    }

    private func installFinished(_ result: Result<Void, Error>) {
        guard !finished else { return }
        progressTimer?.invalidate()
        switch result {
        case .failure(let error):
            abort("installation failed: \(error.localizedDescription)", code: 1)
        case .success:
            OfflinePrepOutput.say("Install finished. Complete Setup Assistant in the window, then shut macOS down from the Apple menu.")
            guard let machine = vm, machine.canStart else {
                abort("the VM did not return to a stopped state after install", code: 1)
            }
            // The delegate is attached only for the setup VM, after the installer has finished. No
            // delegate exists during the installer run, so a stop from that phase cannot publish success.
            machine.delegate = self
            setupStarted = true
            machine.start { [weak self] result in
                Task { @MainActor in
                    if case .failure(let error) = result {
                        self?.abort("could not start the VM for Setup Assistant: \(error.localizedDescription)", code: 1)
                    }
                }
            }
        }
    }

    private func reportProgress() {
        guard !finished, let progress = installer?.progress else { return }
        OfflinePrepOutput.say(String(format: "install progress: %.0f%%", progress.fractionCompleted * 100))
    }

    // MARK: Window and signals

    private func showWindow(for machine: VZVirtualMachine) {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1280, height: 800),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Dial offline macOS base: complete Setup Assistant here only"
        window.isReleasedWhenClosed = false
        window.delegate = self

        let view = VZVirtualMachineView()
        view.virtualMachine = machine
        view.capturesSystemKeys = false
        view.autoresizingMask = [.width, .height]
        window.contentView = view

        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        self.window = window
    }

    private func installSignalHandlers() {
        for number in [SIGINT, SIGTERM] {
            signal(number, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: number, queue: .main)
            source.setEventHandler { [weak self] in
                MainActor.assumeIsolated { self?.cancel("interrupted") }
            }
            source.resume()
            signalSources.append(source)
        }
    }

    private func cancel(_ reason: String) {
        guard !finished else { return }
        abort("cancelled (\(reason))", code: 130)
    }

    func windowWillClose(_ notification: Notification) {
        guard !finished else { return }
        cancel("setup window closed before the guest shut down")
    }

    // MARK: VZVirtualMachineDelegate

    nonisolated func guestDidStop(_ virtualMachine: VZVirtualMachine) {
        // The VM is created on the main queue, so this callback runs on the main actor.
        MainActor.assumeIsolated { guestStopped(virtualMachine) }
    }

    /// Only the setup VM's own clean guest shutdown, after the installer finished, may publish success.
    /// The delegate is attached only in the setup phase, so this is a second guard, not the only one.
    private func guestStopped(_ machine: VZVirtualMachine) {
        guard !finished else { return }
        guard setupStarted, machine === vm else {
            OfflinePrepOutput.warn("the VM stopped during the installer run; waiting for the installer's result")
            return
        }
        finishSuccessfully()
    }

    nonisolated func virtualMachine(_ virtualMachine: VZVirtualMachine, didStopWithError error: Error) {
        MainActor.assumeIsolated { vmStopped(withError: error) }
    }

    private func vmStopped(withError error: Error) {
        guard !finished else { return }
        abort("the VM stopped with an error before setup finished: \(error.localizedDescription)", code: 1)
    }

    // MARK: Completion and cleanup

    private func finishSuccessfully() {
        guard !finished else { return }
        finished = true
        progressTimer?.invalidate()
        do {
            try verifyFinalFiles()
        } catch {
            // Setup ended but the folder is not a complete base: treat it as incomplete.
            abort("final verification failed: \(error)", code: 1, stopVM: false)
        }
        OfflinePrepOutput.say("Offline base is ready. Import exactly these four files:")
        for artifact in OfflinePrepArtifact.allCases {
            OfflinePrepOutput.say("  \(paths.file(artifact).path)")
        }
        exit(0)
    }

    private func verifyFinalFiles() throws {
        for artifact in OfflinePrepArtifact.allCases {
            let url = paths.file(artifact)
            guard let info = OfflinePrepValidate.lstatInfo(url.path), OfflinePrepValidate.isRegularFile(info) else {
                throw OfflinePrepError.failed("\(artifact.rawValue) is missing or not a regular file")
            }
            if artifact == .disk, info.st_size < Int64(options.diskGiB) << 30 {
                throw OfflinePrepError.failed("Disk.img is smaller than requested")
            }
        }
    }

    /// Stops the VM if needed, removes the artifacts this run created, then exits. The "removed"
    /// message is printed only when every removal succeeded; otherwise the leftovers are listed.
    private func abort(_ message: String, code: Int32, stopVM: Bool = true) -> Never {
        finished = true
        progressTimer?.invalidate()
        if stopVM, let machine = vm, machine.canStop {
            machine.stop { _ in }
        }
        if code == 0 {
            OfflinePrepOutput.say(message)
        } else {
            OfflinePrepOutput.warn(message)
        }
        if createdDirectory {
            let failures = removeIncomplete()
            if failures.isEmpty {
                OfflinePrepOutput.say("removed the incomplete preparation from \(paths.dest.path)")
            } else {
                OfflinePrepOutput.warn("cleanup did NOT complete; these were left in place, delete them manually:")
                for failure in failures {
                    OfflinePrepOutput.warn("  " + failure)
                }
            }
        }
        exit(code)
    }

    /// Removes the artifacts this run recorded as created, then their folder if it is empty.
    /// Returns one line per path that could not be removed. An empty result means everything this run
    /// made is gone. A path that is absent (ENOENT) is not a failure.
    private func removeIncomplete() -> [String] {
        var failures: [String] = []
        for artifact in OfflinePrepArtifact.allCases where createdArtifacts.contains(artifact) {
            let path = paths.file(artifact).path
            var info = stat()
            if lstat(path, &info) != 0 {
                if errno != ENOENT {
                    failures.append("\(path): \(String(cString: strerror(errno)))")
                }
                continue
            }
            guard OfflinePrepValidate.isRegularFile(info) else {
                failures.append("\(path): not a regular file, not removed")
                continue
            }
            if unlink(path) != 0 {
                failures.append("\(path): \(String(cString: strerror(errno)))")
            }
        }
        guard failures.isEmpty else {
            // rmdir would fail on a non-empty folder anyway; say why instead of a second error.
            failures.append("\(paths.dest.path): kept because files remain in it")
            return failures
        }
        // rmdir succeeds only when the folder is empty, so unrelated files are never removed.
        if rmdir(paths.dest.path) != 0, errno != ENOENT {
            failures.append("\(paths.dest.path): \(String(cString: strerror(errno)))")
        }
        return failures
    }
}

// MARK: - Entry point

enum OfflinePrepEntry {
    @MainActor static func run() {
        let options: OfflinePrepOptions
        do {
            options = try OfflinePrepArgs.parse(Array(CommandLine.arguments.dropFirst()))
        } catch OfflinePrepError.usage(let message) {
            OfflinePrepOutput.warn(message)
            OfflinePrepOutput.warn(OfflinePrepArgs.usage)
            exit(2)
        } catch OfflinePrepError.refused(let message) {
            OfflinePrepOutput.warn("refused: \(message)")
            exit(2)
        } catch {
            OfflinePrepOutput.warn("refused: \(error)")
            exit(2)
        }

        do {
            try OfflinePrepHost.requireSupportedHost()
        } catch OfflinePrepError.refused(let message) {
            OfflinePrepOutput.warn("refused: \(message)")
            exit(2)
        } catch {
            OfflinePrepOutput.warn("refused: \(error)")
            exit(2)
        }

        let application = NSApplication.shared
        application.setActivationPolicy(.regular)
        Task { @MainActor in
            if options.downloadLatest {
                let ipsw = OfflinePrepValidate.absoluteURL(options.ipswPath)
                if OfflinePrepValidate.lstatInfo(ipsw.path) == nil {
                    do {
                        let url = try await OfflinePrepDownloader.latestURL()
                        try await OfflinePrepDownloader(destination: ipsw).download(url)
                    } catch {
                        OfflinePrepOutput.warn("could not download macOS: \(error)")
                        exit(1)
                    }
                }
            }
            let paths: OfflinePrepPaths
            do {
                paths = try OfflinePrepValidate.validate(options)
            } catch OfflinePrepError.refused(let message) {
                OfflinePrepOutput.warn("refused: \(message)")
                exit(2)
            } catch {
                OfflinePrepOutput.warn("refused: \(error)")
                exit(2)
            }
            let session = OfflinePrepSession(options: options, paths: paths)
            // Keep the session alive for the whole run; the delegate references are weak.
            sessionHolder = session
            await session.start()
        }
        application.run()
    }

    nonisolated(unsafe) static var sessionHolder: OfflinePrepSession?
}

// Top-level code runs on the main thread, which is the main actor's executor.
MainActor.assumeIsolated {
    OfflinePrepEntry.run()
}
