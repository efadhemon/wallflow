import { Copy, ExternalLink, FolderOpen, RefreshCw, ScrollText, User } from "lucide-react";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useState } from "react";
import { Button, Card, Divider, SectionTitle } from "../components/ui";
import { api } from "../lib/api";
import { useApp } from "../state/AppContext";

export function About() {
  const { environment, notify } = useApp();
  const [logs, setLogs] = useState("");
  const [loading, setLoading] = useState(false);

  const loadLogs = useCallback(async () => {
    setLoading(true);
    try {
      setLogs(await api.readLogs());
    } catch (error) {
      setLogs(String(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadLogs();
  }, [loadLogs]);

  const copyLogs = async () => {
    try {
      await navigator.clipboard.writeText(logs);
      notify("Logs copied to the clipboard.");
    } catch (error) {
      notify(String(error), "error");
    }
  };

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6 px-8 py-8">
      <section className="flex items-center gap-4">
        <img src="/icon.png" alt="" width={56} height={56} className="rounded-xl" />
        <div>
          <h1 className="text-lg font-semibold text-ink">Wallflow</h1>
          <p className="text-sm text-ink-muted">
            Version {environment?.appVersion ?? "..."} &middot; {environment?.backend}
          </p>
        </div>
      </section>

      <section className="flex flex-col gap-3">
        <SectionTitle>Developer</SectionTitle>
        <Card>
          <div className="flex items-center justify-between gap-4 px-4 py-3">
            <div className="min-w-0">
              <p className="text-sm font-medium text-ink">Emon Hossain</p>
              <p className="mt-0.5 text-xs text-ink-muted">GitHub @efadhemon</p>
            </div>
            <Button
              size="icon"
              variant="ghost"
              aria-label="Open GitHub profile"
              title="Open GitHub profile"
              onClick={() => void openUrl("https://github.com/efadhemon")}
            >
              <User size={16} />
            </Button>
          </div>
          <Divider />
          <div className="flex items-center justify-between gap-4 px-4 py-3">
            <div className="min-w-0">
              <p className="text-sm font-medium text-ink">LinkedIn</p>
              <p className="mt-0.5 truncate font-mono text-xs text-ink-muted">linkedin.com/in/efadhemon</p>
            </div>
            <Button
              size="icon"
              variant="ghost"
              aria-label="Open LinkedIn profile"
              title="Open LinkedIn profile"
              onClick={() => void openUrl("https://www.linkedin.com/in/efadhemon")}
            >
              <ExternalLink size={16} />
            </Button>
          </div>
        </Card>
      </section>

      <section className="flex flex-col gap-3">
        <SectionTitle>Locations</SectionTitle>
        <Card>
          <PathRow label="Settings file" value={environment?.configPath} />
          <Divider />
          <PathRow label="Log folder" value={environment?.logPath} openable />
          <Divider />
          <PathRow label="Thumbnail cache" value={environment?.cachePath} openable />
        </Card>
      </section>

      <section className="flex min-h-0 flex-col gap-3">
        <div className="flex items-center justify-between">
          <SectionTitle>Recent log</SectionTitle>
          <div className="flex gap-2">
            <Button size="sm" variant="ghost" onClick={() => void loadLogs()} disabled={loading}>
              <RefreshCw size={14} />
              Refresh
            </Button>
            <Button size="sm" onClick={() => void copyLogs()} disabled={!logs}>
              <Copy size={14} />
              Copy
            </Button>
          </div>
        </div>

        <Card className="overflow-hidden">
          {logs ? (
            <pre className="max-h-72 overflow-auto px-4 py-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-ink-muted select-text">
              {logs}
            </pre>
          ) : (
            <p className="flex items-center gap-2 px-4 py-6 text-xs text-ink-muted">
              <ScrollText size={16} />
              Nothing logged yet.
            </p>
          )}
        </Card>
      </section>
    </div>
  );
}

function PathRow({ label, value, openable }: { label: string; value?: string; openable?: boolean }) {
  return (
    <div className="flex items-center justify-between gap-4 px-4 py-3">
      <div className="min-w-0">
        <p className="text-sm font-medium text-ink">{label}</p>
        <p className="mt-0.5 truncate font-mono text-xs text-ink-muted select-text">{value ?? "..."}</p>
      </div>
      {openable && value && (
        <Button
          size="icon"
          variant="ghost"
          aria-label={`Open ${label}`}
          title="Open in file manager"
          onClick={() => void openPath(value)}
        >
          <FolderOpen size={16} />
        </Button>
      )}
    </div>
  );
}
