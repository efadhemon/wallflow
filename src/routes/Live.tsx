import { open } from "@tauri-apps/plugin-dialog";
import { Clapperboard, Film, LoaderCircle, Pause, Play, Plus, Square, Trash2, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Thumbnail } from "../components/Thumbnail";
import { Banner, Button, Card, EmptyState, SectionTitle } from "../components/ui";
import { api, onLiveImport } from "../lib/api";
import type { LiveImportProgress, LiveStatus, LiveVideo } from "../lib/types";
import { cn } from "../lib/utils";
import { useApp } from "../state/AppContext";

const VIDEO_EXTENSIONS = ["mp4", "mkv", "webm", "mov", "m4v", "avi"];

/** How often the page re-reads the engine state (playing / paused by a covered desktop). */
const STATUS_POLL_MS = 3000;

/** 75 → "1:15". */
function formatLength(seconds: number | null): string | null {
  if (seconds == null || !Number.isFinite(seconds)) return null;
  const total = Math.round(seconds);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

function describe(video: LiveVideo): string {
  return [`${video.width}×${video.height}`, `${Math.round(video.fps)} fps`, formatLength(video.duration)]
    .filter(Boolean)
    .join(" · ");
}

export function Live() {
  const { status, notify } = useApp();
  const [live, setLive] = useState<LiveStatus | null>(null);
  const [videos, setVideos] = useState<LiveVideo[]>([]);
  const [imports, setImports] = useState<Map<string, LiveImportProgress>>(new Map());
  const [busy, setBusy] = useState(false);

  const reloadStatus = useCallback(async () => {
    try {
      setLive(await api.liveStatus());
    } catch (error) {
      notify(String(error), "error");
    }
  }, [notify]);

  const reloadVideos = useCallback(async () => {
    try {
      setVideos(await api.liveList());
    } catch (error) {
      notify(String(error), "error");
    }
  }, [notify]);

  useEffect(() => {
    void reloadVideos();
  }, [reloadVideos]);

  // The scheduler republishes when a live wallpaper starts or stops.
  useEffect(() => {
    void reloadStatus();
    const id = window.setInterval(() => void reloadStatus(), STATUS_POLL_MS);
    return () => window.clearInterval(id);
  }, [reloadStatus, status?.live]);

  useEffect(() => {
    const listening = onLiveImport(progress => {
      setImports(current => {
        const next = new Map(current);
        if (progress.state === "preparing") next.set(progress.id, progress);
        else next.delete(progress.id);
        return next;
      });
      if (progress.state === "done") void reloadVideos();
      if (progress.state === "failed" && progress.error) notify(progress.error, "error");
    });
    return () => void listening.then(unlisten => unlisten());
  }, [notify, reloadVideos]);

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
      await Promise.all([reloadStatus(), reloadVideos()]);
    } catch (error) {
      notify(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const addVideos = async () => {
    const selected = await open({
      multiple: true,
      title: "Choose videos",
      filters: [{ name: "Videos", extensions: VIDEO_EXTENSIONS }],
    });
    if (selected == null) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    if (paths.length === 0) return;
    try {
      const added = await api.liveImport(paths);
      if (added.length > 0)
        notify(added.length === 1 ? "Added 1 live wallpaper." : `Added ${added.length} live wallpapers.`);
      await reloadVideos();
    } catch (error) {
      notify(String(error), "error");
    }
  };

  const activeId = status?.live ?? null;
  const active = useMemo(() => videos.find(video => video.id === activeId) ?? null, [videos, activeId]);
  const canPlay = live?.support === "ready" && live.ffmpeg && live.engine;
  const pending = [...imports.values()];

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-6 px-8 py-8">
      <SetupBanner live={live} busy={busy} onSetup={() => void run(() => api.liveInstallHelper())} />

      {active && (
        <Card className="overflow-hidden">
          <div className="flex items-center gap-4 p-4">
            <Thumbnail
              path={active.posterPath}
              alt={active.name}
              eager
              className="aspect-video w-40 shrink-0 rounded-lg"
            />
            <div className="min-w-0 flex-1">
              <p className="text-xs font-medium tracking-wide text-ink-muted uppercase">Live wallpaper</p>
              <p className="mt-0.5 truncate text-base font-semibold text-ink">{active.name}</p>
              <p className="mt-1 flex items-center gap-1.5 text-xs text-ink-muted">
                {live?.paused ? (
                  <>
                    <Pause size={12} />
                    Paused while the desktop is covered
                  </>
                ) : live?.running ? (
                  <>
                    <span className="h-2 w-2 animate-pulse rounded-full bg-emerald-500" />
                    Playing · {describe(active)}
                  </>
                ) : (
                  <>
                    <LoaderCircle size={12} className="animate-spin" />
                    Starting…
                  </>
                )}
              </p>
            </div>
            <Button onClick={() => void run(() => api.liveStop())} disabled={busy}>
              <Square size={14} />
              Stop
            </Button>
          </div>
        </Card>
      )}

      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <SectionTitle>Live wallpapers {videos.length > 0 && `(${videos.length})`}</SectionTitle>
          <Button size="sm" variant="primary" onClick={() => void addVideos()} disabled={!live?.ffmpeg || !live.engine}>
            <Plus size={14} />
            Add videos
          </Button>
        </div>

        {videos.length === 0 && pending.length === 0 ? (
          <Card>
            <EmptyState
              icon={<Clapperboard size={24} />}
              title="No live wallpapers yet"
              description="Add a video and Wallflow plays it behind your desktop. It is prepared once for your screen, and pauses on its own whenever the desktop is covered."
              action={
                <Button variant="primary" onClick={() => void addVideos()} disabled={!live?.ffmpeg || !live.engine}>
                  <Plus size={14} />
                  Add videos
                </Button>
              }
            />
          </Card>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-3">
            {pending.map(progress => (
              <ImportTile key={progress.id} progress={progress} />
            ))}
            {videos.map(video => (
              <VideoTile
                key={video.id}
                video={video}
                active={video.id === activeId}
                disabled={busy || !canPlay}
                onPlay={() => void run(() => api.livePlay(video.id))}
                onRemove={() => void run(() => api.liveRemove(video.id))}
              />
            ))}
          </div>
        )}
      </section>

      <p className="text-xs leading-relaxed text-ink-muted">
        While a live wallpaper plays, the image rotation waits and the video's first frame is your static wallpaper
        (lock screen, login). Picking an image in the Library or pressing Next ends it.
      </p>
    </div>
  );
}

function SetupBanner({ live, busy, onSetup }: { live: LiveStatus | null; busy: boolean; onSetup: () => void }) {
  if (!live) return null;
  if (!live.engine)
    return (
      <Banner tone="error" title="The live wallpaper engine is missing">
        This installation does not include wallflow-live. Reinstall Wallflow.
      </Banner>
    );
  if (!live.ffmpeg)
    return (
      <Banner tone="error" title="FFmpeg is needed for live wallpapers">
        Install it (on Ubuntu: <code>sudo apt install ffmpeg</code>), then come back to this page.
      </Banner>
    );
  if (live.support === "unsupported")
    return <Banner title="Live wallpapers are not available here yet">{live.reason}</Banner>;
  if (live.support === "needsSetup")
    return (
      <Banner
        title="One-time setup for GNOME"
        actions={
          <Button size="sm" variant="primary" onClick={onSetup} disabled={busy}>
            Set up
          </Button>
        }
      >
        GNOME needs a small helper extension to place the video behind your desktop. It does no video work itself, so
        the desktop stays smooth. You will be asked to log out once.
      </Banner>
    );
  if (live.support === "needsLogin")
    return (
      <Banner title="Log out and back in to finish">
        The GNOME helper is installed. GNOME loads it at your next login; after that you can play live wallpapers here.
      </Banner>
    );
  return null;
}

function VideoTile({
  video,
  active,
  disabled,
  onPlay,
  onRemove,
}: {
  video: LiveVideo;
  active: boolean;
  disabled: boolean;
  onPlay: () => void;
  onRemove: () => void;
}) {
  return (
    <div
      className={cn(
        "group relative overflow-hidden rounded-lg border transition",
        active ? "border-brand ring-2 ring-brand/30" : "border-line",
      )}
    >
      <Thumbnail path={video.posterPath} alt={video.name} className="aspect-video w-full" />

      <div className="pointer-events-none absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/75 to-transparent px-2.5 pt-8 pb-2">
        <p className="truncate text-xs font-medium text-white">{video.name}</p>
        <p className="truncate text-[10px] text-white/70">{describe(video)}</p>
      </div>

      {active ? (
        <span className="absolute top-2 left-2 flex items-center gap-1 rounded-full bg-brand px-2 py-0.5 text-[10px] font-semibold text-on-brand">
          <Film size={10} />
          Playing
        </span>
      ) : (
        <Button
          size="sm"
          variant="primary"
          disabled={disabled}
          onClick={onPlay}
          className="absolute top-1/2 left-1/2 h-8 -translate-x-1/2 -translate-y-1/2 px-3 opacity-0 transition group-hover:opacity-100 focus-visible:opacity-100"
        >
          <Play size={14} />
          Play
        </Button>
      )}

      <button
        type="button"
        onClick={onRemove}
        title="Remove from library"
        aria-label={`Remove ${video.name}`}
        className="focus-ring absolute top-2 right-2 flex h-6 w-6 cursor-pointer items-center justify-center rounded-md bg-black/55 text-white opacity-0 transition group-hover:opacity-100 hover:bg-danger"
      >
        <Trash2 size={13} />
      </button>
    </div>
  );
}

function ImportTile({ progress }: { progress: LiveImportProgress }) {
  const percent = Math.round(progress.fraction * 100);
  return (
    <div className="relative flex aspect-video flex-col items-center justify-center gap-2 overflow-hidden rounded-lg border border-dashed border-line bg-surface-muted px-4 text-center">
      {progress.state === "failed" ? (
        <TriangleAlert size={20} className="text-danger" />
      ) : (
        <LoaderCircle size={20} className="animate-spin text-ink-muted" />
      )}
      <p className="w-full truncate text-xs font-medium text-ink">{progress.name}</p>
      <p className="text-[10px] text-ink-muted">Preparing for your screen · {percent}%</p>
      <div className="absolute inset-x-0 bottom-0 h-1 bg-line">
        <div className="h-full bg-brand transition-[width] duration-300" style={{ width: `${percent}%` }} />
      </div>
    </div>
  );
}
