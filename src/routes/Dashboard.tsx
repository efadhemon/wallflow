import { cn } from "cn";
import {
  AlertTriangle,
  ArrowRight,
  Clapperboard,
  ImageIcon,
  Images,
  LoaderCircle,
  Monitor,
  Pause,
  Play,
  SkipBack,
  SkipForward,
  Timer,
} from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Thumbnail } from "../components/Thumbnail";
import { Banner, Button, Card, EmptyState, SectionTitle } from "../components/ui";
import { api } from "../lib/api";
import type { LiveStatus, LiveVideo, Status } from "../lib/types";
import { describeVideo, formatCountdown, formatDuration } from "../lib/utils";
import { useApp } from "../state/AppContext";

/** How often the engine state (playing / paused by a covered desktop) is re-read while a video plays. */
const LIVE_POLL_MS = 3000;

/** Live wallpapers shown for a one-click switch while the image rotation runs. */
const QUICK_PICKS = 3;

/** Re-renders once a second so the countdown ticks. */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!active) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [active]);

  return now;
}

/** Engine state and the video library, kept fresh while a live wallpaper plays. */
function useLive(activeId: string | null) {
  const [live, setLive] = useState<LiveStatus | null>(null);
  const [videos, setVideos] = useState<LiveVideo[]>([]);

  const reload = useCallback(async () => {
    try {
      const [nextLive, nextVideos] = await Promise.all([api.liveStatus(), api.liveList()]);
      setLive(nextLive);
      setVideos(nextVideos);
    } catch {
      // The Live page reports engine problems; the dashboard just shows less.
    }
  }, []);

  useEffect(() => {
    void reload();
    if (!activeId) return;
    const id = window.setInterval(() => void reload(), LIVE_POLL_MS);
    return () => window.clearInterval(id);
  }, [reload, activeId]);

  return { live, videos };
}

export function Dashboard({
  onBrowse,
  onGallery,
  onLive,
}: {
  onBrowse: () => void;
  onGallery: () => void;
  onLive: () => void;
}) {
  const { status, environment, notify } = useApp();
  const { live, videos } = useLive(status?.live ?? null);
  const [busy, setBusy] = useState(false);

  if (!status) return null;

  const run = (action: () => Promise<unknown>) => async () => {
    setBusy(true);
    try {
      await action();
    } catch (error) {
      notify(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const liveAvailable = live != null && live.support !== "unsupported";
  const canPlay = live?.support === "ready" && live.ffmpeg && live.engine;
  const activeVideo = status.live ? (videos.find(video => video.id === status.live) ?? null) : null;

  const liveSection = liveAvailable && (
    <LivePicks
      live={live}
      videos={videos}
      disabled={busy || !canPlay}
      onPlay={id => void run(() => api.livePlay(id))()}
      onLive={onLive}
    />
  );

  if (!status.live && status.playlistSize === 0) {
    return (
      <div className="mx-auto flex max-w-3xl flex-col gap-5 px-8 py-10">
        <Card>
          <EmptyState
            icon={<ImageIcon size={24} />}
            title="No wallpapers yet"
            description="Add a folder, pick files, or browse the Gallery and Wallflow will start rotating your desktop background."
            action={
              <div className="flex flex-wrap justify-center gap-2">
                <Button onClick={onBrowse}>Open Library</Button>
                <Button variant="primary" onClick={onGallery}>
                  Open Gallery
                </Button>
              </div>
            }
          />
        </Card>
        {liveSection}
      </div>
    );
  }

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-5 px-8 py-8">
      {status.lastError && (
        <Banner tone="error" title="Could not change the wallpaper">
          {status.lastError}
        </Banner>
      )}

      {status.live ? (
        <>
          <LiveHero video={activeVideo} live={live} busy={busy} onStop={run(api.liveStop)} onChange={onLive} />
          <RotationOnHold status={status} onBrowse={onBrowse} />
        </>
      ) : (
        <>
          <ImageHero status={status} busy={busy} run={run} />
          {liveSection}
        </>
      )}

      <div className={cn("grid gap-4", liveAvailable ? "grid-cols-4" : "grid-cols-3")}>
        <Stat label="In rotation" value={status.playlistSize.toString()} />
        <Stat label="Found on disk" value={status.totalImages.toString()} />
        {liveAvailable && <Stat label="Live wallpapers" value={videos.length.toString()} />}
        <Stat
          label="Desktop"
          value={environment?.backend ?? "..."}
          icon={
            environment?.backend === "Unknown desktop" ? (
              <AlertTriangle size={14} className="text-danger" />
            ) : (
              <Monitor size={14} />
            )
          }
        />
      </div>
    </div>
  );
}

/** The big "what is on the desktop now" card. Images and videos share its shape so the switch feels like one place. */
function Hero({
  media,
  badge,
  corner,
  title,
  subtitle,
  status,
  controls,
}: {
  media: ReactNode;
  badge: ReactNode;
  corner?: ReactNode;
  title: string;
  subtitle: string;
  status: ReactNode;
  controls: ReactNode;
}) {
  return (
    <Card className="overflow-hidden">
      <div className="relative aspect-video w-full bg-surface-muted">
        {media}

        <div className="absolute top-4 left-4">{badge}</div>
        {corner && <div className="absolute top-4 right-4">{corner}</div>}

        <div className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/75 to-transparent px-5 pt-14 pb-4">
          <p className="truncate text-base font-semibold text-white">{title}</p>
          <p className="mt-0.5 truncate text-xs text-white/70">{subtitle}</p>
        </div>
      </div>

      <div className="flex items-center justify-between gap-4 px-5 py-4">
        <div className="flex min-w-0 items-center gap-2 text-sm text-ink-muted">{status}</div>
        <div className="flex shrink-0 items-center gap-2">{controls}</div>
      </div>
    </Card>
  );
}

function Pill({ children, tone = "dark" }: { children: ReactNode; tone?: "dark" | "brand" }) {
  return (
    <span
      className={cn(
        "flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium tabular-nums",
        tone === "brand" ? "bg-brand text-on-brand" : "bg-black/55 text-white",
      )}
    >
      {children}
    </span>
  );
}

function ImageHero({
  status,
  busy,
  run,
}: {
  status: Status;
  busy: boolean;
  run: (action: () => Promise<unknown>) => () => Promise<void>;
}) {
  const { current, paused, nextChangeAt, playlistSize, position } = status;
  const now = useNow(Boolean(nextChangeAt) && !paused);

  return (
    <Hero
      media={
        current ? (
          <Thumbnail eager path={current.path} alt={current.name} className="h-full w-full" />
        ) : (
          <div className="flex h-full items-center justify-center text-ink-muted">
            <ImageIcon size={28} />
          </div>
        )
      }
      badge={
        <Pill>
          <Images size={12} />
          Image rotation
        </Pill>
      }
      corner={
        <Pill>
          {position} / {playlistSize}
        </Pill>
      }
      title={current?.name ?? "No wallpaper set"}
      subtitle={current?.folder ?? ""}
      status={
        <>
          <Timer size={16} className="shrink-0" />
          {paused ? (
            <span>Paused</span>
          ) : nextChangeAt ? (
            <span>
              Next in <span className="font-semibold text-ink tabular-nums">{formatCountdown(nextChangeAt - now)}</span>
            </span>
          ) : (
            <span>Waiting</span>
          )}
          <span className="truncate text-ink-muted/60">&middot; every {formatDuration(status.intervalSeconds)}</span>
        </>
      }
      controls={
        <>
          <Button
            size="icon"
            onClick={run(api.previous)}
            disabled={busy}
            aria-label="Previous wallpaper"
            title="Previous wallpaper"
          >
            <SkipBack size={16} />
          </Button>
          <Button variant="primary" onClick={run(api.togglePause)} disabled={busy} className="w-28">
            {paused ? <Play size={16} /> : <Pause size={16} />}
            {paused ? "Resume" : "Pause"}
          </Button>
          <Button
            size="icon"
            onClick={run(api.next)}
            disabled={busy}
            aria-label="Next wallpaper"
            title="Next wallpaper"
          >
            <SkipForward size={16} />
          </Button>
        </>
      }
    />
  );
}

function LiveHero({
  video,
  live,
  busy,
  onStop,
  onChange,
}: {
  video: LiveVideo | null;
  live: LiveStatus | null;
  busy: boolean;
  onStop: () => void;
  onChange: () => void;
}) {
  return (
    <Hero
      media={
        video ? (
          <Thumbnail eager path={video.posterPath} alt={video.name} className="h-full w-full" />
        ) : (
          <div className="flex h-full items-center justify-center text-ink-muted">
            <Clapperboard size={28} />
          </div>
        )
      }
      badge={
        <Pill tone="brand">
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-current" />
          Live wallpaper
        </Pill>
      }
      title={video?.name ?? "Live wallpaper"}
      subtitle={video ? describeVideo(video) : ""}
      status={
        live?.paused ? (
          <>
            <Pause size={16} className="shrink-0" />
            <span className="truncate">Paused while the desktop is covered</span>
          </>
        ) : live?.running ? (
          <>
            <span className="h-2 w-2 shrink-0 animate-pulse rounded-full bg-emerald-500" />
            <span className="truncate">Playing behind your desktop</span>
          </>
        ) : (
          <>
            <LoaderCircle size={16} className="shrink-0 animate-spin" />
            <span>Starting…</span>
          </>
        )
      }
      controls={
        <>
          <Button onClick={onChange}>
            <Clapperboard size={16} />
            Change
          </Button>
          <Button variant="primary" onClick={onStop} disabled={busy}>
            <Images size={16} />
            Back to images
          </Button>
        </>
      }
    />
  );
}

/** While a video plays: what the rotation will show again once it stops. */
function RotationOnHold({ status, onBrowse }: { status: Status; onBrowse: () => void }) {
  const { current, playlistSize } = status;

  return (
    <Card className="flex items-center gap-4 p-3">
      <div className="aspect-video w-28 shrink-0 overflow-hidden rounded-lg bg-surface-muted">
        {current ? (
          <Thumbnail path={current.path} alt={current.name} className="h-full w-full" />
        ) : (
          <div className="flex h-full items-center justify-center text-ink-muted">
            <ImageIcon size={18} />
          </div>
        )}
      </div>
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium text-ink">Image rotation is on hold</p>
        <p className="mt-0.5 truncate text-xs text-ink-muted">
          {playlistSize === 0
            ? "No images in rotation yet."
            : `${playlistSize} images, every ${formatDuration(status.intervalSeconds)}. It picks up again${
                current ? ` with ${current.name}` : ""
              } when you go back to images.`}
        </p>
      </div>
      {playlistSize === 0 && (
        <Button size="sm" onClick={onBrowse}>
          Open Library
        </Button>
      )}
    </Card>
  );
}

/** While images rotate: a few videos to switch to in one click, or the way to set live wallpapers up. */
function LivePicks({
  live,
  videos,
  disabled,
  onPlay,
  onLive,
}: {
  live: LiveStatus;
  videos: LiveVideo[];
  disabled: boolean;
  onPlay: (id: string) => void;
  onLive: () => void;
}) {
  const needsAttention = !live.engine || !live.ffmpeg || live.support !== "ready";
  const picks = videos.slice(0, QUICK_PICKS);

  return (
    <section className="flex flex-col gap-3">
      <div className="flex h-8 items-center justify-between">
        <SectionTitle>Live wallpapers</SectionTitle>
        {!needsAttention && picks.length > 0 && (
          <Button size="sm" variant="ghost" onClick={onLive}>
            {videos.length > QUICK_PICKS ? `All ${videos.length}` : "Open Live"}
            <ArrowRight size={14} />
          </Button>
        )}
      </div>

      {needsAttention ? (
        <Card className="flex items-center gap-3 px-4 py-3">
          <Clapperboard size={18} className="shrink-0 text-ink-muted" />
          <p className="min-w-0 flex-1 text-sm text-ink-muted">
            {!live.engine || !live.ffmpeg
              ? "Live wallpapers need a fix before they can play."
              : live.support === "needsLogin"
                ? "Log out and back in once to finish the setup."
                : "A one-time setup is needed before videos can play."}
          </p>
          <Button size="sm" onClick={onLive}>
            Open Live
          </Button>
        </Card>
      ) : picks.length === 0 ? (
        <Card className="flex items-center gap-3 px-4 py-3">
          <Clapperboard size={18} className="shrink-0 text-ink-muted" />
          <p className="min-w-0 flex-1 text-sm text-ink-muted">
            Play a video behind your desktop. It pauses on its own when the desktop is covered.
          </p>
          <Button size="sm" variant="primary" onClick={onLive}>
            Add videos
          </Button>
        </Card>
      ) : (
        <div className="grid grid-cols-3 gap-3">
          {picks.map(video => (
            <button
              key={video.id}
              type="button"
              onClick={() => onPlay(video.id)}
              disabled={disabled}
              title={`Play ${video.name}`}
              className="group focus-ring relative cursor-pointer overflow-hidden rounded-lg border border-line text-left transition hover:border-brand disabled:cursor-not-allowed"
            >
              <Thumbnail path={video.posterPath} alt={video.name} className="aspect-video w-full" />
              <div className="pointer-events-none absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/75 to-transparent px-2.5 pt-8 pb-2">
                <p className="truncate text-xs font-medium text-white">{video.name}</p>
                <p className="truncate text-[10px] text-white/70">{describeVideo(video)}</p>
              </div>
              <span className="pointer-events-none absolute top-1/2 left-1/2 flex h-9 w-9 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full bg-brand text-on-brand opacity-0 shadow transition group-hover:opacity-100 group-focus-visible:opacity-100 group-disabled:hidden">
                <Play size={16} />
              </span>
            </button>
          ))}
        </div>
      )}
    </section>
  );
}

function Stat({ label, value, icon }: { label: string; value: string; icon?: ReactNode }) {
  return (
    <Card className="px-4 py-3">
      <p className="flex items-center gap-1.5 text-xs text-ink-muted">
        {icon}
        {label}
      </p>
      <p className="mt-1 truncate text-lg font-semibold text-ink">{value}</p>
    </Card>
  );
}
