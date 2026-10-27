#!/usr/bin/env python3
"""GTK 4 client like Fragments' "Open torrent": Gtk.FileDialog.open() with
a "Torrent files" (application/x-bittorrent) filter. GTK decides by itself
to use the FileChooser portal (org.freedesktop.portal.Desktop activatable).
Prints "PICKED <path>" or "ERROR <message>".
  gtk4-open.py [save]
"""
import sys

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gio, GLib, Gtk  # noqa: E402

save = len(sys.argv) > 1 and sys.argv[1] == "save"


def on_activate(app):
    win = Gtk.ApplicationWindow(application=app, title="GTK4 portal test")
    win.present()
    dialog = Gtk.FileDialog(title="Save Torrent" if save else "Open Torrent", modal=True)
    flt = Gtk.FileFilter(name="Torrent files")
    flt.add_mime_type("application/x-bittorrent")
    filters = Gio.ListStore.new(Gtk.FileFilter)
    filters.append(flt)
    dialog.set_filters(filters)
    dialog.set_default_filter(flt)

    def done(d, res):
        try:
            f = d.save_finish(res) if save else d.open_finish(res)
            print("PICKED", f.get_path(), flush=True)
        except GLib.Error as e:
            print("ERROR", e.message, flush=True)
        app.quit()

    if save:
        dialog.set_initial_name("new.torrent")
        dialog.save(win, None, done)
    else:
        dialog.open(win, None, done)


app = Gtk.Application(application_id="org.fp6.PickerTest", flags=Gio.ApplicationFlags.NON_UNIQUE)
app.connect("activate", on_activate)
app.run([])
