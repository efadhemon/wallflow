import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Check, Film, RefreshCw, Trash2, Video } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Thumbnail } from "../components/Thumbnail";
import { Banner, Button, Card, EmptyState, SectionTitle } from "../components/ui";
import { api } from "../lib/api";
import type { VideoEntry } from "../lib/types";
import { cn } from "../lib/utils";
import { useApp } from "../state/AppContext";

export function Live() {
  const { status, environment, notify, refresh } = useApp();
  const [videos, setVideos] = useState<VideoEntry[]>([]);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(async () => {
    try {
      setVideos(await api.listVideos());
    } catch (error) {
      notify(String(error), "error");
    }
  }, [notify]);

  useEffect(() => {
    void reload();
  }, [reload, status?.mode, status?.currentLive?.path]);

  const live = status?.mode === "live";
  const support = environment?.liveSupport ?? "none";
  const gnome = environment?.gnomeExtensionStatus ?? "notApplicable";

  const addVideos = async () => {
    const selected = await open({
      multiple: true,
      title: "Choose videos",
      filters: [{ name: "Videos", extensions: ["mp4", "mov", "m4v"] }],
    });
    if (selected == null) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    if (paths.length === 0) return;

    setBusy(true);
    try {
      const added = await api.addVideos(paths);
      if (added === 0) notify("Those videos are already in the library.");
      else notify(added === 1 ? "Added 1 video." : `Added ${added} videos.`);
      await reload();
    } catch (error) {
      notify(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const runAndReload = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
      await refresh();
      await reload();
    } catch (error) {
      notify(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const enableExtension = async () => {
    setBusy(true);
    try {
      const next = await api.enableGnomeExtension();
      await refresh();
      if (next === "needsLogout") {
        notify("Extension installed. Log out and back in, then set a live wallpaper.");
      } else if (next === "active") {
        notify("GNOME extension is active.");
      }
    } catch (error) {
      notify(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-6 px-8 py-8">
      {status?.lastError && (
        <Banner tone="error" title="Could not start live wallpaper">
          {status.lastError}
        </Banner>
      )}

      {support === "none" && (
        <Banner tone="error" title="Live wallpaper is not available here">
          This Wayland session has no public API to draw behind the desktop. Use GNOME with the Wallflow extension, an
          X11 session, or a compositor that supports wlr-layer-shell (Sway, Hyprland, KDE).
        </Banner>
      )}

      {support === "needsGnomeExtension" && gnome !== "active" && (
        <Banner title="GNOME needs a one-time desktop extension">
          Ubuntu on Wayland cannot place a window behind icons without a GNOME Shell extension.
          {gnome === "needsLogout"
            ? " The files are in place, but GNOME Shell only loads a new extension after you log out and back in (once). Then click the video to apply it."
            : " Wallflow will install its own extension (a few kilobytes of JavaScript, no extra app)."}
          <div className="mt-3">
            <Button size="sm" variant="primary" disabled={busy} onClick={() => void enableExtension()}>
              {gnome === "needsLogout" ? "Retry enable" : "Enable GNOME extension"}
            </Button>
          </div>
        </Banner>
      )}

      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <SectionTitle>Live wallpaper</SectionTitle>
          <div className="flex items-center gap-2">
            {live && (
              <Button size="sm" disabled={busy} onClick={() => runAndReload(() => api.stopLiveAndRotate())}>
                Stop and resume rotation
              </Button>
            )}
            <Button size="sm" variant="primary" onClick={() => void addVideos()} disabled={busy}>
              <Video size={14} />
              Add videos
            </Button>
          </div>
        </div>

        <Card>
          <p className="px-4 py-3 text-sm text-ink-muted">
            Apply a looping MP4 or MOV as the desktop. Image rotation pauses until you stop live wallpaper or pick an
            image from Library.
          </p>
        </Card>
      </section>

      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <SectionTitle>Videos {videos.length > 0 && `(${videos.length})`}</SectionTitle>
          <Button size="sm" variant="ghost" onClick={() => void reload()} disabled={busy}>
            <RefreshCw size={14} />
            Refresh
          </Button>
        </div>

        {videos.length === 0 ? (
          <Card>
            <EmptyState
              icon={<Film size={24} />}
              title="No videos yet"
              description="Add MP4, MOV or M4V files. Setting one as live wallpaper pauses Library rotation."
            />
          </Card>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3">
            {videos.map(video => (
              <div
                key={video.path}
                className={cn(
                  "group relative overflow-hidden rounded-lg border transition",
                  video.isCurrent ? "border-brand ring-2 ring-brand/30" : "border-line",
                )}
              >
                <button
                  type="button"
                  disabled={busy || support === "none"}
                  onClick={() => runAndReload(() => api.applyLive(video.path))}
                  onDoubleClick={() => void revealItemInDir(video.path)}
                  title={`${video.name}\nClick to set as live wallpaper`}
                  className="focus-ring block w-full cursor-pointer"
                >
                  <Thumbnail path={video.path} alt={video.name} className="aspect-[4/3] w-full" />
                </button>
                <div className="pointer-events-none absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/70 to-transparent px-2 pt-6 pb-1.5">
                  <p className="truncate text-[11px] text-white">{video.name}</p>
                </div>
                {video.isCurrent && (
                  <span className="absolute top-2 left-2 flex h-5 w-5 items-center justify-center rounded-full bg-brand text-on-brand">
                    <Check size={12} />
                  </span>
                )}
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => runAndReload(() => api.removeVideo(video.path))}
                  title="Remove video"
                  aria-label={`Remove ${video.name}`}
                  className="focus-ring absolute top-2 right-2 flex h-6 w-6 cursor-pointer items-center justify-center rounded-md bg-black/55 text-white opacity-0 transition group-hover:opacity-100"
                >
                  <Trash2 size={13} />
                </button>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
