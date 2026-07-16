export function createActivityLogsResource(ky: any) {
  return {
    listCollections: (params?: URLSearchParams, options?: any) =>
      ky.get("logs/collections", { searchParams: params as any, ...options }).json(),
    listSystem: (params?: URLSearchParams, options?: any) =>
      ky.get("logs/system", { searchParams: params as any, ...options }).json(),
  };
}
