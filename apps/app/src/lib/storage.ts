// Réglages propres à cet appareil (thème). Web : localStorage, qui peut être indisponible (navigation privée).
// Natif (plus tard) : à brancher sur un stockage persistant.

export function readSetting(key: string): string | null {
  try {
    return globalThis.localStorage?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

export function writeSetting(key: string, value: string | null): void {
  try {
    if (value === null) globalThis.localStorage?.removeItem(key);
    else globalThis.localStorage?.setItem(key, value);
  } catch {
    // Réglage non gardé : sans conséquence.
  }
}
