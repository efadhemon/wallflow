import type { GalleryAtLeast, GalleryOrientation, GallerySorting, GalleryTopRange } from "./types";

export const CUSTOM_ID = "custom";

/**
 * Wallhaven category bits: general / anime / people. Themed collections pick the
 * bits that match them so a "Nature" search is not padded with anime art and the
 * anime collection does not need the word "anime" in its query.
 */
const GENERAL = "100";
const ANIME = "010";
const GENERAL_ANIME = "110";
const ALL = "111";

/** How a preset wants its results ordered. The sort menu can override it. */
export interface GallerySort {
  sorting: GallerySorting;
  topRange?: GalleryTopRange;
}

export interface GalleryPreset {
  id: string;
  label: string;
  /**
   * Wallhaven `q`. Plain words fuzzy-match tag names, `+tag` requires a tag and
   * `-tag` excludes one. Empty means "everything in this collection's categories".
   */
  query: string;
  /** Overrides the collection's sort. Discover uses it to switch feeds. */
  sort?: GallerySort;
}

export interface GalleryCollection {
  id: string;
  label: string;
  /** One line shown under the chips so people know what the collection holds. */
  description: string;
  categories: string;
  /** Used when no preset is picked. */
  query: string;
  sort: GallerySort;
  presets: GalleryPreset[];
}

/**
 * Themed collections rank by all-time favourites: a toplist window drops niche
 * tags to a handful of results, while favourites keep them deep and well-liked.
 * Discover covers what is new and trending.
 */
const POPULAR: GallerySort = { sorting: "favorites" };

export const COLLECTIONS: GalleryCollection[] = [
  {
    id: "discover",
    label: "Discover",
    description: "What the Wallhaven community is liking right now.",
    categories: ALL,
    query: "",
    sort: { sorting: "toplist", topRange: "1w" },
    presets: [
      { id: "trending", label: "Trending", query: "", sort: { sorting: "toplist", topRange: "1w" } },
      { id: "hot", label: "Hot", query: "", sort: { sorting: "hot" } },
      { id: "fresh", label: "Fresh", query: "", sort: { sorting: "date_added" } },
      { id: "top-month", label: "Top this month", query: "", sort: { sorting: "toplist", topRange: "1M" } },
      { id: "top-year", label: "Best of the year", query: "", sort: { sorting: "toplist", topRange: "1y" } },
      { id: "most-liked", label: "All-time favorites", query: "", sort: { sorting: "favorites" } },
      { id: "random", label: "Surprise me", query: "", sort: { sorting: "random" } },
    ],
  },
  {
    id: "nature",
    label: "Nature",
    description: "Landscapes and the outdoors, photography and art.",
    categories: GENERAL,
    query: "nature",
    sort: POPULAR,
    presets: [
      { id: "landscape", label: "Landscapes", query: "landscape" },
      { id: "mountains", label: "Mountains", query: "mountains" },
      { id: "forest", label: "Forest", query: "forest" },
      { id: "sea", label: "Ocean", query: "sea" },
      { id: "beach", label: "Beach", query: "beach" },
      { id: "waterfall", label: "Waterfalls", query: "waterfall" },
      { id: "lake", label: "Lakes", query: "lake" },
      { id: "fall", label: "Autumn", query: "fall" },
      { id: "winter", label: "Winter", query: "winter" },
      { id: "flowers", label: "Flowers", query: "flowers" },
      { id: "sunset", label: "Sunset", query: "sunset" },
      { id: "aurora", label: "Aurora", query: "aurorae" },
      { id: "desert", label: "Desert", query: "desert" },
    ],
  },
  {
    id: "minimal",
    label: "Minimal",
    description: "Clean, quiet backgrounds that keep icons readable.",
    categories: GENERAL_ANIME,
    query: "minimalism",
    sort: POPULAR,
    presets: [
      { id: "minimalism", label: "Minimalism", query: "minimalism" },
      { id: "simple", label: "Simple background", query: "simple background" },
      { id: "abstract", label: "Abstract", query: "abstract" },
      { id: "gradient", label: "Gradient", query: "gradient" },
      { id: "geometry", label: "Geometric", query: "geometry" },
      { id: "line-art", label: "Line art", query: "line art" },
      { id: "pastel", label: "Pastel", query: "pastel" },
      { id: "monochrome", label: "Monochrome", query: "monochrome" },
    ],
  },
  {
    id: "dark",
    label: "Dark",
    description: "Low-light and AMOLED-friendly picks for dark setups.",
    categories: GENERAL_ANIME,
    query: "dark",
    sort: POPULAR,
    presets: [
      { id: "dark", label: "Dark", query: "dark" },
      { id: "black-background", label: "Black background", query: "black background" },
      { id: "amoled", label: "AMOLED", query: "black background minimalism" },
      { id: "night", label: "Night", query: "night" },
      { id: "dark-nature", label: "Moody nature", query: "dark nature" },
      { id: "dark-minimal", label: "Dark minimal", query: "dark minimalism" },
    ],
  },
  {
    id: "anime",
    label: "Anime",
    description: "Anime art, from quiet scenery to favourite series.",
    categories: ANIME,
    query: "",
    sort: POPULAR,
    presets: [
      { id: "scenery", label: "Scenery", query: "landscape" },
      { id: "sky", label: "Sky", query: "sky" },
      { id: "city", label: "City", query: "city" },
      { id: "night", label: "Night", query: "night" },
      { id: "sakura", label: "Sakura", query: "cherry blossom" },
      { id: "ghibli", label: "Studio Ghibli", query: "Studio Ghibli" },
      { id: "shinkai", label: "Makoto Shinkai", query: "Makoto Shinkai" },
      { id: "genshin", label: "Genshin Impact", query: "Genshin Impact" },
      { id: "demon-slayer", label: "Demon Slayer", query: "Kimetsu no Yaiba" },
      { id: "jjk", label: "Jujutsu Kaisen", query: "Jujutsu Kaisen" },
      { id: "one-piece", label: "One Piece", query: "One Piece" },
      { id: "naruto", label: "Naruto", query: "Naruto (anime)" },
      { id: "aot", label: "Attack on Titan", query: "Shingeki no Kyojin" },
    ],
  },
  {
    id: "gaming",
    label: "Gaming",
    description: "Key art and screenshots from games people love.",
    categories: GENERAL_ANIME,
    query: "video games",
    sort: POPULAR,
    presets: [
      { id: "video-games", label: "All games", query: "video games" },
      { id: "cyberpunk-2077", label: "Cyberpunk 2077", query: "Cyberpunk 2077" },
      { id: "elden-ring", label: "Elden Ring", query: "Elden Ring" },
      { id: "witcher", label: "The Witcher", query: "The Witcher 3: Wild Hunt" },
      { id: "minecraft", label: "Minecraft", query: "Minecraft" },
      { id: "valorant", label: "Valorant", query: "Valorant" },
      { id: "lol", label: "League of Legends", query: "League of Legends" },
      { id: "honkai", label: "Honkai: Star Rail", query: "Honkai: Star Rail" },
      { id: "pixel-art", label: "Pixel art", query: "pixel art" },
    ],
  },
  {
    id: "space",
    label: "Space",
    description: "Galaxies, planets and the view from orbit.",
    categories: GENERAL,
    query: "space",
    sort: POPULAR,
    presets: [
      { id: "galaxy", label: "Galaxy", query: "galaxy" },
      { id: "nebula", label: "Nebula", query: "nebula" },
      { id: "planet", label: "Planets", query: "planet" },
      { id: "earth", label: "Earth", query: "Earth" },
      { id: "moon", label: "Moon", query: "Moon" },
      { id: "stars", label: "Stars", query: "stars" },
      { id: "astronaut", label: "Astronaut", query: "astronaut" },
      { id: "black-hole", label: "Black hole", query: "black holes" },
      { id: "milky-way", label: "Milky Way", query: "Milky Way" },
    ],
  },
  {
    id: "cities",
    label: "Cities",
    description: "Skylines, streets and architecture, day and night.",
    categories: GENERAL,
    query: "city",
    sort: POPULAR,
    presets: [
      { id: "cityscape", label: "Cityscape", query: "cityscape" },
      { id: "city-lights", label: "City lights", query: "city lights" },
      { id: "skyline", label: "Skyline", query: "skyline" },
      { id: "street", label: "Street", query: "street" },
      { id: "rain", label: "Rainy city", query: "city rain" },
      { id: "tokyo", label: "Tokyo", query: "Tokyo" },
      { id: "nyc", label: "New York", query: "New York City" },
      { id: "architecture", label: "Architecture", query: "architecture" },
      { id: "bridge", label: "Bridges", query: "bridge" },
    ],
  },
  {
    id: "scifi",
    label: "Sci-fi",
    description: "Cyberpunk, synthwave and the far future.",
    categories: GENERAL_ANIME,
    query: "science fiction",
    sort: POPULAR,
    presets: [
      { id: "cyberpunk", label: "Cyberpunk", query: "cyberpunk" },
      { id: "synthwave", label: "Synthwave", query: "synthwave" },
      { id: "retrowave", label: "Retrowave", query: "retrowave" },
      { id: "vaporwave", label: "Vaporwave", query: "vaporwave" },
      { id: "futuristic", label: "Futuristic", query: "futuristic" },
      { id: "spaceship", label: "Spaceships", query: "spaceship" },
      { id: "robot", label: "Robots", query: "robot" },
      { id: "mech", label: "Mecha", query: "mech" },
    ],
  },
  {
    id: "fantasy",
    label: "Fantasy",
    description: "Painted worlds, castles, dragons and magic.",
    categories: GENERAL_ANIME,
    query: "fantasy art",
    sort: POPULAR,
    presets: [
      { id: "fantasy-art", label: "Fantasy art", query: "fantasy art" },
      { id: "landscape", label: "Fantasy landscape", query: "fantasy landscape" },
      { id: "castle", label: "Castles", query: "fantasy art castle" },
      { id: "dragon", label: "Dragons", query: "dragon" },
      { id: "magic", label: "Magic", query: "fantasy art magic" },
      { id: "warrior", label: "Warriors", query: "fantasy art warrior" },
      { id: "lotr", label: "Middle-earth", query: "The Lord of the Rings" },
    ],
  },
  {
    id: "art",
    label: "Art",
    description: "Digital painting, illustration and artistic styles.",
    categories: GENERAL_ANIME,
    query: "digital art",
    sort: POPULAR,
    presets: [
      { id: "digital-art", label: "Digital art", query: "digital art" },
      { id: "illustration", label: "Illustration", query: "illustration" },
      { id: "painting", label: "Painting", query: "painting" },
      { id: "concept-art", label: "Concept art", query: "concept art" },
      { id: "low-poly", label: "Low poly", query: "low poly" },
      { id: "pixel-art", label: "Pixel art", query: "pixel art" },
      { id: "surreal", label: "Surreal", query: "surreal" },
      { id: "watercolor", label: "Watercolor", query: "watercolor" },
    ],
  },
  {
    id: "cars",
    label: "Cars",
    description: "Supercars, classics, JDM and two wheels.",
    categories: GENERAL,
    query: "car",
    sort: POPULAR,
    presets: [
      { id: "car", label: "All cars", query: "car" },
      { id: "supercars", label: "Supercars", query: "supercars" },
      { id: "jdm", label: "JDM", query: "Japanese cars" },
      { id: "classic", label: "Classic cars", query: "classic car" },
      { id: "porsche", label: "Porsche", query: "Porsche" },
      { id: "bmw", label: "BMW", query: "BMW" },
      { id: "lamborghini", label: "Lamborghini", query: "Lamborghini" },
      { id: "motorcycle", label: "Motorcycles", query: "motorcycle" },
      { id: "f1", label: "Formula 1", query: "Formula 1" },
    ],
  },
  {
    id: "animals",
    label: "Animals",
    description: "Pets and wildlife, on land and underwater.",
    categories: GENERAL,
    query: "animals",
    sort: POPULAR,
    presets: [
      { id: "cats", label: "Cats", query: "cats" },
      { id: "dogs", label: "Dogs", query: "dog" },
      { id: "birds", label: "Birds", query: "birds" },
      { id: "wolf", label: "Wolves", query: "wolf" },
      { id: "big-cats", label: "Big cats", query: "big cats" },
      { id: "wildlife", label: "Wildlife", query: "wildlife" },
      { id: "underwater", label: "Underwater", query: "underwater" },
    ],
  },
  {
    id: "screen",
    label: "Movies & TV",
    description: "Stills and posters from films, shows and comics.",
    categories: GENERAL_ANIME,
    query: "movies",
    sort: POPULAR,
    presets: [
      { id: "star-wars", label: "Star Wars", query: "Star Wars" },
      { id: "marvel", label: "Marvel", query: "Marvel Comics" },
      { id: "dc", label: "DC", query: "DC Comics" },
      { id: "batman", label: "Batman", query: "Batman" },
      { id: "dune", label: "Dune", query: "Dune (movie)" },
      { id: "interstellar", label: "Interstellar", query: "Interstellar" },
      { id: "harry-potter", label: "Harry Potter", query: "Harry Potter" },
      { id: "breaking-bad", label: "Breaking Bad", query: "Breaking Bad" },
    ],
  },
  {
    id: "tech",
    label: "Tech",
    description: "For developers and tinkerers: code, distros and hardware.",
    categories: GENERAL_ANIME,
    query: "technology",
    sort: POPULAR,
    presets: [
      { id: "linux", label: "Linux", query: "Linux" },
      { id: "programming", label: "Programming", query: "programming" },
      { id: "code", label: "Code", query: "code" },
      { id: "terminal", label: "Terminal", query: "terminal" },
      { id: "hacker", label: "Hacker", query: "hacker" },
      { id: "circuits", label: "Circuits", query: "circuit boards" },
      { id: "apple", label: "Apple", query: "Apple Inc." },
      { id: "windows", label: "Windows", query: "Windows 11" },
    ],
  },
  {
    id: "cozy",
    label: "Cozy",
    description: "Lo-fi rooms, rainy windows and warm, calm scenes.",
    categories: GENERAL_ANIME,
    query: "interior",
    sort: POPULAR,
    presets: [
      { id: "lofi", label: "Lo-fi", query: "lofi" },
      { id: "interior", label: "Interiors", query: "interior" },
      { id: "rain", label: "Rainy window", query: "rain on glass" },
      { id: "room", label: "Rooms", query: "room" },
      { id: "coffee", label: "Coffee", query: "coffee" },
      { id: "books", label: "Books", query: "books" },
      { id: "cottage", label: "Cottagecore", query: "cottage" },
    ],
  },
];

export const CUSTOM_COLLECTION: GalleryCollection = {
  id: CUSTOM_ID,
  label: "Custom",
  description: "Search all of Wallhaven. Use +tag to require, -tag to exclude, like:ID for similar images.",
  categories: ALL,
  query: "",
  sort: { sorting: "relevance" },
  presets: [],
};

export function collectionById(id: string): GalleryCollection {
  return COLLECTIONS.find(collection => collection.id === id) ?? CUSTOM_COLLECTION;
}

export function presetById(collection: GalleryCollection, id: string | null): GalleryPreset | null {
  if (!id) return null;
  return collection.presets.find(preset => preset.id === id) ?? null;
}

export function presetSort(collection: GalleryCollection, preset: GalleryPreset | null): GallerySort {
  return preset?.sort ?? collection.sort;
}

/** Joins the collection/preset query with whatever the user typed to narrow it. */
export function composeQuery(base: string, refine: string): string {
  const parent = base.trim();
  const extra = refine.trim();
  if (!parent && !extra) return "";
  if (!parent) return extra;
  if (!extra) return parent;
  return `${parent} ${extra}`;
}

export const ORIENTATIONS: { id: GalleryOrientation; label: string }[] = [
  { id: "landscape", label: "Landscape" },
  { id: "ultrawide", label: "Ultrawide" },
  { id: "portrait", label: "Portrait" },
  { id: "square", label: "Square" },
  { id: "any", label: "Any shape" },
];

export const SORTING: { id: GallerySorting; label: string }[] = [
  { id: "toplist", label: "Top" },
  { id: "hot", label: "Hot" },
  { id: "relevance", label: "Relevance" },
  { id: "date_added", label: "Newest" },
  { id: "favorites", label: "Most liked" },
  { id: "views", label: "Most viewed" },
  { id: "random", label: "Random" },
];

export const TOP_RANGES: { id: GalleryTopRange; label: string }[] = [
  { id: "1d", label: "Today" },
  { id: "3d", label: "3 days" },
  { id: "1w", label: "This week" },
  { id: "1M", label: "This month" },
  { id: "3M", label: "3 months" },
  { id: "6M", label: "6 months" },
  { id: "1y", label: "This year" },
];

export const MIN_RESOLUTIONS: { id: GalleryAtLeast; label: string }[] = [
  { id: "1920x1080", label: "1080p+" },
  { id: "2560x1440", label: "1440p+" },
  { id: "3840x2160", label: "4K+" },
  { id: "any", label: "Any size" },
];

/** Wallhaven's fixed colour palette; the API only accepts these hexes. */
export const COLORS: { id: string; label: string }[] = [
  { id: "000000", label: "Black" },
  { id: "424153", label: "Charcoal" },
  { id: "999999", label: "Gray" },
  { id: "cccccc", label: "Silver" },
  { id: "ffffff", label: "White" },
  { id: "660000", label: "Maroon" },
  { id: "990000", label: "Crimson" },
  { id: "cc0000", label: "Red" },
  { id: "cc3333", label: "Brick" },
  { id: "ea4c88", label: "Pink" },
  { id: "993399", label: "Purple" },
  { id: "663399", label: "Violet" },
  { id: "333399", label: "Indigo" },
  { id: "0066cc", label: "Blue" },
  { id: "0099cc", label: "Sky" },
  { id: "66cccc", label: "Teal" },
  { id: "77cc33", label: "Lime" },
  { id: "669900", label: "Green" },
  { id: "336600", label: "Forest" },
  { id: "666600", label: "Olive" },
  { id: "999900", label: "Moss" },
  { id: "cccc33", label: "Chartreuse" },
  { id: "ffff00", label: "Yellow" },
  { id: "ffcc33", label: "Gold" },
  { id: "ff9900", label: "Amber" },
  { id: "ff6600", label: "Orange" },
  { id: "cc6633", label: "Rust" },
  { id: "996633", label: "Tan" },
  { id: "663300", label: "Brown" },
];

export function colorLabel(hex: string): string | null {
  const id = hex.replace("#", "").toLowerCase();
  return COLORS.find(color => color.id === id)?.label ?? null;
}
