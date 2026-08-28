import { api } from "../api";

const skinKey = (uuid: string) => `qookix:offline_skin:${uuid}`;
const variantKey = (uuid: string) => `qookix:offline_variant:${uuid}`;

export interface OfflineSkin {
  /** base64 PNG data URL */
  src: string;
  /** `null` when no variant was recorded → caller should auto-detect */
  variant: "classic" | "slim" | null;
}

/** Read from the localStorage cache only (fast, no IPC). */
export function getOfflineSkinCached(uuid: string): OfflineSkin | null {
  const src = localStorage.getItem(skinKey(uuid));
  if (!src) return null;
  const savedVariant = localStorage.getItem(variantKey(uuid));
  const variant: "classic" | "slim" | null =
    savedVariant === "slim" || savedVariant === "classic" ? savedVariant : null;
  return { src, variant };
}

/**
 * Load a saved offline skin for a uuid.
 * Order: localStorage cache → backend (`skins/offline/<uuid>.png`).
 * The backend is the authoritative source; a cache hit just avoids IPC.
 * When found on the backend it is written back to localStorage.
 */
export async function loadOfflineSkin(uuid: string): Promise<OfflineSkin | null> {
  const cached = getOfflineSkinCached(uuid);
  if (cached) return cached;

  try {
    const res = await api.getOfflineSkin(uuid);
    if (res) {
      const skin: OfflineSkin = { src: res.src, variant: res.variant };
      localStorage.setItem(skinKey(uuid), res.src);
      if (res.variant) localStorage.setItem(variantKey(uuid), res.variant);
      return skin;
    }
  } catch {
    /* backend unavailable — caller decides what to fall back to */
  }
  return null;
}

export function saveOfflineSkinCache(uuid: string, skin: OfflineSkin) {
  localStorage.setItem(skinKey(uuid), skin.src);
  if (skin.variant) localStorage.setItem(variantKey(uuid), skin.variant);
}
