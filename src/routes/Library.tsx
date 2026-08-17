import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  Check,
  EyeOff,
  FolderOpen,
  FolderPlus,
  ImageIcon,
  ImagePlus,
  Images,
  RefreshCw,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Thumbnail } from "../components/Thumbnail";
import { Button, Card, Divider, EmptyState, SectionTitle, Switch } from "../components/ui";
import { api } from "../lib/api";
import type { FolderEntry, ImageEntry } from "../lib/types";
import { cn } from "../lib/utils";
import { useApp } from "../state/AppContext";

/** Tiles rendered per chunk; more are appended as the grid is scrolled. */
const PAGE_SIZE = 120;

export function Library() {
  const { status, setConfig, notify } = useApp();
  const [folders, setFolders] = useState<FolderEntry[]>([]);
  const [images, setImages] = useState<ImageEntry[]>([]);
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const [busy, setBusy] = useState(false);
  const [managedOnly, setManagedOnly] = useState(false);
  const sentinel = useRef<HTMLDivElement>(null);

  const reload = useCallback(async () => {
    try {
      const [nextFolders, nextImages] = await Promise.all([api.listFolders(), api.listImages()]);
      setFolders(nextFolders);
      setImages(nextImages);
    } catch (error) {
      notify(String(error), "error");
    }
  }, [notify]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // The scheduler republishes its status after every rescan, which is the cue
  // that the folder contents on disk may have changed.
  useEffect(() => {
    void reload();
  }, [status?.totalImages, status?.playlistSize, reload]);

  const addedCount = images.filter(image => image.managed).length;
  const gallery = useMemo(() => (managedOnly ? images.filter(image => image.managed) : images), [images, managedOnly]);

  useEffect(() => {
    const node = sentinel.current;
    if (!node) return;

    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) {
        setVisibleCount(count => Math.min(count + PAGE_SIZE, gallery.length));
      }
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [gallery.length]);

  const addFolder = async () => {
    const selected = await open({ directory: true, multiple: false, title: "Choose a wallpaper folder" });
    if (typeof selected !== "string") return;

    setBusy(true);
    try {
      setConfig(await api.addFolder(selected, true));
      await reload();
    } catch (error) {
      notify(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const addImages = async () => {
    const selected = await open({
      multiple: true,
      title: "Choose images",
      filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png", "webp", "bmp"] }],
    });
    if (selected == null) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    if (paths.length === 0) return;

    setBusy(true);
    try {
      const added = await api.addImages(paths);
      if (added === 0) {
        notify("Those images are already in the library.");
      } else {
        notify(added === 1 ? "Added 1 image." : `Added ${added} images.`);
      }
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
      await reload();
    } catch (error) {
      notify(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const visible = gallery.slice(0, visibleCount);

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-6 px-8 py-8">
      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <SectionTitle>Folders</SectionTitle>
          <div className="flex items-center gap-2">
            <Button size="sm" onClick={() => void addImages()} disabled={busy}>
              <ImagePlus size={14} />
              Add images
            </Button>
            <Button size="sm" variant="primary" onClick={() => void addFolder()} disabled={busy}>
              <FolderPlus size={14} />
              Add folder
            </Button>
          </div>
        </div>

        <Card>
          {folders.length === 0 && addedCount === 0 ? (
            <EmptyState
              icon={<FolderOpen size={24} />}
              title="Nothing here yet"
              description="Pick a folder of images, or add individual files. Everything you add joins the rotation."
            />
          ) : (
            <>
              {folders.map((folder, index) => (
                <div key={folder.path}>
                  {index > 0 && <Divider />}
                  <div className="flex items-center gap-4 px-4 py-3">
                    <div className="min-w-0 flex-1">
                      <p className="flex items-center gap-1.5 truncate text-sm font-medium text-ink">
                        {!folder.exists && <TriangleAlert size={14} className="shrink-0 text-danger" />}
                        {folder.path}
                      </p>
                      <p className="mt-0.5 text-xs text-ink-muted">
                        {folder.exists
                          ? `${folder.imageCount} image${folder.imageCount === 1 ? "" : "s"}`
                          : "Folder is missing"}
                      </p>
                    </div>

                    <label className="flex shrink-0 items-center gap-2 text-xs text-ink-muted">
                      Subfolders
                      <Switch
                        label={`Include subfolders of ${folder.path}`}
                        checked={folder.recursive}
                        disabled={busy}
                        onChange={recursive =>
                          runAndReload(() => api.setFolderRecursive(folder.path, recursive).then(setConfig))
                        }
                      />
                    </label>

                    <Button
                      size="icon"
                      variant="ghost"
                      disabled={busy}
                      aria-label={`Remove ${folder.path}`}
                      title="Remove folder"
                      onClick={() => runAndReload(() => api.removeFolder(folder.path).then(setConfig))}
                    >
                      <Trash2 size={16} />
                    </Button>
                  </div>
                </div>
              ))}

              {folders.length > 0 && <Divider />}
              <div className="flex items-center gap-4 px-4 py-3">
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-ink">Added images</p>
                  <p className="mt-0.5 text-xs text-ink-muted">
                    {addedCount} image{addedCount === 1 ? "" : "s"}
                  </p>
                </div>

                <Button
                  size="icon"
                  variant={managedOnly ? "primary" : "ghost"}
                  aria-pressed={managedOnly}
                  aria-label={managedOnly ? "Show all images" : "Show added images only"}
                  title={managedOnly ? "Show all images" : "Show added images only"}
                  onClick={() => {
                    setManagedOnly(current => !current);
                    setVisibleCount(PAGE_SIZE);
                  }}
                >
                  <Images size={16} />
                </Button>
              </div>
            </>
          )}
        </Card>
      </section>

      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <SectionTitle>Images {gallery.length > 0 && `(${gallery.length})`}</SectionTitle>
          <Button size="sm" variant="ghost" onClick={() => void reload()} disabled={busy}>
            <RefreshCw size={14} />
            Rescan
          </Button>
        </div>

        {gallery.length === 0 ? (
          <Card>
            <EmptyState
              icon={<ImageIcon size={24} />}
              title={managedOnly ? "No added images" : "Nothing to show"}
              description={
                managedOnly
                  ? "Add individual images to see them here."
                  : "Add a folder of JPG, PNG, WebP or BMP files, or add individual images."
              }
            />
          </Card>
        ) : (
          <>
            <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3">
              {visible.map(image => (
                <ImageTile
                  key={image.path}
                  image={image}
                  disabled={busy}
                  onApply={() => runAndReload(() => api.applyImage(image.path))}
                  onToggleExclude={() => runAndReload(() => api.toggleExclude(image.path))}
                  onRemove={image.managed ? () => runAndReload(() => api.removeImage(image.path)) : undefined}
                  onReveal={() => void revealItemInDir(image.path)}
                />
              ))}
            </div>
            <div ref={sentinel} className="h-4" />
          </>
        )}
      </section>
    </div>
  );
}

function ImageTile({
  image,
  disabled,
  onApply,
  onToggleExclude,
  onRemove,
  onReveal,
}: {
  image: ImageEntry;
  disabled: boolean;
  onApply: () => void;
  onToggleExclude: () => void;
  onRemove?: () => void;
  onReveal: () => void;
}) {
  return (
    <div
      className={cn(
        "group relative overflow-hidden rounded-lg border transition",
        image.isCurrent ? "border-brand ring-2 ring-brand/30" : "border-line",
      )}
    >
      <button
        type="button"
        disabled={disabled}
        onClick={onApply}
        onDoubleClick={onReveal}
        title={`${image.name}\nClick to apply now`}
        className="focus-ring block w-full cursor-pointer"
      >
        <Thumbnail
          path={image.path}
          alt={image.name}
          className={cn("aspect-[4/3] w-full", image.excluded && "opacity-35 grayscale")}
        />
      </button>

      <div className="pointer-events-none absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/70 to-transparent px-2 pt-6 pb-1.5">
        <p className="truncate text-[11px] text-white">{image.name}</p>
      </div>

      {image.isCurrent && (
        <span className="absolute top-2 left-2 flex h-5 w-5 items-center justify-center rounded-full bg-brand text-on-brand">
          <Check size={12} />
        </span>
      )}

      <div className="absolute top-2 right-2 flex gap-1">
        {onRemove && (
          <button
            type="button"
            disabled={disabled}
            onClick={onRemove}
            title="Remove added image"
            aria-label={`Remove ${image.name}`}
            className="focus-ring flex h-6 w-6 cursor-pointer items-center justify-center rounded-md bg-black/55 text-white opacity-0 transition group-hover:opacity-100"
          >
            <Trash2 size={13} />
          </button>
        )}
        <button
          type="button"
          disabled={disabled}
          onClick={onToggleExclude}
          title={image.excluded ? "Include in rotation" : "Exclude from rotation"}
          aria-label={image.excluded ? "Include in rotation" : "Exclude from rotation"}
          className={cn(
            "focus-ring flex h-6 w-6 cursor-pointer items-center justify-center rounded-md bg-black/55 text-white transition group-hover:opacity-100",
            image.excluded ? "opacity-100" : "opacity-0",
          )}
        >
          <EyeOff size={13} />
        </button>
      </div>
    </div>
  );
}
