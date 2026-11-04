"""A disposable desktop client and two real paste targets on the test X server."""
import base64
import contextlib
import ctypes
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys


class Client:
    def __init__(self, address, grant, service_pid):
        self.stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.stream.settimeout(8)
        self.stream.connect(address)
        pid, uid, _ = struct.unpack("3i", self.stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        assert (pid, uid) == (service_pid, os.geteuid())
        self.reader = self.stream.makefile("rb")
        self.seq = 0
        hello = self.call("hello", {"client_role":"desktop", "client_version":"native-focus-fixture",
            "protocol_version":{"major":1,"minor":0}, "capabilities":["desktop.extended_methods"],
            "required_capabilities":["desktop.quick.paste"], "grant":grant})
        self.base = {k:hello["state"][k] for k in ("service_instance_id",)}
        self.base["expected_epoch"] = hello["state"]["privacy_epoch"]

    def close(self):
        self.reader.close()
        self.stream.close()

    def call(self, method, params, expected_error=None):
        self.seq += 1
        wire = json.dumps({"method":method,"request_id":str(self.seq),"params":params}).encode()
        self.stream.sendall(struct.pack(">I",len(wire)) + wire)
        size, = struct.unpack(">I",self.reader.read(4))
        assert 0 < size <= 1048576
        result = json.loads(self.reader.read(size))
        assert result["request_id"] == str(self.seq)
        actual = (result.get("error") or {}).get("code")
        if actual != expected_error:
            raise AssertionError(f"{method} returned {actual}, expected {expected_error}")
        return result.get("result")


class DesktopWindow:
    def __init__(self):
        self.x = ctypes.CDLL("libX11.so.6")
        display, xid, uint = ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint
        apis = {"XOpenDisplay":(display,[ctypes.c_char_p]), "XDefaultRootWindow":(xid,[display]),
            "XCreateSimpleWindow":(xid,[display,xid,ctypes.c_int,ctypes.c_int,uint,uint,uint,xid,xid]),
            "XMapWindow":(ctypes.c_int,[display,xid]), "XSetInputFocus":(ctypes.c_int,[display,xid,ctypes.c_int,xid]),
            "XSync":(ctypes.c_int,[display,ctypes.c_int]), "XGetInputFocus":(ctypes.c_int,[display,ctypes.POINTER(xid),ctypes.POINTER(ctypes.c_int)]),
            "XCloseDisplay":(ctypes.c_int,[display])}
        for name,(ret,args) in apis.items():
            fn = getattr(self.x,name)
            fn.restype,fn.argtypes=ret,args
        self.display = self.x.XOpenDisplay(None)
        assert self.display
        self.window = self.x.XCreateSimpleWindow(self.display,self.x.XDefaultRootWindow(self.display),0,0,20,20,0,0,0)
        self.x.XMapWindow(self.display,self.window)
        self.x.XSync(self.display,0)

    def activate(self, window):
        self.x.XSetInputFocus(self.display,window,2,0)
        self.x.XSync(self.display,0)
        assert self.focused() == window

    def focused(self):
        window, revert = ctypes.c_ulong(), ctypes.c_int()
        self.x.XGetInputFocus(self.display,ctypes.byref(window),ctypes.byref(revert))
        return window.value

    def close(self):
        self.x.XCloseDisplay(self.display)


@contextlib.contextmanager
def destination(executable, text):
    child = subprocess.Popen([executable,"shift_insert",text],stdout=subprocess.PIPE,text=True)
    try:
        ready = child.stdout.readline().split()
        assert len(ready)==2 and ready[0]=="READY"
        yield child,int(ready[1])
    finally:
        if child.poll() is None:
            child.kill()
        child.wait(timeout=4)
        child.stdout.close()


def run(address, credentials, pid, profile, executable):
    assert os.environ.get("DZC_TEST_XVFB_SCREEN") == "1"
    with os.fdopen(os.open(credentials,os.O_RDONLY | os.O_NOFOLLOW),"rb") as handle:
        st = os.fstat(handle.fileno())
        assert st.st_uid == os.geteuid() and not st.st_mode & 0o077
        grant = json.load(handle)["desktop"]
    with contextlib.closing(Client(address,grant,int(pid))) as cli, contextlib.closing(Client(address,grant,int(pid))) as other, contextlib.closing(DesktopWindow()) as ui:
        text = "Quick focus fixture"
        path = Path(profile)/"focus.json"
        path.write_text(json.dumps({"version":"dezirclip-export-v2","exported_at":1,"entries":[{
            "id":42,"content_type":"text","content":text,"created_at":1,"updated_at":1}]}))
        cli.call("desktop.backup.import",{**cli.base,"mode":"merge","path":str(path)})
        cli.call("desktop.queue.configure",{**cli.base,"key":"app.delete_after_paste","value":"false"})
        desc = cli.call("desktop.history.page",{"limit":1,"offset":0,"query":text})
        chunk = cli.call("desktop.snapshot.read",{**cli.base,"offset":0,"snapshot_id":desc["snapshot_id"]})
        assert chunk["finished"]
        row = json.loads(base64.b64decode(chunk["data_base64"]))["entries"][0]
        with destination(executable,text) as (original, xid):
            def begin():
                ui.activate(xid)
                ticket = cli.call("desktop.quick.begin",cli.base)
                ui.activate(ui.window)
                return {**cli.base,"token":ticket["token"],"expected_history_revision":ticket["state"]["history_revision"],"id":row["id"]}
            stolen = begin()
            other.call("desktop.quick.paste",stolen,"not_found")
            with destination(executable,"decoy") as (_, decoy):
                cli.call("desktop.quick.paste",stolen,"unavailable")
                assert ui.focused() == decoy
            cli.call("desktop.quick.paste",stolen,"not_found")
            dismiss = begin()
            cli.call("desktop.quick.dismiss",{**cli.base,"token":dismiss["token"],"restore":True})
            assert ui.focused() == xid
            selection = begin()
            assert cli.call("desktop.quick.paste",selection)["performed"]
            assert original.wait(timeout=5) == 0
            cli.call("desktop.quick.paste",selection,"not_found")
        print("Verified authorized focus restoration, full target text, foreign focus refusal and one-shot tickets")


if __name__ == "__main__":
    run(*sys.argv[1:])
