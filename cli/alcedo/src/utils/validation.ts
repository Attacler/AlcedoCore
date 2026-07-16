/**
 * Validate that a name contains only safe characters for use in filenames.
 * Rejects names containing path separators (/ or \), null bytes, or "..".
 * Allowed: letters, digits, hyphens, underscores (regex: /^[\w-]+$/).
 * Throws an error with the given label if the name is invalid.
 */
export function assertSafeName(name: string, label: string): void {
  if (!/^[\w-]+$/.test(name)) {
    throw new Error(
      `Invalid ${label} "${name}". Use only letters, digits, hyphens, and underscores.`
    );
  }
}
