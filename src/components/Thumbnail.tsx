import { convertFileSrc } from "@tauri-apps/api/core";
import { ImageOff } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { cn } from "../lib/utils";

/**
 * Thumbnails are generated on demand by the Rust side and served from the app
 * cache. Generation only starts once the tile is close to the viewport so opening
 * a folder of a thousand images does not decode a thousand files.
 */
export function Thumbnail({
  path,
  alt,
  className,
  eager = false,
}: {
  path: string;
  alt: string;
  className?: string;
  eager?: boolean;
}) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [visible, setVisible] = useState(eager);
  const holder = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setSrc(null);
    setFailed(false);
    setVisible(eager);
  }, [path, eager]);

  useEffect(() => {
    if (visible || !holder.current) return;

    const observer = new IntersectionObserver(
      entries => {
        if (entries.some(entry => entry.isIntersecting)) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: "300px" },
    );
    observer.observe(holder.current);
    return () => observer.disconnect();
  }, [visible]);

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;

    api
      .getThumbnail(path)
      .then(file => {
        if (!cancelled) setSrc(convertFileSrc(file));
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });

    return () => {
      cancelled = true;
    };
  }, [path, visible]);

  return (
    <div
      ref={holder}
      className={cn("relative overflow-hidden bg-surface-muted", !src && !failed && "animate-pulse", className)}
    >
      {src && (
        <img
          src={src}
          alt={alt}
          draggable={false}
          className="h-full w-full object-cover"
          onError={() => setFailed(true)}
        />
      )}
      {failed && (
        <div className="flex h-full w-full items-center justify-center text-ink-muted">
          <ImageOff size={20} />
        </div>
      )}
    </div>
  );
}
