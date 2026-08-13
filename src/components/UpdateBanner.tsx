import { Download, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button } from "./ui";
import { useUpdateCheck } from "../lib/useUpdateCheck";

const DOWNLOAD_URL = "https://wallflow.efadhemon.dev/#download";

export function UpdateBanner() {
  const update = useUpdateCheck();
  if (!update) return null;

  return (
    <div
      role="status"
      className="flex shrink-0 items-center gap-3 border-b border-brand/25 bg-brand-soft px-4 py-2 text-xs text-ink"
    >
      <p className="min-w-0 flex-1">
        Wallflow <span className="font-semibold">{update?.version}</span> is available.
      </p>
      <Button
        size="sm"
        variant="primary"
        onClick={() => void openUrl(DOWNLOAD_URL)}
        aria-label={`Download Wallflow ${update?.version}`}
      >
        <Download size={14} />
        Download
      </Button>
      <button
        type="button"
        onClick={update?.dismiss}
        aria-label="Dismiss update notice"
        className="focus-ring shrink-0 cursor-pointer rounded p-1 text-ink-muted opacity-70 hover:opacity-100"
      >
        <X size={14} />
      </button>
    </div>
  );
}
