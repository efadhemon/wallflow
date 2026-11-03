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
  /** Id of the live wallpaper playing, if any. Owned by the backend. */
  liveVideo: string | null;
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
  /** Id of the live wallpaper playing instead of the rotation, if any. */
  live: string | null;
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
  thumbLarge: string;
  color: string | null;
  colors: string[];
  path: string;
  views: number;
  favorites: number;
  category: string;
  fileType: string;
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

export interface DownloadProgress {
  url: string;
  received: number;
  total: number | null;
}

export type GalleryOrientation = "landscape" | "ultrawide" | "portrait" | "square" | "any";
export type GallerySorting = "relevance" | "toplist" | "hot" | "date_added" | "views" | "favorites" | "random";
export type GalleryTopRange = "1d" | "3d" | "1w" | "1M" | "3M" | "6M" | "1y";
export type GalleryAtLeast = "any" | "1920x1080" | "2560x1440" | "3840x2160";

export interface LiveVideo {
  id: string;
  name: string;
  width: number;
  height: number;
  fps: number;
  duration: number | null;
  addedAt: number;
  videoPath: string;
  posterPath: string;
}

/** ready: can play. needsSetup / needsLogin: GNOME helper missing / not loaded yet. */
export type LiveSupport = "ready" | "needsSetup" | "needsLogin" | "unsupported";

export interface LiveStatus {
  support: LiveSupport;
  reason: string | null;
  ffmpeg: boolean;
  engine: boolean;
  running: boolean;
  /** Playing, but paused because the desktop is covered. */
  paused: boolean;
  active: string | null;
}

export interface LiveImportProgress {
  id: string;
  name: string;
  fraction: number;
  state: "preparing" | "done" | "failed";
  error: string | null;
}
