export function createHealthResource(ky: any) {
  return (options?: any) => ky.get("health", options).json();
}
