import { useCallback, useEffect, useState } from "react";
import semver from "semver";
import { api } from "./api";
import { useApp } from "../state/AppContext";

const DISMISSED_KEY = "wallflow:dismissed-update";

function readDismissed(): string | null {
  try {
    return localStorage.getItem(DISMISSED_KEY);
  } catch {
    return null;
  }
}

export function dismissUpdate(version: string) {
  try {
    localStorage.setItem(DISMISSED_KEY, version);
  } catch {
    // ignore quota / private-mode failures
  }
}

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

        if (readDismissed() === latest) {
          if (!cancelled) setDismissed(true);
          return;
        }

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

  const dismiss = useCallback(() => {
    if (!available) return;
    dismissUpdate(available);
    setDismissed(true);
  }, [available]);

  if (!available || dismissed) return null;
  return { version: available, dismiss };
}
