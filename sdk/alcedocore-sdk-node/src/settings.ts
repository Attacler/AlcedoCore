export function createSettingsResource(ky: any) {
  return {
    get: (name: string, options?: any) => ky.get(`plugins/${name}/settings`, options).json(),
    update: (name: string, settings: any, options?: any) =>
      ky.patch(`plugins/${name}/settings`, { json: settings, ...options }).json(),
  };
}
