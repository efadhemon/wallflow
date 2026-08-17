import { AlertTriangle, Film, ImageIcon, Monitor, Pause, Play, SkipBack, SkipForward, Timer } from "lucide-react";
import { useEffect, useState } from "react";
import { Thumbnail } from "../components/Thumbnail";
import { Banner, Button, Card, EmptyState } from "../components/ui";
import { api } from "../lib/api";
import { formatCountdown, formatDuration } from "../lib/utils";
import { useApp } from "../state/AppContext";

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

export function Dashboard({ onBrowse, onGallery }: { onBrowse: () => void; onGallery: () => void }) {
  const { status, environment, notify } = useApp();
  const live = status?.mode === "live";
  const now = useNow(Boolean(status?.nextChangeAt) && !status?.paused && !live);

  if (!status) return null;

  const { current, currentLive, paused, nextChangeAt, playlistSize, position, lastError } = status;
  const preview = live ? currentLive : current;

  const run = (action: () => Promise<unknown>) => () => {
    action().catch(error => notify(String(error), "error"));
  };

  if (playlistSize === 0 && !live) {
    return (
      <div className="mx-auto max-w-3xl px-8 py-10">
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
      </div>
    );
  }

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-5 px-8 py-8">
      {lastError && (
        <Banner tone="error" title="Could not change the wallpaper">
          {lastError}
        </Banner>
      )}

      <Card className="overflow-hidden">
        <div className="relative aspect-video w-full bg-surface-muted">
          {preview ? (
            <Thumbnail eager path={preview.path} alt={preview.name} className="h-full w-full" />
          ) : (
            <div className="flex h-full items-center justify-center text-ink-muted">
              {live ? <Film size={28} /> : <ImageIcon size={28} />}
            </div>
          )}

          <div className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/75 to-transparent px-5 pt-14 pb-4">
            <p className="truncate text-base font-semibold text-white">{preview?.name ?? "No wallpaper set"}</p>
            <p className="mt-0.5 truncate text-xs text-white/70">{live ? "Live wallpaper" : (preview?.folder ?? "")}</p>
          </div>

          <span className="absolute top-4 right-4 rounded-full bg-black/55 px-2.5 py-1 text-xs font-medium text-white tabular-nums">
            {live ? "Live" : `${position} / ${playlistSize}`}
          </span>
        </div>

        <div className="flex items-center justify-between gap-4 px-5 py-4">
          <div className="flex items-center gap-2 text-sm text-ink-muted">
            <Timer size={16} />
            {live ? (
              <span>{paused ? "Video paused" : "Looping"}</span>
            ) : paused ? (
              <span>Paused</span>
            ) : nextChangeAt ? (
              <span>
                Next in{" "}
                <span className="font-semibold text-ink tabular-nums">{formatCountdown(nextChangeAt - now)}</span>
              </span>
            ) : (
              <span>Waiting</span>
            )}
            {!live && (
              <span className="text-ink-muted/60">&middot; every {formatDuration(status.intervalSeconds)}</span>
            )}
          </div>

          <div className="flex items-center gap-2">
            <Button size="icon" onClick={run(api.previous)} aria-label="Previous wallpaper" title="Previous wallpaper">
              <SkipBack size={16} />
            </Button>
            <Button variant="primary" onClick={run(api.togglePause)} className="w-28">
              {paused ? <Play size={16} /> : <Pause size={16} />}
              {paused ? "Resume" : "Pause"}
            </Button>
            <Button size="icon" onClick={run(api.next)} aria-label="Next wallpaper" title="Next wallpaper">
              <SkipForward size={16} />
            </Button>
          </div>
        </div>
      </Card>

      <div className="grid grid-cols-3 gap-4">
        <Stat label="In rotation" value={playlistSize.toString()} />
        <Stat label="Found on disk" value={status.totalImages.toString()} />
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

function Stat({ label, value, icon }: { label: string; value: string; icon?: React.ReactNode }) {
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
