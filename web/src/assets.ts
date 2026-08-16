/**
 * Resolves files shipped beside the interface.
 *
 * RackForge serves its interface from the root of a host, and the published
 * demo serves it from a repository path. Absolute URLs would only work for the
 * first, so anything under `public/` is addressed through the base the build
 * was given.
 */
export function assetUrl(path: string): string {
  return `${import.meta.env.BASE_URL}${path.replace(/^\//, "")}`;
}
