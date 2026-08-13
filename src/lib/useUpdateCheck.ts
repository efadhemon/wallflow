import { useCallback, useEffect, useState } from "react";
import { gt } from "semver";
import { useApp } from "../state/AppContext";

const DISMISSED_KEY = "wallflow:dismissed-update";
const RELEASES_URL = "https://api.github.com/repos/efadhemon/wallflow/releases/latest";

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

  useEffect(() => {
    const current = environment?.appVersion;
    if (!current) return;

    let cancelled = false;

    (async () => {
      try {
        const response = await fetch(RELEASES_URL, {
          headers: { Accept: "application/vnd.github+json" },
        });
        if (!response.ok) return;

        const data = (await response.json()) as { tag_name?: string };
        const raw = data.tag_name?.trim();
        if (!raw) return;

        const latest = raw.startsWith("v") ? raw.slice(1) : raw;
        if (!gt(latest, current)) return;

        if (readDismissed() === latest) {
          if (!cancelled) setDismissed(true);
          return;
        }

        if (!cancelled) {
          setDismissed(false);
          setAvailable(latest);
        }
      } catch {
        // Offline or blocked — no banner.
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [environment?.appVersion]);

  const dismiss = useCallback(() => {
    if (!available) return;
    dismissUpdate(available);
    setDismissed(true);
  }, [available]);

  if (!available || dismissed) return null;
  return { version: available, dismiss };
}
