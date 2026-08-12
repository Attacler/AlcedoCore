<script setup lang="ts">
import { ref, computed, onMounted, watch, defineAsyncComponent, type Component } from 'vue'
import { useCollectionsStore, type FieldDefinition, type CollectionSection } from '@/stores/collections'
import FieldNameLabel from '@/components/FieldNameLabel.vue'
import FormFieldRenderer from '@/components/FormFieldRenderer.vue'
import TableView from '@/components/TableView.vue'
import Select from 'primevue/select'
import { useChildCrud, isTempId } from '@/composables/useChildCrud'
import { useSectionLayout, getColumnFields, parseRelationField } from '@/composables/useSectionLayout'

const RelationalSection = defineAsyncComponent(() => import('@/components/RelationalSection.vue'))

const props = withDefaults(defineProps<{
  collectionName: string
  fieldsOverride?: FieldDefinition[]
  sectionsOverride?: CollectionSection[]
  readonly?: boolean
  hiddenFields?: string[]
  parentItem?: Record<string, any> | null
  deferredChildren?: boolean
  scalarOnly?: boolean
}>(), {
  fieldsOverride: undefined,
  sectionsOverride: undefined,
  readonly: false,
  hiddenFields: () => [],
  parentItem: null,
  deferredChildren: true,
  scalarOnly: false,
})

const model = defineModel<Record<string, any>>({ default: () => ({}) })

const emit = defineEmits<{
  'valid': [isValid: boolean]
}>()

const store = useCollectionsStore()

const fields = ref<FieldDefinition[]>([])
const sections = ref<any[]>([])
const fieldErrors = ref<Record<string, boolean>>({})
const validState = ref<Record<string, boolean>>({})

const sectionItems = ref<Record<string, any[]>>({})
const sectionFields = ref<Record<string, any[]>>({})
const childEditDrafts = ref<Record<string, Record<string, Record<string, any>>>>({})
const childDeletedRows = ref<Record<string, Set<string>>>({})
const availableLayouts = ref<any[]>([])
const activeLayoutId = ref<string | null>(null)

const sectionRefs = ref<Record<string, any>>({})

const orderedSections = computed(() =>
  [...sections.value]
    .filter(s => !(props.scalarOnly && s.section_type === 'relational'))
    .sort((a, b) => (a.ordinal_position || 0) - (b.ordinal_position || 0))
)

// Initialize child CRUD composable with shared state refs
const childCrud = useChildCrud({
  sectionItems,
  sectionFields,
  childEditDrafts,
  childDeletedRows,
  fields,
  collectionName: computed(() => props.collectionName),
})
const { addChildInlineRow, onChildCellEdit, onChildDelete, getChildCollectionName, getSectionChildCollectionName, findParentFKFieldName } = childCrud

// Initialize section layout utilities
const { normalizeSection } = useSectionLayout()

function isHidden(name: string): boolean {
  return props.hiddenFields.includes(name)
}

function getSectionFields(section: any): FieldDefinition[] {
  let list: FieldDefinition[] = []
  if (!section.display_fields || section.display_fields.length === 0) {
    list = fields.value
  } else {
    list = fields.value.filter(f => f.name && section.display_fields.includes(f.name))
  }
  return list.filter(f => !isHidden(f.name))
}

// Column-split field filtering: delegates to shared getColumnFields with local getSectionFields
function getColumnFieldsForSection(section: any, colIdx: number): FieldDefinition[] {
  return getColumnFields(section, colIdx, getSectionFields(section))
}

function onFieldUpdate(name: string, value: any) {
  model.value = { ...model.value, [name]: value }
}

function onFieldValid(name: string, valid: boolean) {
  validState.value[name] = valid
  const allValid = Object.keys(validState.value).length === 0
    || Object.values(validState.value).every(v => v)
  emit('valid', allValid)
}

function validate(): boolean {
  const errors: Record<string, boolean> = {}
  let valid = true
  for (const section of orderedSections.value) {
    if (section.section_type !== 'field_group') continue
    const sectionFields = getSectionFields(section)
    for (const field of sectionFields) {
      if (field.required) {
        const value = model.value[field.name]
        if (value === null || value === undefined || value === '') {
          errors[field.name] = true
          valid = false
        }
      }
    }
  }
  fieldErrors.value = errors
  emit('valid', valid)
  return valid
}

function getPayload(): Record<string, any> {
  const payload: Record<string, any> = {}

  // Collect scalar fields from field_group sections
  for (const section of orderedSections.value) {
    if (section.section_type === 'field_group') {
      for (const field of getSectionFields(section)) {
        let value = model.value[field.name]
        if (value === null || value === undefined || value === '') continue
        if (field.type === 'datetime' && value instanceof Date) {
          value = value.toISOString()
        }
        payload[field.name] = value
      }
    }
  }

  // Collect relational payloads
  for (const section of orderedSections.value) {
    if (section.section_type !== 'relational') continue
    const sectionId = section.id
    const relationField = section.relation_field
    if (!relationField) continue
    // Namespaced "collection.field" relations live on the child collection —
    // their rows are handled via the child create dialog, not the parent payload.
    if (parseRelationField(relationField)?.collection) continue

    const o2mBody: Record<string, any> = {}
    let hasChanges = false

    const drafts = childEditDrafts.value[sectionId]
    if (drafts) {
      const creates: Record<string, any>[] = []
      for (const [rowId, fields] of Object.entries(drafts)) {
        if (Object.keys(fields).length === 0) continue
        if (isTempId(rowId)) {
          const { id: _, ...cleanFields } = fields as Record<string, any>
          creates.push(cleanFields)
          hasChanges = true
        }
      }
      if (creates.length > 0) o2mBody.create = creates
    }

    if (hasChanges) {
      payload[relationField] = o2mBody
    }
  }

  return payload
}

function setSectionRef(id: string, el: any) {
  if (el) {
    sectionRefs.value[id] = el
  } else {
    delete sectionRefs.value[id]
  }
}

function isNamespacedSection(section: any): boolean {
  return section.section_type === 'relational' && !!parseRelationField(section.relation_field)?.collection
}

/** Deferred child ops from all embedded relational sections (Option A flush). */
function collectPendingOps(): any[] {
  const ops: any[] = []
  for (const id of Object.keys(sectionRefs.value)) {
    const el = sectionRefs.value[id]
    if (el && typeof el.collectPendingOps === 'function') {
      ops.push(...el.collectPendingOps())
    }
  }
  return ops
}

/** Execute queued child creates/updates/deletes after the parent record exists. */
async function flushPendingChildren(parentId: string) {
  for (const id of Object.keys(sectionRefs.value)) {
    const el = sectionRefs.value[id]
    if (el && typeof el.flushPending === 'function') {
      await el.flushPending(parentId)
    }
  }
}

/** Nested O2M create bodies for a single parent POST. Returns `{ body, inlinedTempIds }`
 * aggregating all embedded relational sections' queued leaf creates. */
function getCreateBody(): { body: Record<string, any> | null; inlinedTempIds: string[] } {
  const body: Record<string, any> = {}
  const inlinedTempIds: string[] = []
  for (const id of Object.keys(sectionRefs.value)) {
    const el = sectionRefs.value[id]
    if (el && typeof el.getCreateBody === 'function') {
      const part = el.getCreateBody()
      if (part?.body) Object.assign(body, part.body)
      if (part?.inlinedTempIds) inlinedTempIds.push(...part.inlinedTempIds)
    }
  }
  return { body: Object.keys(body).length ? body : null, inlinedTempIds }
}

/** Drop the leaf creates that were inlined into the parent's create body. */
function consumeInlinedCreates(tempIds: string[]) {
  for (const id of Object.keys(sectionRefs.value)) {
    const el = sectionRefs.value[id]
    if (el && typeof el.consumeInlinedCreates === 'function') {
      el.consumeInlinedCreates(tempIds)
    }
  }
}

async function loadData() {
  if (props.fieldsOverride) {
    fields.value = props.fieldsOverride
  } else {
    const coll = await store.getCollection(props.collectionName)
    fields.value = coll.fields || []
  }

  if (props.sectionsOverride) {
    sections.value = props.sectionsOverride.map(normalizeSection)
  } else {
    let resolved: any = null
    try {
      resolved = await store.getResolvedLayout(props.collectionName)
    } catch (e) {
      console.warn('[RecordForm] Failed to resolve layout', e)
    }
    const raw = resolved?.sections || []
    activeLayoutId.value = resolved?.layout?.id ?? null
    if (raw.length > 0) {
      sections.value = raw.map(normalizeSection)
    } else {
      // No layout/sections available — synthesize a field_group holding all fields
      sections.value = [{
        id: undefined,
        name: 'Fields',
        section_type: 'field_group',
        display_fields: fields.value.map((f: any) => f.name),
        ordinal_position: 1,
      }]
    }

    // Layouts for the selector: all layouts (admins), falling back to role-granted ones
    try {
      const all = await store.listLayouts(props.collectionName)
      availableLayouts.value = (all || []).filter((l: any) => l && l.id)
    } catch {
      availableLayouts.value = (resolved?.available_layouts || []).filter((l: any) => l && l.id)
    }
  }

  // Init relational section state
  for (const section of sections.value) {
    if (section.section_type === 'relational') {
      sectionItems.value[section.id] = []
      const childCollName = getChildCollectionName(section)
      if (childCollName) {
        try {
          const childColl = await store.getCollection(childCollName)
          const filteredFields = (childColl.fields || []).filter(
            (f: any) => !['id', 'created_at', 'updated_at', '_row_version'].includes(f.name)
          )
          sectionFields.value[section.id] = filteredFields
        } catch (e) {
          console.warn('[RecordForm] Failed to load child collection', childCollName, e)
          sectionFields.value[section.id] = section.definition_fields || []
        }
      } else {
        sectionFields.value[section.id] = section.definition_fields || []
      }
    }
  }
}

onMounted(loadData)

watch(() => props.collectionName, () => loadData())

async function onLayoutChange(layoutId: string) {
  if (!layoutId) return
  activeLayoutId.value = layoutId
  try {
    const raw = await store.listLayoutSections(props.collectionName, layoutId)
    sections.value = raw.map(normalizeSection)
  } catch (e) {
    console.warn('[RecordForm] Failed to load layout sections', e)
    sections.value = []
  }
  // Reset per-section child state for the new layout
  sectionItems.value = {}
  sectionFields.value = {}
  childEditDrafts.value = {}
  childDeletedRows.value = {}
}

defineExpose({ validate, getPayload, collectPendingOps, flushPendingChildren, getCreateBody, consumeInlinedCreates })
</script>

<template>
  <div class="space-y-6">
    <div v-if="availableLayouts.length > 0" class="flex items-center gap-2">
      <label class="text-xs font-medium text-gray-600">Layout</label>
      <Select
        v-model="activeLayoutId"
        :options="availableLayouts.map((l: any) => ({ label: l.name, value: l.id }))"
        option-label="label"
        option-value="value"
        class="w-56"
        @change="onLayoutChange($event.value)"
      />
    </div>
    <template v-for="section in orderedSections" :key="section.id">
      <section v-if="section.section_type === 'field_group'">
        <h2 class="text-lg font-semibold text-gray-800 mb-3">{{ section.name }}</h2>
        <div v-if="section._columns === 2" class="flex gap-4">
          <div class="flex-1 space-y-4">
            <div :id="'rf-field-' + field.name" v-for="field in getColumnFieldsForSection(section, 0)" :key="field.name">
              <label class="block text-xs font-medium text-gray-600 mb-1">
                <FieldNameLabel :field="field" />
                <span v-if="field.required" class="text-red-400 ml-0.5">*</span>
              </label>
              <FormFieldRenderer
                :collection-name="collectionName"
                :field-name="field.name"
                :model-value="model?.[field.name]"
                :invalid="fieldErrors[field.name] || false"
                :readonly="readonly"
                :inline-create="!readonly"
                @update:model-value="onFieldUpdate(field.name, $event)"
                @valid="onFieldValid(field.name, $event)"
              />
            </div>
          </div>
          <div class="flex-1 space-y-4">
            <div :id="'rf-field-' + field.name" v-for="field in getColumnFieldsForSection(section, 1)" :key="field.name">
              <label class="block text-xs font-medium text-gray-600 mb-1">
                <FieldNameLabel :field="field" />
                <span v-if="field.required" class="text-red-400 ml-0.5">*</span>
              </label>
              <FormFieldRenderer
                :collection-name="collectionName"
                :field-name="field.name"
                :model-value="model?.[field.name]"
                :invalid="fieldErrors[field.name] || false"
                :readonly="readonly"
                :inline-create="!readonly"
                @update:model-value="onFieldUpdate(field.name, $event)"
                @valid="onFieldValid(field.name, $event)"
              />
            </div>
          </div>
        </div>
        <div v-else class="space-y-4">
          <div :id="'rf-field-' + field.name" v-for="field in getSectionFields(section)" :key="field.name">
            <label class="block text-xs font-medium text-gray-600 mb-1">
              <FieldNameLabel :field="field" />
              <span v-if="field.required" class="text-red-400 ml-0.5">*</span>
            </label>
            <FormFieldRenderer
              :collection-name="collectionName"
              :field-name="field.name"
              :model-value="model?.[field.name]"
              :invalid="fieldErrors[field.name] || false"
              :readonly="readonly"
              :inline-create="!readonly"
              @update:model-value="onFieldUpdate(field.name, $event)"
              @valid="onFieldValid(field.name, $event)"
            />
          </div>
        </div>
      </section>

      <section v-else-if="section.section_type === 'relational'">
        <RelationalSection
          v-if="isNamespacedSection(section)"
          :ref="(el: any) => setSectionRef(section.id, el)"
          :section="section"
          :parent-collection-name="collectionName"
          :parent-item="parentItem"
          :parent-fields="fields"
          :deferred="deferredChildren"
        />
        <template v-else>
          <div class="flex items-center justify-between mb-3">
            <h2 class="text-lg font-semibold text-gray-800">{{ section.name }}</h2>
            <Button
              label="Add Row"
              severity="primary"
              size="small"
              @click="addChildInlineRow(section)"
            />
          </div>
          <TableView
            v-if="sectionItems[section.id]?.length"
            :items="sectionItems[section.id]"
            :fields="sectionFields[section.id] || []"
            :editable="true"
            :total="sectionItems[section.id]?.length || 0"
            :page="1"
            :per-page="25"
            :sort-field="''"
            :sort-order="'asc'"
            :filters="{}"
            :system-fields="[]"
            :loading="false"
            :error="null"
            :child-collection-name="getSectionChildCollectionName(section)"
            :parent-fk-field-name="findParentFKFieldName(section)"
            :edit-values="childEditDrafts[section.id] || {}"
            @cell-edit="(row: any, fieldName: string, value: any) => onChildCellEdit(section, row, fieldName, value)"
            @delete-item="(row: any) => onChildDelete(section, row)"
            :embedded="true"
          />
          <div v-else class="text-gray-400 text-sm py-4 text-center">
            No related items yet. Click &quot;Add Row&quot; to add one.
          </div>
        </template>
      </section>
    </template>
  </div>
</template>
