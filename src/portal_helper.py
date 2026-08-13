#!/usr/bin/env python3
# cli-streamer: xdg-desktop-portal ScreenCast (WINDOW) → GStreamer Y4M on stdout
# ポータルセッションを配信中ずっと維持する。

from __future__ import annotations

import os
import signal
import subprocess
import sys
import traceback


def die(msg: str, code: int = 1) -> None:
    print(f"portal-helper: {msg}", file=sys.stderr)
    sys.exit(code)


def main() -> None:
    try:
        import gi

        gi.require_version("Gio", "2.0")
        gi.require_version("GLib", "2.0")
        from gi.repository import Gio, GLib
    except Exception as e:
        die(f"PyGObject (Gio) が必要です: {e}")

    loop = GLib.MainLoop()
    state: dict = {"session": None, "node": None, "error": None}

    try:
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    except Exception as e:
        die(f"セッションバスに接続できません: {e}")

    portal = Gio.DBusProxy.new_sync(
        bus,
        Gio.DBusProxyFlags.NONE,
        None,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.ScreenCast",
        None,
    )

    sender = bus.get_unique_name()
    if not sender:
        die("D-Bus unique name を取得できません")
    sender_token = sender[1:].replace(".", "_")

    def request_path(token: str) -> str:
        return f"/org/freedesktop/portal/desktop/request/{sender_token}/{token}"

    def call_with_request(method: str, build_params, token: str, on_ok) -> None:
        path = request_path(token)

        def on_response(_conn, _sender, _object_path, _iface, _signal, args):
            response, results = args.unpack()
            try:
                bus.signal_unsubscribe(sub_id)
            except Exception:
                pass
            if response != 0:
                state["error"] = f"{method} が拒否/失敗しました (code={response})"
                loop.quit()
                return
            try:
                on_ok(results)
            except Exception as e:
                state["error"] = str(e)
                loop.quit()

        sub_id = bus.signal_subscribe(
            "org.freedesktop.portal.Desktop",
            "org.freedesktop.portal.Request",
            "Response",
            path,
            None,
            Gio.DBusSignalFlags.NONE,
            on_response,
            None,
        )

        try:
            params = build_params(token)
            portal.call_sync(method, params, Gio.DBusCallFlags.NONE, -1, None)
        except Exception as e:
            state["error"] = f"{method}: {e}"
            loop.quit()

    def after_start(results) -> None:
        streams = results.get("streams")
        if not streams:
            raise RuntimeError("streams が空です（ウィンドウが選ばれませんでした）")
        state["node"] = int(streams[0][0])
        loop.quit()

    def after_select(_results) -> None:
        print(
            "portal-helper: 共有ダイアログでウィンドウを選んでください…",
            file=sys.stderr,
        )

        def build(token: str):
            opts = {"handle_token": GLib.Variant("s", token)}
            return GLib.Variant("(osa{sv})", (state["session"], "", opts))

        call_with_request("Start", build, "csstart", after_start)

    def available_cursor_modes() -> int:
        """未対応の cursor_mode を指定するとセッションが閉じるので、 advertised なときだけ使う。"""
        try:
            props = Gio.DBusProxy.new_sync(
                bus,
                Gio.DBusProxyFlags.NONE,
                None,
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.DBus.Properties",
                None,
            )
            variant = props.call_sync(
                "Get",
                GLib.Variant(
                    "(ss)",
                    ("org.freedesktop.portal.ScreenCast", "AvailableCursorModes"),
                ),
                Gio.DBusCallFlags.NONE,
                -1,
                None,
            )
            inner = variant.unpack()[0]
            if isinstance(inner, tuple):
                inner = inner[0]
            return int(inner)
        except Exception:
            return 0

    def after_create(results) -> None:
        session = results.get("session_handle")
        if not session:
            raise RuntimeError("session_handle がありません")
        state["session"] = session
        modes = available_cursor_modes()

        def build(token: str):
            opts = {
                "handle_token": GLib.Variant("s", token),
                "types": GLib.Variant("u", 2),  # WINDOW
                "multiple": GLib.Variant("b", False),
            }
            # Embedded = 2
            if modes & 2:
                opts["cursor_mode"] = GLib.Variant("u", 2)
            return GLib.Variant("(oa{sv})", (state["session"], opts))

        call_with_request("SelectSources", build, "csselect", after_select)

    def build_create(token: str):
        opts = {
            "handle_token": GLib.Variant("s", token),
            "session_handle_token": GLib.Variant("s", token),
        }
        return GLib.Variant("(a{sv})", (opts,))

    print("portal-helper: ScreenCast セッションを開始します…", file=sys.stderr)
    call_with_request("CreateSession", build_create, "cssession", after_create)
    loop.run()

    if state["error"]:
        die(state["error"])
    if not state["session"] or state["node"] is None:
        die("ポータルセッションの確立に失敗しました")

    try:
        _variant, fd_list = portal.call_with_unix_fd_list_sync(
            "OpenPipeWireRemote",
            GLib.Variant("(oa{sv})", (state["session"], {})),
            Gio.DBusCallFlags.NONE,
            -1,
            None,
            None,
        )
        if fd_list is None or fd_list.get_length() < 1:
            die("OpenPipeWireRemote が FD を返しませんでした")
        pw_fd = fd_list.get(0)
    except Exception as e:
        die(f"OpenPipeWireRemote: {e}")

    try:
        os.set_inheritable(pw_fd, True)
    except Exception:
        pass

    node = state["node"]
    try:
        fps = int(os.environ.get("CLI_STREAMER_FPS", "30"))
        if fps < 1 or fps > 120:
            fps = 30
    except ValueError:
        fps = 30
    print(f"portal-helper: gst-launch node={node} fps={fps}", file=sys.stderr)

    gst = subprocess.Popen(
        [
            "gst-launch-1.0",
            "-q",
            "pipewiresrc",
            f"fd={pw_fd}",
            f"path={node}",
            "always-copy=true",
            "do-timestamp=true",
            "!",
            "videoconvert",
            "!",
            "videorate",
            "!",
            f"video/x-raw,format=I420,framerate={fps}/1",
            "!",
            "y4menc",
            "!",
            "fdsink",
            "fd=1",
        ],
        stdout=sys.stdout,
        stderr=sys.stderr,
        pass_fds=(pw_fd,),
    )

    def _stop(_signum=None, _frame=None):
        try:
            gst.send_signal(signal.SIGINT)
        except Exception:
            try:
                gst.terminate()
            except Exception:
                pass

    signal.signal(signal.SIGINT, _stop)
    signal.signal(signal.SIGTERM, _stop)

    code = gst.wait()
    try:
        sess = Gio.DBusProxy.new_sync(
            bus,
            Gio.DBusProxyFlags.NONE,
            None,
            "org.freedesktop.portal.Desktop",
            state["session"],
            "org.freedesktop.portal.Session",
            None,
        )
        sess.call_sync("Close", None, Gio.DBusCallFlags.NONE, -1, None)
    except Exception:
        pass

    try:
        os.close(pw_fd)
    except Exception:
        pass

    sys.exit(code if code is not None else 1)


if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except Exception:
        traceback.print_exc(file=sys.stderr)
        sys.exit(1)
