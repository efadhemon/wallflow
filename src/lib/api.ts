import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AddRemoteResult,
  Config,
  DownloadProgress,
  Environment,
  FolderEntry,
  GalleryPage,
  ImageEntry,
  Status,
} from "./types";

export const STATUS_EVENT = "wallpaper://status";
export const DOWNLOAD_PROGRESS_EVENT = "wallpaper://download-progress";

export const api = {
  getStatus: () => invoke<Status>("get_status"),
  getConfig: () => invoke<Config>("get_config"),
  setConfig: (config: Config) => invoke<Config>("set_config", { config }),

  addFolder: (path: string, recursive: boolean) => invoke<Config>("add_folder", { path, recursive }),
  removeFolder: (path: string) => invoke<Config>("remove_folder", { path }),
  setFolderRecursive: (path: string, recursive: boolean) => invoke<Config>("set_folder_recursive", { path, recursive }),
  listFolders: () => invoke<FolderEntry[]>("list_folders"),

  listImages: () => invoke<ImageEntry[]>("list_images"),
  addImages: (paths: string[]) => invoke<number>("add_images", { paths }),
  removeImage: (path: string) => invoke<void>("remove_image", { path }),
  getThumbnail: (path: string) => invoke<string>("get_thumbnail", { path }),
  getPreview: (path: string) => invoke<string>("get_preview", { path }),
  applyImage: (path: string) => invoke<void>("apply_image", { path }),
  toggleExclude: (path: string) => invoke<boolean>("toggle_exclude", { path }),

  next: () => invoke<void>("next_wallpaper"),
  previous: () => invoke<void>("previous_wallpaper"),
  togglePause: () => invoke<void>("toggle_pause"),

  getEnvironment: () => invoke<Environment>("get_environment"),
  readLogs: () => invoke<string>("read_logs"),
  fetchLatestVersion: (url: string) => invoke<string | null>("fetch_latest_version", { url }),

  searchGallery: (args: {
    query?: string | null;
    ratios?: string | null;
    categories?: string | null;
    sorting?: string | null;
    page: number;
    atleast?: string | null;
    topRange?: string | null;
    colors?: string | null;
  }) => invoke<GalleryPage>("search_gallery", args),
  addRemoteImage: (url: string, expectedSize?: number) =>
    // Tauri maps camelCase keys onto the command's snake_case arguments.
    invoke<AddRemoteResult>("add_remote_image", { url, expectedSize: expectedSize ?? null }),
};

export function onStatus(handler: (status: Status) => void): Promise<UnlistenFn> {
  return listen<Status>(STATUS_EVENT, event => handler(event.payload));
}

export function onDownloadProgress(handler: (progress: DownloadProgress) => void): Promise<UnlistenFn> {
  return listen<DownloadProgress>(DOWNLOAD_PROGRESS_EVENT, event => handler(event.payload));
}
