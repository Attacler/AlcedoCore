export function createSchemaResource(ky: any) {
  return {
    get: (slug: string, options?: any) => ky.get(`plugins/${slug}/schema`, options).json(),
  };
}
