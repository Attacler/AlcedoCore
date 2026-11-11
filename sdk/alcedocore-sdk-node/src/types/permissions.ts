/**
 * Response shape of `POST /api/app/items/{collection}/$permissions`.
 *
 * `create` is the only action implemented server-side today; the `action` field
 * is part of the contract so `update` can be added without a breaking change.
 */
export interface ItemPermissions {
    action: "create" | "update";
    /** Whether any rule accepted this exact payload. */
    allowed: boolean;
    /**
     * One entry per applicable rule (empty when the caller is unrestricted or
     * denied). A form should use these to decide what is settable: a field is
     * only truly permitted as part of a rule that `allowed` it.
     */
    rules: PermissionRule[];
    /**
     * Merged across the rules that did **not** accept, so a form can show every
     * reason the payload is blocked. Empty once any rule accepts.
     */
    violations: PermissionViolation[];
    /** Conditions that could not be decided, merged as above. */
    unresolved: PermissionViolation[];
}

export interface PermissionRule {
    /** The fields this rule permits; empty means any field. */
    fields: string[];
    /** Whether this rule accepted the payload. */
    allowed: boolean;
    violations: PermissionViolation[];
    unresolved: PermissionViolation[];
}

export interface PermissionViolation {
    field: string;
    /** Present for a scalar comparison, e.g. `eq`. */
    operator?: string;
    expected?: unknown;
    actual?: unknown;
    /** Why a non-scalar condition failed: `not_permitted`, `no_pk`, `relation_mismatch`. */
    reason?: string;
}
