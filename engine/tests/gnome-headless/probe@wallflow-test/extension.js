import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
const IFACE = `<node><interface name="test.Probe">
<method name="Report"><arg type="s" direction="out" name="json"/></method>
<method name="Shot"><arg type="s" direction="in" name="path"/></method>
<method name="Overview"><arg type="b" direction="in" name="show"/></method>
<method name="Max"><arg type="s" direction="in" name="cls"/><arg type="b" direction="in" name="on"/></method>
<method name="WorkspaceOf"><arg type="s" direction="in" name="cls"/><arg type="i" direction="out" name="index"/></method>
<method name="DropTargetAt"><arg type="d" direction="in" name="x"/><arg type="d" direction="in" name="y"/><arg type="s" direction="out" name="json"/></method>
<method name="Scenario"><arg type="s" direction="in" name="name"/><arg type="s" direction="out" name="json"/></method>
<method name="Open"><arg type="as" direction="in" name="argv"/></method>
</interface></node>`;
export default class Probe extends Extension {
  enable() {
    this._d = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
    this._d.export(Gio.DBus.session, '/test/Probe');
    Gio.bus_own_name_on_connection(Gio.DBus.session, 'test.Probe', 0, null, null);
  }
  disable() {}
  Report() {
    const bg = Main.layoutManager._backgroundGroup;
    const wg = global.window_group;
    const kids = wg.get_children();
    return JSON.stringify({
      overview: Main.overview.visible,
      windowGroupOrder: kids.map(k => k === bg ? 'BACKGROUND' : (k.meta_window ? `${k.meta_window.get_wm_class()}|${k.meta_window.get_title()}` : k.constructor.name)),
      windows: global.get_window_actors().map(a => { const w = a.meta_window; return {
        cls: w.get_wm_class(), title: w.get_title(), type: w.get_window_type(), skipTaskbar: w.skip_taskbar,
        sticky: w.is_on_all_workspaces(), max: (w.is_maximized ? w.is_maximized() : String(w.get_maximized?.())), canMax: w.can_maximize(), mon: w.get_monitor(), fs: w.is_fullscreen(), rect: (r => [r.x, r.y, r.width, r.height])(w.get_frame_rect()), buf: (r => [r.x, r.y, r.width, r.height])(w.get_buffer_rect()), actorBox: [a.x, a.y, a.width, a.height], parentIsWindowGroup: a.get_parent() === wg }; }),
      tabList: global.display.get_tab_list(0, null).map(w => w.get_wm_class()),
      workArea: (r => [r.x, r.y, r.width, r.height])(Main.layoutManager.getWorkAreaForMonitor(0)),
      overviewGeometry: (() => {
        const out = []; const todo = [Main.layoutManager.overviewGroup];
        while (todo.length) { const a = todo.pop();
          if (a.constructor.name.includes('WorkspaceBackground') && a._backgroundGroup) {
            const g = a._backgroundGroup; const bgActor = a._bgManager?.backgroundActor;
            out.push({bin: [a._bin.x, a._bin.y, a._bin.width, a._bin.height], group: [g.x, g.y, g.width, g.height],
              bg: bgActor ? [bgActor.x, bgActor.y, bgActor.width, bgActor.height] : null,
              radius: bgActor?.content?.rounded_clip_radius,
              kids: g.get_children().map(k => [k.constructor.name, k.x, k.y, k.width, k.height])});
          }
          todo.push(...a.get_children()); }
        return out; })(),
      // Per workspace background: is the video clone the top child, and is
      // every static background actor under it hidden?
      overviewVideoOnTop: (() => {
        const out = []; const todo = [Main.layoutManager.overviewGroup];
        while (todo.length) { const a = todo.pop();
          if (a.constructor.name.includes('WorkspaceBackground') && a._backgroundGroup) {
            const kids = a._backgroundGroup.get_children();
            const top = kids[kids.length - 1];
            const staticShown = kids.some(k => k.constructor.name.includes('BackgroundActor') && k.opacity > 0);
            out.push(top instanceof Clutter.Clone && !staticShown);
          } else todo.push(...a.get_children()); }
        return out.length > 0 && out.every(Boolean); })(),
      // Size of the active workspace's card in the overview (its clip box).
      activeCard: (() => {
        const ws = global.workspace_manager.get_active_workspace();
        const todo = [Main.layoutManager.overviewGroup];
        while (todo.length) { const a = todo.pop();
          if (a.metaWorkspace === ws && a._background?._bin) {
            const [w, h] = a._background._bin.get_transformed_size();
            return [Math.round(w), Math.round(h)];
          }
          todo.push(...a.get_children()); }
        return null; })(),
      overviewClones: (() => {
        let n = 0; const todo = [Main.layoutManager.overviewGroup];
        while (todo.length) { const a = todo.pop();
          if (a instanceof Clutter.Clone && a.source?.meta_window?.get_wm_class() === 'wallflow-live' && a.get_parent()) n++;
          todo.push(...a.get_children()); }
        return n; })(),
    });
  }
  async Shot(path) {
    const s = new Shell.Screenshot();
    const stream = Gio.File.new_for_path(path).replace(null, false, 0, null);
    await s.screenshot(false, stream);
    stream.close(null);
  }
  Overview(show) { show ? Main.overview.show() : Main.overview.hide(); }
  Max(cls, on) {
    for (const a of global.get_window_actors()) {
      const w = a.meta_window;
      if (w.get_wm_class() !== cls) continue;
      if (on) { try { w.maximize(); } catch (e) { w.maximize(3); } }
      else { try { w.unmaximize(); } catch (e) { w.unmaximize(3); } }
    }
  }
  // What drag-and-drop would see under (x, y): DND picks with PickMode.ALL
  // and walks up to the first actor whose _delegate accepts drops.
  DropTargetAt(x, y) {
    const picked = global.stage.get_actor_at_pos(Clutter.PickMode.ALL, x, y);
    let target = null;
    for (let a = picked; a; a = a.get_parent())
      if (a._delegate?.acceptDrop) { target = a._delegate; break; }
    return JSON.stringify({picked: picked?.constructor.name ?? null,
      target: target?.constructor.name ?? null, workspace: target?.metaWorkspace?.index() ?? null});
  }
  // Things that happen in the overview while the video clone is shown.
  Scenario(name) {
    const editor = global.get_window_actors().map(a => a.meta_window).find(w => w.get_wm_class() === 'org.gnome.TextEditor');
    if (name === 'dragBegin' && editor) Main.overview.beginWindowDrag(editor);
    if (name === 'dragCancel' && editor) Main.overview.cancelledWindowDrag(editor);
    if (name === 'dragEnd' && editor) Main.overview.endWindowDrag(editor);
    if (name === 'moveWindow' && editor) Main.moveWindowToMonitorAndWorkspace(editor, 0, 1);
    return JSON.stringify({name, editor: !!editor});
  }
  WorkspaceOf(cls) {
    const w = global.get_window_actors().map(a => a.meta_window).find(w => w.get_wm_class() === cls);
    return w ? w.get_workspace().index() : -1;
  }
  Open(argv) { Gio.Subprocess.new(argv, 0); }
}
