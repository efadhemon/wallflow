import type { GalleryAtLeast, GalleryOrientation, GallerySorting } from "./types";

export const CUSTOM_ID = "custom";

export interface GalleryParent {
  id: string;
  label: string;
  /** Wallhaven `q` keyword. Empty for Anime (category bit) and Custom. */
  keyword: string;
  categories: string;
  subs: string[];
}

export const PARENTS: GalleryParent[] = [
  {
    id: "nature",
    label: "Nature",
    keyword: "nature",
    categories: "111",
    subs: ["mountains", "ocean", "forest", "sunset"],
  },
  {
    id: "anime",
    label: "Anime",
    keyword: "",
    categories: "010",
    subs: ["scenery", "night", "city"],
  },
  {
    id: "fantasy",
    label: "Fantasy",
    keyword: '"fantasy art"',
    categories: "111",
    subs: ["castle", "dragon", "magic"],
  },
  {
    id: "space",
    label: "Space",
    keyword: "space",
    categories: "111",
    subs: ["galaxy", "nebula", "planet"],
  },
  {
    id: "cities",
    label: "Cities",
    keyword: "city",
    categories: "111",
    subs: ["night", "rain", "skyline"],
  },
];

export const CUSTOM_PARENT: GalleryParent = {
  id: CUSTOM_ID,
  label: "Custom",
  keyword: "",
  categories: "111",
  subs: [],
};

export function composeQuery(parentKeyword: string, subquery: string): string {
  const parent = parentKeyword.trim();
  const extra = subquery.trim();
  if (!parent && !extra) return "";
  if (!parent) return extra;
  if (!extra) return parent;
  return `${parent} ${extra}`;
}

export function parentById(id: string): GalleryParent {
  return PARENTS.find(parent => parent.id === id) ?? CUSTOM_PARENT;
}

export const ORIENTATIONS: { id: GalleryOrientation; label: string }[] = [
  { id: "landscape", label: "Landscape" },
  { id: "portrait", label: "Portrait" },
  { id: "square", label: "Square" },
  { id: "any", label: "Any" },
];

export const SORTING: { id: GallerySorting; label: string }[] = [
  { id: "relevance", label: "Relevance" },
  { id: "toplist", label: "Popular" },
  { id: "date_added", label: "Newest" },
  { id: "views", label: "Most viewed" },
  { id: "favorites", label: "Most liked" },
  { id: "random", label: "Random" },
];

export const MIN_RESOLUTIONS: { id: GalleryAtLeast; label: string }[] = [
  { id: "1920x1080", label: "1080p+" },
  { id: "2560x1440", label: "1440p+" },
  { id: "any", label: "Any size" },
];
