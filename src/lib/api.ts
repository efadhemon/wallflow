import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Config, Environment, FolderEntry, ImageEntry, Status } from "./types";

export const STATUS_EVENT = "wallpaper://status";

export const api = {
  getStatus: () => invoke<Status>("get_status"),
  getConfig: () => invoke<Config>("get_config"),
  setConfig: (config: Config) => invoke<Config>("set_config", { config }),

  addFolder: (path: string, recursive: boolean) => invoke<Config>("add_folder", { path, recursive }),
  removeFolder: (path: string) => invoke<Config>("remove_folder", { path }),
  setFolderRecursive: (path: string, recursive: boolean) => invoke<Config>("set_folder_recursive", { path, recursive }),
  listFolders: () => invoke<FolderEntry[]>("list_folders"),

  listImages: () => invoke<ImageEntry[]>("list_images"),
  getThumbnail: (path: string) => invoke<string>("get_thumbnail", { path }),
  applyImage: (path: string) => invoke<void>("apply_image", { path }),
  toggleExclude: (path: string) => invoke<boolean>("toggle_exclude", { path }),

  next: () => invoke<void>("next_wallpaper"),
  previous: () => invoke<void>("previous_wallpaper"),
  togglePause: () => invoke<void>("toggle_pause"),

  getEnvironment: () => invoke<Environment>("get_environment"),
  readLogs: () => invoke<string>("read_logs"),
};

export function onStatus(handler: (status: Status) => void): Promise<UnlistenFn> {
  return listen<Status>(STATUS_EVENT, event => handler(event.payload));
}
