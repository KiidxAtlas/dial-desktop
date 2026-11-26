"""Report whether a Desktop tag can advance the stable updater feed.

stdin: published, non-draft, non-prerelease tags, one per line (all API pages).
An empty input also serves to validate the candidate tag during preflight.
"""
import re
import sys

STABLE = re.compile(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")


def version(tag):
    match = STABLE.fullmatch(tag)
    return tuple(map(int, match.groups())) if match else None


def main(tag, published):
    candidate = version(tag)
    if candidate is None:
        sys.exit(f"Desktop releases require a stable vX.Y.Z tag, got {tag!r}")
    # Consume the whole input, even if an early page has a newer release. This keeps
    # pipefail meaningful (a failed paginated gh request must prevent publication).
    newest = max(filter(None, (version(line.rstrip("\n")) for line in published)), default=candidate)
    print("latest=true" if candidate >= newest else "latest=false")


if __name__ == "__main__":
    main(sys.argv[1], sys.stdin)
