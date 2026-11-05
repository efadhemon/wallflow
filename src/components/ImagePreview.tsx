import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChevronLeft, ChevronRight, LoaderCircle, Maximize, Minimize, Scan, ScanEye, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cn } from "../lib/utils";

/**
 * Full-window image viewer shared by the gallery and the library. It shows the
 * low-res placeholder straight away and swaps in the sharp image once decoded,
 * so stepping through results never sits on a blank screen.
 *
 * Keys: Esc closes, ←/→ step, F toggles the window's full screen, W toggles
 * "as wallpaper" (cover) against "whole image" (contain).
 */
export function ImagePreview({
  src,
  placeholder,
  title,
  meta,
  actions,
  position,
  onPrev,
  onNext,
  onClose,
}: {
  /** Sharp image. Null while it is still being prepared. */
  src: string | null;
  placeholder?: string | null;
  title: string;
  meta?: ReactNode;
  actions?: ReactNode;
  /** e.g. "3 / 120"; hidden when absent. */
  position?: string;
  onPrev?: () => void;
  onNext?: () => void;
  onClose: () => void;
}) {
  const [loaded, setLoaded] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [cover, setCover] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const enteredFullscreen = useRef(false);

  useEffect(() => {
    setFailed(false);
  }, [src]);

  const toggleFullscreen = useCallback(async () => {
    try {
      const window = getCurrentWindow();
      const next = !(await window.isFullscreen());
      await window.setFullscreen(next);
      enteredFullscreen.current = next;
      setFullscreen(next);
    } catch {
      // Full screen is a nicety; the in-window viewer still works without it.
    }
  }, []);

  const close = useCallback(() => {
    if (enteredFullscreen.current) {
      enteredFullscreen.current = false;
      void getCurrentWindow()
        .setFullscreen(false)
        .catch(() => {});
    }
    onClose();
  }, [onClose]);

  // Leaving the screen any other way (route change) must not strand the window in full screen.
  useEffect(
    () => () => {
      if (enteredFullscreen.current)
        void getCurrentWindow()
          .setFullscreen(false)
          .catch(() => {});
    },
    [],
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return;
      if (event.key === "Escape") {
        event.preventDefault();
        close();
      } else if (event.key === "ArrowLeft" && onPrev) {
        event.preventDefault();
        onPrev();
      } else if (event.key === "ArrowRight" && onNext) {
        event.preventDefault();
        onNext();
      } else if (event.key === "f" || event.key === "F") {
        void toggleFullscreen();
      } else if (event.key === "w" || event.key === "W") {
        setCover(current => !current);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close, onNext, onPrev, toggleFullscreen]);

  const sharpReady = src != null && loaded === src && !failed;
  const fit = cover ? "object-cover" : "object-contain";

  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-label={`Preview of ${title}`}
      className="fixed inset-0 z-[60] flex flex-col bg-black/95 text-white select-none"
    >
      <div className="absolute inset-0" onClick={close} aria-hidden />

      <div className="pointer-events-none absolute inset-0">
        {placeholder && !sharpReady && (
          <img
            src={placeholder}
            alt=""
            draggable={false}
            className={cn("h-full w-full scale-[1.01] blur-[2px]", fit)}
          />
        )}
        {src && !failed && (
          <img
            key={src}
            src={src}
            alt={title}
            draggable={false}
            referrerPolicy="no-referrer"
            onLoad={() => setLoaded(src)}
            onError={() => setFailed(true)}
            className={cn(
              "absolute inset-0 h-full w-full transition-opacity duration-200",
              fit,
              sharpReady ? "opacity-100" : "opacity-0",
            )}
          />
        )}
      </div>

      {!sharpReady && !failed && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
          <span className="flex items-center gap-2 rounded-full bg-black/60 px-3 py-1.5 text-xs">
            <LoaderCircle size={14} className="animate-spin" />
            Loading full image…
          </span>
        </div>
      )}
      {failed && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
          <span className="rounded-full bg-black/60 px-3 py-1.5 text-xs">Could not load the full image</span>
        </div>
      )}

      <header className="relative flex items-start gap-3 bg-linear-to-b from-black/80 to-transparent px-5 pt-4 pb-10">
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium">{title}</p>
          {meta && <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-white/70">{meta}</div>}
        </div>
        {position && <span className="pt-0.5 text-xs text-white/60 tabular-nums">{position}</span>}
        <ViewerButton
          label={cover ? "Show whole image (W)" : "Show as wallpaper (W)"}
          onClick={() => setCover(current => !current)}
          active={cover}
        >
          {cover ? <ScanEye size={16} /> : <Scan size={16} />}
        </ViewerButton>
        <ViewerButton
          label={fullscreen ? "Exit full screen (F)" : "Full screen (F)"}
          onClick={() => void toggleFullscreen()}
        >
          {fullscreen ? <Minimize size={16} /> : <Maximize size={16} />}
        </ViewerButton>
        <ViewerButton label="Close (Esc)" onClick={close}>
          <X size={16} />
        </ViewerButton>
      </header>

      <div className="pointer-events-none relative flex flex-1 items-center justify-between px-4">
        {onPrev ? (
          <ViewerButton label="Previous (←)" onClick={onPrev} large>
            <ChevronLeft size={22} />
          </ViewerButton>
        ) : (
          <span />
        )}
        {onNext && (
          <ViewerButton label="Next (→)" onClick={onNext} large>
            <ChevronRight size={22} />
          </ViewerButton>
        )}
      </div>

      {actions && (
        <footer className="relative flex flex-wrap items-center justify-center gap-2 bg-linear-to-t from-black/80 to-transparent px-5 pt-10 pb-5">
          {actions}
        </footer>
      )}
    </div>,
    document.body,
  );
}

function ViewerButton({
  label,
  onClick,
  active,
  large,
  children,
}: {
  label: string;
  onClick: () => void;
  active?: boolean;
  large?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "focus-ring pointer-events-auto flex shrink-0 cursor-pointer items-center justify-center rounded-full text-white transition hover:bg-white/20",
        large ? "h-11 w-11 bg-black/45" : "h-8 w-8 bg-white/10",
        active && "bg-white/25",
      )}
    >
      {children}
    </button>
  );
}

/** Button styled for the dark viewer footer. */
export function ViewerAction({
  onClick,
  primary,
  disabled,
  children,
}: {
  onClick: () => void;
  primary?: boolean;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "focus-ring inline-flex h-9 cursor-pointer items-center gap-1.5 rounded-lg px-4 text-sm font-medium transition disabled:cursor-not-allowed disabled:opacity-60",
        primary ? "bg-brand text-on-brand hover:bg-brand-hover" : "bg-white/12 text-white hover:bg-white/20",
      )}
    >
      {children}
    </button>
  );
}
