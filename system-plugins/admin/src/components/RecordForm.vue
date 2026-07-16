<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useCollectionsStore, type FieldDefinition, type CollectionSection } from '@/stores/collections'
import FieldNameLabel from '@/components/FieldNameLabel.vue'
import FormFieldRenderer from '@/components/FormFieldRenderer.vue'
import TableView from '@/components/TableView.vue'
import { useChildCrud, isTempId } from '@/composables/useChildCrud'
import { useSectionLayout, getColumnFields } from '@/composables/useSectionLayout'

const props = withDefaults(defineProps<{
  collectionName: string
  modelValue?: Record<string, any>
  fieldsOverride?: FieldDefinition[]
  sectionsOverride?: CollectionSection[]
  readonly?: boolean
}>(), {
  modelValue: () => ({}),
  fieldsOverride: undefined,
  sectionsOverride: undefined,
  readonly: false,
})

const emit = defineEmits<{
  'update:modelValue': [value: Record<string, any>]
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

const orderedSections = computed(() =>
  [...sections.value].sort((a, b) => (a.ordinal_position || 0) - (b.ordinal_position || 0))
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

function getSectionFields(section: any): FieldDefinition[] {
  if (!section.display_fields || section.display_fields.length === 0) {
    return fields.value
  }
  return fields.value.filter(f => f.name && section.display_fields.includes(f.name))
}

// Column-split field filtering: delegates to shared getColumnFields with local getSectionFields
function getColumnFieldsForSection(section: any, colIdx: number): FieldDefinition[] {
  return getColumnFields(section, colIdx, getSectionFields(section))
}

function onFieldUpdate(name: string, value: any) {
  emit('update:modelValue', { ...props.modelValue, [name]: value })
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
        const value = props.modelValue[field.name]
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
        let value = props.modelValue[field.name]
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
    const raw = await store.listSections(props.collectionName)
    sections.value = raw.map(normalizeSection)
  }

  // Init relational section state
  for (const section of sections.value) {
    if (section.section_type === 'relational') {
      sectionItems.value[section.id] = []
      const childCollName = getChildCollectionName(section)
      console.log('[RecordForm] loadData relational section', { sectionId: section.id, childCollName, sectionRelationField: section.relation_field })
      if (childCollName) {
        try {
          const childColl = await store.getCollection(childCollName)
          const filteredFields = (childColl.fields || []).filter(
            (f: any) => !['id', 'created_at', 'updated_at', '_row_version'].includes(f.name)
          )
          console.log('[RecordForm] Loaded child fields', { childCollName, fieldCount: filteredFields.length, fieldNames: filteredFields.map((f: any) => f.name) })
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

defineExpose({ validate, getPayload })
</script>

<template>
  <div class="space-y-6">
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
                :model-value="modelValue?.[field.name]"
                :invalid="fieldErrors[field.name] || false"
                :readonly="readonly"
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
                :model-value="modelValue?.[field.name]"
                :invalid="fieldErrors[field.name] || false"
                :readonly="readonly"
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
              :model-value="modelValue?.[field.name]"
              :invalid="fieldErrors[field.name] || false"
              :readonly="readonly"
              @update:model-value="onFieldUpdate(field.name, $event)"
              @valid="onFieldValid(field.name, $event)"
            />
          </div>
        </div>
      </section>
      <section v-else-if="section.section_type === 'relational'">
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
      </section>
    </template>
  </div>
</template>