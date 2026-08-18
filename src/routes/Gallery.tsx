import { openUrl } from "@tauri-apps/plugin-opener";
import { Download, ExternalLink, ImageIcon, LoaderCircle, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button, Card, EmptyState, Select } from "../components/ui";
import { api, onDownloadProgress } from "../lib/api";
import { composeQuery, CUSTOM_ID, MIN_RESOLUTIONS, ORIENTATIONS, PARENTS, parentById, SORTING } from "../lib/gallery";
import type { DownloadProgress, GalleryAtLeast, GalleryOrientation, GalleryPhoto, GallerySorting } from "../lib/types";
import { cn, formatBytes } from "../lib/utils";
import { useApp } from "../state/AppContext";

const SEARCH_DEBOUNCE_MS = 400;
const DOWNLOAD_TOAST = "gallery-download";
const MAX_PARALLEL = 3;

interface ActiveJob {
  id: string;
  url: string;
  received: number;
  total: number | null;
}

interface QueuedJob {
  photo: GalleryPhoto;
  apply: boolean;
}

export function Gallery() {
  const { notify, upsertToast } = useApp();
  const [parentId, setParentId] = useState(PARENTS[0].id);
  const [queryInput, setQueryInput] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [orientation, setOrientation] = useState<GalleryOrientation>("landscape");
  const [sorting, setSorting] = useState<GallerySorting>("relevance");
  const [atleast, setAtLeast] = useState<GalleryAtLeast>("1920x1080");
  const [photos, setPhotos] = useState<GalleryPhoto[]>([]);
  const [page, setPage] = useState(1);
  const [lastPage, setLastPage] = useState(1);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState<Map<string, ActiveJob>>(() => new Map());
  const [queued, setQueued] = useState<QueuedJob[]>([]);
  const requestId = useRef(0);
  const sentinel = useRef<HTMLDivElement>(null);
  const activeRef = useRef(active);
  const queuedRef = useRef(queued);
  const batchRef = useRef({ added: 0, already: 0 });
  const startDownloadRef = useRef<(photo: GalleryPhoto, apply: boolean) => void>(() => {});

  const parent = parentById(parentId);
  const composedQuery = useMemo(() => composeQuery(parent.keyword, debouncedQuery), [parent.keyword, debouncedQuery]);

  useEffect(() => {
    const handle = window.setTimeout(() => setDebouncedQuery(queryInput.trim()), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [queryInput]);

  const fetchPage = useCallback(
    async (nextPage: number, append: boolean) => {
      const id = ++requestId.current;
      if (append) setLoadingMore(true);
      else setLoading(true);
      setError(null);

      try {
        const emptyQuery = composedQuery.length === 0;
        const result = await api.searchGallery({
          query: composedQuery || null,
          ratios: orientation,
          categories: parent.categories,
          sorting: emptyQuery && sorting === "relevance" ? "toplist" : sorting,
          page: nextPage,
          atleast,
        });
        if (id !== requestId.current) return;

        setPhotos(current => (append ? [...current, ...result.photos] : result.photos));
        setPage(result.page);
        setLastPage(result.lastPage);
        setTotal(result.total);
      } catch (err) {
        if (id !== requestId.current) return;
        const message = String(err);
        setError(message);
        if (!append) setPhotos([]);
        notify(message, "error");
      } finally {
        if (id === requestId.current) {
          setLoading(false);
          setLoadingMore(false);
        }
      }
    },
    [atleast, composedQuery, notify, orientation, parent.categories, sorting],
  );

  useEffect(() => {
    void fetchPage(1, false);
  }, [fetchPage]);

  const showBatchToast = useCallback(
    (jobs: Map<string, ActiveJob>) => {
      const summary = batchProgressMessage([...jobs.values()]);
      upsertToast(DOWNLOAD_TOAST, { ...summary, sticky: true, tone: "info" });
    },
    [upsertToast],
  );

  const finishJob = useCallback(
    (id: string, error?: string) => {
      const next = new Map(activeRef.current);
      next.delete(id);
      activeRef.current = next;
      setActive(next);

      const waiting = queuedRef.current;
      if (waiting.length > 0 && next.size < MAX_PARALLEL) {
        const [item, ...rest] = waiting;
        queuedRef.current = rest;
        setQueued(rest);
        if (next.size > 0) showBatchToast(next);
        if (error) notify(error, "error");
        startDownloadRef.current(item.photo, item.apply);
        return;
      }

      if (next.size > 0) {
        showBatchToast(next);
        if (error) notify(error, "error");
        return;
      }

      const { added, already } = batchRef.current;
      if (added > 0 || already > 0) {
        upsertToast(DOWNLOAD_TOAST, {
          message: finishMessage(added, already),
          progress: 100,
          sticky: false,
          tone: "info",
        });
        if (error) notify(error, "error");
      } else if (error) {
        upsertToast(DOWNLOAD_TOAST, { message: error, tone: "error", sticky: false });
      }
    },
    [notify, showBatchToast, upsertToast],
  );

  const startDownload = useCallback(
    (photo: GalleryPhoto, apply: boolean) => {
      const job: ActiveJob = {
        id: photo.id,
        url: photo.path,
        received: 0,
        total: photo.fileSize || null,
      };
      const next = new Map(activeRef.current);
      next.set(photo.id, job);
      activeRef.current = next;
      setActive(next);
      showBatchToast(next);

      void (async () => {
        try {
          const result = await api.addRemoteImage(photo.path, photo.fileSize || undefined);
          if (result.added) batchRef.current.added += 1;
          else batchRef.current.already += 1;
          if (apply) await api.applyImage(result.path);
          finishJob(photo.id);
        } catch (err) {
          finishJob(photo.id, String(err));
        }
      })();
    },
    [finishJob, showBatchToast],
  );

  useEffect(() => {
    startDownloadRef.current = startDownload;
  }, [startDownload]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onDownloadProgress(progress => {
      const current = activeRef.current;
      let found = false;
      const next = new Map(current);
      for (const [id, job] of next) {
        if (job.url !== progress.url) continue;
        next.set(id, { ...job, received: progress.received, total: progress.total ?? job.total });
        found = true;
        break;
      }
      if (!found) return;
      activeRef.current = next;
      setActive(next);
      showBatchToast(next);
    }).then(fn => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [showBatchToast]);

  useEffect(() => {
    const node = sentinel.current;
    if (!node || loading || loadingMore || page >= lastPage) return;

    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) {
        void fetchPage(page + 1, true);
      }
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [fetchPage, lastPage, loading, loadingMore, page, photos.length]);

  const selectParent = (id: string) => {
    setParentId(id);
    setQueryInput("");
    setDebouncedQuery("");
    setSorting(parentById(id).keyword ? "relevance" : "toplist");
  };

  const toggleSub = (sub: string) => {
    setQueryInput(current => (current.trim() === sub ? "" : sub));
  };

  const addPhoto = (photo: GalleryPhoto, apply: boolean) => {
    if (activeRef.current.has(photo.id) || queuedRef.current.some(job => job.photo.id === photo.id)) return;

    if (activeRef.current.size === 0 && queuedRef.current.length === 0) {
      batchRef.current = { added: 0, already: 0 };
    }

    if (activeRef.current.size >= MAX_PARALLEL) {
      const nextQueue = [...queuedRef.current, { photo, apply }];
      queuedRef.current = nextQueue;
      setQueued(nextQueue);
      return;
    }

    startDownload(photo, apply);
  };

  const activeSub = queryInput.trim();
  const queryPending = activeSub !== debouncedQuery;
  const searching = queryPending || loading;

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-5 px-8 py-8">
      <section className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap gap-1.5">
            {PARENTS.map(item => (
              <Chip
                key={item.id}
                label={item.label}
                active={parentId === item.id}
                onClick={() => selectParent(item.id)}
              />
            ))}
            <Chip label="Custom" active={parentId === CUSTOM_ID} onClick={() => selectParent(CUSTOM_ID)} />
          </div>
          <button
            type="button"
            className="focus-ring inline-flex items-center gap-1.5 text-xs text-ink-muted hover:text-ink"
            onClick={() => void openUrl("https://wallhaven.cc")}
          >
            Photos from Wallhaven
            <ExternalLink size={12} />
          </button>
        </div>

        {parent.subs.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {parent.subs.map(sub => (
              <Chip key={sub} label={sub} small active={activeSub === sub} onClick={() => toggleSub(sub)} />
            ))}
          </div>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <label className="relative min-w-48 flex-1">
            <Search size={14} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-ink-muted" />
            <input
              type="search"
              value={queryInput}
              onChange={event => setQueryInput(event.target.value)}
              placeholder={parentId === CUSTOM_ID ? "Search Wallhaven" : `Search in ${parent.label.toLowerCase()}`}
              aria-busy={searching}
              className={cn(
                "focus-ring h-9.5 w-full rounded-lg border border-line bg-surface pl-9 text-sm text-ink outline-none placeholder:text-ink-muted",
                searching ? "pr-9" : "pr-3",
              )}
            />
            {searching && (
              <LoaderCircle
                size={14}
                className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 animate-spin text-ink-muted"
              />
            )}
          </label>
          <Select
            aria-label="Orientation"
            value={orientation}
            onChange={event => setOrientation(event.target.value as GalleryOrientation)}
          >
            {ORIENTATIONS.map(option => (
              <option key={option.id} value={option.id}>
                {option.label}
              </option>
            ))}
          </Select>
          <Select
            aria-label="Sort"
            value={sorting}
            onChange={event => setSorting(event.target.value as GallerySorting)}
          >
            {SORTING.map(option => (
              <option key={option.id} value={option.id}>
                {option.label}
              </option>
            ))}
          </Select>
          <Select
            aria-label="Minimum resolution"
            value={atleast}
            onChange={event => setAtLeast(event.target.value as GalleryAtLeast)}
          >
            {MIN_RESOLUTIONS.map(option => (
              <option key={option.id} value={option.id}>
                {option.label}
              </option>
            ))}
          </Select>
        </div>
      </section>

      {error && photos.length === 0 && !loading ? (
        <Card>
          <EmptyState
            icon={<ImageIcon size={24} />}
            title="Could not load the gallery"
            description={error}
            action={
              <Button variant="primary" onClick={() => void fetchPage(1, false)}>
                Try again
              </Button>
            }
          />
        </Card>
      ) : loading ? (
        <div
          className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-3"
          aria-busy="true"
          aria-label="Loading gallery"
        >
          {Array.from({ length: 8 }, (_, index) => (
            <div key={index} className="aspect-16/10 animate-pulse rounded-lg bg-surface-muted" />
          ))}
        </div>
      ) : photos.length === 0 ? (
        <Card>
          <EmptyState
            icon={<ImageIcon size={24} />}
            title="No wallpapers match"
            description="Try another parent, subquery, or orientation."
          />
        </Card>
      ) : (
        <>
          <p className="text-xs text-ink-muted">
            {total.toLocaleString()} wallpaper{total === 1 ? "" : "s"}
          </p>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-3">
            {photos.map(photo => (
              <GalleryTile
                key={photo.id}
                photo={photo}
                busy={active.has(photo.id)}
                waiting={queued.some(job => job.photo.id === photo.id)}
                progress={active.get(photo.id) ?? null}
                onAdd={() => addPhoto(photo, false)}
                onUse={() => addPhoto(photo, true)}
                onOpen={() => void openUrl(photo.url)}
              />
            ))}
          </div>
          <div ref={sentinel} className="flex h-10 items-center justify-center">
            {loadingMore && <LoaderCircle size={16} className="animate-spin text-ink-muted" />}
          </div>
        </>
      )}
    </div>
  );
}

function batchProgressMessage(jobs: ActiveJob[]): { message: string; progress: number | null } {
  if (jobs.length === 0) return { message: "Downloading…", progress: 0 };

  const received = jobs.reduce((sum, job) => sum + job.received, 0);
  const allHaveTotal = jobs.every(job => job.total && job.total > 0);
  const total = allHaveTotal ? jobs.reduce((sum, job) => sum + (job.total ?? 0), 0) : 0;
  const percent = allHaveTotal && total > 0 ? Math.min(100, Math.round((received / total) * 100)) : null;

  if (jobs.length === 1) {
    const job = jobs[0];
    if (percent != null && job.total) {
      return {
        message: `Downloading ${formatBytes(job.received)} / ${formatBytes(job.total)} (${percent}%)`,
        progress: percent,
      };
    }
    return { message: `Downloading ${formatBytes(job.received)}`, progress: percent };
  }

  return {
    message: percent != null ? `Downloading ${jobs.length} wallpapers (${percent}%)` : `Downloading ${jobs.length} wallpapers`,
    progress: percent,
  };
}

function finishMessage(added: number, already: number): string {
  if (added > 0 && already > 0) {
    const addedLabel = added === 1 ? "Added 1 to the library" : `Added ${added} to the library`;
    const alreadyLabel = already === 1 ? "1 was already there" : `${already} were already there`;
    return `${addedLabel}. ${alreadyLabel}.`;
  }
  if (added === 1) return "Added to the library.";
  if (added > 1) return `Added ${added} to the library.`;
  if (already === 1) return "That image is already in the library.";
  return `${already} images are already in the library.`;
}

function Chip({
  label,
  active,
  small,
  onClick,
}: {
  label: string;
  active: boolean;
  small?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className={cn(
        "focus-ring cursor-pointer rounded-full font-medium capitalize transition",
        small ? "px-2.5 py-1 text-[11px]" : "px-3 py-1.5 text-xs",
        active ? "bg-brand text-on-brand" : "bg-surface-muted text-ink-muted hover:bg-line hover:text-ink",
      )}
    >
      {label}
    </button>
  );
}

function GalleryTile({
  photo,
  busy,
  waiting,
  progress,
  onAdd,
  onUse,
  onOpen,
}: {
  photo: GalleryPhoto;
  busy: boolean;
  waiting: boolean;
  progress: Pick<DownloadProgress, "received" | "total"> | null;
  onAdd: () => void;
  onUse: () => void;
  onOpen: () => void;
}) {
  const percent =
    progress?.total && progress.total > 0
      ? Math.min(100, Math.round((progress.received / progress.total) * 100))
      : null;
  const progressLabel =
    percent != null ? `${percent}%` : progress ? formatBytes(progress.received) : "Downloading…";

  return (
    <div className="group relative overflow-hidden rounded-lg border border-line">
      <div className="aspect-16/10 w-full" style={{ backgroundColor: photo.color ?? undefined }}>
        <img src={photo.thumb} alt="" loading="lazy" draggable={false} className="h-full w-full object-cover" />
      </div>

      <div className="pointer-events-none absolute inset-x-0 bottom-0 bg-linear-to-t from-black/75 to-transparent px-2 pt-8 pb-1.5">
        <p className="truncate text-[11px] text-white">
          {photo.resolution} · {formatBytes(photo.fileSize)}
        </p>
      </div>

      {(busy || waiting) && (
        <div
          className="absolute inset-0 flex flex-col items-center justify-center bg-black/55 px-4"
          role={busy ? "progressbar" : "status"}
          aria-valuemin={busy ? 0 : undefined}
          aria-valuemax={busy ? 100 : undefined}
          aria-valuenow={busy ? (percent ?? undefined) : undefined}
          aria-label={waiting ? "Waiting to download" : "Download progress"}
        >
          {waiting ? (
            <p className="text-[11px] text-white">Waiting…</p>
          ) : (
            <>
              <div className="h-1 w-full overflow-hidden rounded-full bg-white/25">
                <div
                  className={cn(
                    "h-full rounded-full bg-white transition-[width] duration-150",
                    percent == null && "animate-pulse",
                  )}
                  style={{ width: `${percent ?? 40}%` }}
                />
              </div>
              <p className="mt-1.5 text-[11px] text-white">{progressLabel}</p>
            </>
          )}
        </div>
      )}

      <div className="absolute top-2 right-2 flex gap-1 opacity-0 transition group-hover:opacity-100">
        <button
          type="button"
          title="Open on Wallhaven"
          aria-label="Open on Wallhaven"
          onClick={onOpen}
          className="focus-ring flex h-6 w-6 cursor-pointer items-center justify-center rounded-md bg-black/55 text-white"
        >
          <ExternalLink size={12} />
        </button>
      </div>

      {!busy && !waiting && (
        <div className="absolute inset-x-2 bottom-7 flex gap-1 opacity-0 transition group-hover:opacity-100">
          <Button size="sm" className="h-7 flex-1 px-2 text-[11px]" onClick={onAdd}>
            <Download size={12} />
            Add
          </Button>
          <Button size="sm" variant="primary" className="h-7 flex-1 px-2 text-[11px]" onClick={onUse}>
            Use now
          </Button>
        </div>
      )}
    </div>
  );
}
