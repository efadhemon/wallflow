import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { cn } from "../lib/utils";

/**
 * Client-side window chrome for Linux. GNOME paints CSD buttons over the
 * webview, so they do not receive clicks (or hover) until the window is
 * resized. Drawing the controls in the page avoids that.
 */
export function Titlebar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const window = getCurrentWindow();
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const sync = async () => {
      const next = await window.isMaximized();
      if (!disposed) setMaximized(next);
    };

    void sync();
    void window.onResized(sync).then(fn => {
      unlisten = fn;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const onToggleMaximize = () => {
    void getCurrentWindow().toggleMaximize();
  };

  return (
    <div className="flex h-10 shrink-0 items-center border-b border-line bg-surface">
      <div
        className="flex h-full min-w-0 flex-1 items-center justify-center"
        data-tauri-drag-region
        onDoubleClick={onToggleMaximize}
      >
        <span className="pointer-events-none text-xs text-ink-muted">Wallflow</span>
      </div>

      <div className="flex items-center gap-1.5 pr-2.5">
        <WindowButton label="Minimize" onClick={() => void getCurrentWindow().minimize()}>
          <Minus size={12} strokeWidth={2.4} />
        </WindowButton>
        <WindowButton label={maximized ? "Restore" : "Maximize"} onClick={onToggleMaximize}>
          {maximized ? <Copy size={11} strokeWidth={2.2} /> : <Square size={11} strokeWidth={2.2} />}
        </WindowButton>
        <WindowButton label="Close" danger onClick={() => void getCurrentWindow().hide()}>
          <X size={12} strokeWidth={2.4} />
        </WindowButton>
      </div>
    </div>
  );
}

function WindowButton({
  label,
  onClick,
  danger = false,
  children,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      onMouseDown={event => event.stopPropagation()}
      className={cn(
        "flex h-6 w-6 cursor-pointer items-center justify-center rounded-full bg-ink/10 text-ink-muted transition-colors",
        danger ? "hover:bg-danger hover:text-white" : "hover:bg-ink/20 hover:text-ink",
      )}
    >
      {children}
    </button>
  );
}
