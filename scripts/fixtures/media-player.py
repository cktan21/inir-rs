#!/usr/bin/env python3
"""Owned, silent MPRIS presentation fixture; never controls another player.

Reports a progressing synthetic track, but does not produce audio. EOF on stdin
releases its bus name so a benchmark controller can reliably clean it up.
"""
import sys
import time

import dbus
import dbus.mainloop.glib
import dbus.service
from gi.repository import GLib

BUS_NAME = "org.mpris.MediaPlayer2.inir_perf_fixture"
ROOT_IFACE = "org.mpris.MediaPlayer2"
PLAYER_IFACE = ROOT_IFACE + ".Player"
PROPS_IFACE = "org.freedesktop.DBus.Properties"


class Player(dbus.service.Object):
    def __init__(self, bus):
        self.started = time.monotonic()
        self.offset = 0
        self.playing = True
        self.track = dbus.ObjectPath("/org/mpris/MediaPlayer2/track/fixture")
        super().__init__(bus, "/org/mpris/MediaPlayer2")

    def position(self):
        return self.offset + (int((time.monotonic() - self.started) * 1e6) if self.playing else 0)

    def properties(self, interface):
        if interface == ROOT_IFACE:
            return {"CanQuit": False, "CanRaise": False, "HasTrackList": False,
                    "Identity": "iNiR performance fixture", "DesktopEntry": "inir-perf-fixture",
                    "SupportedUriSchemes": dbus.Array(["file"], signature="s"),
                    "SupportedMimeTypes": dbus.Array(["audio/wav"], signature="s")}
        if interface == PLAYER_IFACE:
            return {"PlaybackStatus": "Playing" if self.playing else "Paused", "LoopStatus": "None",
                    "Rate": 1.0, "Shuffle": False, "Volume": 0.0,
                    "Position": dbus.Int64(self.position()), "MinimumRate": 1.0, "MaximumRate": 1.0,
                    "CanGoNext": False, "CanGoPrevious": False, "CanPlay": True,
                    "CanPause": True, "CanSeek": True, "CanControl": True,
                    "Metadata": dbus.Dictionary({"mpris:trackid": self.track,
                        "mpris:length": dbus.Int64(3600 * 1000000),
                        "xesam:title": "Controlled presentation benchmark",
                        "xesam:artist": dbus.Array(["Silent fixture"], signature="s")}, signature="sv")}
        raise dbus.exceptions.DBusException("Unknown interface")

    @dbus.service.method(PROPS_IFACE, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface):
        return self.properties(interface)

    @dbus.service.method(PROPS_IFACE, in_signature="ss", out_signature="v")
    def Get(self, interface, name):
        return self.properties(interface)[name]

    @dbus.service.method(PROPS_IFACE, in_signature="ssv")
    def Set(self, interface, name, value):
        if interface == PLAYER_IFACE and name == "Volume":
            return
        raise dbus.exceptions.DBusException("Read-only fixture property")

    @dbus.service.signal(PROPS_IFACE, signature="sa{sv}as")
    def PropertiesChanged(self, interface, changed, invalidated):
        pass

    def set_playing(self, playing):
        self.offset = self.position()
        self.started = time.monotonic()
        self.playing = playing
        self.PropertiesChanged(PLAYER_IFACE, {"PlaybackStatus": "Playing" if playing else "Paused"}, [])

    @dbus.service.method(PLAYER_IFACE)
    def Play(self):
        self.set_playing(True)

    @dbus.service.method(PLAYER_IFACE)
    def Pause(self):
        self.set_playing(False)

    @dbus.service.method(PLAYER_IFACE)
    def PlayPause(self):
        self.set_playing(not self.playing)

    @dbus.service.method(PLAYER_IFACE)
    def Stop(self):
        self.set_playing(False)

    @dbus.service.method(PLAYER_IFACE, in_signature="x")
    def Seek(self, offset):
        self.offset = max(0, self.position() + int(offset))
        self.started = time.monotonic()
        self.Seeked(dbus.Int64(self.offset))

    @dbus.service.method(PLAYER_IFACE, in_signature="ox")
    def SetPosition(self, track, position):
        if track == self.track:
            self.offset = max(0, int(position))
            self.started = time.monotonic()
            self.Seeked(dbus.Int64(self.offset))

    @dbus.service.signal(PLAYER_IFACE, signature="x")
    def Seeked(self, position):
        pass


def main():
    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()
    name = dbus.service.BusName(BUS_NAME, bus, do_not_queue=True)
    player = Player(bus)
    loop = GLib.MainLoop()
    def eof(_fd, condition):
        if condition & GLib.IO_HUP or not sys.stdin.readline():
            loop.quit()
            return False
        return True
    GLib.io_add_watch(sys.stdin.fileno(), GLib.IO_IN | GLib.IO_HUP, eof)
    print("ready", flush=True)
    loop.run()
    # Keep the name and exported object alive until stdin closes.
    del player, name


if __name__ == "__main__":
    main()
