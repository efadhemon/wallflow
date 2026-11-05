/**
 * Dev only: lets `yarn dev` run in a plain browser by standing in for the Tauri
 * backend with sample data, for working on the UI without the desktop shell.
 * `window.__mock` holds the state, so a screen can be switched from the console
 * (e.g. `__mock.live.support = "needsLogin"`). Not part of production builds.
 */

type Callback = (event: { event: string; payload: unknown; id: number }) => void;

const SCREENSHOTS = "/website/assets/screenshots/";
const POSTERS = ["dashboard.png", "gallery.png", "library.png"].map(name => SCREENSHOTS + name);

export function installBrowserMock() {
  const callbacks = new Map<number, Callback>();
  const listeners: { id: number; event: string; handler: number }[] = [];
  let nextId = 1;

  const mock = {
    status: {
      current: { path: POSTERS[0], name: "sunset.jpg", folder: "~/Pictures" },
      paused: false,
      nextChangeAt: Date.now() + 120_000,
      intervalSeconds: 300,
      totalImages: 12,
      playlistSize: 12,
      position: 3,
      lastError: null,
      live: null as string | null,
    },
    config: {
      version: 1,
      folders: [],
      intervalSeconds: 300,
      order: "shuffle",
      fillMode: "fill",
      perMonitor: false,
      changeOnStartup: true,
      excluded: [],
      autostart: true,
      startMinimized: true,
      theme: "system",
      liveVideo: null,
    },
    live: {
      support: "ready",
      reason: null as string | null,
      ffmpeg: true,
      engine: true,
      running: false,
      paused: false,
    },
    videos: [
      { id: "a1", name: "Neon particles", width: 1920, height: 1080, fps: 30, duration: 14.2, addedAt: 3 },
      { id: "b2", name: "Ink in water", width: 1920, height: 1080, fps: 25, duration: 32, addedAt: 2 },
      { id: "c3", name: "Aurora loop", width: 2560, height: 1440, fps: 24, duration: 75, addedAt: 1 },
    ].map((video, i) => ({ ...video, videoPath: `/videos/${video.id}.mp4`, posterPath: POSTERS[i] })),
    emit(event: string, payload: unknown) {
      for (const listener of listeners)
        if (listener.event === event) callbacks.get(listener.handler)?.({ event, payload, id: listener.id });
    },
    /** Simulates adding a video: progress events, then a new library entry. */
    importVideo(name: string) {
      const id = `new${nextId++}`;
      let fraction = 0;
      const timer = window.setInterval(() => {
        fraction = Math.min(1, fraction + 0.1);
        const done = fraction >= 1;
        if (done) {
          window.clearInterval(timer);
          mock.videos.unshift({ ...mock.videos[0], id, name, addedAt: Date.now() });
        }
        mock.emit("live://import", { id, name, fraction, state: done ? "done" : "preparing", error: null });
      }, 400);
    },
  };

  const commands: Record<string, (args: Record<string, unknown>) => unknown> = {
    "plugin:event|listen": args => {
      const id = nextId++;
      listeners.push({ id, event: String(args.event), handler: Number(args.handler) });
      return id;
    },
    get_status: () => mock.status,
    get_config: () => mock.config,
    set_config: args => (mock.config = args.config as typeof mock.config),
    get_environment: () => ({
      os: "linux",
      backend: "GNOME",
      monitorCount: 1,
      supportsPerMonitor: false,
      appVersion: "dev",
      configPath: "",
      logPath: "",
      cachePath: "",
    }),
    get_thumbnail: args => args.path,
    get_preview: args => args.path,
    list_images: () => [],
    list_folders: () => [],
    live_status: () => ({ ...mock.live, active: mock.status.live }),
    live_list: () => mock.videos,
    live_import: () => {
      mock.importVideo("City lights");
      return [];
    },
    live_install_helper: () => {
      mock.live.support = "needsLogin";
      return { ...mock.live, active: mock.status.live };
    },
    live_play: args => {
      mock.status = { ...mock.status, live: String(args.id) };
      mock.live.running = true;
      mock.emit("wallpaper://status", mock.status);
    },
    live_stop: () => {
      mock.status = { ...mock.status, live: null };
      mock.live.running = false;
      mock.emit("wallpaper://status", mock.status);
    },
    live_remove: args => {
      mock.videos = mock.videos.filter(video => video.id !== args.id);
    },
    "plugin:dialog|open": () => ["/videos/city-lights.mp4"],
  };

  Object.assign(window, {
    __mock: mock,
    __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
    __TAURI_INTERNALS__: {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
      transformCallback(callback: Callback) {
        const id = nextId++;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback(id: number) {
        callbacks.delete(id);
      },
      convertFileSrc: (path: string) => path,
      invoke: async (cmd: string, args: Record<string, unknown> = {}) => commands[cmd]?.(args) ?? null,
    },
  });
}
