// GNOME Shell extension exposing a small D-Bus surface for the beckon CLI.
//
// Why this exists: GNOME Wayland (Mutter) intentionally has no protocol that
// lets an external process enumerate or focus arbitrary windows. The only
// in-process API is GNOME Shell itself. So beckon ships this extension —
// the JS side runs inside gnome-shell, calls Mutter directly, and re-exposes
// just enough to D-Bus for the Rust client to drive the focus algorithm.
//
// Surface (org.gnome.Shell.Extensions.Beckon at /com/github/xom11/beckon):
//   ListWindows()         a(tssbu)   (id, class, title, focused, monitor) MRU order
//   GetFocusedWindow()    t          stable_sequence of focused window or 0
//   ActivateWindow(t)     b          Main.activateWindow — handles workspace + unminimize
//   MinimizeWindow(t)     b          meta_window.minimize()
//   Version (property)    s          probe target — Rust client reads this to verify
//                                    the extension is loaded before trusting the bus
//
// Window id = MetaWindow.get_stable_sequence(). Stable for the lifetime of
// the window, available on every supported GNOME version, fits in a uint64.

import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const BUS_PATH = '/com/github/xom11/beckon';

const IFACE_XML = `
<node>
  <interface name="org.gnome.Shell.Extensions.Beckon">
    <method name="ListWindows">
      <arg type="a(tssbu)" direction="out" name="windows"/>
    </method>
    <method name="GetFocusedWindow">
      <arg type="t" direction="out" name="window_id"/>
    </method>
    <method name="ActivateWindow">
      <arg type="t" direction="in" name="window_id"/>
      <arg type="b" direction="out" name="ok"/>
    </method>
    <method name="MinimizeWindow">
      <arg type="t" direction="in" name="window_id"/>
      <arg type="b" direction="out" name="ok"/>
    </method>
    <property name="Version" type="s" access="read"/>
  </interface>
</node>`;

export default class BeckonExtension extends Extension {
    enable() {
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE_XML, this);
        this._dbus.export(Gio.DBus.session, BUS_PATH);
    }

    disable() {
        if (this._dbus) {
            this._dbus.unexport();
            this._dbus = null;
        }
    }

    get Version() {
        // metadata.version is a number (extension manifest "version" field);
        // version-name is a string. Either is fine for a probe.
        return (this.metadata['version-name'] || String(this.metadata.version || '0'));
    }

    // Class identity ladder, mirroring beckon's other Linux backends:
    //   1. WM_CLASS — set by every X11 client and most Wayland toolkits
    //   2. GTK application id — Wayland-native GTK apps (org.gnome.Console etc.)
    //   3. Sandboxed app id — Flatpak fallback
    _windowClass(win) {
        const cls = win.get_wm_class();
        if (cls) return cls;
        if (typeof win.get_gtk_application_id === 'function') {
            const gid = win.get_gtk_application_id();
            if (gid) return gid;
        }
        if (typeof win.get_sandboxed_app_id === 'function') {
            const sid = win.get_sandboxed_app_id();
            if (sid) return sid;
        }
        return '';
    }

    // MRU-ordered window list. NORMAL_ALL covers every workspace; the tab
    // list is what alt-tab walks, so it already reflects user focus history.
    // Anything Mutter has but isn't in the tab list (very newly-mapped
    // windows) is appended at the end.
    _orderedWindows() {
        const display = global.display;
        const tab = display.get_tab_list(Meta.TabList.NORMAL_ALL, null) || [];
        const seen = new Set(tab.map(w => w.get_stable_sequence()));
        const out = [...tab];
        for (const actor of global.get_window_actors()) {
            const w = actor.meta_window;
            if (!seen.has(w.get_stable_sequence())) out.push(w);
        }
        return out;
    }

    _findWindow(id) {
        // id is uint64 over the wire; stable_sequence is uint32. Coerce.
        const target = Number(id);
        for (const actor of global.get_window_actors()) {
            const w = actor.meta_window;
            if (w.get_stable_sequence() === target) return w;
        }
        return null;
    }

    ListWindows() {
        const out = [];
        const windows = this._orderedWindows();
        for (const w of windows) {
            if (w.is_skip_taskbar()) continue;
            const cls = this._windowClass(w);
            if (!cls) continue;
            out.push([
                w.get_stable_sequence(),
                cls,
                w.get_title() || '',
                w.has_focus(),
                w.get_monitor(),
            ]);
        }
        return out;
    }

    GetFocusedWindow() {
        const f = global.display.focus_window;
        return f ? f.get_stable_sequence() : 0;
    }

    ActivateWindow(window_id) {
        const w = this._findWindow(window_id);
        if (!w) return false;
        // Main.activateWindow handles: switch to the window's workspace,
        // unminimize if minimized, raise above siblings, give it focus.
        // It also passes a recent timestamp so Mutter's focus-stealing
        // prevention doesn't reject the request.
        Main.activateWindow(w);
        return true;
    }

    MinimizeWindow(window_id) {
        const w = this._findWindow(window_id);
        if (!w) return false;
        w.minimize();
        return true;
    }
}
