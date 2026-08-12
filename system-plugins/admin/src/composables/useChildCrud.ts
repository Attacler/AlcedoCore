import { type Ref, type ComputedRef } from 'vue'
import type { FieldDefinition } from '@/stores/collections'
import { getSectionChildCollectionName as resolveChildCollectionName } from '@/composables/useSectionLayout'

/**
 * Shared child CRUD logic extracted from RecordForm.vue and RecordDetail.vue.
 *
 * Both components had identical or near-identical implementations of:
 *   - TEMP_ID_PREFIX / generateTempId / isTempId
 *   - addChildInlineRow
 *   - onChildCellEdit
 *   - onChildDelete
 *   - getChildCollectionName / getSectionChildCollectionName
 *   - findParentFKFieldName
 */

export const TEMP_ID_PREFIX = '__new__'

export function isTempId(id: string): boolean {
  return id.startsWith(TEMP_ID_PREFIX)
}

export function useChildCrud(config: {
  sectionItems: Ref<Record<string, any[]>>
  sectionFields: Ref<Record<string, any[]>>
  childEditDrafts: Ref<Record<string, Record<string, Record<string, any>>>>
  childDeletedRows: Ref<Record<string, Set<string>>>
  fields: ComputedRef<FieldDefinition[]> | Ref<FieldDefinition[]>
  collectionName: ComputedRef<string> | Ref<string>
}) {
  let tempIdCounter = 0

  function generateTempId(): string {
    tempIdCounter++
    return `${TEMP_ID_PREFIX}${tempIdCounter}_${Date.now()}`
  }

  /** Add a new blank child row inline (for relational sections) */
  function addChildInlineRow(section: any) {
    const sectionId = section.id
    const sectionFieldList = config.sectionFields.value[sectionId] || section.definition_fields || []
    if (!sectionFieldList.length) return

    const tempId = generateTempId()
    const newRow: Record<string, any> = { id: tempId }
    for (const field of sectionFieldList) {
      if (['id', 'created_at', 'updated_at', '_row_version'].includes(field.name)) continue
      newRow[field.name] = field.type === 'string' ? '' : null
    }

    if (!config.sectionItems.value[sectionId]) {
      config.sectionItems.value[sectionId] = []
    }
    config.sectionItems.value[sectionId] = [...config.sectionItems.value[sectionId], newRow]

    if (!config.childEditDrafts.value[sectionId]) {
      config.childEditDrafts.value[sectionId] = {}
    }
    config.childEditDrafts.value[sectionId][tempId] = {}
  }

  /** Track a cell edit on a child row (relational sections) */
  function onChildCellEdit(section: any, row: any, fieldName: string, value: any) {
    const sectionId = section.id
    const rowId = row.id
    if (!sectionId || !rowId) return
    if (!config.childEditDrafts.value[sectionId]) {
      config.childEditDrafts.value[sectionId] = {}
    }
    if (!config.childEditDrafts.value[sectionId][rowId]) {
      config.childEditDrafts.value[sectionId][rowId] = {}
    }
    config.childEditDrafts.value[sectionId][rowId][fieldName] = value
  }

  /** Mark a child row for deletion — batch-deleted when parent saves */
  function onChildDelete(section: any, row: any) {
    const sectionId = section.id
    const rowId = row.id
    if (!sectionId || !rowId) return
    if (!config.childDeletedRows.value[sectionId]) {
      config.childDeletedRows.value[sectionId] = new Set()
    }
    config.childDeletedRows.value[sectionId].add(rowId)
    // Remove from local list immediately for responsive UX
    if (config.sectionItems.value[sectionId]) {
      config.sectionItems.value[sectionId] = config.sectionItems.value[sectionId].filter((i: any) => i.id !== rowId)
    }
    // Clean up any pending edits for this row
    if (config.childEditDrafts.value[sectionId]) {
      delete config.childEditDrafts.value[sectionId][rowId]
    }
  }

  /** Resolve the related collection name from a section's relation_field */
  function getChildCollectionName(section: any): string {
    return resolveChildCollectionName(section?.relation_field, config.fields.value)
  }

  /** Template-friendly alias for getChildCollectionName */
  function getSectionChildCollectionName(section: any): string {
    return getChildCollectionName(section)
  }

  /** Find the FK field name on the child collection that points back to the parent */
  function findParentFKFieldName(section: any): string {
    const childFields = config.sectionFields.value[section.id] || []
    const fkField = childFields.find(
      (f: any) => f.type === 'relationship' && f.related_collection === config.collectionName.value
    )
    return fkField?.name || ''
  }

  return {
    addChildInlineRow,
    onChildCellEdit,
    onChildDelete,
    getChildCollectionName,
    getSectionChildCollectionName,
    findParentFKFieldName,
  }
}
