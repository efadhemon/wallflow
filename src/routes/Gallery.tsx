import { useVirtualizer } from "@tanstack/react-virtual";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Download, ExternalLink, Eye, Heart, ImageIcon, LoaderCircle, Maximize2, Search, Sparkles } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ImagePreview, ViewerAction } from "../components/ImagePreview";
import { Button, Card, EmptyState, Select } from "../components/ui";
import { api, onDownloadProgress } from "../lib/api";
import {
  collectionById,
  colorLabel,
  COLLECTIONS,
  COLORS,
  composeQuery,
  CUSTOM_ID,
  MIN_RESOLUTIONS,
  ORIENTATIONS,
  presetById,
  presetSort,
  SORTING,
  TOP_RANGES,
  type GallerySort,
} from "../lib/gallery";
import type {
  DownloadProgress,
  GalleryAtLeast,
  GalleryOrientation,
  GalleryPhoto,
  GallerySorting,
  GalleryTopRange,
} from "../lib/types";
import { cn, formatBytes } from "../lib/utils";
import { useApp } from "../state/AppContext";

const COL_MIN_WIDTH = 180;
const GAP = 12;
const ASPECT = 10 / 16; // height / width for 16:10 tiles

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
  const [collectionId, setCollectionId] = useState(COLLECTIONS[0].id);
  const [presetId, setPresetId] = useState<string | null>(null);
  const [queryInput, setQueryInput] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [orientation, setOrientation] = useState<GalleryOrientation>("landscape");
  const [sorting, setSorting] = useState<GallerySorting>(COLLECTIONS[0].sort.sorting);
  const [topRange, setTopRange] = useState<GalleryTopRange>(COLLECTIONS[0].sort.topRange ?? "1M");
  const [atleast, setAtLeast] = useState<GalleryAtLeast>("1920x1080");
  const [color, setColor] = useState("");
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);
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
  const [gridNode, setGridNode] = useState<HTMLDivElement | null>(null);
  const scrollParentRef = useRef<HTMLElement | null>(null);
  const activeRef = useRef(active);
  const queuedRef = useRef(queued);
  const batchRef = useRef({ added: 0, already: 0 });
  const startDownloadRef = useRef<(photo: GalleryPhoto, apply: boolean) => void>(() => {});

  const collection = collectionById(collectionId);
  const preset = presetById(collection, presetId);
  const baseQuery = preset ? preset.query : collection.query;
  const composedQuery = useMemo(() => composeQuery(baseQuery, debouncedQuery), [baseQuery, debouncedQuery]);

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
          categories: collection.categories,
          sorting: emptyQuery && sorting === "relevance" ? "toplist" : sorting,
          page: nextPage,
          atleast,
          topRange,
          colors: color || null,
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
    [atleast, collection.categories, color, composedQuery, notify, orientation, sorting, topRange],
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
    const listening = onDownloadProgress(progress => {
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
    });
    // Unsubscribes even when this unmounts before the listener is registered.
    return () => void listening.then(unlisten => unlisten());
  }, [showBatchToast]);

  // --- Column measurement ---
  const [columns, setColumns] = useState(4);
  useLayoutEffect(() => {
    if (!gridNode) return;
    const measure = () => {
      const width = gridNode.clientWidth;
      setColumns(Math.max(1, Math.floor((width + GAP) / (COL_MIN_WIDTH + GAP))));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(gridNode);
    return () => observer.disconnect();
  }, [gridNode]);

  // --- Resolve the scroll parent (<main>) once ---
  useLayoutEffect(() => {
    if (!gridNode) return;
    let el: HTMLElement | null = gridNode.parentElement;
    while (el) {
      const overflow = getComputedStyle(el).overflowY;
      if (overflow === "auto" || overflow === "scroll") {
        scrollParentRef.current = el;
        return;
      }
      el = el.parentElement;
    }
  }, [gridNode]);

  // --- Row virtualizer ---
  const rowCount = Math.ceil(photos.length / columns);
  const colWidth = gridNode ? (gridNode.clientWidth - (columns - 1) * GAP) / columns : COL_MIN_WIDTH;
  const estimatedRowHeight = colWidth * ASPECT + GAP;

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollParentRef.current,
    estimateSize: () => estimatedRowHeight,
    overscan: 3,
  });

  // --- Infinite scroll: fetch next page when the last virtual row is visible ---
  const virtualItems = virtualizer.getVirtualItems();
  const lastVirtualRow = virtualItems[virtualItems.length - 1];
  useEffect(() => {
    if (!lastVirtualRow) return;
    if (lastVirtualRow?.index >= rowCount - 1 && !loading && !loadingMore && page < lastPage) {
      void fetchPage(page + 1, true);
    }
  }, [lastVirtualRow, rowCount, loading, loadingMore, page, lastPage, fetchPage]);

  // Re-measure when column count changes
  useEffect(() => {
    virtualizer.measure();
  }, [columns, virtualizer]);

  const applySort = (sort: GallerySort) => {
    setSorting(sort.sorting);
    if (sort.topRange) setTopRange(sort.topRange);
  };

  const selectCollection = (id: string) => {
    const next = collectionById(id);
    setCollectionId(id);
    setPresetId(null);
    setQueryInput("");
    setDebouncedQuery("");
    setPreviewIndex(null);
    applySort(next.sort);
  };

  const togglePreset = (id: string) => {
    const nextId = presetId === id ? null : id;
    setPresetId(nextId);
    setPreviewIndex(null);
    applySort(presetSort(collection, presetById(collection, nextId)));
  };

  /** Wallhaven's `like:` search finds wallpapers visually and tag-wise close to one. */
  const findSimilar = (photo: GalleryPhoto) => {
    setCollectionId(CUSTOM_ID);
    setPresetId(null);
    setQueryInput(`like:${photo.id}`);
    setDebouncedQuery(`like:${photo.id}`);
    setSorting("relevance");
    setPreviewIndex(null);
  };

  // Stepping to the end of the loaded results in the preview pulls the next page in.
  useEffect(() => {
    if (previewIndex == null) return;
    if (previewIndex >= photos.length - 3 && !loading && !loadingMore && page < lastPage) {
      void fetchPage(page + 1, true);
    }
  }, [previewIndex, photos.length, loading, loadingMore, page, lastPage, fetchPage]);

  const previewPhoto = previewIndex != null ? (photos[previewIndex] ?? null) : null;

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

  const queryPending = queryInput.trim() !== debouncedQuery;
  const searching = queryPending || loading;

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-5 px-8 py-8">
      <section className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap gap-1.5">
            {COLLECTIONS.map(item => (
              <Chip
                key={item.id}
                label={item.label}
                active={collectionId === item.id}
                onClick={() => selectCollection(item.id)}
              />
            ))}
            <Chip label="Custom" active={collectionId === CUSTOM_ID} onClick={() => selectCollection(CUSTOM_ID)} />
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

        <p className="text-xs text-ink-muted">{collection.description}</p>

        {collection.presets.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {collection.presets.map(item => (
              <Chip
                key={item.id}
                label={item.label}
                small
                active={presetId === item.id}
                onClick={() => togglePreset(item.id)}
              />
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
              placeholder={
                collectionId === CUSTOM_ID
                  ? "Search Wallhaven: mountains, +tag, -tag, like:ID"
                  : `Narrow ${preset ? preset.label : collection.label}…`
              }
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
          {sorting === "toplist" && (
            <Select
              aria-label="Top range"
              value={topRange}
              onChange={event => setTopRange(event.target.value as GalleryTopRange)}
            >
              {TOP_RANGES.map(option => (
                <option key={option.id} value={option.id}>
                  {option.label}
                </option>
              ))}
            </Select>
          )}
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
          <div className="relative inline-flex items-center">
            {color && (
              <span
                aria-hidden
                className="pointer-events-none absolute left-3 h-3 w-3 rounded-full border border-line"
                style={{ backgroundColor: `#${color}` }}
              />
            )}
            <Select
              aria-label="Colour"
              value={color}
              onChange={event => setColor(event.target.value)}
              className={color ? "pl-8" : undefined}
            >
              <option value="">Any colour</option>
              {COLORS.map(option => (
                <option key={option.id} value={option.id}>
                  {option.label}
                </option>
              ))}
            </Select>
          </div>
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
            description="Try another collection, a broader search, or a different shape, size or colour."
          />
        </Card>
      ) : (
        <>
          <p className="text-xs text-ink-muted">
            {total.toLocaleString()} wallpaper{total === 1 ? "" : "s"}
          </p>
          <div ref={setGridNode} style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {virtualItems.map(virtualRow => {
              const startIdx = virtualRow.index * columns;
              const rowPhotos = photos.slice(startIdx, startIdx + columns);
              return (
                <div
                  key={virtualRow.key}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                >
                  <div className="grid gap-3" style={{ gridTemplateColumns: `repeat(${columns}, 1fr)` }}>
                    {rowPhotos.map((photo, offset) => (
                      <GalleryTile
                        key={photo.id}
                        photo={photo}
                        busy={active.has(photo.id)}
                        waiting={queued.some(job => job.photo.id === photo.id)}
                        progress={active.get(photo.id) ?? null}
                        onAdd={() => addPhoto(photo, false)}
                        onUse={() => addPhoto(photo, true)}
                        onOpen={() => void openUrl(photo.url)}
                        onPreview={() => setPreviewIndex(startIdx + offset)}
                      />
                    ))}
                  </div>
                </div>
              );
            })}
          </div>
          {loadingMore && (
            <div className="flex h-10 items-center justify-center">
              <LoaderCircle size={16} className="animate-spin text-ink-muted" />
            </div>
          )}
        </>
      )}

      {previewPhoto && previewIndex != null && (
        <GalleryPreview
          photo={previewPhoto}
          position={`${previewIndex + 1} / ${total.toLocaleString()}`}
          progress={active.get(previewPhoto.id) ?? null}
          busy={active.has(previewPhoto.id)}
          waiting={queued.some(job => job.photo.id === previewPhoto.id)}
          onPrev={previewIndex > 0 ? () => setPreviewIndex(previewIndex - 1) : undefined}
          onNext={previewIndex < photos.length - 1 ? () => setPreviewIndex(previewIndex + 1) : undefined}
          onClose={() => setPreviewIndex(null)}
          onAdd={() => addPhoto(previewPhoto, false)}
          onUse={() => addPhoto(previewPhoto, true)}
          onSimilar={() => findSimilar(previewPhoto)}
          onColor={hex => {
            setColor(hex);
            setPreviewIndex(null);
          }}
        />
      )}
    </div>
  );
}

function GalleryPreview({
  photo,
  position,
  progress,
  busy,
  waiting,
  onPrev,
  onNext,
  onClose,
  onAdd,
  onUse,
  onSimilar,
  onColor,
}: {
  photo: GalleryPhoto;
  position: string;
  progress: Pick<DownloadProgress, "received" | "total"> | null;
  busy: boolean;
  waiting: boolean;
  onPrev?: () => void;
  onNext?: () => void;
  onClose: () => void;
  onAdd: () => void;
  onUse: () => void;
  onSimilar: () => void;
  onColor: (hex: string) => void;
}) {
  const percent =
    progress?.total && progress.total > 0
      ? Math.min(100, Math.round((progress.received / progress.total) * 100))
      : null;

  return (
    <ImagePreview
      src={photo.path}
      placeholder={photo.thumbLarge}
      title={`Wallhaven ${photo.id}`}
      position={position}
      onPrev={onPrev}
      onNext={onNext}
      onClose={onClose}
      meta={
        <>
          <span>{photo.resolution}</span>
          <span>{formatBytes(photo.fileSize)}</span>
          <span className="capitalize">{photo.category}</span>
          <span className="inline-flex items-center gap-1">
            <Eye size={12} />
            {photo.views.toLocaleString()}
          </span>
          <span className="inline-flex items-center gap-1">
            <Heart size={12} />
            {photo.favorites.toLocaleString()}
          </span>
          {photo.colors.length > 0 && (
            <span className="inline-flex items-center gap-1">
              {photo.colors.map(hex => {
                const label = colorLabel(hex);
                return (
                  <button
                    key={hex}
                    type="button"
                    disabled={!label}
                    title={label ? `Find more ${label.toLowerCase()} wallpapers` : hex}
                    aria-label={label ? `Filter by ${label}` : hex}
                    onClick={() => onColor(hex.replace("#", "").toLowerCase())}
                    className="focus-ring h-3.5 w-3.5 cursor-pointer rounded-full border border-white/40 disabled:cursor-default"
                    style={{ backgroundColor: hex }}
                  />
                );
              })}
            </span>
          )}
        </>
      }
      actions={
        busy || waiting ? (
          <span className="inline-flex h-9 items-center gap-2 rounded-lg bg-white/12 px-4 text-sm">
            <LoaderCircle size={14} className="animate-spin" />
            {waiting ? "Waiting…" : percent != null ? `Downloading ${percent}%` : "Downloading…"}
          </span>
        ) : (
          <>
            <ViewerAction onClick={onSimilar}>
              <Sparkles size={14} />
              Find similar
            </ViewerAction>
            <ViewerAction onClick={() => void openUrl(photo.url)}>
              <ExternalLink size={14} />
              Wallhaven
            </ViewerAction>
            <ViewerAction onClick={onAdd}>
              <Download size={14} />
              Add to library
            </ViewerAction>
            <ViewerAction primary onClick={onUse}>
              Use now
            </ViewerAction>
          </>
        )
      }
    />
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
    message:
      percent != null ? `Downloading ${jobs.length} wallpapers (${percent}%)` : `Downloading ${jobs.length} wallpapers`,
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
  onPreview,
}: {
  photo: GalleryPhoto;
  busy: boolean;
  waiting: boolean;
  progress: Pick<DownloadProgress, "received" | "total"> | null;
  onAdd: () => void;
  onUse: () => void;
  onOpen: () => void;
  onPreview: () => void;
}) {
  const percent =
    progress?.total && progress.total > 0
      ? Math.min(100, Math.round((progress.received / progress.total) * 100))
      : null;
  const progressLabel = percent != null ? `${percent}%` : progress ? formatBytes(progress.received) : "Downloading…";

  return (
    <div className="group relative overflow-hidden rounded-lg border border-line">
      <button
        type="button"
        onClick={onPreview}
        title="Preview full screen"
        aria-label={`Preview wallpaper ${photo.id}`}
        className="focus-ring block aspect-16/10 w-full cursor-zoom-in"
        style={{ backgroundColor: photo.color ?? undefined }}
      >
        <img src={photo.thumb} alt="" loading="lazy" draggable={false} className="h-full w-full object-cover" />
      </button>

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
          title="Preview full screen"
          aria-label="Preview full screen"
          onClick={onPreview}
          className="focus-ring flex h-6 w-6 cursor-pointer items-center justify-center rounded-md bg-black/55 text-white"
        >
          <Maximize2 size={12} />
        </button>
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
