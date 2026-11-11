import { ref } from "vue";

export const TEMP_ID_PREFIX = "__new__";

/** A queued create with a client-side temp id used for optimistic rendering. */
export interface RelationCreateEntry {
    tempId: string;
    values: Record<string, any>;
}

/**
 * Nested relation body for a single relational section, matching the backend's
 * Directus-style shape: `{ create: [...], update: [{ id, ... }], delete: [ids] }`.
 * Sent verbatim inside the parent POST/PATCH so the write is atomic.
 */
export interface RelationBody {
    create: RelationCreateEntry[];
    update: (Record<string, any> & { id: string })[];
    delete: string[];
}

/** Serialized fragment keyed by child collection, as expected by the backend. */
export type RelationBodyFragment = Record<
    string,
    {
        create?: Record<string, any>[];
        update?: (Record<string, any> & { id: string })[];
        delete?: string[];
    }
>;

export function isTempId(id: unknown): id is string {
    return typeof id === "string" && id.startsWith(TEMP_ID_PREFIX);
}

export function generateTempId(): string {
    return `${TEMP_ID_PREFIX}${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
}

export function createRelationBody(): RelationBody {
    return { create: [], update: [], delete: [] };
}

export function isRelationBodyEmpty(body: RelationBody): boolean {
    return (
        body.create.length === 0 &&
        body.update.length === 0 &&
        body.delete.length === 0
    );
}

export function addCreate(
    body: RelationBody,
    values: Record<string, any>,
): string {
    const tempId = generateTempId();
    body.create.push({ tempId, values: { ...values } });
    return tempId;
}

export function replaceCreate(
    body: RelationBody,
    tempId: string,
    values: Record<string, any>,
): void {
    const entry = body.create.find((e) => e.tempId === tempId);
    if (entry) entry.values = { ...values };
}

export function removeCreate(body: RelationBody, tempId: string): void {
    body.create = body.create.filter((e) => e.tempId !== tempId);
}

export function upsertUpdate(
    body: RelationBody,
    id: string,
    values: Record<string, any>,
): void {
    const existing = body.update.find((u) => u.id === id);
    if (existing) {
        Object.assign(existing, values, { id });
    } else {
        body.update.push({ ...values, id });
    }
}

export function removeUpdate(body: RelationBody, id: string): void {
    body.update = body.update.filter((u) => u.id !== id);
}

export function addDelete(body: RelationBody, id: string): void {
    removeUpdate(body, id);
    if (!body.delete.includes(id)) body.delete.push(id);
}

function stripFk(
    values: Record<string, any>,
    fkFieldName?: string,
): Record<string, any> {
    const copy = { ...values };
    if (fkFieldName) delete copy[fkFieldName];
    return copy;
}

/** Serialize one section's body into the backend nested-write fragment. */
export function serializeRelationBody(
    body: RelationBody,
    childCollection: string,
    fkFieldName?: string,
): RelationBodyFragment | null {
    if (!childCollection || isRelationBodyEmpty(body)) return null;
    const fragment: RelationBodyFragment[string] = {};
    if (body.create.length > 0) {
        fragment.create = body.create.map((e) =>
            stripFk(e.values, fkFieldName),
        );
    }
    if (body.update.length > 0) {
        fragment.update = body.update.map((u) => stripFk(u, fkFieldName));
    }
    if (body.delete.length > 0) {
        fragment.delete = [...body.delete];
    }
    return { [childCollection]: fragment };
}

/** Merge serialized fragments (e.g. multiple sections) into one body object. */
export function mergeRelationBodies(
    target: RelationBodyFragment,
    source: RelationBodyFragment | null | undefined,
): RelationBodyFragment {
    if (!source) return target;
    for (const [key, fragment] of Object.entries(source)) {
        const existing = (target[key] ??= {});
        if (fragment.create) {
            existing.create = [...(existing.create || []), ...fragment.create];
        }
        if (fragment.update) {
            existing.update = [...(existing.update || []), ...fragment.update];
        }
        if (fragment.delete) {
            existing.delete = [...(existing.delete || []), ...fragment.delete];
        }
    }
    return target;
}

/**
 * Reactive relation body for a relational section. `childCollection` and
 * `fkFieldName` are getters because the section's fields load asynchronously.
 */
export function useRelationBody(
    childCollection: () => string,
    fkFieldName: () => string | undefined,
) {
    const body = ref<RelationBody>(createRelationBody());

    function reset(): void {
        body.value = createRelationBody();
    }

    function serialize(): RelationBodyFragment | null {
        return serializeRelationBody(
            body.value,
            childCollection(),
            fkFieldName(),
        );
    }

    function isEmpty(): boolean {
        return isRelationBodyEmpty(body.value);
    }

    return {
        body,
        reset,
        serialize,
        isEmpty,
        addCreate: (values: Record<string, any>) =>
            addCreate(body.value, values),
        replaceCreate: (tempId: string, values: Record<string, any>) =>
            replaceCreate(body.value, tempId, values),
        removeCreate: (tempId: string) => removeCreate(body.value, tempId),
        upsertUpdate: (id: string, values: Record<string, any>) =>
            upsertUpdate(body.value, id, values),
        removeUpdate: (id: string) => removeUpdate(body.value, id),
        addDelete: (id: string) => addDelete(body.value, id),
    };
}
