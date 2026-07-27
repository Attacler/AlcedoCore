export function createAuthResource(ky: any) {
  return {
    me: (options?: any) => ky.get("auth/me", options).json(),
    login: (data: any, options?: any) => ky.post("auth/login", { json: data, ...options }).json(),
    logout: (options?: any) => ky.post("auth/logout", options).json(),
  };
}
