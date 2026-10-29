// localStorage can be missing or blocked (private mode, disabled storage);
// reads and writes then simply behave as if nothing were stored.
export function readLocal(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeLocal(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* unavailable storage is not an error for the user */
  }
}
