import Clutter from "gi://Clutter";
import Cogl from "gi://Cogl";
import Gio from "gi://Gio";
import GLib from "gi://GLib";
import Gst from "gi://Gst";
import Shell from "gi://Shell";
import St from "gi://St";
import * as Main from "resource:///org/gnome/shell/ui/main.js";
import { Extension } from "resource:///org/gnome/shell/extensions/extension.js";

const DBUS_IFACE = `
<node>
  <interface name="com.efadhemon.Wallflow.Live">
    <method name="SetVideo">
      <arg type="s" direction="in" name="path"/>
      <arg type="s" direction="in" name="fillMode"/>
      <arg type="b" direction="in" name="muted"/>
    </method>
    <method name="Stop"/>
    <method name="Pause">
      <arg type="b" direction="in" name="paused"/>
    </method>
    <method name="Ping">
      <arg type="b" direction="out" name="ok"/>
    </method>
  </interface>
</node>`;

const GST_PLAY_FLAG_VIDEO = 1;
const GST_PLAY_FLAG_AUDIO = 2;
const GST_PLAY_FLAG_SOFT_VOLUME = 16;
const GST_PLAY_FLAG_DEINTERLACE = 512;

export default class WallflowLive extends Extension {
  enable() {
    try {
      Gst.init(null);
    } catch (error) {
      console.error("Wallflow: Gst.init failed", error);
    }
    this._actors = [];
    this._overviewOverlays = [];
    this._pipeline = null;
    this._sink = null;
    this._bus = null;
    this._content = null;
    this._tick = 0;
    this._clock = null;
    this._monitorId = 0;
    this._childAddedId = 0;
    this._childRemovedId = 0;
    this._overviewShowingId = 0;
    this._overviewShownId = 0;
    this._overviewHiddenId = 0;
    this._overviewChildId = 0;
    this._overviewRetryId = 0;
    this._group = null;
    this._overviewGroup = null;
    this._path = null;
    this._fillMode = "fill";
    this._muted = true;
    this._overviewNeedsContent = false;
    this._owner = Gio.DBus.session.own_name("com.efadhemon.Wallflow.Live", Gio.BusNameOwnerFlags.NONE, null, null);
    this._dbus = Gio.DBusExportedObject.wrapJSObject(DBUS_IFACE, this);
    this._dbus.export(Gio.DBus.session, "/com/efadhemon/Wallflow/Live");
    this._monitorId = Main.layoutManager.connect("monitors-changed", () => {
      if (this._path && this._pipeline) this.SetVideo(this._path, this._fillMode, this._muted);
    });
    this._overviewShowingId = Main.overview.connect("showing", () => this._syncOverview(true));
    this._overviewShownId = Main.overview.connect("shown", () => this._syncOverview(true));
    this._overviewHiddenId = Main.overview.connect("hidden", () => this._syncOverview(false));
  }

  disable() {
    this.Stop();
    if (this._monitorId) {
      Main.layoutManager.disconnect(this._monitorId);
      this._monitorId = 0;
    }
    if (this._overviewShowingId) Main.overview.disconnect(this._overviewShowingId);
    if (this._overviewShownId) Main.overview.disconnect(this._overviewShownId);
    if (this._overviewHiddenId) Main.overview.disconnect(this._overviewHiddenId);
    this._overviewShowingId = 0;
    this._overviewShownId = 0;
    this._overviewHiddenId = 0;
    if (this._dbus) {
      this._dbus.unexport();
      this._dbus = null;
    }
    if (this._owner) {
      Gio.DBus.session.unown_name(this._owner);
      this._owner = null;
    }
  }

  Ping() {
    return true;
  }

  Pause(paused) {
    if (!this._pipeline) return;
    this._pipeline.set_state(paused ? Gst.State.PAUSED : Gst.State.PLAYING);
  }

  Stop() {
    this._path = null;
    this._overviewNeedsContent = false;
    this._stopClock();
    this._unwatchGroup();
    this._unwatchOverviewGroup();
    this._stopOverviewRetry();
    this._clearOverviewOverlays();
    this._content = null;
    this._sink = null;
    if (this._bus) {
      try {
        this._bus.remove_signal_watch();
      } catch {
        /* already gone */
      }
      this._bus = null;
    }
    if (this._pipeline) {
      this._pipeline.set_state(Gst.State.NULL);
      this._pipeline.get_state(80 * Gst.MSECOND);
      this._pipeline = null;
    }
    for (const actor of this._actors) {
      try {
        actor.destroy();
      } catch {
        /* already destroyed with the wallpaper actor */
      }
    }
    this._actors = [];
  }

  SetVideo(path, fillMode, muted) {
    const keepPath = path;
    this.Stop();
    this._path = keepPath;
    this._fillMode = fillMode;
    this._muted = muted;

    const pipe = Gst.ElementFactory.make("playbin", "wallflow-live");
    if (!pipe) throw new Error("GStreamer playbin is not available");

    const { width, height } = _desktopSize();
    const bin = _sinkBin(width, height, fillMode);
    const sink = bin.get_by_name("sink");
    if (!sink) throw new Error("GStreamer appsink is not available");
    sink.set_property("emit-signals", false);

    pipe.uri = Gio.File.new_for_path(path).get_uri();
    pipe.mute = muted;
    pipe.video_sink = bin;
    let flags = GST_PLAY_FLAG_VIDEO | GST_PLAY_FLAG_DEINTERLACE;
    if (!muted) flags |= GST_PLAY_FLAG_AUDIO | GST_PLAY_FLAG_SOFT_VOLUME;
    pipe.set_property("flags", flags);

    const actors = _makeActors(fillMode);
    if (actors.length === 0) {
      pipe.set_state(Gst.State.NULL);
      throw new Error("GNOME Shell has no desktop background to play on");
    }
    this._watchGroup();

    const format = _pixelFormat();
    const bus = pipe.get_bus();
    bus.add_signal_watch();
    bus.connect("message::error", (_bus, message) => {
      const [error] = message.parse_error();
      console.error(`Wallflow live wallpaper: ${error.message}`);
    });
    bus.connect("message::eos", () => {
      if (this._pipeline !== pipe) return;
      pipe.seek_simple(Gst.Format.TIME, Gst.SeekFlags.FLUSH, 0);
    });

    const change = pipe.set_state(Gst.State.PLAYING);
    if (change === Gst.StateChangeReturn.FAILURE) {
      bus.remove_signal_watch();
      pipe.set_state(Gst.State.NULL);
      for (const actor of actors) actor.destroy();
      throw new Error("GStreamer could not decode this video. Install gstreamer1.0-libav.");
    }

    this._pipeline = pipe;
    this._sink = sink;
    this._bus = bus;
    this._actors = actors;
    this._startClock(() => {
      if (this._pipeline !== pipe || this._sink !== sink) return false;
      _pullFrame(this, sink, format, this._actors);
      return true;
    });
    if (Main.overview.visible) this._syncOverview(true);
  }

  _onContentReady() {
    if (this._overviewNeedsContent || Main.overview.visible) this._syncOverview(true);
  }

  _syncOverview(visible) {
    this._stopOverviewRetry();
    if (!visible) {
      this._overviewNeedsContent = false;
      this._unwatchOverviewGroup();
      this._clearOverviewOverlays();
      return;
    }
    if (!this._path) return;
    if (!this._content) {
      this._overviewNeedsContent = true;
      this._watchOverviewGroup();
      return;
    }
    this._attachOverview();
    this._watchOverviewGroup();
    let tries = 0;
    this._overviewRetryId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 50, () => {
      if (!this._path || !(Main.overview.visible || Main.overview._shown)) {
        this._overviewRetryId = 0;
        return GLib.SOURCE_REMOVE;
      }
      this._attachOverview();
      tries += 1;
      if (this._overviewOverlays.length > 0 && tries >= 2) {
        this._overviewRetryId = 0;
        return GLib.SOURCE_REMOVE;
      }
      return tries < 12 ? GLib.SOURCE_CONTINUE : GLib.SOURCE_REMOVE;
    });
  }

  _stopOverviewRetry() {
    if (this._overviewRetryId) {
      GLib.source_remove(this._overviewRetryId);
      this._overviewRetryId = 0;
    }
  }

  _attachOverview() {
    if (!this._content || !this._path) return;
    this._overviewNeedsContent = false;
    for (const parent of _overviewOverlayParents()) this._addOverviewOverlay(parent);
  }

  _addOverviewOverlay(parent) {
    if (!parent || !this._content) return;
    if (this._overviewOverlays.some(actor => {
      try {
        return actor.get_parent() === parent;
      } catch {
        return false;
      }
    })) return;

    const actor = new St.Widget({
      reactive: false,
      x_expand: true,
      y_expand: true,
    });
    actor.content_gravity = _gravity(this._fillMode);
    actor.set_content(this._content);
    try {
      parent.add_child(actor);
      if (parent.set_child_above_sibling) parent.set_child_above_sibling(actor, null);
      this._overviewOverlays.push(actor);
    } catch {
      try {
        actor.destroy();
      } catch {
        /* ignore */
      }
    }
  }

  _clearOverviewOverlays() {
    for (const actor of this._overviewOverlays) {
      try {
        actor.destroy();
      } catch {
        /* overview already tore this down */
      }
    }
    this._overviewOverlays = [];
  }

  _watchOverviewGroup() {
    if (this._overviewChildId) return;
    const group = Main.layoutManager.overviewGroup;
    if (!group) return;
    this._overviewGroup = group;
    this._overviewChildId = group.connect("child-added", () => {
      GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
        if (this._path && this._content && Main.overview.visible) this._attachOverview();
        return GLib.SOURCE_REMOVE;
      });
    });
  }

  _unwatchOverviewGroup() {
    if (this._overviewGroup && this._overviewChildId) {
      try {
        this._overviewGroup.disconnect(this._overviewChildId);
      } catch {
        /* already disconnected */
      }
    }
    this._overviewGroup = null;
    this._overviewChildId = 0;
  }

  _startClock(tick) {
    const stage = global.stage;
    try {
      const clock = stage.get_frame_clock?.() ?? stage.context?.get_frame_clock?.();
      if (clock) {
        this._clock = clock;
        this._tick = clock.connect("new-frame", () => tick());
        return;
      }
    } catch {
      /* fall back to a timer */
    }
    this._tick = GLib.timeout_add(GLib.PRIORITY_HIGH, 16, () => {
      return tick() ? GLib.SOURCE_CONTINUE : GLib.SOURCE_REMOVE;
    });
  }

  _stopClock() {
    if (this._clock && this._tick) {
      this._clock.disconnect(this._tick);
      this._clock = null;
      this._tick = 0;
      return;
    }
    if (this._tick) {
      GLib.source_remove(this._tick);
      this._tick = 0;
    }
  }

  _watchGroup() {
    this._unwatchGroup();
    const group = Main.layoutManager._backgroundGroup;
    if (!group) return;
    this._group = group;
    this._childAddedId = group.connect("child-added", () => this._ensureActors());
    this._childRemovedId = group.connect("child-removed", () => this._ensureActors());
  }

  _unwatchGroup() {
    if (this._group && this._childAddedId) this._group.disconnect(this._childAddedId);
    if (this._group && this._childRemovedId) this._group.disconnect(this._childRemovedId);
    this._group = null;
    this._childAddedId = 0;
    this._childRemovedId = 0;
  }

  _ensureActors() {
    if (!this._path) return;
    const live = this._actors.filter(actor => {
      try {
        return Boolean(actor.get_parent());
      } catch {
        return false;
      }
    });
    if (live.length === 0) {
      this._actors = _makeActors(this._fillMode);
      if (this._content) {
        for (const actor of this._actors) actor.set_content(this._content);
      }
      return;
    }
    this._actors = live;
    _raise(live);
  }
}

function _overviewOverlayParents() {
  const parents = [];
  const seen = new Set();
  const add = actor => {
    if (!actor || seen.has(actor)) return;
    seen.add(actor);
    parents.push(actor);
  };

  try {
    const views = Main.overview._overview?.controls?._workspacesDisplay?._workspacesViews ?? [];
    for (const view of views) {
      const workspaces = view._workspaces ?? (view._workspace ? [view._workspace] : []);
      for (const workspace of workspaces) {
        const background = workspace._background;
        add(background?._backgroundGroup ?? background?._bin ?? background);
      }
    }
  } catch {
    /* private overview layout differs */
  }

  const roots = [Main.layoutManager.overviewGroup, Main.overview?._overview].filter(Boolean);
  for (const root of roots) {
    _walk(root, actor => {
      try {
        if (actor._backgroundGroup && _isWorkspaceBackground(actor)) add(actor._backgroundGroup);
        else if (actor._bgManager?.backgroundActor) add(actor._backgroundGroup ?? actor._bgManager.backgroundActor.get_parent());
      } catch {
        /* skip */
      }
    });
  }
  return parents;
}

function _isWorkspaceBackground(actor) {
  try {
    if (Shell.WorkspaceBackground && actor instanceof Shell.WorkspaceBackground) return true;
  } catch {
    /* type not available */
  }
  try {
    return Boolean(actor.has_style_class_name?.("workspace-background"));
  } catch {
    return false;
  }
}

function _walk(root, visit, depth = 0) {
  if (!root || depth > 14) return;
  try {
    visit(root);
  } catch {
    return;
  }
  let children = [];
  try {
    children = root.get_children?.() ?? [];
  } catch {
    return;
  }
  for (const child of children) _walk(child, visit, depth + 1);
}

function _desktopSize() {
  let width = 1920;
  let height = 1080;
  for (const monitor of Main.layoutManager.monitors) {
    width = Math.max(width, monitor.width);
    height = Math.max(height, monitor.height);
  }
  return { width: width - (width % 2), height: height - (height % 2) };
}

function _sinkBin(width, height, fillMode) {
  const scale =
    fillMode === "fit" || fillMode === "center"
      ? `videoscale method=4 add-borders=true ! video/x-raw,width=${width},height=${height}`
      : fillMode === "stretch"
        ? `videoscale method=4 add-borders=false ! video/x-raw,width=${width},height=${height}`
        : `aspectratiocrop aspect-ratio=${width}/${height} ! videoscale method=4 add-borders=false ! video/x-raw,width=${width},height=${height}`;
  const desc = `queue leaky=downstream max-size-buffers=2 max-size-bytes=0 max-size-time=0 ! ${scale} ! videoconvert dither=none ! video/x-raw,format=RGBA ! appsink name=sink max-buffers=1 drop=true sync=true`;
  try {
    return Gst.parse_bin_from_description(desc, true);
  } catch {
    return Gst.parse_bin_from_description(
      `videoscale method=4 add-borders=false ! video/x-raw,width=${width},height=${height} ! videoconvert dither=none ! video/x-raw,format=RGBA ! appsink name=sink max-buffers=1 drop=true sync=true`,
      true,
    );
  }
}

function _makeActors(fillMode) {
  const actors = [];
  const group = Main.layoutManager._backgroundGroup;
  if (!group) return actors;
  for (const monitor of Main.layoutManager.monitors) {
    const actor = new St.Widget({
      reactive: false,
      x: monitor.x,
      y: monitor.y,
      width: monitor.width,
      height: monitor.height,
    });
    actor.content_gravity = _gravity(fillMode);
    group.add_child(actor);
    actors.push(actor);
  }
  _raise(actors);
  return actors;
}

function _raise(actors) {
  const group = Main.layoutManager._backgroundGroup;
  if (!group?.set_child_above_sibling) return;
  for (const actor of actors) {
    try {
      if (actor.get_parent() === group) group.set_child_above_sibling(actor, null);
    } catch {
      /* actor was destroyed */
    }
  }
}

function _pullFrame(self, sink, format, actors) {
  let sample = null;
  try {
    sample = sink.emit("try-pull-sample", 0);
    let extra = sink.emit("try-pull-sample", 0);
    while (extra) {
      sample = extra;
      extra = sink.emit("try-pull-sample", 0);
    }
  } catch (error) {
    if (!self._loggedPullError) {
      self._loggedPullError = true;
      console.error("Wallflow: could not pull a video frame", error);
    }
    return;
  }
  if (!sample) return;

  const caps = sample.get_caps();
  const buffer = sample.get_buffer();
  const mapped = buffer.map(Gst.MapFlags.READ);
  const ok = Array.isArray(mapped) ? mapped[0] : Boolean(mapped);
  const map = Array.isArray(mapped) ? mapped[1] : mapped;
  if (!ok || !map?.data) return;
  try {
    self._latest = {
      bytes: GLib.Bytes.new(map.data),
      width: _capsInt(caps, "width"),
      height: _capsInt(caps, "height"),
      stride: _stride(caps, map.data.length),
    };
  } finally {
    buffer.unmap(map);
  }
  _paint(self, format, actors);
}

function _paint(self, format, actors) {
  const frame = self._latest;
  if (!frame || actors.length === 0) return;
  try {
    const created = !self._content || self._contentWidth !== frame.width || self._contentHeight !== frame.height;
    if (created) {
      self._content = St.ImageContent.new_with_preferred_size(frame.width, frame.height);
      self._contentWidth = frame.width;
      self._contentHeight = frame.height;
      for (const actor of actors) actor.set_content(self._content);
    }
    _setBytes(self._content, frame.bytes, format, frame.width, frame.height, frame.stride);
    for (const actor of actors) {
      if (actor.content !== self._content) actor.set_content(self._content);
    }
    for (const overlay of self._overviewOverlays ?? []) {
      try {
        if (overlay.content !== self._content) overlay.set_content(self._content);
        overlay.queue_redraw();
      } catch {
        /* overlay already destroyed */
      }
    }
    if (created) self._onContentReady();
  } catch (error) {
    if (!self._loggedPaintError) {
      self._loggedPaintError = true;
      console.error("Wallflow: could not paint a video frame", error);
    }
  }
}

function _capsInt(caps, name) {
  const [, value] = caps.get_structure(0).get_int(name);
  return value;
}

function _stride(caps, byteLength) {
  const width = _capsInt(caps, "width");
  const height = _capsInt(caps, "height");
  if (width > 0 && height > 0) return Math.max(width * 4, Math.floor(byteLength / height));
  return Math.max(width, 1) * 4;
}

function _pixelFormat() {
  return Cogl.PixelFormat.RGBA_8888 ?? Cogl.PixelFormat.Rgba8888 ?? Cogl.PixelFormat.RGBA8888;
}

function _setBytes(content, bytes, format, width, height, rowStride) {
  const backend = global.stage?.context?.get_backend?.();
  const ctx = backend?.get_cogl_context?.();
  if (ctx) content.set_bytes(ctx, bytes, format, width, height, rowStride);
  else content.set_bytes(bytes, format, width, height, rowStride);
}

function _gravity(fillMode) {
  switch (fillMode) {
    case "stretch":
      return Clutter.ContentGravity.RESIZE_FILL;
    case "fit":
    case "center":
    case "tile":
      return Clutter.ContentGravity.RESIZE_ASPECT;
    default:
      return Clutter.ContentGravity.RESIZE_ASPECT_FILL;
  }
}
