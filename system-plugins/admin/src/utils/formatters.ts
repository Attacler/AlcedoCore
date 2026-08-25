export function formatDate(iso?: string): string {
    if (!iso) return "-";
    return new Date(iso).toLocaleDateString();
}

export function formatLogTime(iso: string): string {
    return new Date(iso).toLocaleString();
}

export function formatFileSize(bytes: number): string {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
}

export function actionSeverity(action: string): string {
    switch (action) {
        case "create":
            return "success";
        case "read":
            return "info";
        case "update":
            return "warn";
        case "delete":
            return "danger";
        default:
            return "info";
    }
}

export function slugify(input: string): string {
    return input.replace(/[^a-zA-Z0-9]/g, "_").toLowerCase();
}
