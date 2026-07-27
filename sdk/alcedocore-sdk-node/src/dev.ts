export function createDevResource(ky: any) {
  return {
    start: (slug: string, url: string, ttl_secs?: number, options?: any) => {
      if (!url.startsWith("http://") && !url.startsWith("https://")) {
        return Promise.reject(
          new Error(`Invalid URL scheme: only http/https URLs are allowed: ${url}`),
        );
      }
      return ky.post("dev/start", {
        json: { slug, url, ttl_secs: ttl_secs ?? 3600 },
        ...options,
      }).json();
    },
    stop: (slug: string, options?: any) =>
      ky.post("dev/stop", { json: { slug }, ...options }).json(),
  };
}
