export function createMigrationsResource(ky: any) {
  return {
    list: (name: string, options?: any) => ky.get(`plugins/${name}/migrations`, options).json(),
    run: (name: string, options?: any) =>
      ky.post(`plugins/${name}/migrations`, options).json().then(() => void 0),
    rollback: (name: string, version: string, options?: any) =>
      ky.post(`plugins/${name}/rollback/${version}`, options).json(),
  };
}
