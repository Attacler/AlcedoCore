<script setup lang="ts">
import { ref, computed, onMounted, watch, defineAsyncComponent } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useCollectionsStore, type FieldDefinition } from '@/stores/collections'
import FieldNameLabel from '@/components/FieldNameLabel.vue'

const RelationalSection = defineAsyncComponent(() => import('@/components/RelationalSection.vue'))

interface ExpandedRowState {
  loading: boolean
  error: string | null
  item: Record<string, any> | null
}
import { DISPLAY_COMPONENTS } from '@/display'
import DateTimeDisplay from '@/display/DateTimeDisplay.vue'
import ActivityTimeline from '@/components/ActivityTimeline.vue'
import FormFieldRenderer from '@/components/FormFieldRenderer.vue'
import Dialog from 'primevue/dialog'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import { useToast } from '@/composables/useToast'
import { useChildCrud, isTempId } from '@/composables/useChildCrud'
import { useSectionLayout, getColumnFields as getColumnFieldsShared, parseRelationField, getSectionChildCollectionName as getSectionChildCollectionNameShared } from '@/composables/useSectionLayout'

const route = useRoute()
const router = useRouter()
const collectionsStore = useCollectionsStore()
const { client } = useAlcedoClient()
const toast = useToast()

const collectionName = computed(() => route.params.collection as string)
const itemId = computed(() => route.params.id as string)

// Allowed field names from $permissions (null = all fields allowed)
const allowedFields = computed(() => {
  return item.value?.$permissions?.fields ?? null
})

function canEditField(fieldName: string): boolean {
  // If no field restrictions, all fields are editable
  if (!allowedFields.value) return true
  return allowedFields.value.includes(fieldName)
}

const showSidebar = ref(false)
const isLargeScreen = ref(window.innerWidth >= 1024)

const references = ref<any[]>([])
const referencesLoading = ref(false)
const referencesError = ref<string | null>(null)
const item = ref<any>(null)
const collection = ref<{ name: string; fields: FieldDefinition[] } | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

const isEditing = ref(false)
const editValues = ref<Record<string, any>>({})
const inlineParentEditValues = ref<Record<string, any>>({})
const errors = ref<Record<string, string>>({})
const saving = ref(false)
const SYSTEM_FIELD_NAMES = ['id', 'created_at', 'updated_at'] as const
const EDIT_SYSTEM_FIELD_NAMES = ['id', 'created_at', 'updated_at', '_row_version']

const SYSTEM_FIELD_LABELS: Record<string, string> = {
  id: 'ID',
  created_at: 'Created At',
  updated_at: 'Updated At',
}

function makeSystemField(key: string): FieldDefinition {
  return {
    name: key,
    display_name: SYSTEM_FIELD_LABELS[key] || key,
    type: 'string',
    required: false,
    unique: false,
    default_value: null,
    display_type: 'default',
    ordinal_position: 0,
  }
}
const CHILD_CREATE_EXCLUDED_FIELDS = ['id', 'created_at', 'updated_at', '_row_version']

const sections = ref<any[]>([])
const availableLayouts = ref<{ id: string; name: string }[]>([])
const activeLayoutId = ref<string | null>(null)

const orderedDetailSections = computed(() => {
  return [...sections.value].sort((a, b) => (a.ordinal_position || 0) - (b.ordinal_position || 0))
})

function getDetailSectionFields(section: any): any[] {
  if (section.section_type !== 'field_group' || !section.display_fields) return []
  const fieldNames = section.display_fields as string[]
  return fields.value
    .filter(f => f.name && fieldNames.includes(f.name) && !EDIT_SYSTEM_FIELD_NAMES.includes(f.name))
    .sort((a, b) => {
      const aIdx = fieldNames.indexOf(a.name)
      const bIdx = fieldNames.indexOf(b.name)
      return aIdx - bIdx
    })
}
const sectionLoading = ref<Record<string, boolean>>({})
const sectionError = ref<Record<string, string | null>>({})
const sectionItems = ref<Record<string, any[]>>({})
const sectionFields = ref<Record<string, any[]>>({})
const sectionTotal = ref<Record<string, number>>({})
const sectionPage = ref<Record<string, number>>({})
const sectionExpandedData = ref<Record<string, Record<string, ExpandedRowState>>>({})
const nestedCollectionFields = ref<Record<string, FieldDefinition[]>>({})

// --- Child inline editing state ---
/** Draft values for child records being edited inline: sectionId → rowId → fieldName → value */
const childEditDrafts = ref<Record<string, Record<string, Record<string, any>>>>({})

/** Tracks which child rows have been deleted (pending removal) */
const childDeletedRows = ref<Record<string, Set<string>>>({})

const fields = computed(() => {
  if (!collection.value?.fields) return []
  return [...collection.value.fields].sort((a, b) => (a.ordinal_position ?? 0) - (b.ordinal_position ?? 0))
})

/** Resolve the child collection for a relational section (namespaced "collection.field" or legacy parent-owned field). */
function getSectionChildCollection(section: any): string {
  return getSectionChildCollectionNameShared(section?.relation_field, collection.value?.fields || [])
}

// Initialize child CRUD composable with shared state refs
const childCrud = useChildCrud({
  sectionItems,
  sectionFields,
  childEditDrafts,
  childDeletedRows,
  fields,
  collectionName,
})
const { addChildInlineRow, onChildCellEdit, onChildDelete, getChildCollectionName, getSectionChildCollectionName, findParentFKFieldName } = childCrud

// Initialize section layout utilities
const { normalizeSection } = useSectionLayout()

// --- Inline Child Create/Edit State (Phase 86) ---
const childCreateDialogVisible = ref(false)
const childCreateSection = ref<any>(null)
const childCreateEditItem = ref<any>(null)  // non-null = edit mode
const childCreateValues = ref<Record<string, any>>({})
const childCreateErrors = ref<Record<string, string>>({})
const childCreateSaving = ref(false)
const childCreateSaveError = ref<string | null>(null)
const childCreateLoadingFields = ref(false)
const childCreateParentFKField = ref<FieldDefinition | null>(null)
const childCreatePermission = ref<Record<string, boolean>>({})

const childCreateCollectionName = computed(() => {
  const section = childCreateSection.value
  if (!section) return ''
  return getSectionChildCollection(section)
})

// Refs to rendered RelationalSection components (namespaced relations), for Option B flush
const relSectionRefs = ref<Record<string, any>>({})

function setRelSectionRef(id: string, el: any) {
  if (el) {
    relSectionRefs.value[id] = el
  } else {
    delete relSectionRefs.value[id]
  }
}

function isNamespacedRelational(section: any): boolean {
  return section.section_type === 'relational' && !!parseRelationField(section.relation_field)?.collection
}

async function flushRelationalSections() {
  for (const id of Object.keys(relSectionRefs.value)) {
    const el = relSectionRefs.value[id]
    if (el && typeof el.flushPending === 'function') {
      await el.flushPending(itemId.value)
    }
  }
}

const activeTab = ref('details')

const tabItems = [
  { id: 'details', label: 'Details' },
  { id: 'activity', label: 'Activity' },
  { id: 'references', label: 'References' },
]

const sidebarSections = computed(() => {
  const sectionItems = orderedDetailSections.value
    .filter(s => s.section_type === 'field_group' || shouldShowSection(s))
    .map(s => ({
      id: `section-${s.id}`,
      label: s.name,
      type: s.section_type as string,
      count: s.section_type === 'relational' ? (sectionTotal.value[s.id] ?? null) : null,
    }))
  return sectionItems
})

const activeSection = ref<string>('')

const sidebarSectionGroups = computed(() => {
  const sc = sidebarSections.value
  const fieldSections = sc.filter(s => s.type === 'field_group')
  const relationalS = sc.filter(s => s.type === 'relational')
  const groups: { label: string; items: typeof sc }[] = []
  if (fieldSections.length > 0) groups.push({ label: 'Sections', items: fieldSections })
  if (relationalS.length > 0) groups.push({ label: 'Relational', items: relationalS })
  if (sc.length > 0 && !activeSection.value) {
    activeSection.value = sc[0].id
  }
  return groups
})

const formFields = computed(() => {
  return fields.value.filter(f => !EDIT_SYSTEM_FIELD_NAMES.includes(f.name))
})

const systemFieldKeys = computed(() => {
  if (!item.value) return []
  const fieldNames = new Set(fields.value.map(f => f.name))
  return SYSTEM_FIELD_NAMES.filter(k => k in item.value && !fieldNames.has(k))
})



const inlineParentFields = computed(() => {
  if (!item.value || !collection.value) return []
  const results: { fieldName: string; relatedCollection: string; fields: { name: string; value: any }[] }[] = []
  for (const field of collection.value.fields) {
    const inlineFields = (field as any).inline_parent_fields as string[] | undefined
    if (field.type === 'relationship' && inlineFields?.length && field.related_collection) {
      const parentObj = item.value[field.name + '__inline_parent']
      if (parentObj && typeof parentObj === 'object') {
        const fields = inlineFields.map(fname => ({
          name: fname,
          value: parentObj[fname],
        }))
        results.push({ fieldName: field.name, relatedCollection: field.related_collection, fields })
      }
    }
  }
  return results
})

function scrollToSection(sectionId: string) {
  activeSection.value = sectionId
  showSidebar.value = false
  const el = document.getElementById(sectionId)
  if (el) el.scrollIntoView({ behavior: 'smooth', block: 'start' })
}

function onScroll() {
  if (activeTab.value !== 'details') return
  const ids = orderedDetailSections.value.map((s: any) => `section-${s.id}`)
  for (const id of ids) {
    const el = document.getElementById(id)
    if (el) {
      const rect = el.getBoundingClientRect()
      if (rect.top <= 120) {
        activeSection.value = id
      }
    }
  }
}

async function fetchRecord() {
  if (!collectionName.value || !itemId.value) return

  loading.value = true
  error.value = null

  try {
    const res = await client.items.get(collectionName.value, itemId.value) as any

    const data = res.data || res
    item.value = data || null
    if (!item.value) {
      throw new Error(`Record with id "${itemId.value}" not found`)
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load record'
    item.value = null
  } finally {
    loading.value = false
  }
}

function enterEditMode() {
  const values: Record<string, any> = {}
  const parentValues: Record<string, any> = {}
  for (const field of formFields.value) {
    const existingValue = item.value ? item.value[field.name] : undefined
    const hasExisting = existingValue !== undefined && existingValue !== null

    if (hasExisting) {
      if (field.type === 'datetime') {
        values[field.name] = typeof existingValue === 'string' ? new Date(existingValue) : existingValue
      } else if (field.type === 'int') {
        values[field.name] = typeof existingValue === 'string' ? parseInt(existingValue, 10) || null : existingValue
      } else if (field.type === 'float') {
        values[field.name] = typeof existingValue === 'string' ? parseFloat(existingValue) || null : existingValue
      } else {
        values[field.name] = existingValue
      }
    } else {
      const dv = field.default_value
      if (field.type === 'datetime') {
        values[field.name] = dv ? new Date(dv) : null
      } else if (field.type === 'int') {
        values[field.name] = dv ? parseInt(dv, 10) || null : null
      } else if (field.type === 'float') {
        values[field.name] = dv ? parseFloat(dv) || null : null
      } else if (field.type === 'relationship') {
        values[field.name] = existingValue ?? null
      } else {
        values[field.name] = dv ?? ''
      }
    }
  }

  // Populate inline parent edit values
  for (const pf of inlineParentFields.value) {
    for (const f of pf.fields) {
      const key = `__parent__${pf.fieldName}__${f.name}`
      parentValues[key] = f.value ?? ''
    }
  }
  inlineParentEditValues.value = parentValues
  originalInlineParentValues.value = JSON.parse(JSON.stringify(parentValues))

  editValues.value = values
  errors.value = {}
  isEditing.value = true
}

function removeTempRows() {
  for (const sectionId of Object.keys(sectionItems.value)) {
    const items = sectionItems.value[sectionId]
    if (items) {
      sectionItems.value[sectionId] = items.filter((i: any) => !isTempId(i.id))
    }
  }
}

function cancelEdit() {
  isEditing.value = false
  editValues.value = {}
  errors.value = {}
}

const showDeleteDialog = ref(false)
const deletingItem = ref(false)

function confirmDelete() {
  if (!item.value || !collectionName.value) return
  showDeleteDialog.value = true
}

async function handleDeleteConfirmed() {
  if (!item.value || !collectionName.value) return
  deletingItem.value = true
  try {
    await client.items.delete(collectionName.value, { filter: { id: { _eq: item.value.id } } })
    toast.show('Item deleted', 'success')
    router.back()
  } catch (e) {
    toast.show('Failed to delete item', 'error')
  } finally {
    deletingItem.value = false
    showDeleteDialog.value = false
  }
}

function validate(): boolean {
  const newErrors: Record<string, string> = {}
  for (const field of formFields.value) {
    const value = editValues.value[field.name]
    if (field.required) {
      // DatePicker may emit null on mount; treat as valid if item has a value
      const itemHasValue = item.value && item.value[field.name] !== null && item.value[field.name] !== undefined
      if ((value === null || value === undefined || value === '') && !itemHasValue) {
        newErrors[field.name] = `${field.name} is required`
      }
    }
  }
  errors.value = newErrors
  return Object.keys(newErrors).length === 0
}

const originalInlineParentValues = ref<Record<string, any>>({})

const hasInlineParentChanges = computed(() => {
  const current = inlineParentEditValues.value
  return Object.keys(current).some(
    key => current[key] !== (originalInlineParentValues.value[key] ?? '')
  )
})

/** Compare a field's edit value against its original to detect actual changes */
function fieldChanged(field: FieldDefinition, editValue: any, originalValue: any): boolean {
  // Both null/undefined → no change
  if (editValue == null && originalValue == null) return false
  // One is null/undefined, the other has value → changed
  if (editValue == null || originalValue == null) return true

  if (field.type === 'datetime') {
    const editIso = editValue instanceof Date ? editValue.toISOString() : String(editValue)
    const origIso = new Date(originalValue).toISOString()
    return editIso !== origIso
  }
  if (field.type === 'int' || field.type === 'float') {
    return Number(editValue) !== Number(originalValue)
  }
  // string, text, uuid → direct comparison
  return String(editValue) !== String(originalValue)
}

function buildPayload(): Record<string, any> {
  const payload: Record<string, any> = {}
  for (const field of formFields.value) {
    const value = editValues.value[field.name]
    const original = item.value?.[field.name]
    // Relationship fields:
    // - Pending inline create (nested object, no id) → send as nested M:1 create.
    // - Otherwise they're handled by the child edit loop / inline parent flow.
    if (field.type === 'relationship') {
      if (value && typeof value === 'object' && !Array.isArray(value)) {
        payload[field.name] = value
      }
      continue
    }
    // Skip fields that haven't actually changed
    if (!fieldChanged(field, value, original)) continue
    if (field.type === 'datetime' && value instanceof Date) {
      payload[field.name] = value.toISOString()
    } else {
      payload[field.name] = value
    }
  }

  // Add inline parent values with __parent__ prefix
  for (const [key, val] of Object.entries(inlineParentEditValues.value)) {
    if (val !== null && val !== undefined && val !== '') {
      payload[key] = val
    }
  }

  return payload
}

async function saveEdit() {
  if (!validate() || !item.value) return

  // Show confirmation dialog if there are inline parent changes
  if (hasInlineParentChanges.value) {
    showParentConfirm.value = true
    return
  }

  await doSave()
}

const showParentConfirm = ref(false)

async function doSave() {
  if (!item.value) return
  saving.value = true
  showParentConfirm.value = false
  try {
    // Build a single payload: parent scalars + inline parent fields + relational sections
    const payload = buildPayload()
    const parentPayload = { ...payload, ...inlineParentEditValues.value }

    // Add relational sections with create/update/delete
    for (const section of orderedDetailSections.value) {
      if (section.section_type !== 'relational') continue
      const sectionId = section.id
      // Namespaced "collection.field" relations live on the child collection —
      // their child rows are edited via the child dialog, not the parent payload.
      if (parseRelationField(section.relation_field)?.collection) continue
      const o2mBody: Record<string, any> = {}
      let hasChanges = false

      // Collect creates (temp ID rows)
      const drafts = childEditDrafts.value[sectionId]
      if (drafts) {
        const creates: Record<string, any>[] = []
        const updates: Record<string, any>[] = []
        for (const [rowId, fields] of Object.entries(drafts)) {
          if (Object.keys(fields).length === 0) continue
          if (isTempId(rowId)) {
            creates.push(fields)
            hasChanges = true
          } else {
            updates.push({ id: rowId, ...fields })
            hasChanges = true
          }
        }
        if (creates.length > 0) o2mBody.create = creates
        if (updates.length > 0) o2mBody.update = updates
      }

      // Collect deletes
      const deletedSet = childDeletedRows.value[sectionId]
      if (deletedSet && deletedSet.size > 0) {
        o2mBody.delete = Array.from(deletedSet)
        hasChanges = true
      }

      if (hasChanges) {
        parentPayload[section.relation_field] = o2mBody
      }
    }

    // Single PATCH — backend handles parent + children atomically
    const hasParentChanges = Object.keys(parentPayload).length > 0
    if (hasParentChanges) {
      const body = await client.items.patch(collectionName.value, itemId.value, parentPayload) as any
      const updated = body.updated
      if (updated) item.value = { ...item.value, ...updated }
    }

    // Flush deferred child edits for namespaced relational sections (Option B)
    await flushRelationalSections()

    // Refresh all relational sections to get latest child data with real IDs
    for (const section of orderedDetailSections.value) {
      if (section.section_type !== 'relational') continue
      if (isNamespacedRelational(section)) continue
      const sec = sections.value.find((s: any) => s.id === section.id)
      if (sec) loadSectionData(sec)
    }

    toast.show('Record saved successfully', 'success')
    editValues.value = {}
    inlineParentEditValues.value = {}
    originalInlineParentValues.value = {}
    errors.value = {}
    childEditDrafts.value = {}
    childDeletedRows.value = {}
    removeTempRows()
    isEditing.value = false
  } catch (e) {
    const message = e instanceof Error ? e.message : 'Failed to save record'
    toast.show(message, 'error')
  } finally {
    saving.value = false
  }
}

async function loadReferences() {
  referencesLoading.value = true
  referencesError.value = null
  references.value = []
  try {
    references.value = await collectionsStore.fetchReferences(collectionName.value, itemId.value)
  } catch (e) {
    referencesError.value = e instanceof Error ? e.message : 'Failed to load references'
  } finally {
    referencesLoading.value = false
  }
}

async function loadRecordData(coll: string, _id: string) {
  error.value = null
  try {
    collection.value = await collectionsStore.getCollection(coll)
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load collection schema'
  }

  await fetchRecord()
  if (item.value) {
    loadReferences()
  }
  await loadSections()
}

function handleResize() {
  isLargeScreen.value = window.innerWidth >= 1024
}

watch([collectionName, itemId], ([name, id]) => {
  if (name && id) loadRecordData(name, id)
})

onMounted(() => {
  loadRecordData(collectionName.value, itemId.value)
  window.addEventListener('scroll', onScroll, { passive: true })
  window.addEventListener('resize', handleResize)
})

import { onUnmounted, markRaw, type Component } from 'vue'
import TableView from '@/components/TableView.vue'
import CardsView from '@/components/CardsView.vue'

const viewComponentMap: Record<string, Component> = {
  table: markRaw(TableView),
  cards: markRaw(CardsView),
}

function sectionViewComponent(viewType: string): Component {
  return viewComponentMap[viewType] || viewComponentMap.table
}

function sectionViewFields(section: any): any[] {
  const all = sectionFields.value[section.id] || []
  const viewSettings = section.default_filter?.view_settings
  const displayFields = viewSettings?.displayFields
  if (displayFields?.length > 0) {
    return all.filter((f: any) => displayFields.includes(f.name))
  }
  return all
}

function getSectionNestedDepth(section: any): number {
  const depth = (section as any).nested_depth
  if (typeof depth === 'number' && depth >= 1 && depth <= 5) return depth
  return 2
}

/** Column-split field filtering: delegates to shared getColumnFields with local getDetailSectionFields */
function getColumnFields(section: any, colIdx: number): any[] {
  return getColumnFieldsShared(section, colIdx, getDetailSectionFields(section))
}

async function fetchChildCreatePermission(section: any) {
  const relName = getSectionChildCollection(section)
  if (!relName) return
  try {
    const policy = await client.collections.getCreatePolicy(relName) as any
    childCreatePermission.value[section.id] = policy?.$permissions?.create === true
  } catch {
    childCreatePermission.value[section.id] = false
  }
}

async function loadSections() {
  try {
    const response = await collectionsStore.getResolvedLayout(collectionName.value) as any
    const layoutData = response.data || response
    const raw = layoutData.sections || []
    const avail = layoutData.available_layouts || []
    availableLayouts.value = avail
    if (avail.length > 0) {
      activeLayoutId.value = layoutData.layout?.id || avail[0].id
    }
    sections.value = raw.map(normalizeSection)
    for (const section of sections.value) {
      if (section.section_type === 'relational') {
        if (isNamespacedRelational(section)) continue
        await loadSectionFields(section)
        await loadSectionData(section)
        loadNestedCollectionFields(section)
        fetchChildCreatePermission(section)
      }
    }
  } catch (e) { console.warn('[RecordDetail] Failed to load sections', e); sections.value = [] }
}

async function switchLayout(layoutId: string) {
  if (layoutId === activeLayoutId.value) return
  activeLayoutId.value = layoutId
  try {
    const raw = await collectionsStore.listLayoutSections(collectionName.value, layoutId)
    sections.value = raw.map(normalizeSection)
    for (const section of sections.value) {
      if (section.section_type === 'relational') {
        if (isNamespacedRelational(section)) continue
        await loadSectionFields(section)
        await loadSectionData(section)
        loadNestedCollectionFields(section)
        fetchChildCreatePermission(section)
      }
    }
  } catch (e) { console.warn('[RecordDetail] Failed to switch layout', e) }
}

async function loadSectionFields(section: any) {
  try {
    const relName = getSectionChildCollection(section)
    if (!relName) return
    const relColl = await collectionsStore.getCollection(relName)
    sectionFields.value[section.id] = relColl.fields || []
  } catch (e) { console.warn('[RecordDetail] Failed to load section fields', e); sectionFields.value[section.id] = [] }
}

async function loadNestedCollectionFields(section: any) {
  try {
    const relName = getSectionChildCollection(section)
    if (!relName) return
    const relColl = await collectionsStore.getCollection(relName)
    const relFields = (relColl.fields || []).filter(
      (f: FieldDefinition) => f.type === 'relationship' && f.related_collection
    )
    for (const relField of relFields) {
      if (relField.related_collection && !nestedCollectionFields.value[relField.related_collection]) {
        const nestedColl = await collectionsStore.getCollection(relField.related_collection)
        nestedCollectionFields.value[relField.related_collection] = nestedColl.fields || []
      }
    }
  } catch (e) {
    console.warn('[RecordDetail] Failed to load nested collection fields', e)
  }
}

async function loadSectionData(section: any) {
  sectionLoading.value[section.id] = true
  sectionError.value[section.id] = null
  try {
    const relName = getSectionChildCollection(section)
    if (!relName) return
    const queryParams = new URLSearchParams()
    queryParams.set('limit', String(section.item_limit || 25))
    queryParams.set('offset', '0')

    // Build filter: combine section's default_filter.filter with FK filter
    let filterObj: Record<string, any> = {}
    if (section.default_filter?.filter) {
      filterObj = { ...section.default_filter.filter }
    }
    // Find the FK field in the child collection that references back to parent
    const childFields = sectionFields.value[section.id] || []
    const fkField = childFields.find((f: any) =>
      f.type === 'relationship' && f.related_collection === collectionName.value
    )
    if (fkField && item.value?.id) {
      filterObj[fkField.name] = { _eq: item.value.id }
    }
    if (Object.keys(filterObj).length > 0) {
      queryParams.set('filter', JSON.stringify(filterObj))
    }

    const res = await client.items.list(relName, queryParams as unknown as Record<string, string>) as any
    const data = res.data || res
    sectionItems.value[section.id] = data.data || data.items || data || []
    sectionTotal.value[section.id] = data.total || sectionItems.value[section.id].length
    sectionPage.value[section.id] = 1
  } catch (e) {
    sectionError.value[section.id] = e instanceof Error ? e.message : 'Failed to load section'
  } finally {
    sectionLoading.value[section.id] = false
  }
}

async function handleRowExpand(section: any, item: any) {
  const sectionId = section.id
  const itemId = item?.id
  if (!sectionId || !itemId) return

  // Initialize expanded tracking for this section if needed
  if (!sectionExpandedData.value[sectionId]) {
    sectionExpandedData.value[sectionId] = {}
  }

  // Set loading state
  sectionExpandedData.value[sectionId][itemId] = {
    loading: true,
    error: null,
    item: null,
  }

  try {
    const relName = getSectionChildCollection(section)
    if (!relName) return

    const res = await client.items.get(relName, itemId) as any

    const data = res.data || res
    sectionExpandedData.value[sectionId][itemId] = {
      loading: false,
      error: null,
      item: data,
    }
  } catch (e) {
    sectionExpandedData.value[sectionId][itemId] = {
      loading: false,
      error: e instanceof Error ? e.message : 'Failed to load nested data',
      item: null,
    }
  }
}

async function loadSectionPage(section: any, page: number) {
  sectionPage.value[section.id] = page
  sectionLoading.value[section.id] = true
  try {
    const relName = getSectionChildCollection(section)
    if (!relName) return
    const offset = (page - 1) * (section.item_limit || 25)
    const queryParams = new URLSearchParams()
    queryParams.set('limit', String(section.item_limit || 25))
    queryParams.set('offset', String(offset))
    let filterObj: Record<string, any> = {}
    if (section.default_filter?.filter) {
      filterObj = { ...section.default_filter.filter }
    }
    const childFields = sectionFields.value[section.id] || []
    const fkField = childFields.find((f: any) =>
      f.type === 'relationship' && f.related_collection === collectionName.value
    )
    if (fkField && item.value?.id) {
      filterObj[fkField.name] = { _eq: item.value.id }
    }
    if (Object.keys(filterObj).length > 0) {
      queryParams.set('filter', JSON.stringify(filterObj))
    }
    const res = await client.items.list(relName, queryParams as unknown as Record<string, string>) as any
    const data = res.data || res
    sectionItems.value[section.id] = data.data || data.items || data || []
    } catch (e) { console.warn('[RecordDetail] Failed to load section page', e) }
  finally { sectionLoading.value[section.id] = false }
}

// --- Section Visibility ---

const sectionVisibilityPassed = ref<Record<string, boolean>>({})
const sectionVisibilityChecked = ref<Record<string, boolean>>({})

function matchConditions(condition: any, item: any): boolean {
  if (!condition) return true
  if (condition.operator && condition.conditions) {
    const results = condition.conditions.map((c: any) => matchConditions(c, item))
    return condition.operator === 'and' ? results.every(Boolean) : results.some(Boolean)
  }
  if (condition.field) {
    const val = item?.[condition.field]
    switch (condition.operator) {
      case '_eq': return val == condition.value
      case '_neq': return val != condition.value
      case '_contains': return String(val ?? '').includes(condition.value ?? '')
      case '_startsWith': return String(val ?? '').startsWith(condition.value ?? '')
      case '_gt': return Number(val) > Number(condition.value)
      case '_lt': return Number(val) < Number(condition.value)
      case '_gte': return Number(val) >= Number(condition.value)
      case '_lte': return Number(val) <= Number(condition.value)
      case '_in': return Array.isArray(condition.value) && condition.value.includes(val)
      case '_isNull': return val === null || val === undefined
      default: return true
    }
  }
  return true
}

async function checkChildVisibility(section: any) {
  if (sectionVisibilityChecked.value[section.id]) return
  sectionVisibilityChecked.value[section.id] = true

  const vis = section.default_filter?.section_visibility
  if (!vis?.child) { sectionVisibilityPassed.value[section.id] = true; return }

  const relName = getSectionChildCollection(section)
  if (!relName) { sectionVisibilityPassed.value[section.id] = true; return }

  try {
    let filterObj: any = {}
    const childFields = sectionFields.value[section.id] || []
    const fkField = childFields.find((f: any) =>
      f.type === 'relationship' && f.related_collection === collectionName.value
    )
    if (fkField && item.value?.id) {
      filterObj[fkField.name] = { _eq: item.value.id }
    }
    // Merge visibility child conditions
    if (vis.child) {
      if (filterObj[fkField?.name]) {
        filterObj.$and = [vis.child, { [fkField!.name]: { _eq: item.value!.id } }]
        delete filterObj[fkField!.name]
      } else {
        delete filterObj[fkField?.name]
        filterObj = vis.child
      }
    }

    const res = await client.items.list(relName, {
      limit: '1',
      filter: JSON.stringify(filterObj),
    }) as any
    const data = res.data || res
    const items: any[] = data.data || data.items || data || []
    sectionVisibilityPassed.value[section.id] = items.length > 0
  } catch (e) {
    console.warn('[RecordDetail] Failed to check child visibility', e)
    sectionVisibilityPassed.value[section.id] = false
  }
}

function shouldShowSection(section: any): boolean {
  const vis = section.default_filter?.section_visibility
  const record = item.value
  if (!vis) {
    return true
  }

  // Parent conditions (client-side)
  if (vis.parent) {
    const match = matchConditions(vis.parent, record)
    return match
  }

  // Child conditions (async check)
  if (vis.child) {
    if (!sectionVisibilityChecked.value[section.id]) {
      checkChildVisibility(section)
    }
    return sectionVisibilityPassed.value[section.id] ?? true
  }

  return true
}

// --- Inline Child Creation Helpers (Phase 86) ---

/** Find the M:1 relationship field on child collection that references parent */
function findParentFKField(section: any): FieldDefinition | null {
  const childFields = sectionFields.value[section.id] || []
  return childFields.find(
    f => f.type === 'relationship' && f.related_collection === collectionName.value
  ) || null
}

/** Computed: form fields for child collection (exclude system fields and parent FK) */
const childCreateFormFields = computed(() => {
  const section = childCreateSection.value
  if (!section) return []
  const fields = sectionFields.value[section.id] || []
  const fkField = findParentFKField(section)
  return fields.filter(f => {
    if (CHILD_CREATE_EXCLUDED_FIELDS.includes(f.name)) return false
    if (fkField && f.name === fkField.name) return false  // FK handled separately
    return true
  })
})

/** Open the child creation dialog for a section.
 * If editItem is provided, pre-fills the form with existing data (edit mode).
 */
async function openChildCreateDialog(section: any, editItem: any) {
  childCreateSection.value = section
  childCreateEditItem.value = editItem
  childCreateDialogVisible.value = true
  childCreateSaveError.value = null
  childCreateErrors.value = {}

  // Detect parent FK field
  const fkField = findParentFKField(section)
  childCreateParentFKField.value = fkField

  // Ensure section fields are loaded
  if (!sectionFields.value[section.id]) {
    childCreateLoadingFields.value = true
    try {
      await loadSectionFields(section)
    } catch (e) { console.warn('[RecordDetail] Failed to load section fields for child create', e) }
    childCreateLoadingFields.value = false
  }

  // Initialize form values
  const fields = sectionFields.value[section.id] || []
  const values: Record<string, any> = {}
  for (const field of fields) {
    if (CHILD_CREATE_EXCLUDED_FIELDS.includes(field.name)) continue
    if (fkField && field.name === fkField.name) {
      values[field.name] = item.value?.id  // Auto-fill parent FK
      continue
    }
    // If editing, pre-fill from existing item data
    if (editItem && editItem[field.name] !== undefined) {
      const existingValue = editItem[field.name]
      if (field.type === 'datetime') {
        values[field.name] = typeof existingValue === 'string' ? new Date(existingValue) : existingValue
      } else if (field.type === 'int') {
        values[field.name] = typeof existingValue === 'string' ? parseInt(existingValue, 10) || null : existingValue
      } else if (field.type === 'float') {
        values[field.name] = typeof existingValue === 'string' ? parseFloat(existingValue) || null : existingValue
      } else {
        values[field.name] = existingValue
      }
      continue
    }
    const dv = field.default_value
    if (field.type === 'datetime') {
      values[field.name] = dv ? new Date(dv) : undefined
    } else if (field.type === 'int') {
      values[field.name] = dv ? parseInt(dv, 10) || null : null
    } else if (field.type === 'float') {
      values[field.name] = dv ? parseFloat(dv) || null : null
    } else if (field.type === 'relationship') {
      values[field.name] = null
    } else {
      values[field.name] = dv ?? ''
    }
  }
  childCreateValues.value = values
}

/** Close the child creation dialog and reset state */
function closeChildCreateDialog() {
  childCreateDialogVisible.value = false
  childCreateSection.value = null
  childCreateEditItem.value = null
  childCreateValues.value = {}
  childCreateErrors.value = {}
  childCreateSaveError.value = null
  childCreateSaving.value = false
  childCreateParentFKField.value = null
}

/** Validate child creation form fields */
function validateChildCreate(): boolean {
  const newErrors: Record<string, string> = {}
  for (const field of childCreateFormFields.value) {
    const value = childCreateValues.value[field.name]
    if (field.required) {
      if (field.type === 'datetime') continue
      if (value === null || value === undefined || value === '') {
        newErrors[field.name] = `${field.name} is required`
      }
    }
  }
  childCreateErrors.value = newErrors
  return Object.keys(newErrors).length === 0
}

/** Save child record via API — POST to child collection's items endpoint */
async function saveChildCreate() {
  if (!validateChildCreate()) return

  const section = childCreateSection.value
  if (!section) return

  childCreateSaving.value = true
  childCreateSaveError.value = null

  try {
    // Build payload from form values
    const payload: Record<string, any> = {}
    for (const field of childCreateFormFields.value) {
      let value = childCreateValues.value[field.name]
      if (value === null || value === undefined || value === '') continue
      if (field.type === 'datetime' && value instanceof Date) {
        value = value.toISOString()
      }
      payload[field.name] = value
    }

    // Add parent FK (auto-filled in childCreateValues by openChildCreateDialog)
    const fkField = childCreateParentFKField.value
    if (fkField) {
      payload[fkField.name] = item.value?.id
    }

    // POST (create) or PATCH (edit) to child collection's items endpoint
    const relName = getChildCollectionName(section)
    if (!relName) {
      throw new Error('Could not determine child collection name')
    }

    if (childCreateEditItem.value?.id) {
      await client.items.patch(relName, childCreateEditItem.value.id, payload)
    } else {
      await client.items.create(relName, payload)
    }

    // Success: toast, close dialog, refresh section
    toast.show(childCreateEditItem.value ? 'Item updated successfully' : 'Item created successfully', 'success')
    childCreateDialogVisible.value = false

    // Refresh section data to show the newly created child record
    await loadSectionData(section)

    // Reset state
    childCreateSection.value = null
    childCreateValues.value = {}
    childCreateErrors.value = {}
    childCreateSaving.value = false
    childCreateParentFKField.value = null
    childCreateSaveError.value = null

  } catch (e) {
    // Error: show in dialog, keep open for retry
    childCreateSaveError.value = e instanceof Error ? e.message : 'Failed to create item'
    childCreateSaving.value = false
  }
}

onUnmounted(() => {
  window.removeEventListener('scroll', onScroll)
  window.removeEventListener('resize', handleResize)
})
</script>

<template>
  <div class="p-6 pb-20">
    <!-- Back Navigation -->
    <router-link
      :to="`/collections/${collectionName}/data`"
      class="inline-block mb-4 text-blue-500 text-sm hover:underline"
    >
      ← Back to {{ collectionName }}
    </router-link>

    <!-- Loading State -->
    <div v-if="loading" class="flex flex-col items-center justify-center py-16 gap-3">
      <svg class="animate-spin h-8 w-8 text-blue-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>
      </svg>
      <span class="text-gray-500 text-sm">Loading record...</span>
    </div>

    <!-- Error / Not Found State -->
    <div v-else-if="error" class="text-center py-16">
      <div class="bg-red-50 border border-red-200 rounded-lg p-6 max-w-md mx-auto">
        <h2 class="text-lg font-semibold text-red-700 mb-2">Record not found</h2>
        <p class="text-red-600 text-sm mb-4">{{ error }}</p>
        <Button label="Retry" severity="primary" @click="fetchRecord" />
      </div>
    </div>

    <!-- Record Display -->
    <div v-else-if="item" class="animate-fade-in">
      <div class="flex items-center justify-between mb-4">
        <h1 class="text-2xl font-bold text-gray-900">Record Detail</h1>
        <Button v-if="!showSidebar" icon="pi pi-bars" text severity="secondary" @click="showSidebar = true" class="lg:hidden" />
      </div>

      <!-- Sidebar toggle overlay for mobile -->
      <div v-if="showSidebar && !isLargeScreen" class="fixed inset-0 bg-black/30 z-40 lg:hidden" @click="showSidebar = false" />

      <div class="flex flex-col lg:flex-row gap-6 relative">
        <!-- Left Sidebar Navigation -->
        <aside
          class="w-full lg:w-56 flex-shrink-0"
          :class="{
            'fixed inset-y-0 left-0 z-50 bg-white shadow-xl p-4 w-64 lg:static lg:shadow-none lg:p-0 lg:bg-transparent lg:z-auto': !isLargeScreen,
            'hidden lg:block': !showSidebar && !isLargeScreen,
            'block': showSidebar || isLargeScreen,
          }"
        >
            <div class="lg:sticky lg:top-6 lg:self-start space-y-1">
              <div class="flex items-center justify-between mb-3 lg:hidden">
                <span class="text-sm font-semibold text-gray-700">Sections</span>
                <Button icon="pi pi-times" text severity="secondary" size="small" @click="showSidebar = false" />
              </div>

              <div v-if="availableLayouts.length > 1" class="px-3 py-2 border-b border-gray-200 mb-2">
                <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">Layout</div>
                <div class="flex flex-wrap gap-1">
                  <button v-for="l in availableLayouts" :key="l.id"
                    class="text-xs px-2 py-1 rounded transition-colors"
                    :class="l.id === activeLayoutId ? 'bg-blue-100 text-blue-700 font-medium' : 'text-gray-500 hover:bg-gray-100'"
                    @click="switchLayout(l.id)">{{ l.name }}</button>
                </div>
              </div>

              <template v-for="(group, gIdx) in sidebarSectionGroups" :key="gIdx">
              <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider px-3 pt-3 pb-1">{{ group.label }}</div>
              <button
                v-for="section in group.items"
                :key="section.id"
                class="w-full text-left px-3 py-2 rounded-md text-sm transition-colors flex items-center justify-between"
                :class="activeSection === section.id ? 'bg-blue-50 text-blue-700 font-medium' : 'text-gray-600 hover:bg-gray-100'"
                @click="scrollToSection(section.id)"
              >
                <span>{{ section.label }}</span>
                <span v-if="section.type === 'relational' && section.count !== null"
                  class="text-xs bg-gray-100 text-gray-500 px-1.5 py-0.5 rounded-full min-w-[1.5rem] text-center">
                  {{ section.count }}
                </span>
              </button>
            </template>
          </div>
        </aside>

        <!-- Main Content -->
        <div class="flex-1 min-w-0">
          <!-- Tab Bar -->
          <div class="flex gap-1 mb-4 border-b border-gray-200">
            <button v-for="t in tabItems" :key="t.id"
              class="px-4 py-2 text-sm font-medium rounded-t-lg transition-colors border-b-2 -mb-px"
              :class="activeTab === t.id ? 'border-blue-500 text-blue-700' : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300'"
              @click="activeTab = t.id"
            >{{ t.label }}</button>
          </div>

          <div v-show="activeTab === 'details'">
          <!-- Sections rendered in ordinal order (field_group + relational interleaved) -->
          <template v-for="section in orderedDetailSections" :key="section.id">

            <!-- Field Group Section -->
            <section v-if="section.section_type === 'field_group'" :id="`section-${section.id}`" class="scroll-mt-6">
              <h2 class="text-lg font-semibold text-gray-800 mb-3">{{ section.name }}</h2>
              <div v-if="section._columns === 2" class="flex gap-6">
                <div class="flex-1 min-w-0 bg-white rounded-lg shadow-sm">
                  <div class="text-xs font-medium text-gray-400 uppercase tracking-wider px-4 py-2 border-b border-gray-100">Column 1</div>
                  <div class="divide-y divide-gray-100">
                    <div v-for="field in getColumnFields(section, 0)" :key="field.name" class="py-3 px-4">
                      <dt class="text-xs font-medium text-gray-500 mb-1"><FieldNameLabel :field="field" /></dt>
                      <dd>
                        <template v-if="!isEditing">
                          <component :is="(DISPLAY_COMPONENTS as any)[field.type]"
                            :value="item[field.name]"
                            :related-collection="field.related_collection"
                            :related-field="field.name"
                            :display-value="item[field.name + '__display_value']"
                          />
                        </template>
                        <div v-else class="space-y-1">
                          <FormFieldRenderer :collection-name="collectionName" :field-name="field.name"
                            v-model="editValues[field.name]" :invalid="errors[field.name] || false"
                            :readonly="!canEditField(field.name)"
                            :inline-create="isEditing" />
                        </div>
                      </dd>
                    </div>
                  </div>
                </div>
                <div class="flex-1 min-w-0 bg-white rounded-lg shadow-sm">
                  <div class="text-xs font-medium text-gray-400 uppercase tracking-wider px-4 py-2 border-b border-gray-100">Column 2</div>
                  <div class="divide-y divide-gray-100">
                    <div v-for="field in getColumnFields(section, 1)" :key="field.name" class="py-3 px-4">
                      <dt class="text-xs font-medium text-gray-500 mb-1"><FieldNameLabel :field="field" /></dt>
                      <dd>
                        <template v-if="!isEditing">
                          <component :is="(DISPLAY_COMPONENTS as any)[field.type]"
                            :value="item[field.name]"
                            :related-collection="field.related_collection"
                            :related-field="field.name"
                            :display-value="item[field.name + '__display_value']"
                          />
                        </template>
                        <div v-else class="space-y-1">
                          <FormFieldRenderer :collection-name="collectionName" :field-name="field.name"
                            v-model="editValues[field.name]" :invalid="errors[field.name] || false"
                            :readonly="!canEditField(field.name)"
                            :inline-create="isEditing" />
                        </div>
                      </dd>
                    </div>
                  </div>
                </div>
              </div>
              <div v-else class="bg-white rounded-lg shadow-sm divide-y divide-gray-100">
                <div v-for="field in getDetailSectionFields(section)" :key="field.name" class="py-3 px-4">
                  <dt class="text-xs font-medium text-gray-500 mb-1"><FieldNameLabel :field="field" /></dt>
                  <dd>
                    <template v-if="!isEditing">
                      <component :is="(DISPLAY_COMPONENTS as any)[field.type]"
                        :value="item[field.name]"
                        :related-collection="field.related_collection"
                        :related-field="field.name"
                        :display-value="item[field.name + '__display_value']"
                      />
                    </template>
                    <div v-else class="space-y-1">
                      <FormFieldRenderer :collection-name="collectionName" :field-name="field.name"
                        v-model="editValues[field.name]" :invalid="errors[field.name] || false"
                        :readonly="!canEditField(field.name)"
                        :inline-create="isEditing" />
                    </div>
                  </dd>
                </div>
              </div>
            </section>

            <!-- Relational Section -->
            <section v-else-if="shouldShowSection(section)" :id="`section-${section.id}`" class="scroll-mt-6 mt-8">
              <!-- Namespaced relations: reusable component (RecordForm popup, deferred flush in edit mode) -->
              <RelationalSection
                v-if="isNamespacedRelational(section)"
                :ref="(el: any) => setRelSectionRef(section.id, el)"
                :section="section"
                :parent-collection-name="collectionName"
                :parent-item="item"
                :parent-fields="fields"
                :deferred="isEditing"
                @count="(n: number) => { sectionTotal[section.id] = n }"
              />
              <template v-else>
              <div class="flex items-center justify-between mb-3">
                <h2 class="text-lg font-semibold text-gray-800">{{ section.name }}</h2>
                <Button
                  v-if="isEditing || childCreatePermission[section.id] !== false"
                  :label="isEditing ? 'Add Row' : 'Add'"
                  :icon="isEditing ? '' : 'pi pi-plus'"
                  severity="primary"
                  size="small"
                  @click="isEditing ? addChildInlineRow(section) : openChildCreateDialog(section, null)"
                />
              </div>
              <div>
                <div v-if="sectionLoading[section.id]" class="flex items-center justify-center py-8">
                  <svg class="animate-spin h-6 w-6 text-blue-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>
                  </svg>
                </div>

              <!-- Error state -->
              <div v-else-if="sectionError[section.id]" class="text-red-500 text-sm py-4">
                <p>{{ sectionError[section.id] }}</p>
                <Button label="Retry" severity="warn" size="small" @click="loadSectionData(section)" />
              </div>

              <!-- View component -->
              <component
                v-else-if="sectionItems[section.id]"
                :is="sectionViewComponent(section.view_type)"
                :items="sectionItems[section.id]"
                :fields="sectionViewFields(section)"
                :collection-name="getSectionChildCollection(section)"
                :loading="false"
                :error="null"
                :total="sectionTotal[section.id] || 0"
                :page="sectionPage[section.id] || 1"
                :per-page="section.item_limit || 25"
                sort-field=""
                sort-order="asc"
                :filters="{}"
                :system-fields="['id']"
                :embedded="!isEditing"
                :enable-expand="false"
                :nested-depth="getSectionNestedDepth(section)"
                :current-depth="0"
                :collection-fields="sectionFields[section.id] || []"
                :expanded-row-data="sectionExpandedData[section.id] || {}"
                :editable="isEditing"
                :child-collection-name="getSectionChildCollectionName(section)"
                :parent-fk-field-name="findParentFKFieldName(section)"
                :edit-values="childEditDrafts[section.id] || {}"
                @update:page="(p: number) => loadSectionPage(section, p)"
                @row-expand="(item: any) => handleRowExpand(section, item)"
                @edit-item="(itm: any) => openChildCreateDialog(section, itm)"
                @cell-edit="(row: any, fieldName: string, value: any) => onChildCellEdit(section, row, fieldName, value)"
                @delete-item="(row: any) => onChildDelete(section, row)"
              />

              <!-- Empty state -->
              <div v-else class="text-gray-400 text-sm py-4 text-center">
                No related items found.
              </div>
            </div>
              </template>
          </section>
          </template>

          <!-- Inline Parent Fields (Phase 75) -->
          <div v-if="inlineParentFields.length > 0" class="mt-6">
            <template v-for="pf in inlineParentFields" :key="pf.fieldName">
              <div class="border border-blue-200 bg-blue-50/30 rounded-lg p-4 mb-4">
                <div class="flex items-center gap-2 mb-3">
                  <span class="text-xs font-semibold text-blue-700 uppercase tracking-wider">Parent: {{ pf.fieldName }}</span>
                  <i class="pi pi-arrow-right text-blue-400 text-xs"></i>
                  <span class="text-xs text-blue-500">{{ pf.relatedCollection }}</span>
                </div>
                <div class="space-y-2">
                  <div v-for="parentField in pf.fields" :key="parentField.name" class="py-1">
                    <dt class="text-xs font-medium text-gray-500 mb-0.5">{{ parentField.name }}</dt>
                    <dd class="text-sm">
                      <template v-if="!isEditing">
                        <span>{{ parentField.value ?? '—' }}</span>
                      </template>
                      <template v-else>
                        <InputText
                          v-model="inlineParentEditValues[`__parent__${pf.fieldName}__${parentField.name}`]"
                          class="w-full"
                          fluid
                        />
                      </template>
                    </dd>
                  </div>
                </div>
              </div>
            </template>
          </div>

          <!-- System Fields Section -->
          <div v-if="systemFieldKeys.length > 0 && !isEditing" class="mt-6">
            <h2 class="text-sm font-semibold text-gray-500 uppercase mb-3">System Fields</h2>
            <div class="bg-white rounded-lg shadow-sm divide-y divide-gray-100">
              <div v-for="sysField in systemFieldKeys" :key="sysField" class="py-3 px-4">
                <dt class="text-xs font-medium text-gray-500 mb-1"><FieldNameLabel :field="makeSystemField(sysField)" /></dt>
                <dd>
                  <code v-if="sysField === 'id'" class="text-xs text-gray-500 font-mono">{{ item[sysField] }}</code>
                  <DateTimeDisplay v-else :value="item[sysField]" />
                </dd>
              </div>
            </div>
          </div>
          </div>

          <!-- Activity Tab -->
          <div v-show="activeTab === 'activity'">
          <section id="section-activity" class="scroll-mt-6">
            <h2 class="text-lg font-semibold text-gray-800 mb-3">Activity</h2>
            <div class="bg-white rounded-lg shadow-sm border border-gray-200 p-4">
              <ActivityTimeline
                :collection-name="collectionName"
                :item-id="itemId"
              />
            </div>
          </section>
          </div>

          <!-- References Tab -->
          <div v-show="activeTab === 'references'">
          <section id="section-references" class="scroll-mt-6">
            <h2 class="text-lg font-semibold text-gray-800 mb-3">References</h2>
            <div class="bg-white rounded-lg shadow-sm border border-gray-200 p-4">
              <div v-if="referencesLoading" class="text-gray-400 text-sm py-4">
                Loading references...
              </div>
              <div v-else-if="referencesError" class="text-red-500 text-sm py-4">
                <p>{{ referencesError }}</p>
                <Button label="Retry" severity="warn" size="small" @click="loadReferences" />
              </div>
              <div v-else-if="references.length === 0" class="text-gray-400 text-sm py-4">
                No items reference this item.
              </div>
              <div v-else class="space-y-4">
                <div v-for="group in references" :key="group.collection_name + '-' + group.field_name" class="border border-gray-200 rounded-md p-3">
                  <div class="flex items-center justify-between mb-2">
                    <h4 class="text-sm font-medium text-gray-700">
                      <router-link :to="`/collections/${group.collection_name}/data`" class="text-blue-500 hover:underline">
                        {{ group.collection_name }}
                      </router-link>
                      <span class="text-gray-400 mx-1">·</span>
                      <span class="text-gray-500 text-xs">via {{ group.field_name }}</span>
                      <span class="ml-2 inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium"
                        :class="group.relationship_type === 'one_to_one' ? 'bg-purple-100 text-purple-800' : 'bg-blue-100 text-blue-800'"
                      >
                        {{ group.relationship_type === 'one_to_one' ? '1:1' : 'M:1' }}
                      </span>
                    </h4>
                    <span class="text-xs text-gray-400">{{ group.items.length }} item{{ group.items.length !== 1 ? 's' : '' }}</span>
                  </div>
                  <div class="space-y-1">
                    <div v-for="(refItem, refIdx) in group.items" :key="refItem.id || refIdx" class="text-sm text-gray-600 flex items-center gap-2">
                      <router-link :to="`/detail/${group.collection_name}/${refItem.id}`" class="text-blue-500 hover:underline font-mono text-xs truncate">
                        {{ refItem.id }}
                      </router-link>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </section>
          </div>

          <!-- Inline Child Create/Edit Dialog -->
          <Dialog
            v-model:visible="childCreateDialogVisible"
            :header="childCreateEditItem ? `Edit ${childCreateSection?.name || 'Record'}` : `Add ${childCreateSection?.name || 'Record'}`"
            :modal="true"
            :style="{ width: '500px' }"
            :draggable="false"
            :closable="!childCreateSaving"
          >
            <!-- Loading state if fields not yet loaded -->
            <div v-if="childCreateLoadingFields" class="text-center py-8 text-gray-500 text-sm">
              Loading fields...
            </div>

            <template v-else>
              <div class="space-y-3">
                <!-- Render each form field per child field type -->
                <div v-for="field in childCreateFormFields" :key="field.name">
                  <label :for="'cc-' + field.name" class="block mb-1 text-sm font-medium text-gray-700">
                    <FieldNameLabel :field="field" />
                    <span v-if="field.required" class="text-red-500 ml-0.5">*</span>
                  </label>

                  <!-- Parent FK field: hidden, show auto-link note -->
                  <div v-if="childCreateParentFKField?.name === field.name" class="text-sm text-blue-600 bg-blue-50 rounded px-3 py-2 border border-blue-200">
                    (auto-linked to parent record)
                  </div>

                  <FormFieldRenderer
                    v-else
                    :collection-name="childCreateCollectionName"
                    :field-name="field.name"
                    v-model="childCreateValues[field.name]"
                    :invalid="childCreateErrors[field.name] || false"
                  />

                  <p v-if="childCreateErrors[field.name]" class="mt-1 text-xs text-red-500">
                    {{ childCreateErrors[field.name] }}
                  </p>
                </div>

                <!-- API error -->
                <div v-if="childCreateSaveError" class="text-sm text-red-500 bg-red-50 border border-red-200 rounded p-3">
                  {{ childCreateSaveError }}
                </div>
              </div>
            </template>

            <template #footer>
              <div class="flex gap-2 justify-end">
                <Button
                  label="Cancel"
                  severity="secondary"
                  :disabled="childCreateSaving"
                  @click="closeChildCreateDialog"
                />
                <Button
                  :label="childCreateEditItem ? 'Update' : 'Save'"
                  :loading="childCreateSaving"
                  @click="saveChildCreate"
                />
              </div>
            </template>
          </Dialog>

        </div>
      </div>
    </div>

    <!-- Confirmation Dialog for Cross-Collection Save -->
    <Dialog :visible="showParentConfirm" @update:visible="v => { if (!v) showParentConfirm = false }" header="Update Parent Record?" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <p class="text-sm text-gray-600">You are about to update fields on the parent record. This change will be saved in the same transaction as the child record update. Do you want to continue?</p>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showParentConfirm = false" />
        <Button label="Continue Saving" severity="primary" @click="doSave" />
      </template>
    </Dialog>

    <!-- Delete Confirmation Dialog -->
    <Dialog v-model:visible="showDeleteDialog" header="Confirm Delete" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <p class="text-gray-600">Are you sure you want to delete this item?</p>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined :disabled="deletingItem" @click="showDeleteDialog = false" />
        <Button label="Delete" severity="danger" :loading="deletingItem" @click="handleDeleteConfirmed" />
      </template>
    </Dialog>

    <!-- Floating Action Buttons -->
    <div class="fixed bottom-6 right-6 flex gap-3 z-50">
      <!-- View mode: Edit + Delete buttons -->
      <Button
        v-if="!isEditing && item && item.$permissions?.update !== false"
        label="Edit"
        icon="pi pi-pencil"
        severity="info"
        @click="enterEditMode"
      />
      <Button
        v-if="!isEditing && item && item.$permissions?.delete !== false"
        label="Delete"
        icon="pi pi-trash"
        severity="danger"
        @click="confirmDelete"
      />
      <!-- Edit mode: Cancel + Save buttons -->
      <template v-else-if="isEditing">
        <Button
          label="Cancel"
          severity="secondary"
          :disabled="saving"
          @click="cancelEdit"
        />
        <Button
          label="Save"
          icon="pi pi-check"
          :loading="saving"
          @click="saveEdit"
        />
      </template>
    </div>
  </div>
</template>

<style scoped>
.animate-fade-in {
  animation: fadeIn 0.2s ease-out;
}
@keyframes fadeIn {
  from { opacity: 0; transform: translateY(4px); }
  to { opacity: 1; transform: translateY(0); }
}
</style>