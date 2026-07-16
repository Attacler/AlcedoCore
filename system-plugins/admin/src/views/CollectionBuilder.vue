<script setup lang="ts">
import { ref, computed, onMounted, nextTick, watch, markRaw } from 'vue'
import type { Component } from 'vue'
import { useRoute } from 'vue-router'
import { useCollectionsStore, type FieldDefinition, type FieldType, type CollectionSection, type Collection, type CollectionLayout } from '@/stores/collections'
import { useRolesStore } from '@/stores/rolesStore'
import FieldNameLabel from '@/components/FieldNameLabel.vue'
import CollectionNameLabel from '@/components/CollectionNameLabel.vue'
import { useToast } from '@/composables/useToast'
import { useExtensionRegistryStore } from '@/stores/extensionRegistry'
import { getDisplayTypeGroups, getDisplayType, DISPLAY_TYPE_REGISTRY } from '@/stores/displayTypeRegistry'
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import Checkbox from 'primevue/checkbox'
import Dialog from 'primevue/dialog'
import InputNumber from 'primevue/inputnumber'
import ToggleSwitch from 'primevue/toggleswitch'
import Drawer from 'primevue/drawer'
import TableViewSettings from '@/components/TableViewSettings.vue'
import CardsViewSettings from '@/components/CardsViewSettings.vue'
import KanbanViewSettings from '@/components/KanbanViewSettings.vue'
import FilterBuilder from '@/components/FilterBuilder.vue'

const route = useRoute()
const store = useCollectionsStore()
const rolesStore = useRolesStore()
const toast = useToast()
const editorNameInput = ref<any>(null)

const collectionName = computed(() => route.params.name as string)
const loading = ref(true)
const loadError = ref<string | null>(null)
const saving = ref(false)
const fields = ref<(FieldDefinition & { _key: string })[]>([])
const allCollections = ref<{ name: string }[]>([])
const editingField = ref<FieldDefinition | null>(null)
const editingFieldKey = ref<string | null>(null)
const focusKey = ref<string | null>(null)
const dropBeforeKey = ref<string | null>(null)
const isDragging = ref(false)
const sections = ref<CollectionSection[]>([])
const showSectionEditor = ref(false)
const editingSection = ref<any>(null)
const sectionTypeChoice = ref<'field_group' | 'relational' | null>(null)
const showSectionTypeDialog = ref(false)
const sectionFormData = ref<any>(null)

// Layout state
const collLayouts = ref<CollectionLayout[]>([])
const activeLayoutId = ref<string | null>(null)
const showLayoutDialog = ref(false)
const layoutDialogName = ref('')
const showLayoutRolesDialog = ref(false)
const layoutRoles = ref<{ role_id: string; role_name: string }[]>([])
const selectedLayoutRoleIds = ref<string[]>([])

/** Columns config derived from sectionFormData.default_filter */
const sectionColumns = computed({
  get: () => (sectionFormData.value as any)?._columns ?? 1,
  set: (val: number) => {
    if (sectionFormData.value) {
      (sectionFormData.value as any)._columns = val
      if (val === 2) {
        // Initialize field_columns for all fields in the section
        const fc: Record<string, number> = {}
        for (const name of (sectionFormData.value as any).display_fields || []) {
          fc[name] = (sectionFormData.value as any)._field_columns?.[name] || 1
        }
        (sectionFormData.value as any)._field_columns = fc
      }
    }
  },
})

// Section drag-and-drop reordering
const dragSectionId = ref<string | null>(null)
const dragOverSectionId = ref<string | null>(null)
const dropBeforeSectionKey = ref<string | null>(null)
const dropAfterLastSection = ref(false)
const dragOverEmptySectionId = ref<string | null>(null)

function onSectionDragStart(section: any) {
  dragSectionId.value = section.id || section._key
}

function onSectionDragOver(section: any) {
  if (!dragSectionId.value) return
  dragOverSectionId.value = section.id || section._key
  dropBeforeSectionKey.value = section.id || section._key
  dropAfterLastSection.value = false
}

function clearSectionDropIndicator(section: any) {
  if (dropBeforeSectionKey.value === (section.id || section._key)) {
    dropBeforeSectionKey.value = null
  }
}

function onDragOverEmptySection(section: any) {
  if (!dragType && !dragFieldKey) return
  dragOverEmptySectionId.value = section.id || section._key
}

function onDragLeaveEmptySection(section: any) {
  if (dragOverEmptySectionId.value === (section.id || section._key)) {
    dragOverEmptySectionId.value = null
  }
}

function onSectionDrop(target: any) {
  const dragId = dragSectionId.value
  const targetId = target.id || target._key
  if (!dragId || !targetId || dragId === targetId) {
    dragSectionId.value = null; dragOverSectionId.value = null; return
  }

  const ordered = orderedSections.value
  const fromIdx = ordered.findIndex((s: any) => (s.id || s._key) === dragId)
  const toIdx = ordered.findIndex((s: any) => (s.id || s._key) === targetId)
  if (fromIdx === -1 || toIdx === -1) {
    dragSectionId.value = null; dragOverSectionId.value = null; return
  }

  // Swap ordinal positions
  const fromSection = ordered[fromIdx]
  const toSection = ordered[toIdx]
  const tempPos = fromSection.ordinal_position
  fromSection.ordinal_position = toSection.ordinal_position
  toSection.ordinal_position = tempPos

  dragSectionId.value = null
  dragOverSectionId.value = null
  dropBeforeSectionKey.value = null
  dropAfterLastSection.value = false
}

// Collection details drawer
const showCollectionDrawer = ref(false)
const collectionMeta = ref<Collection | null>(null)
const collectionDisplayName = ref('')
const savingDetails = ref(false)

async function saveCollectionDetails() {
  if (!collectionMeta.value) return
  savingDetails.value = true
  try {
    const validFields = fields.value.filter(f => f.name && /^[a-z][a-z0-9_]*$/.test(f.name))
    const payload = validFields.map((f, i) => {
      const p: any = { name: f.name, display_name: f.display_name || null, type: f.type, required: f.required, unique: f.unique,
        default_value: f.default_value, display_type: f.display_type, ordinal_position: i + 1 }
      const a = f as any
      if (a.full_width) p.full_width = true
      if (a.related_collection) { p.related_collection = a.related_collection; p.relationship_type = a.relationship_type }
      if (a.display_field) { p.display_field = a.display_field }
      if (a.inline_parent_fields?.length > 0) { p.inline_parent_fields = a.inline_parent_fields }
      if (a.options && (Array.isArray(a.options) ? a.options.length > 0 : true)) { p.options = a.options }
      return p
    })
    await store.updateCollection(collectionMeta.value.name, { fields: payload, display_name: collectionDisplayName.value || null })
    collectionMeta.value.display_name = collectionDisplayName.value || undefined
    toast.show('Collection details saved', 'success')
    showCollectionDrawer.value = false
  } catch (e) {
    toast.show(`Failed to save: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    savingDetails.value = false
  }
}

const extensionRegistry = useExtensionRegistryStore()

let keyCounter = 0
function nextKey(): string { return `f_${++keyCounter}_${Date.now()}` }

/**
 * Palette groups combining static display type registry entries with
 * plugin-registered input widgets shown in a "Plugin" group.
 * Reactive — updates when plugins register/unregister.
 */
const paletteGroups = computed(() => {
  const groups = getDisplayTypeGroups()

  const pluginWidgets = extensionRegistry.allInputWidgets
  if (pluginWidgets.length > 0) {
    groups.push({
      label: 'Plugin',
      items: pluginWidgets.map(w => ({
        type: w.type,
        label: w.label,
        icon: '&#9881;',
        group: 'Plugin',
        dbType: w.supportedFieldTypes[0] || 'string',
        isRel: false,
        component: null, // plugin widgets use async component resolution
      })),
    })
  }

  return groups
})

const allFields = computed(() => fields.value)
const namedFields = computed(() => fields.value.filter(f => f.name && /^[a-z][a-z0-9_]*$/.test(f.name)))
const unnamedFields = computed(() => fields.value.filter(f => !f.name || !/^[a-z][a-z0-9_]*$/.test(f.name)))
const namedFieldCount = computed(() => namedFields.value.length)
const availableDisplayTypes = computed(() => {
  if (!editingField.value) return []
  const pkType = (editingField.value as any)._displayType || editingField.value.display_type || editingField.value.type
  const pkEntry = getDisplayType(pkType)

  const types: { type: string; label: string }[] = [{ type: 'default', label: 'Default' }]

  const effectiveDbType = pkEntry?.dbType || editingField.value.type

  types.push(...DISPLAY_TYPE_REGISTRY
    .filter(e => e.dbType === effectiveDbType && e.type !== pkType)
    .map(e => ({ type: e.type, label: e.label })))

  types.push(...extensionRegistry
    .getInputWidgetsForFieldType(editingField.value.type)
    .map(w => ({ type: w.type, label: w.label })))

  return types
})

let dragType: string | null = null
let dragFieldKey: string | null = null

function makeField(displayType: string, pos: number): any {
  const entry = getDisplayType(displayType)

  // Determine field type: static registry type or fall back to plugin's supported type
  let fieldType: FieldType = 'string'
  if (entry?.dbType) {
    fieldType = entry.dbType as FieldType
  } else {
    // Plugin input widget: use its first supported field type
    const pluginWidget = extensionRegistry.getInputWidget(displayType)
    if (pluginWidget && pluginWidget.supportedFieldTypes.length > 0) {
      fieldType = pluginWidget.supportedFieldTypes[0]
    }
  }

  const tmpKey = nextKey()
  const field: any = {
    _key: tmpKey,
    name: '',
    _tempName: `__new_${tmpKey}`,
    type: fieldType,
    required: false, unique: false,
    default_value: null, display_type: 'default',
    ordinal_position: pos,
    _displayType: displayType,
  }
    if (entry?.isRel) {
      field.type = 'relationship' as FieldType
      const RELATION_TYPE_MAP: Record<string, string> = {
        'relation-many-to-one': 'many_to_one',
        'relation-one-to-one': 'one_to_one',
        'relation-one-to-many': 'one_to_many',
      }
      field.relationship_type = RELATION_TYPE_MAP[displayType] || 'many_to_one'
    }
  return field
}

function openNewFieldEditor(field: any) {
  editingField.value = field
  editingFieldKey.value = field._key
  nextTick(() => { editorNameInput.value?.focus() })
}

function onDragStart(event: DragEvent, type: string) {
  dragType = type; dragFieldKey = null; isDragging.value = true
  if (event.dataTransfer) { event.dataTransfer.effectAllowed = 'copy'; event.dataTransfer.setData('text/plain', type) }
}

function onFieldDragStart(event: DragEvent, key: string) {
  dragType = null; dragFieldKey = key; isDragging.value = true
  if (event.dataTransfer) { event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData('text/plain', key) }
}

function onDragOverField(key: string) { dropBeforeKey.value = key }
function onDragLeaveField(key: string) { if (dropBeforeKey.value === key) dropBeforeKey.value = null }

function finishDrag() {
  dragType = null; dragFieldKey = null; dropBeforeKey.value = null; isDragging.value = false
  dragSectionId.value = null; dragOverSectionId.value = null
  dropBeforeSectionKey.value = null; dropAfterLastSection.value = false
  dragOverEmptySectionId.value = null
}

function onDragOverEmpty() { isDragging.value = true }
function onDragLeaveEmpty() { isDragging.value = false }

function onDropAtEnd(_event: DragEvent) {
  if (dragType) {
    const maxPos = fields.value.reduce((m, f) => Math.max(m, f.ordinal_position || 0), 0)
    const field = makeField(dragType, maxPos + 1)
    fields.value.push(field)
    finishDrag()
    openNewFieldEditor(field)
    return
  }
  if (dragFieldKey) {
    const idx = fields.value.findIndex(f => f._key === dragFieldKey)
    if (idx !== -1) { const [m] = fields.value.splice(idx, 1); fields.value.push(m) }
    finishDrag(); return
  }
  if (dragSectionId.value) {
    // Move section to the end: give it the highest ordinal_position
    const ordered = orderedSections.value
    const fromIdx = ordered.findIndex((s: any) => (s.id || s._key) === dragSectionId.value)
    if (fromIdx !== -1) {
      const maxOrdinal = ordered.reduce((m: number, s: any) => Math.max(m, s.ordinal_position || 0), 0)
      ordered[fromIdx].ordinal_position = maxOrdinal + 1
    }
  }
  finishDrag()
}

/** Find which section owns a given field name */
function findSectionForField(fieldName: string): any {
  return sections.value.find((s: any) =>
    s.section_type === 'field_group' && s.display_fields?.includes(fieldName)
  ) || null
}

function onFieldDrop(_event: DragEvent, targetKey: string) {
  // Parse targetKey to determine section and insertion reference
  // Keys: __gap_first_{sectionId}[_c0|_c1] | __gap_after_{fieldKey}
  let sectionId: string | null = null
  let refFieldName: string | null = null  // insert after this field name
  let isColumnTarget = false
  let colIdx = -1

  if (targetKey.startsWith('__gap_first_')) {
    let base = targetKey.replace('__gap_first_', '')
    const cMatch = base.match(/_c(\d)$/)
    if (cMatch) {
      colIdx = parseInt(cMatch[1])
      base = base.slice(0, -3)
    }
    sectionId = base
    isColumnTarget = true
  } else if (targetKey.startsWith('__gap_after_')) {
    const fieldKey = targetKey.replace('__gap_after_', '')
    const field = fields.value.find(f => f._key === fieldKey)
    if (field) {
      const section = findSectionForField(field.name)
      if (section) {
        sectionId = section.id || section._key
        refFieldName = field.name
      }
    }
  }

  if (!sectionId) { finishDrag(); return }

  const section = sections.value.find((s: any) => (s.id || s._key) === sectionId)
  if (!section || !section.display_fields) { finishDrag(); return }

  if (dragType) {
    const maxPos = fields.value.reduce((m, f) => Math.max(m, f.ordinal_position || 0), 0)
    const newField = makeField(dragType, maxPos + 1)
    fields.value.push(newField)

    if (!section._field_columns) section._field_columns = {}
    const insertPos = isColumnTarget
      ? (colIdx === 0
        ? 0
        : section.display_fields.findIndex((n: string) => (section._field_columns || {})[n] === 2))
      : refFieldName ? section.display_fields.indexOf(refFieldName) + 1 : -1

    const col = isColumnTarget ? colIdx + 1 : (refFieldName ? section._field_columns[refFieldName] || 1 : 1)
    section._field_columns[newField._tempName] = col

    if (insertPos >= 0 && insertPos <= section.display_fields.length) {
      section.display_fields.splice(insertPos, 0, newField._tempName)
    } else {
      section.display_fields.push(newField._tempName)
    }

    finishDrag()
    openNewFieldEditor(newField)
    return
  }

  if (dragFieldKey) {
    const movedField = fields.value.find(f => f._key === dragFieldKey)
    if (!movedField) { finishDrag(); return }

    const sourceSection = findSectionForField(movedField.name)

    // Remove from source section first
    if (sourceSection?.display_fields) {
      const idx = sourceSection.display_fields.indexOf(movedField.name)
      if (idx >= 0) sourceSection.display_fields.splice(idx, 1)
    }

    // Ensure field_columns exists
    if (!section._field_columns) section._field_columns = {}

    // Compute insertion index from the post-removal display_fields array
    let insertPos: number | null = null
    if (isColumnTarget) {
      if (colIdx === 0) {
        insertPos = 0
      } else {
        const firstCol2 = section.display_fields.findIndex(
          (n: string) => (section._field_columns ?? {})[n] === 2
        )
        insertPos = firstCol2 >= 0 ? firstCol2 : section.display_fields.length
      }
      // Set field column
      if (!section._field_columns) section._field_columns = {}
      section._field_columns[movedField.name] = colIdx + 1
    } else if (refFieldName) {
      const pos = section.display_fields.indexOf(refFieldName)
      insertPos = pos >= 0 ? pos + 1 : null
      // Inherit column from reference field
      if (!section._field_columns) section._field_columns = {}
      section._field_columns[movedField.name] = section._field_columns[refFieldName] || 1
    }

    // Insert at computed position
    if (!section.display_fields.includes(movedField.name)) {
      if (insertPos !== null && insertPos <= section.display_fields.length) {
        section.display_fields.splice(insertPos, 0, movedField.name)
      } else {
        section.display_fields.push(movedField.name)
      }
    }
  }
  finishDrag()
}

function isType(field: any, types: string[]): boolean {
  return types.includes(field._displayType || field.type)
}

function fieldTypeLabel(field: any): string {
  const dt = field._displayType || field.type
  const entry = getDisplayType(dt)
  if (entry) return entry.label
  const pluginWidget = extensionRegistry.getInputWidget(dt)
  return pluginWidget?.label || dt
}

function defaultValuePlaceholder(type: string): string {
  if (['int', 'long-int'].includes(type)) return '0'
  if (['float', 'number', 'decimal', 'currency', 'percent'].includes(type)) return '0.0'
  if (['datetime', 'date', 'date/time'].includes(type)) return 'Now'
  if (type === 'uuid') return '(auto-generated)'
  return ''
}

function isRelType(type: string): boolean {
  return ['relationship', 'lookup', 'multi-select-lookup'].includes(type)
}

function onDraftNameInput(key: string, event: Event) {
  const f = fields.value.find(f => f._key === key)
  if (f) f.name = (event.target as HTMLInputElement).value.toLowerCase()
}

function commitDraft(key: string) {
  const f = fields.value.find(f => f._key === key)
  if (!f) return
  if (!f.name || !/^[a-z][a-z0-9_]*$/.test(f.name)) {
    toast.show('Field name is required — use only lowercase letters, numbers, and underscores', 'error')
    return
  }
  // Replace temp name with real name in all section references
  const tempName = f._tempName
  if (tempName) {
    for (const section of sections.value) {
      if (section.section_type === 'field_group' && section.display_fields) {
        const idx = section.display_fields.indexOf(tempName)
        if (idx !== -1) {
          section.display_fields[idx] = f.name
        }
        if (section._field_columns && section._field_columns[tempName] !== undefined) {
          section._field_columns[f.name] = section._field_columns[tempName]
          delete section._field_columns[tempName]
        }
      }
    }
    delete f._tempName
  }
  openNewFieldEditor(f)
  toast.show(`Field "${f.name}" added`, 'success')
}

function discardDraft(key: string) {
  const idx = fields.value.findIndex(f => f._key === key)
  if (idx !== -1) fields.value.splice(idx, 1)
}

const currentSettingsComponent = computed(() => {
  if (!editingField.value) return null
  const dt = editingField.value.display_type || 'default'
  const entry = getDisplayType(dt)
  if (entry?.settingsComponent) return entry.settingsComponent
  const pluginWidget = extensionRegistry.getInputWidget(dt)
  if (pluginWidget?.settingsComponent) return pluginWidget.settingsComponent
  return null
})

function onDisplayTypeChange(value: string) {
  if (!editingField.value) return
  editingField.value.display_type = value === 'default' ? undefined : value
}

function openFieldEditor(field: any) {
  editingField.value = field; editingFieldKey.value = field._key
  if (field.type === 'file') {
    if (!field.options) field.options = {}
    if (field.options.multiple === undefined) field.options.multiple = false
    if (field.options.max_file_size === undefined) field.options.max_file_size = 10485760
    if (!field.options.allowed_mime_types) field.options.allowed_mime_types = []
  }
}

function replaceTempName(field: any) {
  const tempName = field._tempName
  if (!tempName || !field.name || !/^[a-z][a-z0-9_]*$/.test(field.name)) return
  for (const section of sections.value) {
    if (section.section_type === 'field_group' && section.display_fields) {
      const idx = section.display_fields.indexOf(tempName)
      if (idx !== -1) {
        section.display_fields[idx] = field.name
      }
      if (section._field_columns && section._field_columns[tempName] !== undefined) {
        section._field_columns[field.name] = section._field_columns[tempName]
        delete section._field_columns[tempName]
      }
    }
  }
  delete field._tempName
}

function closeFieldEditor() {
  if (editingField.value) replaceTempName(editingField.value)
  editingField.value = null; editingFieldKey.value = null
}

function onEditName(event: Event) { if (editingField.value) editingField.value.name = (event.target as HTMLInputElement).value.toLowerCase() }
function onEditDisplayName(event: Event) { if (editingField.value) { const v = (event.target as HTMLInputElement).value; editingField.value.display_name = (v || null) as string | undefined } }
function onEditDefault(event: Event) { if (editingField.value) { const v = (event.target as HTMLInputElement).value; editingField.value.default_value = v === '' ? null : v } }

function deleteEditingField() {
  if (!editingField.value) return
  const idx = fields.value.findIndex(f => f === editingField.value)
  if (idx !== -1) fields.value.splice(idx, 1)
  closeFieldEditor()
}

const fieldNameError = computed(() => {
  if (!editingField.value) return false
  if (!editingField.value.name) return false
  return !/^[a-z][a-z0-9_]*$/.test(editingField.value.name)
})

async function loadCollection(name: string) {
  loading.value = true; loadError.value = null
  try {
    const c = await store.getCollection(name)
    collectionMeta.value = c
    collectionDisplayName.value = c.display_name || ''
    fields.value = (c.fields || []).map((f: FieldDefinition) => ({ ...f, _key: nextKey() }))
  } catch (e) { loadError.value = e instanceof Error ? e.message : 'Failed to load collection' }
  finally { loading.value = false }
}

onMounted(async () => {
  try { await loadCollection(collectionName.value) } catch (e) { console.warn('[CollectionBuilder] Failed to load collection', e) }
  try { allCollections.value = await store.fetchCollectionsLight() } catch (e) { console.warn('[CollectionBuilder] Failed to fetch collections light', e) }
  try { await loadLayouts() } catch (e) { console.warn('[CollectionBuilder] loadLayouts error', e) }
  if (collLayouts.value.length > 0 && !activeLayoutId.value) {
    activeLayoutId.value = collLayouts.value[0].id
  }
  await loadSections()
})

const BUILTIN_VIEW_SETTINGS: Record<string, Component> = {
  table: markRaw(TableViewSettings),
  cards: markRaw(CardsViewSettings),
  kanban: markRaw(KanbanViewSettings),
}

const childCollectionFields = ref<FieldDefinition[]>([])

const currentViewSettingsComponent = computed(() => {
  if (!sectionFormData.value?.relation_field) return null
  const vt = sectionFormData.value.view_type || 'table'
  if (BUILTIN_VIEW_SETTINGS[vt]) return BUILTIN_VIEW_SETTINGS[vt]
  const extReg = useExtensionRegistryStore()
  const pluginView = extReg.getViewType(vt)
  if (pluginView?.settingsComponent) return pluginView.settingsComponent
  return null
})

watch(() => sectionFormData.value?.relation_field, async (rf) => {
  if (!rf) { childCollectionFields.value = []; return }
  const f = fields.value.find(f => f.name === rf)
  if (!f?.related_collection) { childCollectionFields.value = []; return }
  try {
    const coll = await store.getCollection(f.related_collection)
    childCollectionFields.value = coll.fields || []
  } catch (e) { console.warn('[CollectionBuilder] Failed to load child collection fields', e); childCollectionFields.value = [] }
})

function onViewSettingsChange(key: string, value: any) {
  if (!sectionFormData.value) return
  if (!sectionFormData.value.view_settings) sectionFormData.value.view_settings = {}
  sectionFormData.value.view_settings[key] = value
}

function getChildCollectionName(section: any): string {
  if (!section?.relation_field) return ''
  const f = fields.value.find(f => f.name === section.relation_field)
  return f?.related_collection || ''
}

const relationFieldOptions = computed(() => {
  return namedFields.value
    .filter(f => f.type === 'relationship')
    .map(f => ({ label: `${f.display_name || f.name} → ${f.related_collection || '?'}`, value: f.name }))
})

const orderedSections = computed(() => {
  return [...sections.value].sort((a, b) => (a.ordinal_position || 0) - (b.ordinal_position || 0))
})

function getSectionFields(section: any): any[] {
  if (section.section_type !== 'field_group' || !section.display_fields) return []
  const fieldNames = section.display_fields as string[]
  return fields.value
    .filter(f => f.name && /^[a-z][a-z0-9_]*$/.test(f.name) && fieldNames.includes(f.name))
    .sort((a, b) => {
      const aIdx = fieldNames.indexOf(a.name)
      const bIdx = fieldNames.indexOf(b.name)
      return aIdx - bIdx
    })
}

/** Flatten fields into a list with interleaved gap zone items for drag-and-drop. */
function flatSectionFields(section: any): any[] {
  const sectionFields = getSectionFields(section)
  const sectionId = section.id || section._key
  const result: any[] = []
  result.push({ _key: `__gap_first_${sectionId}`, _isGap: true })
  for (const field of sectionFields) {
    result.push(field)
    result.push({ _key: `__gap_after_${field._key}`, _isGap: true })
  }
  return result
}

/** Flatten fields for a single column (0 = left, 1 = right) in a 2-column section. */
function flatColumnFields(section: any, colIdx: number): any[] {
  const fieldColumns = section._field_columns || {}
  const allFields = getSectionFields(section)
  const colFields = allFields.filter(f => {
    const col = fieldColumns[f.name]
    return col === undefined ? colIdx === 0 : col === colIdx + 1
  })
  const sectionId = section.id || section._key
  const result: any[] = []
  result.push({ _key: `__gap_first_${sectionId}_c${colIdx}`, _isGap: true })
  for (const field of colFields) {
    result.push(field)
    result.push({ _key: `__gap_after_${field._key}`, _isGap: true })
  }
  return result
}

async function loadLayouts() {
  try {
    const resp = await fetch('/api/collections/' + collectionName.value + '/layouts', { credentials: 'include' })
    const json = await resp.json()
    const raw = json.layouts || []
    collLayouts.value = raw
    if (raw.length > 0 && !activeLayoutId.value) {
      activeLayoutId.value = raw[0].id
    } else if (raw.length === 0) {
      activeLayoutId.value = null
    } else if (activeLayoutId.value && !raw.find(l => l.id === activeLayoutId.value)) {
      activeLayoutId.value = raw[0].id
    }
  } catch (e) {
    console.warn('[CollectionBuilder] Failed to load layouts', e)
    collLayouts.value = []
  }
}



async function loadSections() {
  if (!activeLayoutId.value) { sections.value = []; return }
  try {
    const raw = await store.listLayoutSections(collectionName.value, activeLayoutId.value)
    sections.value = raw.map((s: any) => {
      let _columns = 1
      let _field_columns: Record<string, number> = {}
      if (s.default_filter && typeof s.default_filter === 'object') {
        _columns = s.default_filter._columns || 1
        if (s.default_filter._field_columns) {
          _field_columns = s.default_filter._field_columns
        } else if (s.default_filter._column_split && s.display_fields) {
          const split = s.default_filter._column_split
          s.display_fields.forEach((name: string, i: number) => {
            _field_columns[name] = i < split ? 1 : 2
          })
        }
      }
      return {
        ...s,
        section_type: s.section_type || 'relational',
        display_fields: s.display_fields || [],
        _columns,
        _field_columns,
      }
    })
  } catch (e) {
    console.warn('[CollectionBuilder] Failed to load sections', e)
    sections.value = []
  }
}

function onLayoutChange() {
  loadSections()
}

async function openCreateLayoutDialog() {
  layoutDialogName.value = ''
  showLayoutDialog.value = true
}

async function saveLayout() {
  if (!layoutDialogName.value.trim()) return
  try {
    await store.createLayout(collectionName.value, layoutDialogName.value.trim())
    showLayoutDialog.value = false
    layoutDialogName.value = ''
    await loadLayouts()
    activeLayoutId.value = collLayouts.value[collLayouts.value.length - 1]?.id || null
    if (activeLayoutId.value) await loadSections()
    toast.show('Layout created', 'success')
  } catch (e) {
    toast.show(`Failed to create layout: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  }
}

async function deleteCurrentLayout() {
  if (!activeLayoutId.value) return
  if (!confirm('Delete this layout? All sections in this layout will be removed.')) return
  try {
    await store.deleteLayout(collectionName.value, activeLayoutId.value)
    await loadLayouts()
    if (activeLayoutId.value) await loadSections()
    toast.show('Layout deleted', 'success')
  } catch (e) {
    toast.show(`Failed to delete layout: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  }
}

async function openLayoutRolesDialog() {
  if (!activeLayoutId.value) return
  try {
    await rolesStore.fetchRoles()
    const assigned = await store.getLayoutRoles(collectionName.value, activeLayoutId.value)
    layoutRoles.value = assigned
    selectedLayoutRoleIds.value = assigned.map((r: any) => r.role_id)
    showLayoutRolesDialog.value = true
  } catch (e) {
    toast.show(`Failed to load roles: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  }
}

async function saveLayoutRoles() {
  if (!activeLayoutId.value) return
  try {
    await store.setLayoutRoles(collectionName.value, activeLayoutId.value, selectedLayoutRoleIds.value)
    showLayoutRolesDialog.value = false
    toast.show('Layout roles assigned', 'success')
  } catch (e) {
    toast.show(`Failed to save layout roles: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  }
}

function toggleLayoutRole(roleId: string) {
  const idx = selectedLayoutRoleIds.value.indexOf(roleId)
  if (idx >= 0) {
    selectedLayoutRoleIds.value.splice(idx, 1)
  } else {
    selectedLayoutRoleIds.value.push(roleId)
  }
}

function openNewSectionEditor() {
  if (!sectionTypeChoice.value) return
  showSectionTypeDialog.value = false
  const maxPos = sections.value.reduce((m: number, s: any) => Math.max(m, s.ordinal_position || 0), 0)
  sectionFormData.value = {
    _key: `new_${Date.now()}`,
    name: '',
    section_type: sectionTypeChoice.value,
    relation_field: null,
    view_type: 'table',
    display_fields: [],
    item_limit: 25,
    ordinal_position: maxPos + 1,
    _columns: 1,
    _field_columns: {},
    view_settings: {},
    section_filter: null,
    visibility_parent: null,
    visibility_child: null,
  }
  sectionTypeChoice.value = null
  showSectionEditor.value = true
}

function editSection(section: any) {
  const df = section.default_filter || {}
  sectionFormData.value = {
    ...section,
    _key: section.id || `edit_${Date.now()}`,
    view_settings: df.view_settings || {},
    section_filter: df.filter || null,
    visibility_parent: df.section_visibility?.parent || null,
    visibility_child: df.section_visibility?.child || null,
  }
  showSectionEditor.value = true
  // Load child fields for settings/filter
  if (section.relation_field) {
    const f = fields.value.find(f => f.name === section.relation_field)
    if (f?.related_collection) {
      store.getCollection(f.related_collection).then(coll => {
        childCollectionFields.value = coll.fields || []
      }).catch((e: any) => { console.warn('[CollectionBuilder] Failed to load child collection fields in editSection', e); childCollectionFields.value = [] })
    }
  }
}

function closeSectionEditor() {
  showSectionEditor.value = false
  sectionFormData.value = null
}

async function saveSection() {
  if (!sectionFormData.value || !sectionFormData.value.name) return
  try {
    const fg = sectionFormData.value
    const payload: any = {
      name: fg.name,
      section_type: fg.section_type,
      display_fields: fg.section_type === 'field_group' ? (fg.display_fields || []) : null,
      relation_field: fg.section_type === 'relational' ? fg.relation_field || '' : null,
      view_type: fg.section_type === 'relational' ? (fg.view_type || 'table') : null,
      item_limit: fg.section_type === 'relational' ? (fg.item_limit || 25) : null,
      ordinal_position: fg.ordinal_position,
    }
    // Store layout/filter in default_filter
    if (fg.section_type === 'field_group') {
      const cols = (fg as any)._columns
      const fieldColumns = (fg as any)._field_columns || {}
      if (cols && cols > 1) {
        payload.default_filter = { _columns: cols, _field_columns: fieldColumns }
      } else {
        payload.default_filter = null
      }
    } else {
      const df: any = {}
      if (fg.view_settings && Object.keys(fg.view_settings).length > 0) {
        df.view_settings = fg.view_settings
      }
      if (fg.section_filter) {
        df.filter = fg.section_filter
      }
      const sv: any = {}
      if (fg.visibility_parent) sv.parent = fg.visibility_parent
      if (fg.visibility_child) sv.child = fg.visibility_child
      if (Object.keys(sv).length > 0) df.section_visibility = sv
      payload.default_filter = Object.keys(df).length > 0 ? df : null
    }

    if (!activeLayoutId.value) {
      toast.show('No active layout selected', 'error'); return
    }
    if (sectionFormData.value.id) {
      await store.updateLayoutSection(collectionName.value, activeLayoutId.value, sectionFormData.value.id, payload)
    } else {
      await store.createLayoutSection(collectionName.value, activeLayoutId.value, payload)
    }
    closeSectionEditor()
    await loadSections()
    toast.show('Section saved', 'success')
  } catch (e) {
    toast.show(`Failed to save section: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  }
}

function isFieldInSection(fieldKey: string): boolean {
  const field = fields.value.find(f => f._key === fieldKey)
  if (!field || !field.name) return false
  return sectionFormData.value?.display_fields?.includes(field.name) || false
}

function toggleFieldInSection(fieldKey: string) {
  const field = fields.value.find(f => f._key === fieldKey)
  if (!field || !field.name || !sectionFormData.value) return
  const current = sectionFormData.value.display_fields || []
  if (current.includes(field.name)) {
    sectionFormData.value.display_fields = current.filter((f: string) => f !== field.name)
  } else {
    sectionFormData.value.display_fields = [...current, field.name]
    // Default to column 1
    if (!(sectionFormData.value as any)._field_columns) {
      (sectionFormData.value as any)._field_columns = {}
    }
    (sectionFormData.value as any)._field_columns[field.name] = 1
  }
}

const availableFieldsForSection = computed(() => {
  if (!sectionFormData.value) return []
  const currentFields = sectionFormData.value.display_fields || []
  return fields.value.filter(f => {
    if (!f.name) return false
    if (currentFields.includes(f.name)) return true
    return !sections.value.some((s: any) =>
      s !== sectionFormData.value &&
      s.section_type === 'field_group' &&
      s.display_fields?.includes(f.name)
    )
  })
})

function onDropInSection(_event: DragEvent, _section: any) {
  if (dragType) {
    const maxPos = fields.value.reduce((m, f) => Math.max(m, f.ordinal_position || 0), 0)
    const field = makeField(dragType, maxPos + 1)
    fields.value.push(field)
    if (!_section.display_fields) _section.display_fields = []
    _section.display_fields.push(field._tempName)
    finishDrag()
    openNewFieldEditor(field)
    return
  }
  if (dragFieldKey) {
    const idx = fields.value.findIndex(f => f._key === dragFieldKey)
    if (idx !== -1) {
      const [moved] = fields.value.splice(idx, 1)
      fields.value.push(moved)
    }
  }
  finishDrag()
}

async function deleteSection(section: any) {
  if (!section.id) {
    sections.value = sections.value.filter(s => s !== section)
    return
  }
  if (!activeLayoutId.value) return
  try {
    await store.deleteLayoutSection(collectionName.value, activeLayoutId.value, section.id)
    await loadSections()
  } catch (e) { console.warn('[CollectionBuilder] Failed to delete section', e) }
}

watch(() => route.params.name, async (n, oldN) => { if (n && typeof n === 'string' && n !== oldN) { activeLayoutId.value = null; await loadCollection(n); await loadLayouts(); if (collLayouts.value.length > 0 && !activeLayoutId.value) activeLayoutId.value = collLayouts.value[0].id; await loadSections() } })

async function handleSave() {
  const validFields = fields.value.filter(f => f.name && /^[a-z][a-z0-9_]*$/.test(f.name))
  if (validFields.length === 0) { toast.show('No valid fields to save', 'error'); return }
  saving.value = true
  try {
    const payload = validFields.map((f, i) => {
      const p: any = { name: f.name, display_name: f.display_name || null, type: f.type, required: f.required, unique: f.unique,
        default_value: f.default_value, display_type: f.display_type, ordinal_position: i + 1 }
      const a = f as any
      if (a.full_width) p.full_width = true
      if (a.related_collection) { p.related_collection = a.related_collection; p.relationship_type = a.relationship_type }
      if (a.display_field) { p.display_field = a.display_field }
      if (a.inline_parent_fields?.length > 0) { p.inline_parent_fields = a.inline_parent_fields }
      if (a.options && (Array.isArray(a.options) ? a.options.length > 0 : true)) { p.options = a.options }
      return p
    })
    await store.updateCollection(collectionName.value, { fields: payload })

    // Persist section display_fields so newly added fields appear in their sections
    if (!activeLayoutId.value) { saving.value = false; return }
    for (const section of sections.value) {
      if (section.id) {
        await store.updateLayoutSection(collectionName.value, activeLayoutId.value, section.id, {
          name: section.name,
          section_type: 'field_group',
          display_fields: section.display_fields || [],
          ordinal_position: section.ordinal_position,
          default_filter: (section._columns ?? 0) > 1
            ? { _columns: section._columns, _field_columns: section._field_columns || {} }
            : null,
        }).catch((e: any) => console.warn('[CollectionBuilder] Failed to persist section', section.name, e))
      }
    }

    fields.value = validFields.map(f => ({ ...f, _key: nextKey() }))
    toast.show('Collection saved successfully', 'success')
  } catch (e) { toast.show(`Failed to save: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error') }
  finally { saving.value = false }
}
</script>

<template>
  <div class="p-6">
    <div v-if="loading" class="flex items-center justify-center h-64">
      <p class="text-gray-500 text-lg">Loading collection...</p>
    </div>
    <div v-else-if="loadError" class="text-center py-12">
      <p class="text-red-500 mb-4">{{ loadError }}</p>
      <router-link to="/collections" class="text-blue-500 text-sm hover:underline">&#8592; Back to Collections</router-link>
    </div>
    <div v-else>
      <div class="flex justify-between items-start mb-6">
        <div>
          <router-link to="/collections" class="inline-block mb-2 text-blue-500 text-sm hover:underline">&#8592; Back to Collections</router-link>
          <h1 class="text-2xl font-bold text-gray-900"><CollectionNameLabel :collection="collectionMeta as any" /></h1>
        </div>
        <div class="flex gap-2">
          <Button v-if="!collectionMeta?.is_system" label="Browse Data" severity="secondary" outlined as="router-link" :to="`/collections/${collectionName}/data`" />
          <Button label="Edit Collection Details" severity="secondary" outlined @click="showCollectionDrawer = true" />
          <Button :label="saving ? 'Saving...' : 'Save Changes'" severity="primary" :disabled="saving" @click="handleSave" />
        </div>
      </div>

      <div class="flex gap-6">
        <!-- LEFT: Draggable Field Type Palette (2-col grid) -->
        <div class="w-64 flex-shrink-0">
          <div class="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden">
            <div class="bg-gray-50 px-4 py-3 border-b border-gray-200 flex items-center justify-between">
              <span class="font-semibold text-gray-800 text-sm">Field Types</span>
              <span class="text-xs text-gray-400">Drag into form</span>
            </div>
            <div class="p-3 space-y-3">
              <div v-for="group in paletteGroups" :key="group.label">
                <div class="text-xs font-semibold text-gray-500 px-1 mb-1 uppercase tracking-wider">{{ group.label }}</div>
                <div class="grid grid-cols-2 gap-1">
                  <div v-for="item in group.items" :key="item.type" draggable="true"
                    class="flex items-center gap-1.5 px-2 py-1.5 text-sm text-gray-700 rounded-md cursor-grab active:cursor-grabbing hover:bg-blue-50 hover:text-blue-700 border border-transparent hover:border-blue-200 transition-colors select-none"
                    @dragstart="onDragStart($event, item.type)">
                    <span class="w-4 h-4 flex items-center justify-center text-xs text-gray-400 shrink-0" v-html="item.icon"></span>
                    <span class="truncate">{{ item.label }}</span>
                  </div>
                </div>
              </div>
            </div>
            <div class="border-t border-gray-200 px-4 py-2 text-xs text-gray-400">{{ namedFieldCount }} field{{ namedFieldCount !== 1 ? 's' : '' }} defined</div>
          </div>
        </div>

        <!-- RIGHT: Form Preview -->
        <div class="flex-1">
          <div v-if="allFields.length === 0" class="flex items-center justify-center h-64 bg-white rounded-lg border-2 border-dashed border-gray-300"
               @dragover.prevent="onDragOverEmpty" @dragleave="onDragLeaveEmpty" @drop="onDropAtEnd($event)">
            <p class="text-gray-400 text-sm" :class="{'text-blue-500': isDragging}">
              {{ isDragging ? 'Drop here to add field' : 'Drag field types here to build your form' }}
            </p>
          </div>
          <div v-else class="bg-white rounded-lg border border-gray-200 overflow-hidden"
               @dragover.prevent @drop="onDropAtEnd($event)">
            
            <!-- Unnamed drafts section -->
            <div v-if="unnamedFields.length > 0" class="border-b border-yellow-200 bg-yellow-50/50">
              <div class="px-4 py-2 text-xs font-medium text-yellow-700 border-b border-yellow-200 flex items-center gap-2">
                <span>&#9888;</span>
                <span>{{ unnamedFields.length }} unnamed field{{ unnamedFields.length !== 1 ? 's' : '' }}</span>
              </div>
              <div class="grid grid-cols-2 gap-4 p-4">
                <div v-for="field in unnamedFields" :key="field._key"
                     class="relative border-2 border-dashed border-yellow-300 rounded-lg p-3 bg-white hover:border-yellow-400 transition-colors">
                  <label class="block text-xs font-medium text-gray-500 mb-1">API Name <span class="text-red-400">*</span></label>
                  <InputText :ref="(el: any) => { if (el && field._key === focusKey) (el.$el as HTMLInputElement).focus(); }"
                         :value="field.name" @input="onDraftNameInput(field._key, $event)" @keydown.enter="commitDraft(field._key)"
                         maxlength="59"
                         placeholder="field_name" class="w-full mb-2" fluid :pt="{
                           root: { class: 'bg-yellow-50 border-yellow-300' }
                         }" />
                  <div class="flex items-center justify-between mt-2">
                    <span class="text-xs text-gray-400 italic">{{ fieldTypeLabel(field) }}</span>
                    <div class="flex gap-1">
                      <Button icon="pi pi-check" text severity="success" @click="commitDraft(field._key)" title="Commit" />
                      <Button icon="pi pi-times" text severity="danger" @click="discardDraft(field._key)" title="Discard" />
                    </div>
                  </div>
                </div>
              </div>
            </div>

              <!-- Layout selector -->
              <div class="flex items-center justify-between px-4 py-2 bg-gray-50 border-b border-gray-200">
                <div class="flex items-center gap-2">
                  <span class="text-xs font-medium text-gray-500">Layout</span>
                  <Select v-if="collLayouts.length > 0" :modelValue="activeLayoutId" @update:modelValue="(v: string) => { activeLayoutId = v; onLayoutChange() }"
                    :options="collLayouts" optionLabel="name" optionValue="id" class="w-48" size="small" />
                  <span v-else class="text-xs text-gray-400">No layouts</span>
                </div>
                <div class="flex gap-1">
                  <Button icon="pi pi-plus" severity="secondary" size="small" label="Layout" @click="openCreateLayoutDialog" />
                  <Button v-if="activeLayoutId" icon="pi pi-users" severity="secondary" size="small" label="Roles" @click="openLayoutRolesDialog" />
                  <Button v-if="collLayouts.length > 1 && activeLayoutId" icon="pi pi-trash" severity="danger" size="small" @click="deleteCurrentLayout" />
                </div>
              </div>

              <!-- Sections organized by ordinal_position -->
             <template v-for="section in orderedSections" :key="section.id || section._key">
               <!-- Section drop indicator (before each section) -->
               <div v-if="dropBeforeSectionKey === (section.id || section._key)" class="h-1 bg-blue-400 rounded mx-2"></div>
               
               <div class="border-b border-gray-200 last:border-b-0 transition-all"
                    :class="dragSectionId === (section.id || section._key) ? 'opacity-50' : ''"
                    @dragover.prevent="onSectionDragOver(section)"
                    @dragleave="dragOverSectionId = null; clearSectionDropIndicator(section)"
                    @drop="onSectionDrop(section)">
                 
                 <!-- Section Header -->
                 <div class="flex items-center justify-between px-4 py-2 bg-gray-50 border-b border-gray-200"
                      :class="dragOverSectionId === (section.id || section._key) ? 'bg-blue-50' : ''">
                   <div class="flex items-center gap-2">
                     <span draggable="true" class="text-gray-300 cursor-grab text-xs select-none" @dragstart="onSectionDragStart(section)">&#9776;</span>
                     <span class="text-sm font-semibold text-gray-700">{{ section.name }}</span>
                     <span class="text-xs text-gray-400 px-1.5 py-0.5 rounded-full bg-gray-100">
                       {{ section.section_type === 'field_group' ? 'Field Group' : 'Relational' }}
                     </span>
                   </div>
                   <div class="flex gap-1">
                     <Button icon="pi pi-pencil" text severity="secondary" size="small" @click="editSection(section)" />
                     <Button icon="pi pi-trash" text severity="danger" size="small" @click="deleteSection(section)" />
                   </div>
                 </div>
                 
                  <!-- Field Group: 1 or 2 column field layout -->
                  <div v-if="section.section_type === 'field_group'" class="p-4">
                    <template v-if="getSectionFields(section).length === 0">
                      <div class="text-xs text-gray-400 text-center py-20 border-2 border-dashed rounded-lg transition-colors cursor-pointer"
                           :class="dragOverEmptySectionId === (section.id || section._key)
                             ? 'border-blue-400 bg-blue-50/50 text-blue-500'
                             : 'border-gray-200 hover:border-blue-300'"
                           @dragover.prevent="onDragOverEmptySection(section)"
                           @dragleave="onDragLeaveEmptySection(section)"
                           @drop="onDropInSection($event, section)">
                        <template v-if="dragType || dragFieldKey">Drop fields here</template>
                        <template v-else>Drop fields here</template>
                      </div>
                    </template>
                    <template v-else>
                      <!-- 2-column layout -->
                      <div v-if="section._columns === 2" class="flex gap-4">
                        <div class="flex-1 min-w-0 space-y-2">
                          <div class="text-xs font-medium text-gray-400 uppercase tracking-wider mb-1">Column 1</div>
                          <template v-for="(field) in flatColumnFields(section, 0)" :key="field._key">
                            <div v-if="field._isGap"
                              class="w-full h-8 rounded transition-colors"
                              :class="dropBeforeKey === field._key
                                ? 'border-2 border-blue-400 bg-blue-100/60'
                                : isDragging ? 'border-2 border-dashed border-transparent hover:border-blue-300 hover:bg-blue-50 cursor-pointer' : ''"
                              @dragover.prevent="onDragOverField(field._key)"
                              @dragleave="onDragLeaveField(field._key)"
                              @drop.stop="onFieldDrop($event, field._key)"></div>
                            <div v-else draggable="true"
                              class="group relative border rounded-lg px-3 py-0.5 transition-all cursor-grab active:cursor-grabbing border-gray-200 hover:border-blue-300 hover:shadow-sm"
                              @dragstart="onFieldDragStart($event, field._key)">
                              <div class="flex items-center justify-between">
                                <div class="flex items-center gap-1.5 min-w-0">
                                  <span v-if="!field.is_system" class="text-gray-300 group-hover:text-gray-400 text-xs cursor-grab select-none">&#9776;</span>
                                  <span v-else class="material-symbols-outlined text-gray-300 text-sm">lock</span>
                                  <label class="block text-xs font-medium truncate" :class="field.is_system ? 'text-gray-400' : 'text-gray-600'"><FieldNameLabel :field="field" /><span v-if="field.required" class="text-red-400 ml-0.5">*</span></label>
                                </div>
                                <div class="flex items-center gap-0.5">
                                  <Button v-if="!field.is_system" icon="pi pi-cog" text severity="secondary" size="small" @click="openFieldEditor(field)" title="Settings" class="opacity-0 group-hover:opacity-100" />
                                </div>
                              </div>
                            </div>
                          </template>
                        </div>
                        <div class="flex-1 min-w-0 space-y-2">
                          <div class="text-xs font-medium text-gray-400 uppercase tracking-wider mb-1">Column 2</div>
                          <template v-for="(field) in flatColumnFields(section, 1)" :key="field._key">
                            <div v-if="field._isGap"
                              class="w-full h-8 rounded transition-colors"
                              :class="dropBeforeKey === field._key
                                ? 'border-2 border-blue-400 bg-blue-100/60'
                                : isDragging ? 'border-2 border-dashed border-transparent hover:border-blue-300 hover:bg-blue-50 cursor-pointer' : ''"
                              @dragover.prevent="onDragOverField(field._key)"
                              @dragleave="onDragLeaveField(field._key)"
                              @drop.stop="onFieldDrop($event, field._key)"></div>
                            <div v-else draggable="true"
                              class="group relative border rounded-lg px-3 py-0.5 transition-all cursor-grab active:cursor-grabbing border-gray-200 hover:border-blue-300 hover:shadow-sm"
                              @dragstart="onFieldDragStart($event, field._key)">
                              <div class="flex items-center justify-between">
                                <div class="flex items-center gap-1.5 min-w-0">
                                  <span v-if="!field.is_system" class="text-gray-300 group-hover:text-gray-400 text-xs cursor-grab select-none">&#9776;</span>
                                  <span v-else class="material-symbols-outlined text-gray-300 text-sm">lock</span>
                                  <label class="block text-xs font-medium truncate" :class="field.is_system ? 'text-gray-400' : 'text-gray-600'"><FieldNameLabel :field="field" /><span v-if="field.required" class="text-red-400 ml-0.5">*</span></label>
                                </div>
                                <div class="flex items-center gap-0.5">
                                  <Button v-if="!field.is_system" icon="pi pi-cog" text severity="secondary" size="small" @click="openFieldEditor(field)" title="Settings" class="opacity-0 group-hover:opacity-100" />
                                </div>
                              </div>
                            </div>
                          </template>
                        </div>
                      </div>
                      <!-- 1-column layout -->
                      <div v-else class="flex flex-wrap gap-2">
                        <template v-for="field in flatSectionFields(section)" :key="field._key">
                          <div v-if="field._isGap"
                            class="w-full h-8 rounded transition-colors"
                            :class="dropBeforeKey === field._key
                              ? 'border-2 border-blue-400 bg-blue-100/60'
                              : isDragging ? 'border-2 border-dashed border-transparent hover:border-blue-300 hover:bg-blue-50 cursor-pointer' : ''"
                            @dragover.prevent="onDragOverField(field._key)"
                            @dragleave="onDragLeaveField(field._key)"
                            @drop.stop="onFieldDrop($event, field._key)"></div>
                          <div v-else draggable="true"
                            :class="[
                              (field as any).full_width ? 'w-full' : 'flex-1 min-w-[200px]',
                              editingFieldKey === field._key ? 'border-blue-300 shadow-sm' : 'border-gray-200 hover:border-blue-300 hover:shadow-sm',
                            ]"
                            class="group relative border rounded-lg px-3 py-0.5 transition-all cursor-grab active:cursor-grabbing"
                            @dragstart="onFieldDragStart($event, field._key)">
                              <div class="flex items-center justify-between">
                                <div class="flex items-center gap-1.5 min-w-0">
                                  <span v-if="!field.is_system" class="text-gray-300 group-hover:text-gray-400 text-xs cursor-grab select-none">&#9776;</span>
                                  <span v-else class="material-symbols-outlined text-gray-300 text-sm">lock</span>
                                  <label class="block text-xs font-medium truncate" :class="field.is_system ? 'text-gray-400' : 'text-gray-600'"><FieldNameLabel :field="field" /><span v-if="field.required" class="text-red-400 ml-0.5">*</span></label>
                                </div>
                                <div class="flex items-center gap-0.5">
                                  <Button v-if="!field.is_system" icon="pi pi-cog" text severity="secondary" size="small" @click="openFieldEditor(field)" title="Settings" class="opacity-0 group-hover:opacity-100" />
                                </div>
                              </div>
                          </div>
                        </template>
                      </div>
                    </template>
                  </div>
                 
                 <!-- Relational: compact info row -->
                 <div v-else class="px-4 py-3 text-sm text-gray-500">
                   <span class="italic">via {{ section.relation_field }} &#183; {{ section.view_type }} view</span>
                 </div>
               </div>
             </template>
 
             <!-- Drop indicator after last section -->
             <div v-if="orderedSections.length > 0 && dropAfterLastSection" class="h-1 bg-blue-400 rounded mx-2"></div>
 
             <!-- Empty sections state — also a drop zone -->
             <div v-if="orderedSections.length === 0 && namedFields.length > 0"
                  class="p-4 text-center text-xs border-2 border-dashed rounded-lg transition-colors"
                  :class="dragSectionId ? 'border-blue-400 bg-blue-50/50 text-blue-500' : 'border-transparent text-gray-400'"
                  @dragover.prevent="dropAfterLastSection = true; dropBeforeSectionKey = null"
                  @dragleave="dropAfterLastSection = false"
                  @drop="onDropAtEnd($event)">
               {{ dragSectionId ? 'Drop section here' : 'No sections yet' }}
             </div>
 
             <!-- Drop zone for last position (section drag) -->
             <div v-if="orderedSections.length > 0 && dragSectionId"
                  class="mt-2 py-6 border-2 border-dashed border-transparent rounded-lg text-center text-xs transition-colors"
                  :class="dropAfterLastSection ? 'border-blue-400 bg-blue-50/50 text-blue-500' : 'hover:border-gray-300 text-gray-400'"
                  @dragover.prevent="dropAfterLastSection = true; dropBeforeSectionKey = null"
                  @dragleave="dropAfterLastSection = false"
                  @drop="onDropAtEnd($event)">
               Drop section at end
             </div>
           </div>
 
            <!-- Add Section Button -->
            <div class="flex gap-2 mt-4">
              <Button label="Add Section" icon="pi pi-plus" severity="secondary" size="small" @click="showSectionTypeDialog = true" />
            </div>

          <!-- Section Type Selection Dialog -->
          <Dialog :visible="showSectionTypeDialog" @update:visible="v => { if (!v) showSectionTypeDialog = false }" header="New Section" :modal="true" :style="{ width: '400px' }" :draggable="false">
            <div class="space-y-3">
              <p class="text-sm text-gray-600">Choose the type of section to add:</p>
              <div class="grid grid-cols-2 gap-3">
                <div class="border rounded-lg p-4 cursor-pointer hover:border-blue-400 hover:bg-blue-50 transition-colors text-center"
                     :class="{'border-blue-400 bg-blue-50': sectionTypeChoice === 'field_group'}"
                     @click="sectionTypeChoice = 'field_group'">
                  <div class="text-lg font-bold text-gray-700 mb-1">Field Group</div>
                  <div class="text-xs text-gray-400">Group fields into a named section</div>
                </div>
                <div class="border rounded-lg p-4 cursor-pointer hover:border-blue-400 hover:bg-blue-50 transition-colors text-center"
                     :class="{'border-blue-400 bg-blue-50': sectionTypeChoice === 'relational'}"
                     @click="sectionTypeChoice = 'relational'">
                  <div class="text-lg font-bold text-gray-700 mb-1">Relational</div>
                  <div class="text-xs text-gray-400">Display related records</div>
                </div>
              </div>
            </div>
            <template #footer>
              <Button label="Cancel" severity="secondary" outlined @click="showSectionTypeDialog = false; sectionTypeChoice = null" />
              <Button label="Next" severity="primary" :disabled="!sectionTypeChoice" @click="openNewSectionEditor" />
            </template>
          </Dialog>

          <!-- Section Editor Dialog -->
          <Dialog :visible="showSectionEditor" @update:visible="v => { if (!v) closeSectionEditor() }" :header="editingSection?.id ? 'Edit Section' : 'New Section'" :modal="true" :style="{ width: '500px' }" :draggable="false">
            <div v-if="sectionFormData" class="space-y-4">
              <div>
                <label class="block text-xs font-medium text-gray-600 mb-1">Section Name</label>
                <InputText v-model="sectionFormData.name" placeholder="e.g., Basic Info" class="w-full" fluid />
              </div>
              
               <!-- Field Group specific -->
              <template v-if="sectionFormData.section_type === 'field_group'">
                <div>
                  <label class="block text-xs font-medium text-gray-600 mb-1">Fields</label>
                  <div class="border border-gray-200 rounded-md p-3 space-y-1 max-h-48 overflow-y-auto">
                    <label v-for="field in availableFieldsForSection" :key="field._key" class="flex items-center gap-2 cursor-pointer">
                      <Checkbox :binary="true" :model-value="isFieldInSection(field._key)" @change="toggleFieldInSection(field._key)" />
                      <span class="text-sm text-gray-700"><FieldNameLabel :field="field" /></span>
                    </label>
                    <div v-if="availableFieldsForSection.length === 0" class="text-xs text-gray-400 text-center py-2">
                      No available fields
                    </div>
                  </div>
                  <p class="text-xs text-gray-400 mt-1">Check fields to include in this section</p>
                </div>
                <!-- Columns layout -->
                <div class="border-t pt-3 mt-3">
                  <label class="block text-xs font-medium text-gray-600 mb-2">Columns</label>
                  <div class="flex gap-2">
                    <Button :label="'1'" :severity="sectionColumns === 1 ? 'primary' : 'secondary'" size="small" @click="sectionColumns = 1" class="flex-1" />
                    <Button :label="'2'" :severity="sectionColumns === 2 ? 'primary' : 'secondary'" size="small" @click="sectionColumns = 2" class="flex-1" />
                  </div>
                  <div v-if="sectionColumns === 2" class="mt-2">
                    <p class="text-xs text-gray-400">Drag fields between columns to assign them. Each field keeps its column assignment.</p>
                  </div>
                </div>
              </template>
              
              <!-- Relational specific -->
              <template v-if="sectionFormData.section_type === 'relational'">
                <div>
                  <label class="block text-xs font-medium text-gray-600 mb-1">Relation Field</label>
                  <Select v-model="sectionFormData.relation_field" :options="relationFieldOptions" option-label="label" option-value="value" placeholder="Select..." class="w-full" />
                </div>
                <div>
                  <label class="block text-xs font-medium text-gray-600 mb-1">View Type</label>
                  <Select v-model="sectionFormData.view_type" :options="[{label:'Table', value:'table'}, {label:'Cards', value:'cards'}, {label:'Kanban', value:'kanban'}]" option-label="label" option-value="value" class="w-full" />
                </div>
                <div>
                  <label class="block text-xs font-medium text-gray-600 mb-1">Item Limit</label>
                  <InputNumber v-model="sectionFormData.item_limit" :min="1" :max="100" class="w-full" fluid />
                </div>
                <!-- View Settings Component -->
                <div v-if="currentViewSettingsComponent && childCollectionFields.length > 0" class="border-t pt-4 mt-2">
                  <component
                    :is="currentViewSettingsComponent"
                    :fields="childCollectionFields"
                    :system-fields="['id', 'created_at', 'updated_at']"
                    :settings="sectionFormData.view_settings || {}"
                    :on-change="onViewSettingsChange"
                  />
                </div>
                <!-- Filter Builder -->
                <div v-if="sectionFormData.relation_field" class="border-t pt-4 mt-2">
                  <label class="block text-xs font-medium text-gray-600 mb-2">Default Filter</label>
                  <p class="text-xs text-gray-400 mb-2">Only show items matching these conditions.</p>
                  <FilterBuilder
                    v-model="sectionFormData.section_filter"
                    :fields="childCollectionFields"
                    :collection-name="getChildCollectionName(sectionFormData)"
                  />
                </div>
                <!-- Section Visibility -->
                <div v-if="sectionFormData.relation_field" class="border-t pt-4 mt-2 space-y-3">
                  <label class="block text-xs font-medium text-gray-600 mb-1">Section Visibility</label>
                  <p class="text-xs text-gray-400 mb-2">Only show this section when conditions are met.</p>
                  <div class="border border-gray-200 rounded-md p-3 space-y-3">
                    <div>
                      <label class="block text-xs font-medium text-gray-500 mb-1">Parent matches</label>
                      <FilterBuilder
                        v-model="sectionFormData.visibility_parent"
                        :fields="namedFields"
                        :collection-name="collectionName"
                      />
                    </div>
                    <div class="border-t pt-3">
                      <label class="block text-xs font-medium text-gray-500 mb-1">Any child matches</label>
                      <FilterBuilder
                        v-model="sectionFormData.visibility_child"
                        :fields="childCollectionFields"
                        :collection-name="getChildCollectionName(sectionFormData)"
                      />
                    </div>
                  </div>
                </div>
              </template>
            </div>
            <template #footer>
              <Button label="Cancel" severity="secondary" outlined @click="closeSectionEditor" />
              <Button label="Save Section" severity="primary" @click="saveSection" />
            </template>
          </Dialog>
        </div>
      </div>

      <!-- Field Properties Drawer -->
      <Drawer
        :visible="editingField !== null"
        @update:visible="val => { if (!val) closeFieldEditor() }"
        header="Field Properties"
        position="right"
        :style="{ width: '500px' }"
      >
        <div v-if="editingField" class="space-y-4">
          <div>
            <label class="block text-xs font-medium text-gray-600 mb-1">API Name</label>
            <InputText :value="editingField.name" @input="onEditName($event)" maxlength="59"
              :invalid="fieldNameError"
              placeholder="field_name" ref="editorNameInput" class="w-full" fluid />
            <p v-if="fieldNameError" class="text-xs text-red-500 mt-1">Lowercase letters, numbers, and underscores only</p>
            <p class="text-xs text-gray-400 mt-1 text-right">{{ (editingField.name || '').length }}/59</p>
          </div>
          <div>
            <label class="block text-xs font-medium text-gray-600 mb-1">Display Name</label>
            <InputText :value="editingField.display_name ?? ''" @input="onEditDisplayName($event)"
              placeholder="Display name (shown in UI)" class="w-full" fluid />
          </div>
          <div>
            <label class="block text-xs font-medium text-gray-600 mb-1">Type</label>
            <div class="w-full px-3 py-2 text-sm bg-gray-50 border border-gray-200 rounded-md text-gray-600">{{ fieldTypeLabel(editingField) }}</div>
          </div>
          <div>
            <label class="block text-xs font-medium text-gray-600 mb-2">Constraints</label>
            <div class="flex gap-4">
              <label class="flex items-center gap-2 cursor-pointer">
                <Checkbox :binary="true" v-model="editingField.required" />
                <span class="text-sm text-gray-700">Required</span>
              </label>
              <label class="flex items-center gap-2 cursor-pointer">
                <Checkbox :binary="true" v-model="editingField.unique" />
                <span class="text-sm text-gray-700">Unique</span>
              </label>
              <label class="flex items-center gap-2 cursor-pointer">
                <Checkbox :binary="true" :model-value="(editingField as any).full_width" @update:model-value="(val: any) => { (editingField as any).full_width = val }" />
                <span class="text-sm text-gray-700">Full Width</span>
              </label>
            </div>
          </div>
          <div v-if="!isRelType(editingField.type) && !isType(editingField, ['auto-number'])">
            <label class="block text-xs font-medium text-gray-600 mb-1">Default Value</label>
            <InputText :value="editingField.default_value ?? ''" @input="onEditDefault($event)" class="w-full" fluid :placeholder="defaultValuePlaceholder(editingField.type)" />
          </div>
          <div>
            <label class="block text-xs font-medium text-gray-600 mb-1">Display Type</label>
            <Select :value="editingField.display_type || 'default'" @change="onDisplayTypeChange($event.value)" :options="availableDisplayTypes" option-label="label" option-value="type" class="w-full" />
          </div>
          <!-- Settings Component: rendered based on display_type -->
          <div v-if="currentSettingsComponent">
            <component :is="currentSettingsComponent" :field="editingField" :collection-name="collectionName" />
          </div>

          <!-- File Options -->
          <div v-if="editingField.type === 'file'" class="border-t pt-4 mt-4 space-y-4">
            <h4 class="text-sm font-medium text-gray-700">File Options</h4>

            <div>
              <label class="block text-xs font-medium text-gray-600 mb-1">Allow Multiple</label>
              <ToggleSwitch v-model="(editingField as any).options.multiple" />
              <p class="text-xs text-gray-400 mt-1">Allow uploading multiple files to this field</p>
            </div>

            <div>
              <label class="block text-xs font-medium text-gray-600 mb-1">Max File Size (bytes)</label>
              <InputNumber v-model="(editingField as any).options.max_file_size" :min="0" :step="1048576" class="w-full" fluid />
              <p class="text-xs text-gray-400 mt-1">Maximum file size in bytes. 0 = no limit. Default: 10MB (10485760)</p>
            </div>

            <div>
              <label class="block text-xs font-medium text-gray-600 mb-1">Allowed MIME Types</label>
              <InputText
                :model-value="((editingField as any).options?.allowed_mime_types || []).join(', ')"
                @update:model-value="(val: any) => { (editingField as any).options.allowed_mime_types = (val || '').split(',').map((s: string) => s.trim()).filter((s: string) => s.length > 0) }"
                placeholder="image/*, application/pdf"
                class="w-full" fluid />
              <p class="text-xs text-gray-400 mt-1">Leave empty to allow all types. Use glob patterns like image/*</p>
            </div>
          </div>
        </div>
        <template #footer>
          <div class="flex justify-between">
            <Button label="Delete Field" severity="danger" text @click="deleteEditingField" />
            <div class="flex gap-2">
              <Button label="Cancel" severity="secondary" outlined @click="closeFieldEditor" />
              <Button label="Done" severity="primary" @click="closeFieldEditor" />
            </div>
          </div>
        </template>
      </Drawer>

    </div>

    <!-- Collection Details Drawer -->
    <Drawer v-model:visible="showCollectionDrawer" header="Collection Details" position="right" :style="{ width: '400px' }">
      <div v-if="collectionMeta" class="space-y-4">
        <div>
          <label class="block text-xs font-medium text-gray-600 mb-1">API Name</label>
          <InputText :value="collectionMeta.name" disabled class="w-full" fluid />
          <p class="text-xs text-gray-400 mt-1">The internal database table name (read-only)</p>
        </div>
        <div>
          <label class="block text-xs font-medium text-gray-600 mb-1">Display Name</label>
          <InputText v-model="collectionDisplayName" placeholder="Display name (shown in UI)" class="w-full" fluid />
        </div>
        <div class="pt-4 border-t border-gray-200">
          <Button label="Save" severity="primary" :disabled="savingDetails" @click="saveCollectionDetails" />
        </div>
      </div>
    </Drawer>

    <!-- Create Layout Dialog -->
    <Dialog :visible="showLayoutDialog" @update:visible="v => showLayoutDialog = v" header="New Layout" :modal="true" :style="{ width: '400px' }" :draggable="false">
      <div class="space-y-4">
        <div>
          <label class="block text-xs font-medium text-gray-600 mb-1">Layout Name</label>
          <InputText v-model="layoutDialogName" placeholder="e.g. Editor Layout" class="w-full" fluid @keydown.enter="saveLayout" />
        </div>
        <div class="flex justify-end gap-2 pt-2">
          <Button label="Cancel" severity="secondary" @click="showLayoutDialog = false" />
          <Button label="Create" severity="primary" @click="saveLayout" />
        </div>
      </div>
    </Dialog>

    <!-- Layout Roles Dialog -->
    <Dialog :visible="showLayoutRolesDialog" @update:visible="v => showLayoutRolesDialog = v" header="Assign Roles to Layout" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <div class="space-y-3">
        <p class="text-sm text-gray-500">Users with these roles will see this layout.</p>
        <div v-if="rolesStore.roles.length === 0" class="text-sm text-gray-400 text-center py-4">No roles found</div>
        <div v-for="role in rolesStore.roles" :key="role.id" class="flex items-center gap-3 py-1">
          <Checkbox :inputId="'role-' + role.id" :binary="true" :modelValue="selectedLayoutRoleIds.includes(role.id)" @update:modelValue="toggleLayoutRole(role.id)" />
          <label :for="'role-' + role.id" class="text-sm cursor-pointer">{{ role.name }}</label>
        </div>
        <div class="flex justify-end gap-2 pt-2 border-t border-gray-200">
          <Button label="Cancel" severity="secondary" @click="showLayoutRolesDialog = false" />
          <Button label="Save" severity="primary" @click="saveLayoutRoles" />
        </div>
      </div>
    </Dialog>
  </div>
</template>