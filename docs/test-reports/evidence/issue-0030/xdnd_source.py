#!/usr/bin/env python3
"""XDND ソース（Issue #30 実験用、使い捨て）。

実ファイルマネージャの代わりに、XDND プロトコル（version 5）で
対象ウィンドウへファイルドロップを送る。対象は winit/nannou の
実装が本物のドロップとして扱うため、検証は実ドロップと等価。

使い方: python3 xdnd_source.py <window-title> <x> <y> <file-uri>
例:     python3 xdnd_source.py dnd-probe 300 200 file:///tmp/dnd-test.wav
"""
import ctypes
import sys
import time

X11 = ctypes.CDLL("libX11.so.6")

X11.XOpenDisplay.restype = ctypes.c_void_p
X11.XInternAtom.restype = ctypes.c_ulong
X11.XFetchName.restype = ctypes.c_int
X11.XDefaultRootWindow.restype = ctypes.c_ulong
X11.XCreateSimpleWindow.restype = ctypes.c_ulong
X11.XGetSelectionOwner.restype = ctypes.c_ulong

CurrentTime = 0
ClientMessage = 33
SelectionRequest = 30
SelectionNotify = 31
PropModeReplace = 0


class XClientMessageEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("window", ctypes.c_ulong),
        ("message_type", ctypes.c_ulong),
        ("format", ctypes.c_int),
        ("data", ctypes.c_long * 5),
    ]


class XSelectionRequestEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("owner", ctypes.c_ulong),
        ("requestor", ctypes.c_ulong),
        ("selection", ctypes.c_ulong),
        ("target", ctypes.c_ulong),
        ("property", ctypes.c_ulong),
        ("time", ctypes.c_ulong),
    ]


class XSelectionEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("requestor", ctypes.c_ulong),
        ("selection", ctypes.c_ulong),
        ("target", ctypes.c_ulong),
        ("property", ctypes.c_ulong),
        ("time", ctypes.c_ulong),
    ]


class XEvent(ctypes.Union):
    _fields_ = [
        ("type", ctypes.c_int),
        ("xclient", XClientMessageEvent),
        ("xselectionrequest", XSelectionRequestEvent),
        ("xselection", XSelectionEvent),
        ("pad", ctypes.c_long * 24),
    ]


def atom(dpy, name):
    return X11.XInternAtom(dpy, name.encode(), False)


def find_window(dpy, root, title):
    """ルートの子からタイトル一致のウィンドウを探す。"""
    children = ctypes.c_ulong()
    n = ctypes.c_uint()
    parent = ctypes.c_ulong()
    X11.XQueryTree(
        dpy,
        root,
        ctypes.byref(ctypes.c_ulong()),
        ctypes.byref(parent),
        ctypes.byref(children),
        ctypes.byref(n),
    )
    found = None
    arr = ctypes.cast(children.value, ctypes.POINTER(ctypes.c_ulong))
    for i in range(n.value):
        name = ctypes.c_char_p()
        if X11.XFetchName(dpy, arr[i], ctypes.byref(name)) and name.value:
            if name.value.decode(errors="replace") == title:
                found = arr[i]
            X11.XFree(name)
    if children:
        X11.XFree(children)
    return found


def send_client(dpy, target, source, msg_type, l0, l1, l2, l3, l4):
    ev = XEvent()
    ev.xclient.type = ClientMessage
    ev.xclient.window = target
    ev.xclient.message_type = msg_type
    ev.xclient.format = 32
    ev.xclient.data[0] = l0
    ev.xclient.data[1] = l1
    ev.xclient.data[2] = l2
    ev.xclient.data[3] = l3
    ev.xclient.data[4] = l4
    # Xdnd のクライアントメッセージは対象ウィンドウへ NoEventMask で送る
    X11.XSendEvent(dpy, target, False, 0, ctypes.byref(ev))
    X11.XFlush(dpy)


def main():
    title, xs, ys, uri = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
    dpy = X11.XOpenDisplay(None)
    if not dpy:
        print("cannot open display")
        return 1
    root = X11.XDefaultRootWindow(dpy)
    target = find_window(dpy, root, title)
    if not target:
        print(f"window not found: {title}")
        return 1
    print(f"target window: {target:#x}")

    src = X11.XCreateSimpleWindow(dpy, root, 0, 0, 10, 10, 0, 0, 0)
    X11.XSelectInput(dpy, src, 0)  # イベントは XNextEvent で受ける

    a_xdnd_selection = atom(dpy, "XdndSelection")
    a_enter = atom(dpy, "XdndEnter")
    a_position = atom(dpy, "XdndPosition")
    a_status = atom(dpy, "XdndStatus")
    a_drop = atom(dpy, "XdndDrop")
    a_finished = atom(dpy, "XdndFinished")
    a_action_copy = atom(dpy, "XdndActionCopy")
    a_uri_list = atom(dpy, "text/uri-list")
    a_targets = atom(dpy, "TARGETS")

    X11.XSetSelectionOwner(dpy, a_xdnd_selection, src, CurrentTime)
    if X11.XGetSelectionOwner(dpy, a_xdnd_selection) != src:
        print("failed to own XdndSelection")
        return 1

    data = (uri + "\r\n").encode()

    def handle_selection_request(ev):
        req = ev.xselectionrequest
        prop = req.property
        if req.target == a_uri_list:
            X11.XChangeProperty(
                dpy, req.requestor, prop, a_uri_list, 8, PropModeReplace,
                data, len(data),
            )
            print("selection served: text/uri-list")
        elif req.target == a_targets:
            targets = (ctypes.c_ulong * 2)(a_targets, a_uri_list)
            X11.XChangeProperty(
                dpy, req.requestor, prop, atom(dpy, "ATOM"), 32,
                PropModeReplace, targets, 2,
            )
            print("selection served: TARGETS")
        else:
            prop = 0
            print(f"selection refused: target={req.target}")
        notify = XEvent()
        notify.xselection.type = SelectionNotify
        notify.xselection.requestor = req.requestor
        notify.xselection.selection = req.selection
        notify.xselection.target = req.target
        notify.xselection.property = prop
        notify.xselection.time = req.time
        X11.XSendEvent(dpy, req.requestor, False, 0, ctypes.byref(notify))
        X11.XFlush(dpy)

    def pump(seconds, stop_on_finished=False):
        """イベントを処理しながら待つ。XdndStatus の受理フラグを返す。"""
        accepted = None
        end = time.time() + seconds
        while time.time() < end:
            while X11.XPending(dpy):
                ev = XEvent()
                X11.XNextEvent(dpy, ctypes.byref(ev))
                if ev.type == SelectionRequest:
                    handle_selection_request(ev)
                elif ev.type == ClientMessage and ev.xclient.message_type == a_status:
                    accepted = bool(ev.xclient.data[1] & 1)
                    print(f"XdndStatus: accepted={accepted}")
                elif ev.type == ClientMessage and ev.xclient.message_type == a_finished:
                    print("XdndFinished received")
                    if stop_on_finished:
                        return accepted
            time.sleep(0.02)
        return accepted

    version_flags = (5 << 24)  # version 5、タイプは3つ以下
    send_client(dpy, target, src, a_enter, src, version_flags, a_uri_list, 0, 0)
    pump(0.5)
    pos = (xs << 16) | (ys & 0xFFFF)
    send_client(dpy, target, src, a_position, src, 0, pos, CurrentTime, a_action_copy)
    accepted = pump(1.0)
    send_client(dpy, target, src, a_drop, src, 0, CurrentTime, 0, 0)
    pump(2.0, stop_on_finished=True)
    send_client(dpy, target, src, atom(dpy, "XdndLeave"), src, 0, 0, 0, 0)
    print("source done")
    return 0


if __name__ == "__main__":
    sys.exit(main())
