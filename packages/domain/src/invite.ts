// Liens d'invitation collés à la main.

/** Jeton d'un lien d'invitation collé tel quel (`https://…/invitation/abc`) ou seul (`abc`) ; nul sinon. */
export function inviteToken(text: string): string | null {
  const value = text.trim();
  if (!value) return null;
  const fromLink = value.match(/\/invitation\/([^/?#\s]+)/);
  if (fromLink) return fromLink[1];
  return /^[A-Za-z0-9_-]{8,}$/.test(value) ? value : null;
}
