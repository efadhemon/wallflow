import { Film, Images, Info, LayoutDashboard, LayoutGrid, Settings2 } from "lucide-react";
import type { ComponentType } from "react";
import { cn } from "../lib/utils";
import { useApp } from "../state/AppContext";

export type Route = "dashboard" | "library" | "live" | "gallery" | "settings" | "about";

const NAV: { id: Route; label: string; icon: ComponentType<{ size?: number }> }[] = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "library", label: "Library", icon: Images },
  { id: "live", label: "Live", icon: Film },
  { id: "gallery", label: "Gallery", icon: LayoutGrid },
  { id: "settings", label: "Settings", icon: Settings2 },
  { id: "about", label: "About", icon: Info },
];

export function Sidebar({ active, onNavigate }: { active: Route; onNavigate: (route: Route) => void }) {
  const { status } = useApp();

  return (
    <nav className="flex w-56 shrink-0 flex-col border-r border-line bg-surface">
      <div className="flex items-center gap-2.5 px-5 py-5">
        <img src="/icon.png" alt="" width={26} height={26} className="rounded-md" />
        <span className="text-sm font-semibold text-ink">Wallflow</span>
      </div>

      <div className="flex flex-1 flex-col gap-0.5 px-3">
        {NAV.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            onClick={() => onNavigate(id)}
            aria-current={active === id ? "page" : undefined}
            className={cn(
              "focus-ring flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition",
              active === id ? "bg-brand-soft text-brand" : "text-ink-muted hover:bg-surface-muted hover:text-ink",
            )}
          >
            <Icon size={17} />
            {label}
          </button>
        ))}
      </div>

      <div className="px-5 py-4 text-xs text-ink-muted">
        <span className="flex items-center gap-2">
          <span
            className={cn(
              "h-2 w-2 rounded-full",
              status?.lastError
                ? "bg-danger"
                : status?.mode === "live"
                  ? "bg-brand"
                  : status?.paused
                    ? "bg-ink-muted"
                    : "bg-emerald-500",
            )}
          />
          {status?.lastError
            ? "Error"
            : status?.mode === "live"
              ? "Live wallpaper"
              : status?.paused
                ? "Paused"
                : "Rotating"}
        </span>
      </div>
    </nav>
  );
}
