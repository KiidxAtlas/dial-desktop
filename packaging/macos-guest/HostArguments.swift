import Darwin
import Foundation

/// What `helper run` was asked to start.
struct RunRequest: Equatable {
    let runDirectory: URL
    /// Optional project ISO, attached read-only as USB mass storage.
    let projectISO: URL?
}

/// Files the Rust runner copies into RUN_DIR before starting the helper.
enum RunFileName {
    static let disk = "Disk.img"
    static let auxiliaryStorage = "AuxiliaryStorage"
    static let machineIdentifier = "MachineIdentifier"
    static let hardwareModel = "HardwareModel"
    static let required = [disk, auxiliaryStorage, machineIdentifier, hardwareModel]
}

/// The file type bits of `path`, or `nil` if it does not exist. `follow` decides whether a final
/// symlink is resolved.
func fileType(_ path: String, follow: Bool) -> mode_t? {
    var info = stat()
    let status = follow ? stat(path, &info) : lstat(path, &info)
    guard status == 0 else {
        return nil
    }
    return info.st_mode & S_IFMT
}

/// Parse `run RUN_DIR [PROJECT_ISO]` (arguments without the program name, as the runner passes them) and check the
/// inputs exist with the expected kinds. Nothing is opened or created here.
func parseRunArguments(_ arguments: [String]) throws -> RunRequest {
    let usage = "usage: <helper> run RUN_DIR [PROJECT_ISO]"
    guard arguments.count == 2 || arguments.count == 3, arguments[0] == "run" else {
        throw HostFailure.usage(usage)
    }
    let runDirectory = URL(fileURLWithPath: arguments[1]).standardizedFileURL
    guard fileType(runDirectory.path, follow: true) == S_IFDIR else {
        throw HostFailure.configuration("RUN_DIR is not a directory: \(runDirectory.path)")
    }
    for name in RunFileName.required {
        let path = runDirectory.appendingPathComponent(name).path
        guard fileType(path, follow: false) == S_IFREG else {
            throw HostFailure.configuration("\(name) is missing from RUN_DIR or is not a regular file")
        }
    }
    let disk = runDirectory.appendingPathComponent(RunFileName.disk).path
    guard access(disk, W_OK) == 0 else {
        throw HostFailure.configuration("\(RunFileName.disk) is not writable")
    }
    var projectISO: URL?
    if arguments.count == 3 {
        let iso = URL(fileURLWithPath: arguments[2]).standardizedFileURL
        guard iso.pathExtension.lowercased() == "iso" else {
            throw HostFailure.usage("PROJECT_ISO must be an .iso file")
        }
        guard fileType(iso.path, follow: true) == S_IFREG else {
            throw HostFailure.configuration("PROJECT_ISO is not a regular file")
        }
        projectISO = iso
    }
    return RunRequest(runDirectory: runDirectory, projectISO: projectISO)
}
