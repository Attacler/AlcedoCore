let app: string | null = null;
let version: string | null = null;

export function setAppHeaders(a: string | null, v: string | null) {
    app = a;
    version = v;
}

export function getAppHeaders(): { app: string | null; version: string | null } {
    return { app, version };
}

export function appPath(path: string): string {
    const p = path.startsWith("/") ? path : `/${path}`;
    if (app && version) {
        return `/app/${encodeURIComponent(app)}/${encodeURIComponent(version)}${p}`;
    }
    return p;
}
