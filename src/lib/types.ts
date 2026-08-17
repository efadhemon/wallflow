export type Order = "shuffle" | "sequential" | "random";
export type FillMode = "fill" | "fit" | "stretch" | "center" | "tile";
export type Theme = "light" | "dark" | "system";

export interface Folder {
  path: string;
  recursive: boolean;
}

export interface Config {
  version: number;
  folders: Folder[];
  intervalSeconds: number;
  order: Order;
  fillMode: FillMode;
  perMonitor: boolean;
  changeOnStartup: boolean;
  excluded: string[];
  autostart: boolean;
  startMinimized: boolean;
  theme: Theme;
}

export interface ImageInfo {
  path: string;
  name: string;
  folder: string;
}

export interface Status {
  current: ImageInfo | null;
  paused: boolean;
  /** Unix milliseconds, or null when paused or idle. */
  nextChangeAt: number | null;
  intervalSeconds: number;
  totalImages: number;
  playlistSize: number;
  position: number;
  lastError: string | null;
}

export interface ImageEntry {
  path: string;
  name: string;
  excluded: boolean;
  isCurrent: boolean;
  managed: boolean;
}

export interface FolderEntry {
  path: string;
  recursive: boolean;
  exists: boolean;
  imageCount: number;
}

export interface Environment {
  os: string;
  backend: string;
  monitorCount: number;
  supportsPerMonitor: boolean;
  appVersion: string;
  configPath: string;
  logPath: string;
  cachePath: string;
}

export interface GalleryPhoto {
  id: string;
  url: string;
  resolution: string;
  fileSize: number;
  thumb: string;
  color: string | null;
  path: string;
}

export interface GalleryPage {
  photos: GalleryPhoto[];
  page: number;
  lastPage: number;
  total: number;
}

export interface AddRemoteResult {
  added: boolean;
  path: string;
}

export type GalleryOrientation = "landscape" | "portrait" | "square" | "any";
export type GallerySorting = "relevance" | "toplist" | "date_added" | "views" | "favorites" | "random";
export type GalleryAtLeast = "any" | "1920x1080" | "2560x1440";
