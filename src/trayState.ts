/** A tray update must never replace a draft that the main window is editing. */
export function mergeTrayDraft<T>(
  draft: T,
  previous: T | undefined,
  next: T,
): T {
  return previous !== undefined &&
    JSON.stringify(draft) === JSON.stringify(previous)
    ? next
    : draft;
}
