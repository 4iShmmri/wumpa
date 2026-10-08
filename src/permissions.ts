import type { DocEntry } from "./pdf";

// pdf.js PermissionFlag values
export const PERM = {
  PRINT: 4,
  MODIFY: 8,
  COPY: 16,
  ANNOTATE: 32,
  FILL_FORMS: 256,
  ACCESSIBILITY: 512,
  ASSEMBLE: 1024,
  PRINT_HQ: 2048,
} as const;

export const allows = (entry: Pick<DocEntry, "perms">, flag: number) =>
  !entry.perms || entry.perms.includes(flag);

export const isRestricted = (entry: Pick<DocEntry, "perms">) =>
  !!entry.perms && Object.values(PERM).some((f) => !entry.perms!.includes(f));

// Any rewrite of a protected file would drop its encryption, so it is only
// allowed after the user removes security with the right password.
export const isProtected = (entry: Pick<DocEntry, "perms" | "needsPassword">) =>
  entry.needsPassword || isRestricted(entry);
