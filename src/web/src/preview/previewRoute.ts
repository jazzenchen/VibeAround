const OWNER_PREVIEW_PATH = /^\/va\/preview\/u\/([^/]+)\/?$/;

export function ownerPreviewSlug(pathname: string): string | null {
  const match = OWNER_PREVIEW_PATH.exec(pathname);
  if (!match) return null;
  try {
    return decodeURIComponent(match[1]);
  } catch {
    return null;
  }
}
