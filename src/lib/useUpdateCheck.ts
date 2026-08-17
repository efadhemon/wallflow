import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useState } from "react";
import semver from "semver";
import { api } from "./api";
import { useApp } from "../state/AppContext";

/** Returns the newer available version string, or null when up to date / offline / dismissed. */
export function useUpdateCheck(): { version: string; dismiss: () => void } | null {
  const { environment } = useApp();
  const [available, setAvailable] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const versionApiUrl = import.meta.env.VITE_VERSION_API_URL;

  useEffect(() => {
    const current = environment?.appVersion;
    if (!current || !versionApiUrl) return;

    let cancelled = false;

    (async () => {
      try {
        // Rust-side HTTP avoids webview CSP/CORS restrictions.
        const raw = await api.fetchLatestVersion(versionApiUrl);
        if (!raw) return;

        const latest = raw.startsWith("v") ? raw.slice(1) : raw;
        if (!semver.gt(latest, current)) return;

        if (!cancelled) {
          setDismissed(false);
          setAvailable(latest);
        }
      } catch {
        // Offline or unreachable — no banner.
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [environment?.appVersion, versionApiUrl]);

  // Closing the window only hides it, so React state survives. Bring the banner
  // back the next time the window is shown from the tray.
  useEffect(() => {
    const window = getCurrentWindow();
    let hidden = false;

    const unlisten = window.onFocusChanged(async ({ payload: focused }) => {
      if (!focused) {
        hidden = !(await window.isVisible());
        return;
      }
      if (hidden) {
        hidden = false;
        setDismissed(false);
      }
    });

    return () => {
      void unlisten.then(stop => stop());
    };
  }, []);

  const dismiss = useCallback(() => {
    setDismissed(true);
  }, []);

  if (!available || dismissed) return null;
  return { version: available, dismiss };
}
