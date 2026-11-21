import Foundation

/// A failure that ends the helper. The exit code tells the Rust runner which stage failed.
struct HostFailure: Error {
    /// Exit status: 2 usage, 3 configuration, 4 boot or guest readiness, 5 guest transport.
    let exitCode: Int32
    let message: String

    static func usage(_ message: String) -> HostFailure {
        HostFailure(exitCode: 2, message: message)
    }

    static func configuration(_ message: String) -> HostFailure {
        HostFailure(exitCode: 3, message: message)
    }

    static func boot(_ message: String) -> HostFailure {
        HostFailure(exitCode: 4, message: message)
    }

    static func transport(_ message: String) -> HostFailure {
        HostFailure(exitCode: 5, message: message)
    }
}

/// Write all of `data` to `fd`, ignoring a closed reader (SIGPIPE is ignored by the entry point).
func writeFully(_ data: Data, to fd: Int32) {
    data.withUnsafeBytes { raw in
        var offset = 0
        while offset < raw.count {
            let written = write(fd, raw.baseAddress! + offset, raw.count - offset)
            if written > 0 {
                offset += written
            } else if written < 0 && errno == EINTR {
                continue
            } else {
                return
            }
        }
    }
}

func writeStandardOutput(_ data: Data) {
    writeFully(data, to: STDOUT_FILENO)
}

func writeStandardError(_ message: String) {
    writeFully(Data((message + "\n").utf8), to: STDERR_FILENO)
}
