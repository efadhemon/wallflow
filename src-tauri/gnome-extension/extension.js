import Clutter from "gi://Clutter";
import Cogl from "gi://Cogl";
import Gio from "gi://Gio";
import GLib from "gi://GLib";
import Gst from "gi://Gst";
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

export default class WallflowLive extends Extension {
  enable() {
    try {
      Gst.init(null);
    } catch (error) {
      logError(error, "Wallflow: Gst.init failed");
    }
    this._actors = [];
    this._pipeline = null;
    this._owner = Gio.DBus.session.own_name("com.efadhemon.Wallflow.Live", Gio.BusNameOwnerFlags.NONE, null, null);
    this._dbus = Gio.DBusExportedObject.wrapJSObject(DBUS_IFACE, this);
    this._dbus.export(Gio.DBus.session, "/com/efadhemon/Wallflow/Live");
  }

  disable() {
    this.Stop();
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
    if (this._pipeline) {
      this._pipeline.set_state(Gst.State.NULL);
      this._pipeline = null;
    }
    for (const actor of this._actors) actor.destroy();
    this._actors = [];
  }

  SetVideo(path, fillMode, muted) {
    this.Stop();
    const group = Main.layoutManager._backgroundGroup;
    if (!group) return;

    const pipe = Gst.ElementFactory.make("playbin", "wallflow-live");
    if (!pipe) throw new Error("GStreamer playbin is not available");

    const bin = Gst.parse_bin_from_description(
      "videoconvert ! video/x-raw,format=RGBA ! appsink name=sink emit-signals=true max-buffers=1 drop=true sync=true",
      true,
    );
    const sink = bin.get_by_name("sink");
    if (!sink) throw new Error("GStreamer appsink is not available");

    pipe.uri = Gio.File.new_for_path(path).get_uri();
    pipe.mute = muted;
    pipe.video_sink = bin;

    const actors = [];
    for (const monitor of Main.layoutManager.monitors) {
      const actor = new St.Widget({
        reactive: false,
        x: monitor.x,
        y: monitor.y,
        width: monitor.width,
        height: monitor.height,
      });
      actor.content_gravity = _gravity(fillMode);
      // Above the still wallpaper, still inside the background group so icons stay on top.
      group.add_child(actor);
      actors.push(actor);
    }

    const format = Cogl.PixelFormat.RGBA_8888 ?? Cogl.PixelFormat.Rgba8888;
    sink.connect("new-sample", appsink => {
      const sample = appsink.emit("pull-sample");
      if (!sample) return Gst.FlowReturn.OK;
      const caps = sample.get_caps();
      const structure = caps.get_structure(0);
      const [, width] = structure.get_int("width");
      const [, height] = structure.get_int("height");
      const buffer = sample.get_buffer();
      const [, map] = buffer.map(Gst.MapFlags.READ);
      try {
        const bytes = GLib.Bytes.new(map.data);
        const content = St.ImageContent.new_with_preferred_size(width, height);
        _setBytes(content, bytes, format, width, height, width * 4);
        for (const actor of actors) actor.set_content(content);
      } catch (error) {
        logError(error, "Wallflow: could not paint a video frame");
      } finally {
        buffer.unmap(map);
      }
      return Gst.FlowReturn.OK;
    });

    const bus = pipe.get_bus();
    bus.add_signal_watch();
    bus.connect("message::error", (_bus, message) => {
      const [error] = message.parse_error();
      log(`Wallflow live wallpaper: ${error.message}`);
    });
    bus.connect("message::eos", () => {
      pipe.seek_simple(Gst.Format.TIME, Gst.SeekFlags.FLUSH | Gst.SeekFlags.KEY_UNIT, 0);
    });

    pipe.set_state(Gst.State.PLAYING);
    this._pipeline = pipe;
    this._actors = actors;
  }
}

function _setBytes(content, bytes, format, width, height, rowStride) {
  const args = [bytes, format, width, height, rowStride];
  const backend = global.stage?.context?.get_backend?.();
  // GNOME 48+: set_bytes(coglContext, bytes, format, width, height, rowStride)
  if (content.set_bytes.length >= 6 && backend?.get_cogl_context) args.unshift(backend.get_cogl_context());
  content.set_bytes(...args);
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
