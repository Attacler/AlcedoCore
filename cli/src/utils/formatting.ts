/**
 * Shared naming/convention utilities used by CLI generator commands.
 */

/**
 * Convert a name to PascalCase for display labels and class names.
 * "dashboard" -> "Dashboard"
 * "user-management" -> "UserManagement"
 * "get_users" -> "GetUsers"
 */
export function toPascalCase(name: string): string {
  return name
    .split(/[_-]/)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join("");
}

/**
 * Convert a name to camelCase for function/variable names.
 * "get_users" -> "getUsers"
 */
export function toCamelCase(name: string): string {
  const pascal = toPascalCase(name);
  return pascal.charAt(0).toLowerCase() + pascal.slice(1);
}

/**
 * Derive a URL path from an endpoint name.
 * "get_users" -> "/api/get_users"
 * "health" -> "/health"
 */
export function derivePath(name: string): string {
  const normalized = name.replace(/[_-]/g, "_");
  if (normalized.startsWith("get_") || normalized.startsWith("post_") ||
      normalized.startsWith("put_") || normalized.startsWith("delete_") ||
      normalized.startsWith("patch_")) {
    return "/" + normalized.replace(/_/g, "/");
  }
  return "/api/" + normalized.replace(/_/g, "/");
}

/**
 * Derive HTTP method from endpoint name.
 * "get_users" -> "GET"
 * "create_user" -> "POST"
 * "health" -> "GET" (default)
 */
export function deriveMethod(name: string): string {
  const prefix = name.split("_")[0].toLowerCase();
  const methodMap: Record<string, string> = {
    get: "GET",
    post: "POST",
    create: "POST",
    put: "PUT",
    update: "PUT",
    delete: "DELETE",
    remove: "DELETE",
    patch: "PATCH",
    list: "GET",
  };
  return methodMap[prefix] || "GET";
}
