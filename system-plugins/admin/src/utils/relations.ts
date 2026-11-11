export function relationId(v: any): any {
    return v && typeof v === "object" ? v.id : v;
}

export function relationLabel(
    v: any,
    opts: { displayField?: string | null; displayValue?: string | null } = {},
): any {
    if (opts.displayField && v && v[opts.displayField]) {
        return v[opts.displayField];
    }
    return opts.displayValue || v;
}
