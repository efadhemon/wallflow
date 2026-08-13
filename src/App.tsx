import { X } from "lucide-react";
import { useState } from "react";
import { Sidebar, type Route } from "./components/Sidebar";
import { Titlebar } from "./components/Titlebar";
import { About } from "./routes/About";
import { Dashboard } from "./routes/Dashboard";
import { Library } from "./routes/Library";
import { Settings } from "./routes/Settings";
import { isLinuxDesktop } from "./lib/platform";
import { cn } from "./lib/utils";
import { useApp } from "./state/AppContext";

export function App() {
  const [route, setRoute] = useState<Route>("dashboard");
  const linux = isLinuxDesktop();

  return (
    <div className="flex h-full flex-col">
      {linux && <Titlebar />}

      <div className="flex min-h-0 flex-1">
        <Sidebar active={route} onNavigate={setRoute} />

        <main className="flex-1 overflow-y-auto">
          {route === "dashboard" && <Dashboard onBrowse={() => setRoute("library")} />}
          {route === "library" && <Library />}
          {route === "settings" && <Settings />}
          {route === "about" && <About />}
        </main>
      </div>

      <Toasts />
    </div>
  );
}

function Toasts() {
  const { toasts, dismissToast } = useApp();

  if (toasts.length === 0) return null;

  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-0 z-50 flex flex-col items-center gap-2 p-5">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          role={toast.tone === "error" ? "alert" : "status"}
          className={cn(
            "pointer-events-auto flex max-w-lg items-start gap-3 rounded-lg border px-4 py-2.5 text-xs shadow-lg",
            toast.tone === "error" ? "border-danger/30 bg-danger-soft text-danger" : "border-line bg-surface text-ink",
          )}
        >
          <span className="min-w-0 flex-1 break-words">{toast.message}</span>
          <button
            type="button"
            onClick={() => dismissToast(toast.id)}
            aria-label="Dismiss"
            className="focus-ring shrink-0 cursor-pointer opacity-60 hover:opacity-100"
          >
            <X size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
