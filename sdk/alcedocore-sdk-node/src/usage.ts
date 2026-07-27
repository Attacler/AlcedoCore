export function createUsageResource(ky: any) {
  return (name: string, options?: any) => ky.get(`usage/${name}`, options).json();
}
