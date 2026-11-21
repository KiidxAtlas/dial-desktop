# Guest-only VirtualBox console helper, run by Dial as `python3 -I -u -c <this file> <uuid>`.
#
# It talks to one registered machine through VirtualBox's own Python SDK: XPCOM bindings on macOS
# and Linux, COM through pywin32 on Windows. It never runs host input, never powers the VM off,
# never saves, discards or changes its settings. A powered-off VM is started headless; a running VM
# is attached with a shared session. Closing (or losing stdin) releases only this helper's session.
#
# Protocol: one JSON object per line on stdin; one JSON reply per line on stdout. A frame reply's
# line names `pixels`, the count of raw BGRA bytes written right after it (stdout is binary, so
# nothing is translated). Native library output is redirected to stderr so it cannot corrupt the
# protocol. Replies carry only fixed codes and fixed text: native exception strings, paths and
# machine names are never echoed.
#
# Environment (set by Dial, absolute, from the VirtualBox install that owns VBoxManage):
#   VBOX_PROGRAM_PATH  the directory with VBoxManage and VBoxPython3.so
#   VBOX_SDK_PATH      the directory with sdk/bindings/xpcom and sdk/installer
import json
import os
import sys
import time

LAUNCH_TIMEOUT_MS = 120000
RUNNING_WAIT_S = 30.0
# Largest guest screen the helper asks VirtualBox to capture. Checked before the SDK call.
MAX_FRAME_WIDTH = 3840
MAX_FRAME_HEIGHT = 2160
RUNNING_STATES = ("Running", "Paused", "Stuck", "Teleporting", "LiveSnapshotting",
                  "TeleportingPausedVM")


class Refused(Exception):
    """A request VirtualBox will not serve in the machine's current state."""


class Stale(Exception):
    """The machine, session or display changed since the caller last looked."""


def normalize(identifier):
    return str(identifier).strip("{}").lower()


def load_sdk():
    sdk = os.environ["VBOX_SDK_PATH"]
    sys.path.insert(0, os.path.join(sdk, "installer", "python", "vboxapi", "src"))
    from vboxapi import VirtualBoxManager
    # Windows reaches VirtualBox through COM (pywin32); macOS and Linux use XPCOM.
    style = "MSCOM" if sys.platform == "win32" else "XPCOM"
    return VirtualBoxManager(style, None)


def machine_state(mgr, machine):
    # SDK aliases such as FirstOnline share Running's value. Reverse lookup can
    # return the alias, so compare the numeric values of the states we support.
    state = machine.state
    for name in RUNNING_STATES + ("PoweredOff", "Saved"):
        if state == getattr(mgr.constants, "MachineState_" + name):
            return name
    return None


def resolution(console):
    res = console.display.getScreenResolution(0)
    return int(res[0]), int(res[1])


def require_console_session(mgr, session, launched):
    c = mgr.constants
    if session.state != c.SessionState_Locked:
        raise Stale("the VM session is not locked")
    # launchVMProcess can return a remote session for the newly started VM;
    # an explicit shared attach must never yield a direct/write-lock session.
    if session.type != c.SessionType_Shared and not (launched and session.type == c.SessionType_Remote):
        raise Stale("the VM session is not a console session")


def launch(mgr, machine, session):
    # The environment changes are a SOAP/XPCOM string array; an empty array means no changes.
    progress = machine.launchVMProcess(session, "headless", [])
    progress.waitForCompletion(LAUNCH_TIMEOUT_MS)
    if not progress.completed:
        raise Refused("starting the VM took too long")
    if progress.resultCode != 0:
        raise Refused("VirtualBox could not start the VM")
    deadline = time.monotonic() + RUNNING_WAIT_S
    while time.monotonic() < deadline:
        if machine_state(mgr, machine) in RUNNING_STATES:
            return
        time.sleep(0.25)
    raise Refused("the VM did not reach the running state")


def reply(out, payload=b"", **fields):
    out.write((json.dumps(fields) + "\n").encode("utf-8"))
    if payload:
        out.write(payload)
    out.flush()


def reply_error(out, exc, mgr):
    # Only our own fixed messages and codes leave this process. Native text stays on stderr.
    if isinstance(exc, Refused):
        return reply(out, ok=False, code="refused", error=str(exc))
    if isinstance(exc, Stale):
        return reply(out, ok=False, code="stale", error="the VM changed or stopped")
    try:
        dead = mgr is not None and mgr.xcptIsDeadInterface(exc)
    except Exception:
        dead = False
    if dead:
        return reply(out, ok=False, code="stale", error="the VM changed or stopped")
    sys.stderr.write("vbox helper: %s\n" % type(exc).__name__)
    reply(out, ok=False, code="error", error="VirtualBox reported an error")


def do_frame(out, mgr, console):
    width, height = resolution(console)
    if width == 0 or height == 0:
        raise Refused("the guest display is not ready")
    if width > MAX_FRAME_WIDTH or height > MAX_FRAME_HEIGHT:
        raise Refused("the guest display is larger than the console can show")
    # Raw 32-bit pixels, blue first: no PNG encode here and no decode in Dial.
    pixels = bytes(console.display.takeScreenShotToArray(
        0, width, height, mgr.constants.BitmapFormat_BGRA))
    if len(pixels) != width * height * 4:
        raise Refused("VirtualBox returned a screen of the wrong size")
    reply(out, pixels, ok=True, width=width, height=height, format="bgra", pixels=len(pixels))


def do_send(out, req, mgr, console, held):
    width, height = int(req["width"]), int(req["height"])
    steps = req["steps"]
    if any("mouse" in step for step in steps):
        if not console.mouse.absoluteSupported:
            raise Refused("the VM has no absolute pointer (USB tablet or Guest Additions)")
        if resolution(console) != (width, height):
            raise Stale("the guest display changed size; take a new screenshot")
    for step in steps:
        if "mouse" in step:
            x, y, buttons = (int(v) for v in step["mouse"])
            console.mouse.putMouseEventAbsolute(x, y, 0, 0, buttons)
            held["pos"] = (x, y)
            held["buttons"] = buttons
        elif "keys" in step:
            console.keyboard.putScancodes([int(c) for c in step["keys"]])
        else:
            raise Refused("unknown input step")
    reply(out, ok=True)


def release_buttons(console, held):
    if held.get("buttons"):
        x, y = held["pos"]
        try:
            console.mouse.putMouseEventAbsolute(x, y, 0, 0, 0)
        except Exception:
            pass
        held["buttons"] = 0


def break_codes(make):
    # Set 1: the break code is the make code with bit 7 set; an E0 prefix stays as is.
    return [c if c == 0xE0 else c | 0x80 for c in make]


def release_keys(console, held):
    keys = held.get("keys") or []
    while keys:
        make = keys.pop()
        try:
            console.keyboard.putScancodes(break_codes(list(make)))
        except Exception:
            pass


def do_stream(out, req, mgr, console, held):
    # The UI's live input: buttons and keys stay held until a later step lets go of them (a drag,
    # a held modifier). Whatever is still held is let go when this helper closes.
    width, height = int(req["width"]), int(req["height"])
    steps = req["steps"]
    pointer = [s for s in steps if "mouse" in s or "wheel" in s]
    if pointer:
        if not console.mouse.absoluteSupported:
            raise Refused("the VM has no absolute pointer (USB tablet or Guest Additions)")
        if resolution(console) != (width, height):
            raise Stale("the guest display changed size; take a new screenshot")
    keys = held.setdefault("keys", [])
    for step in steps:
        if "mouse" in step:
            x, y, buttons = (int(v) for v in step["mouse"])
            console.mouse.putMouseEventAbsolute(x, y, 0, 0, buttons & 7)
            held["pos"] = (x, y)
            held["buttons"] = buttons & 7
        elif "wheel" in step:
            x, y, dz, buttons = (int(v) for v in step["wheel"])
            dz = max(-10, min(10, dz))
            console.mouse.putMouseEventAbsolute(x, y, dz, 0, buttons & 7)
            held["pos"] = (x, y)
        elif "key" in step:
            make = tuple(int(c) & 0xFF for c in step["key"]["make"])
            if not make or len(make) > 2:
                raise Refused("unknown input step")
            if step["key"]["down"]:
                console.keyboard.putScancodes(list(make))
                if make not in keys:
                    keys.append(make)
            else:
                console.keyboard.putScancodes(break_codes(list(make)))
                if make in keys:
                    keys.remove(make)
        else:
            raise Refused("unknown input step")
    reply(out, ok=True)


def serve(out, mgr, console):
    held = {}
    try:
        serve_requests(out, mgr, console, held)
    finally:
        release_keys(console, held)
        release_buttons(console, held)


def serve_requests(out, mgr, console, held):
    for raw in sys.stdin:
        try:
            req = json.loads(raw)
            op = req["op"]
        except Exception:
            reply(out, ok=False, code="error", error="malformed request")
            continue
        if op == "close":
            return
        try:
            if op == "frame":
                do_frame(out, mgr, console)
            elif op == "stream":
                do_stream(out, req, mgr, console, held)
            elif op == "send":
                try:
                    do_send(out, req, mgr, console, held)
                finally:
                    release_buttons(console, held)
            else:
                raise Refused("unknown operation")
        except Exception as exc:
            reply_error(out, exc, mgr)

def main():
    uuid = sys.argv[1]
    out = os.fdopen(os.dup(1), "wb")
    os.dup2(2, 1)
    mgr = None
    session = None
    locked = False
    try:
        mgr = load_sdk()
        vbox = mgr.getVirtualBox()
        machine = vbox.findMachine(uuid)
        if normalize(machine.id) != normalize(uuid):
            raise Stale("the VM identity changed")
        state = machine_state(mgr, machine)
        session = mgr.getSessionObject(vbox)
        if state in ("PoweredOff", "Saved"):
            launched = True
            locked = True
            launch(mgr, machine, session)
        elif state in RUNNING_STATES:
            launched = False
            machine.lockMachine(session, mgr.constants.LockType_Shared)
            locked = True
        else:
            raise Refused("the VM is not in a state that can be used")
        require_console_session(mgr, session, launched)
        console = session.console
        reply(out, ok=True, launched=launched, state=machine_state(mgr, machine))
        serve(out, mgr, console)
    except Exception as exc:
        reply_error(out, exc, mgr)
    finally:
        # Releases only this helper's session. The VM itself keeps running.
        if locked and session is not None:
            try:
                session.unlockMachine()
            except Exception:
                pass
        if mgr is not None:
            try:
                mgr.deinit()
            except Exception:
                pass


if __name__ == "__main__":
    main()
