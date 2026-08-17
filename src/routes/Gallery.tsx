import { openUrl } from "@tauri-apps/plugin-opener";
import { Download, ExternalLink, ImageIcon, LoaderCircle, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button, Card, EmptyState, Select } from "../components/ui";
import { api } from "../lib/api";
import { composeQuery, CUSTOM_ID, MIN_RESOLUTIONS, ORIENTATIONS, PARENTS, parentById, SORTING } from "../lib/gallery";
import type { GalleryAtLeast, GalleryOrientation, GalleryPhoto, GallerySorting } from "../lib/types";
import { cn, formatBytes } from "../lib/utils";
import { useApp } from "../state/AppContext";

const SEARCH_DEBOUNCE_MS = 400;

export function Gallery() {
  const { notify } = useApp();
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
  const [busyId, setBusyId] = useState<string | null>(null);
  const requestId = useRef(0);
  const sentinel = useRef<HTMLDivElement>(null);

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

  const addPhoto = async (photo: GalleryPhoto, apply: boolean) => {
    setBusyId(photo.id);
    try {
      const result = await api.addRemoteImage(photo.path);
      if (result.added) {
        notify("Added to the library.");
      } else {
        notify("That image is already in the library.");
      }
      if (apply) {
        await api.applyImage(result.path);
      }
    } catch (err) {
      notify(String(err), "error");
    } finally {
      setBusyId(null);
    }
  };

  const activeSub = queryInput.trim();

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
              className="focus-ring h-9.5 w-full rounded-lg border border-line bg-surface pr-3 pl-9 text-sm text-ink outline-none placeholder:text-ink-muted"
            />
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
      ) : loading && photos.length === 0 ? (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-3">
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
                busy={busyId === photo.id}
                disabled={busyId !== null}
                onAdd={() => void addPhoto(photo, false)}
                onUse={() => void addPhoto(photo, true)}
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
  disabled,
  onAdd,
  onUse,
  onOpen,
}: {
  photo: GalleryPhoto;
  busy: boolean;
  disabled: boolean;
  onAdd: () => void;
  onUse: () => void;
  onOpen: () => void;
}) {
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

      <div className="absolute inset-x-2 bottom-7 flex gap-1 opacity-0 transition group-hover:opacity-100">
        <Button size="sm" className="h-7 flex-1 px-2 text-[11px]" disabled={disabled} onClick={onAdd}>
          {busy ? <LoaderCircle size={12} className="animate-spin" /> : <Download size={12} />}
          Add
        </Button>
        <Button size="sm" variant="primary" className="h-7 flex-1 px-2 text-[11px]" disabled={disabled} onClick={onUse}>
          Use now
        </Button>
      </div>
    </div>
  );
}
