import { getCurrentWindow } from "@tauri-apps/api/window";
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, onStatus } from "../lib/api";
import type { Config, Environment, Status, Theme } from "../lib/types";

export interface Toast {
  id: number;
  key?: string;
  message: string;
  tone: "info" | "error";
  progress?: number | null;
  sticky?: boolean;
}

export interface ToastPayload {
  message: string;
  tone?: "info" | "error";
  progress?: number | null;
  sticky?: boolean;
}

interface AppContextValue {
  status: Status | null;
  config: Config | null;
  environment: Environment | null;
  /** Merges a partial change into the config and persists it. */
  updateConfig: (patch: Partial<Config>) => Promise<void>;
  setConfig: (config: Config) => void;
  refresh: () => Promise<void>;
  notify: (message: string, tone?: "info" | "error") => void;
  upsertToast: (key: string, payload: ToastPayload) => void;
  toasts: Toast[];
  dismissToast: (id: number) => void;
}

const AppContext = createContext<AppContextValue | null>(null);

// eslint-disable-next-line react-refresh/only-export-components
export function useApp(): AppContextValue {
  const value = useContext(AppContext);
  if (!value) throw new Error("useApp must be used inside <AppProvider>");
  return value;
}

function applyTheme(theme: Theme) {
  const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const dark = theme === "dark" || (theme === "system" && prefersDark);
  document.documentElement.classList.toggle("dark", dark);
  // Native chrome (macOS titlebar) follows this; in-page titlebars use CSS.
  void getCurrentWindow().setTheme(theme === "system" ? null : theme);
}

export function AppProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<Status | null>(null);
  const [config, setConfigState] = useState<Config | null>(null);
  const [environment, setEnvironment] = useState<Environment | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextToastId = useRef(1);
  const toastKeys = useRef(new Map<string, number>());
  const toastTimers = useRef(new Map<number, number>());

  const clearTimer = useCallback((id: number) => {
    const handle = toastTimers.current.get(id);
    if (handle != null) {
      window.clearTimeout(handle);
      toastTimers.current.delete(id);
    }
  }, []);

  const forgetToast = useCallback(
    (id: number) => {
      clearTimer(id);
      for (const [key, value] of toastKeys.current) {
        if (value === id) toastKeys.current.delete(key);
      }
    },
    [clearTimer],
  );

  const scheduleDismiss = useCallback(
    (id: number, tone: "info" | "error") => {
      clearTimer(id);
      const handle = window.setTimeout(
        () => {
          forgetToast(id);
          setToasts(current => current.filter(toast => toast.id !== id));
        },
        tone === "error" ? 7000 : 3500,
      );
      toastTimers.current.set(id, handle);
    },
    [clearTimer, forgetToast],
  );

  const notify = useCallback(
    (message: string, tone: "info" | "error" = "info") => {
      const id = nextToastId.current++;
      setToasts(current => [...current, { id, message, tone }]);
      scheduleDismiss(id, tone);
    },
    [scheduleDismiss],
  );

  const upsertToast = useCallback(
    (key: string, payload: ToastPayload) => {
      const existingId = toastKeys.current.get(key);
      const tone = payload.tone ?? "info";
      const id = existingId ?? nextToastId.current++;
      if (existingId == null) {
        toastKeys.current.set(key, id);
        setToasts(current => [
          ...current,
          {
            id,
            key,
            message: payload.message,
            tone,
            progress: payload.progress,
            sticky: payload.sticky,
          },
        ]);
      } else {
        setToasts(current =>
          current.map(toast =>
            toast.id === id
              ? {
                  ...toast,
                  message: payload.message,
                  tone: payload.tone ?? toast.tone,
                  progress: payload.progress,
                  sticky: payload.sticky,
                }
              : toast,
          ),
        );
      }
      if (payload.sticky) clearTimer(id);
      else scheduleDismiss(id, tone);
    },
    [clearTimer, scheduleDismiss],
  );

  const dismissToast = useCallback(
    (id: number) => {
      forgetToast(id);
      setToasts(current => current.filter(toast => toast.id !== id));
    },
    [forgetToast],
  );

  const refresh = useCallback(async () => {
    const [nextStatus, nextConfig, nextEnvironment] = await Promise.all([
      api.getStatus(),
      api.getConfig(),
      api.getEnvironment(),
    ]);
    setStatus(nextStatus);
    setConfigState(nextConfig);
    setEnvironment(nextEnvironment);
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    (async () => {
      try {
        const [nextStatus, nextConfig, nextEnvironment] = await Promise.all([
          api.getStatus(),
          api.getConfig(),
          api.getEnvironment(),
        ]);
        setStatus(nextStatus);
        setConfigState(nextConfig);
        setEnvironment(nextEnvironment);
        unlisten = await onStatus(setStatus);
      } catch (error) {
        notify(String(error), "error");
      }
    })();

    return () => unlisten?.();
  }, [notify]);

  // Follow the OS theme live while "system" is selected.
  useEffect(() => {
    if (!config) return;
    applyTheme(config.theme);

    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const listener = () => applyTheme(config.theme);
    media.addEventListener("change", listener);
    return () => media.removeEventListener("change", listener);
  }, [config?.theme, config]);

  const updateConfig = useCallback(
    async (patch: Partial<Config>) => {
      if (!config) return;
      const merged = { ...config, ...patch };
      // Optimistic so switches and sliders feel instant.
      setConfigState(merged);
      try {
        setConfigState(await api.setConfig(merged));
      } catch (error) {
        setConfigState(config);
        notify(String(error), "error");
      }
    },
    [config, notify],
  );

  const value = useMemo<AppContextValue>(
    () => ({
      status,
      config,
      environment,
      updateConfig,
      setConfig: setConfigState,
      refresh,
      notify,
      upsertToast,
      toasts,
      dismissToast,
    }),
    [status, config, environment, updateConfig, refresh, notify, upsertToast, toasts, dismissToast],
  );

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}
