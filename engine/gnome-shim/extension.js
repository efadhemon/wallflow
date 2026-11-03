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
// The overview draws its own copy of the background for every workspace, so
// while it is open the shim lays a clone of the video window over those copies:
// the overview shows the frame playback paused on instead of the old static
// image, and the desktop picks up from that same frame when it closes.
//
// It also knows when the desktop cannot be seen — overview open, screen locked,
// a fullscreen or maximized window on top — and pauses the renderer then, so a
// covered wallpaper costs nothing and the shell's own animations get the GPU.

import Clutter from 'gi://Clutter';
import Cogl from 'gi://Cogl';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as Workspace from 'resource:///org/gnome/shell/ui/workspace.js';
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

// The overview draws each workspace background with rounded corners (on the
// work area, radius growing as the overview opens). This gives the video clone
// the same corners. Only the corner squares are touched: the straight edges are
// already cropped by the workspace's own clip.
const ROUNDED_DECLARATIONS = `
uniform vec2 size;
uniform vec4 bounds;
uniform float radius;
`;
const ROUNDED_CODE = `
vec2 point = cogl_tex_coord_in[0].xy * size;
vec2 inner_lo = bounds.xy + radius;
vec2 inner_hi = bounds.xy + bounds.zw - radius;
if (radius > 0.0 &&
    (point.x < inner_lo.x || point.x > inner_hi.x) &&
    (point.y < inner_lo.y || point.y > inner_hi.y)) {
    vec2 nearest = clamp(point, inner_lo, inner_hi);
    // +1 (not +0.5) so the arc meets the straight edges without a 1 px step.
    cogl_color_out *= clamp(radius - length(point - nearest) + 1.0, 0.0, 1.0);
}
`;

const RoundedLikeBackground = GObject.registerClass(
class RoundedLikeBackground extends Shell.GLSLEffect {
    _init(background) {
        super._init();
        this._background = background;
        this._size = this.get_uniform_location('size');
        this._bounds = this.get_uniform_location('bounds');
        this._radius = this.get_uniform_location('radius');
    }

    vfunc_build_pipeline() {
        this.add_glsl_snippet(Cogl.SnippetHook.FRAGMENT,
            ROUNDED_DECLARATIONS, ROUNDED_CODE, false);
    }

    vfunc_paint_target(...args) {
        const clone = this.get_actor();
        const background = this._background;
        const group = background._backgroundGroup;
        const monitor = Main.layoutManager.monitors[background._monitorIndex];
        const content = background._bgManager?.backgroundActor?.content;
        const source = clone.source;
        if (monitor && content && group && source?.width > 0 && clone.width > 0) {
            // GNOME maps the work area onto the background group per axis; the
            // clone sits in that group at its own offset. The offscreen target
            // spans the source's size, measured in the clone's own units, so
            // texture coordinates times the source size are clone coordinates.
            const sx = group.width / monitor.width;
            const sy = group.height / monitor.height;
            const area = Main.layoutManager.getWorkAreaForMonitor(background._monitorIndex);
            this.set_uniform_float(this._size, 2, [source.width, source.height]);
            this.set_uniform_float(this._bounds, 4, [
                (area.x - monitor.x) * sx - clone.x, (area.y - monitor.y) * sy - clone.y,
                area.width * sx, area.height * sy,
            ]);
            this.set_uniform_float(this._radius, 1, [content.rounded_clip_radius]);
        }
        super.vfunc_paint_target(...args);
    }
});

// The overview's workspace boxes do not have the monitor's aspect ratio (and it
// changes while the overview animates). Like the static background, the video
// is scaled evenly to cover the box and centred; the box crops the overflow.
const CoverClone = GObject.registerClass(
class CoverClone extends Clutter.Clone {
    vfunc_allocate(box) {
        const source = this.source;
        const [width, height] = [box.get_width(), box.get_height()];
        if (!source || source.width <= 0 || source.height <= 0 || width <= 0 || height <= 0) {
            super.vfunc_allocate(box);
            return;
        }
        const scale = Math.max(width / source.width, height / source.height);
        const [w, h] = [source.width * scale, source.height * scale];
        const cover = new Clutter.ActorBox();
        cover.set_origin(box.x1 + (width - w) / 2, box.y1 + (height - h) / 2);
        cover.set_size(w, h);
        super.vfunc_allocate(cover);
    }
});

export default class WallflowLiveShim extends Extension {
    enable() {
        this._client = null;
        this._process = null;
        this._stdin = null;
        // MetaWindow → signal ids, one window per monitor.
        this._windows = new Map();
        this._paused = false;
        this._syncId = 0;
        // WorkspaceBackground → the clone of our window laid over it.
        this._clones = new Map();

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
            [Main.overview, Main.overview.connect('showing', () => {
                this._cloneIntoOverview();
                sync();
            })],
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
        this._removeClones();
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
        this._removeClones();
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

    // Every workspace (per monitor) in the overview has a WorkspaceBackground
    // holding a copy of the static background; put the video over it. They are
    // long-lived and created by the shell, so they are found by walking the
    // overview rather than by patching the class.
    _cloneIntoOverview() {
        const sources = new Map();
        for (const win of this._windows.keys()) {
            const actor = win.get_compositor_private();
            if (actor)
                sources.set(win.get_monitor(), actor);
        }
        if (sources.size === 0)
            return;

        const pending = [Main.layoutManager.overviewGroup];
        while (pending.length > 0) {
            const actor = pending.pop();
            if (actor instanceof Workspace.WorkspaceBackground)
                this._cloneOver(actor, sources.get(actor._monitorIndex));
            else
                pending.push(...actor.get_children());
        }
    }

    _cloneOver(background, source) {
        const group = background._backgroundGroup;
        if (!source || !group)
            return;
        const existing = this._clones.get(background);
        if (existing?.source === source)
            return;
        existing?.destroy();

        // Gets the whole background group (a BinLayout) and covers it.
        const clone = new CoverClone({source, x_expand: true, y_expand: true});
        clone.add_effect(new RoundedLikeBackground(background));
        group.add_child(clone);
        group.set_child_above_sibling(clone, null);
        // The static image underneath would show through the antialiased
        // corners; with it hidden they reveal the overview, as GNOME's do.
        const staticActor = background._bgManager?.backgroundActor;
        if (staticActor)
            staticActor.opacity = 0;
        this._clones.set(background, clone);
        clone.connect('destroy', () => {
            if (this._clones.get(background) === clone)
                this._clones.delete(background);
            const current = background._bgManager?.backgroundActor;
            if (current)
                current.opacity = 255;
        });
    }

    _removeClones() {
        for (const clone of [...this._clones.values()])
            clone.destroy();
        this._clones.clear();
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
