// Wallflow live wallpaper shim for GNOME Shell.
//
// GNOME has no protocol for putting a client window behind the desktop, so this
// extension does only that: it starts Wallflow's renderer (`wallflow-live`) as a
// trusted Wayland client, turns its window into a sticky desktop-type window that
// is hidden from the dock, Alt-Tab and the overview, and paints it just above the
// static background.
//
// Nothing is decoded or drawn in here. A video pipeline inside gnome-shell would
// share the shell's main loop and stall its animations (overview, hot corner),
// which is exactly what this design avoids.
//
// It also knows when the desktop cannot be seen — overview open, screen locked,
// a fullscreen or maximized window on top — and pauses the renderer then, so a
// covered wallpaper costs nothing and the shell's own animations get the GPU.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const BUS_NAME = 'com.efadhemon.Wallflow.LiveShim';
const OBJECT_PATH = '/com/efadhemon/Wallflow/LiveShim';
const ENGINE_BINARY = 'wallflow-live';
const API_VERSION = 1;

const IFACE = `
<node>
  <interface name="${BUS_NAME}">
    <method name="Start">
      <arg type="as" direction="in" name="argv"/>
    </method>
    <method name="Stop"/>
    <method name="Send">
      <arg type="s" direction="in" name="line"/>
    </method>
    <property name="Version" type="u" access="read"/>
    <property name="Running" type="b" access="read"/>
    <property name="Paused" type="b" access="read"/>
  </interface>
</node>`;

export default class WallflowLiveShim extends Extension {
    enable() {
        this._client = null;
        this._process = null;
        this._stdin = null;
        // MetaWindow → signal ids, one window per monitor.
        this._windows = new Map();
        this._paused = false;
        this._syncId = 0;

        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this._dbus.export(Gio.DBus.session, OBJECT_PATH);
        this._nameId = Gio.bus_own_name_on_connection(
            Gio.DBus.session, BUS_NAME, Gio.BusNameOwnerFlags.NONE, null, null);

        const sync = () => this._queueSync();
        this._signals = [
            [global.display, global.display.connect('window-created', (_d, win) => this._onWindowCreated(win))],
            [global.display, global.display.connect('restacked', () => this._keepAboveBackground())],
            [global.display, global.display.connect('in-fullscreen-changed', sync)],
            [global.window_manager, global.window_manager.connect('size-change', sync)],
            [global.window_manager, global.window_manager.connect('minimize', sync)],
            [global.window_manager, global.window_manager.connect('unminimize', sync)],
            [global.window_manager, global.window_manager.connect('map', sync)],
            [global.window_manager, global.window_manager.connect('destroy', sync)],
            [global.workspace_manager, global.workspace_manager.connect('active-workspace-changed', sync)],
            [Main.overview, Main.overview.connect('showing', sync)],
            [Main.overview, Main.overview.connect('hidden', sync)],
            [Main.sessionMode, Main.sessionMode.connect('updated', sync)],
            [Main.layoutManager, Main.layoutManager.connect('monitors-changed', () => this._placeWindows())],
        ];
    }

    disable() {
        for (const [object, id] of this._signals)
            object.disconnect(id);
        this._signals = [];
        if (this._syncId) {
            GLib.source_remove(this._syncId);
            this._syncId = 0;
        }
        this.Stop();
        this._dbus.unexport();
        this._dbus = null;
        Gio.bus_unown_name(this._nameId);
    }

    // --- D-Bus API, called by the Wallflow app ---

    get Version() {
        return API_VERSION;
    }

    get Running() {
        return this._process !== null;
    }

    get Paused() {
        return this._paused;
    }

    Start(argv) {
        // Only Wallflow's own renderer gets to be a trusted desktop client.
        if (argv.length === 0 || GLib.path_get_basename(argv[0]) !== ENGINE_BINARY ||
            !GLib.file_test(argv[0], GLib.FileTest.IS_EXECUTABLE))
            throw new Error(`Start expects the path to ${ENGINE_BINARY}`);

        this.Stop();
        const launcher = new Gio.SubprocessLauncher({flags: Gio.SubprocessFlags.STDIN_PIPE});
        this._client = Meta.WaylandClient.new_subprocess(global.context, launcher, argv);
        this._process = this._client.get_subprocess();
        this._stdin = this._process.get_stdin_pipe();
        this._paused = false;

        const process = this._process;
        process.wait_async(null, () => {
            if (this._process !== process)
                return;
            console.warn(`Wallflow: renderer exited (${process.get_exit_status()})`);
            this._forget();
        });
        this._queueSync();
    }

    Stop() {
        if (!this._process)
            return;
        const process = this._process;
        // Closing stdin asks the renderer to exit; force it if it does not.
        try {
            this._stdin?.close(null);
        } catch (_) {}
        GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 2, () => {
            process.force_exit();
            return GLib.SOURCE_REMOVE;
        });
        this._forget();
    }

    Send(line) {
        this._send(line);
    }

    // --- internals ---

    _forget() {
        for (const win of [...this._windows.keys()])
            this._releaseWindow(win);
        this._client = null;
        this._process = null;
        this._stdin = null;
    }

    _send(line) {
        if (!this._stdin)
            return;
        try {
            this._stdin.write_all(new TextEncoder().encode(`${line.trim()}\n`), null);
        } catch (error) {
            console.warn(`Wallflow: could not reach the renderer: ${error.message}`);
        }
    }

    _onWindowCreated(win) {
        if (!this._client?.owns_window(win))
            return;

        win.set_type(Meta.WindowType.DESKTOP);
        win.hide_from_window_list();
        win.stick();

        this._windows.set(win, [
            win.connect('unmanaged', () => this._releaseWindow(win)),
            win.connect_after('raised', () => this._keepAboveBackground()),
            // The renderer names each window after its monitor's origin.
            win.connect('notify::title', () => this._placeWindow(win)),
            win.connect('shown', () => {
                this._placeWindow(win);
                this._keepAboveBackground();
            }),
        ]);
        this._placeWindow(win);
        this._keepAboveBackground();
        this._queueSync();
    }

    _releaseWindow(win) {
        for (const id of this._windows.get(win) ?? [])
            win.disconnect(id);
        this._windows.delete(win);
        this._queueSync();
    }

    _monitorFor(win) {
        const monitors = Main.layoutManager.monitors;
        const match = /@(-?\d+),(-?\d+)/.exec(win.get_title() ?? '');
        if (match) {
            const [x, y] = [Number(match[1]), Number(match[2])];
            const exact = monitors.find(m => m.x === x && m.y === y);
            if (exact)
                return exact;
            // Scaled layouts report physical origins; fall back to the order.
            const index = [...this._windows.keys()].indexOf(win);
            if (index >= 0 && index < monitors.length)
                return monitors[index];
        }
        return Main.layoutManager.primaryMonitor;
    }

    _placeWindow(win) {
        const monitor = this._monitorFor(win);
        if (monitor)
            win.move_resize_frame(false, monitor.x, monitor.y, monitor.width, monitor.height);
    }

    _placeWindows() {
        for (const win of this._windows.keys())
            this._placeWindow(win);
    }

    // Desktop-type windows are stacked at the bottom, but other desktop windows
    // (desktop icons) must still be drawn on top of the video, so the actor is
    // pinned directly above the background group after every restack.
    _keepAboveBackground() {
        const background = Main.layoutManager._backgroundGroup;
        if (!background)
            return;
        for (const win of this._windows.keys()) {
            const actor = win.get_compositor_private();
            const parent = actor?.get_parent();
            if (parent && parent === background.get_parent())
                parent.set_child_above_sibling(actor, background);
        }
    }

    _queueSync() {
        if (this._syncId)
            return;
        this._syncId = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            this._syncId = 0;
            this._syncPause();
            return GLib.SOURCE_REMOVE;
        });
    }

    // Hidden means no monitor showing the video has any desktop in view.
    _desktopHidden() {
        if (Main.overview.visible || Main.sessionMode.isLocked)
            return true;
        const ours = new Set([...this._windows.keys()].map(win => win.get_monitor()));
        if (ours.size === 0)
            return false;
        const workspace = global.workspace_manager.get_active_workspace();
        const covered = new Set();
        for (const win of global.display.get_tab_list(Meta.TabList.NORMAL, workspace)) {
            if (win.minimized)
                continue;
            // GNOME 49 replaced get_maximized() with is_maximized().
            const maximized = win.is_maximized?.() ??
                (win.get_maximized?.() === Meta.MaximizeFlags.BOTH);
            if (win.is_fullscreen() || maximized)
                covered.add(win.get_monitor());
        }
        return [...ours].every(monitor => covered.has(monitor));
    }

    _syncPause() {
        if (!this._process)
            return;
        const paused = this._desktopHidden();
        if (paused === this._paused)
            return;
        this._paused = paused;
        this._send(JSON.stringify({cmd: paused ? 'pause' : 'resume'}));
    }
}
