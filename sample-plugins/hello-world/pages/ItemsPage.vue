<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'

const BASE = '/p/hello-world/api/items/orders'

interface Item {
  id: number
  description: string | null
  status: string
  order_date: string | null
  customer_name: string | null
  created_at: string | null
  updated_at: string | null
}

const items = ref<Item[]>([])
const loading = ref(false)
const error = ref('')
const editId = ref<number | null>(null)
const showAdd = ref(false)
const showFilters = ref(false)

const filters = ref({
  status: '',
  description: '',
})

const addForm = ref({ description: '', status: 'pending' })
const editForm = ref({ description: '', status: '' })

const hasActiveFilters = computed(() => {
  return filters.value.status || filters.value.description
})

function buildFilterPayload() {
  const conditions: Record<string, Record<string, unknown>>[] = []

  if (filters.value.status) {
    conditions.push({ status: { _eq: filters.value.status } })
  }

  if (filters.value.description) {
    conditions.push({ description: { _contains: filters.value.description } })
  }

  if (conditions.length === 0) return undefined
  if (conditions.length === 1) return conditions[0]
  return { _and: conditions }
}

async function fetchItems() {
  loading.value = true
  error.value = ''
  try {
    const params = new URLSearchParams()
    params.set('limit', '100')
    params.set('offset', '0')
    params.set('sort', 'id')
    params.set('order', 'asc')

    const filter = buildFilterPayload()
    if (filter) params.set('filter', JSON.stringify(filter))

    const r = await fetch(`${BASE}?${params.toString()}`, {
      method: 'GET',
    })
    const data = await r.json()
    items.value = (data.items || []).map((row: any) => ({
      id: row.id,
      description: row.description || null,
      status: row.status || '',
      order_date: row.order_date || null,
      customer_name: row.customer__display_value || null,
      created_at: row.created_at || null,
      updated_at: row.updated_at || null,
    }))
  } catch {
    error.value = 'Failed to load items'
  } finally {
    loading.value = false
  }
}

function clearFilters() {
  filters.value = { status: '', description: '' }
  fetchItems()
}

async function addItem() {
  if (!addForm.value.description.trim()) return
  error.value = ''
  try {
    const r = await fetch(BASE, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        items: [{
          description: addForm.value.description,
          status: addForm.value.status,
          order_date: new Date().toISOString(),
        }],
      }),
    })
    if (!r.ok) {
      const e = await r.json()
      error.value = e.error || 'Failed to create'
      return
    }
    addForm.value = { description: '', status: 'pending' }
    showAdd.value = false
    await fetchItems()
  } catch {
    error.value = 'Failed to create item'
  }
}

async function updateItem() {
  if (editId.value === null) return
  error.value = ''
  try {
    const r = await fetch(BASE, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        filter: { id: editId.value },
        update: {
          description: editForm.value.description || '',
          status: editForm.value.status,
        },
      }),
    })
    if (!r.ok) {
      const e = await r.json()
      try { error.value = JSON.parse(e).error || 'Failed to update' } catch { error.value = e || 'Failed to update' }
      return
    }
    editId.value = null
    await fetchItems()
  } catch {
    error.value = 'Failed to update item'
  }
}

async function deleteItem(id: number) {
  error.value = ''
  try {
    const r = await fetch(BASE, {
      method: 'DELETE',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ pk_values: [id] }),
    })
    if (!r.ok) {
      const e = await r.json()
      error.value = e.error || 'Failed to delete'
      return
    }
    await fetchItems()
  } catch {
    error.value = 'Failed to delete item'
  }
}

function startEdit(item: Item) {
  editForm.value = { description: item.description || '', status: item.status }
  editId.value = item.id
  showAdd.value = false
}

function cancelEdit() {
  editId.value = null
  fetchItems()
}

function fmtDate(d: string | null) {
  if (!d) return '\u2014'
  return new Date(d).toLocaleDateString()
}

onMounted(fetchItems)
</script>

<template>
  <div class="items-page">
    <div class="page-header">
      <h1>Items</h1>
      <p class="subtitle">CRUD management via /api/items/hello-world</p>
    </div>

    <div class="toolbar">
      <button class="btn btn-primary" @click="showAdd = !showAdd; if(showAdd) editId = null">
        {{ showAdd ? 'Cancel' : '+ Add Item' }}
      </button>
      <button class="btn btn-secondary" @click="fetchItems" :disabled="loading">Refresh</button>
      <button class="btn btn-filter" :class="{ 'btn-filter-active': showFilters || hasActiveFilters }" @click="showFilters = !showFilters">
        {{ hasActiveFilters ? 'Filters active' : 'Filters' }}
      </button>
      <button v-if="hasActiveFilters" class="btn btn-clear" @click="clearFilters">Clear</button>
    </div>

    <div v-if="showFilters" class="filter-bar">
      <div class="filter-row">
        <div class="filter-field">
          <label class="filter-label">Status</label>
          <input v-model="filters.status" class="input filter-input" placeholder="eq..." @input="fetchItems" />
        </div>
        <div class="filter-field">
          <label class="filter-label">Description</label>
          <input v-model="filters.description" class="input filter-input" placeholder="contains..." @input="fetchItems" />
        </div>
      </div>
    </div>

    <div v-if="error" class="error-msg">{{ error }}</div>

    <div class="items-table-wrapper">
    <table class="items-table">
      <thead>
        <tr>
          <th>ID</th><th>Description</th><th>Status</th><th>Order Date</th><th>Customer</th><th>Actions</th>
        </tr>
      </thead>
      <tbody>
        <tr v-if="showAdd" class="edit-row">
          <td class="cell-muted">—</td>
          <td><input v-model="addForm.description" placeholder="Description" class="input" /></td>
          <td>
            <span class="cat-group">
              <button :class="addForm.status === 'pending' ? 'cat-on' : 'cat-off'" @click="addForm.status = 'pending'">pending</button>
              <button :class="addForm.status === 'processing' ? 'cat-on' : 'cat-off'" @click="addForm.status = 'processing'">proc</button>
              <button :class="addForm.status === 'shipped' ? 'cat-on' : 'cat-off'" @click="addForm.status = 'shipped'">ship</button>
              <button :class="addForm.status === 'delivered' ? 'cat-on' : 'cat-off'" @click="addForm.status = 'delivered'">delv</button>
            </span>
          </td>
          <td class="cell-muted">auto</td>
          <td class="cell-muted">—</td>
          <td>
            <button class="btn btn-sm btn-save" @click="addItem">Save</button>
            <button class="btn btn-sm btn-cancel" @click="showAdd = false; addForm = { description: '', status: 'pending' }">X</button>
          </td>
        </tr>
        <tr v-for="item in items" :key="item.id" :class="{ 'edit-row': editId === item.id }">
          <td class="cell-id">{{ String(item.id).slice(0, 8) }}...</td>
          <td v-if="editId !== item.id">{{ item.description || '—' }}</td>
          <td v-if="editId !== item.id"><span class="badge">{{ item.status }}</span></td>
          <td v-if="editId !== item.id" class="cell-muted">{{ fmtDate(item.order_date) }}</td>
          <td v-if="editId !== item.id">{{ item.customer_name || '—' }}</td>
          <td v-if="editId !== item.id">
            <button class="btn btn-sm btn-edit" @click="startEdit(item)">Edit</button>
            <button class="btn btn-sm btn-del" @click="deleteItem(item.id)">Del</button>
          </td>
          <td v-if="editId === item.id" :colspan="5">
            <div class="inline-form">
              <input v-model="editForm.description" class="input" placeholder="description" />
              <span class="cat-group">
                <button :class="editForm.status === 'pending' ? 'cat-on' : 'cat-off'" @click="editForm.status = 'pending'">pending</button>
                <button :class="editForm.status === 'processing' ? 'cat-on' : 'cat-off'" @click="editForm.status = 'processing'">proc</button>
                <button :class="editForm.status === 'shipped' ? 'cat-on' : 'cat-off'" @click="editForm.status = 'shipped'">ship</button>
                <button :class="editForm.status === 'delivered' ? 'cat-on' : 'cat-off'" @click="editForm.status = 'delivered'">delv</button>
              </span>
              <button class="btn btn-sm btn-save" @click="updateItem()">Save</button>
              <button class="btn btn-sm btn-cancel" @click="cancelEdit()">X</button>
            </div>
          </td>
        </tr>
        <tr v-if="!loading && items.length === 0 && !showAdd">
          <td colspan="6" class="empty-row">No items found.</td>
        </tr>
      </tbody>
    </table>
    </div>

    <div v-if="loading" class="loading-bar">Loading...</div>
  </div>
</template>

<style>
.items-page { padding: 1.5rem; max-width: 960px; margin: 0 auto; }
.page-header { margin-bottom: 1rem; }
.page-header h1 { font-size: 1.5rem; font-weight: 700; color: #1f2937; margin: 0; }
.subtitle { color: #6b7280; font-size: 0.875rem; margin-top: 0.25rem; }
.toolbar { display: flex; gap: 0.5rem; margin-bottom: 0.75rem; flex-wrap: wrap; }

.btn { border: none; cursor: pointer; transition: all 0.15s; font-size: 0.875rem; white-space: nowrap; }
.btn:active { transform: scale(0.97); }
.btn-primary { background: #3b82f6; color: white; padding: 0.5rem 1rem; border-radius: 0.375rem; font-weight: 500; }
.btn-primary:hover { background: #2563eb; }
.btn-secondary { background: #f3f4f6; color: #4b5563; padding: 0.5rem 1rem; border-radius: 0.375rem; }
.btn-secondary:hover { background: #e5e7eb; }
.btn-filter { background: #f3f4f6; color: #4b5563; padding: 0.5rem 1rem; border-radius: 0.375rem; }
.btn-filter:hover { background: #e5e7eb; }
.btn-filter-active { background: #dbeafe; color: #2563eb; }
.btn-clear { background: #fef2f2; color: #dc2626; padding: 0.5rem 1rem; border-radius: 0.375rem; }
.btn-clear:hover { background: #fecaca; }
.btn:disabled { opacity: 0.5; cursor: not-allowed; }

.btn-sm { padding: 0.25rem 0.5rem; border-radius: 0.25rem; font-size: 0.75rem; margin: 0 0.15rem; }
.btn-edit { background: #dbeafe; color: #2563eb; }
.btn-edit:hover { background: #bfdbfe; }
.btn-del { background: #fef2f2; color: #dc2626; }
.btn-del:hover { background: #fecaca; }
.btn-save { background: #d1fae5; color: #059669; }
.btn-save:hover { background: #a7f3d0; }
.btn-cancel { background: #f3f4f6; color: #6b7280; }
.btn-cancel:hover { background: #e5e7eb; }

.filter-bar { background: #f8fafc; border: 1px solid #e2e8f0; border-radius: 0.5rem; padding: 0.75rem; margin-bottom: 0.75rem; }
.filter-row { display: flex; gap: 0.75rem; flex-wrap: wrap; align-items: flex-end; }
.filter-field { display: flex; flex-direction: column; gap: 0.25rem; flex: 1 1 auto; min-width: 120px; }
.filter-label { font-size: 0.75rem; font-weight: 600; color: #64748b; text-transform: uppercase; letter-spacing: 0.05em; white-space: nowrap; }
.filter-input { width: 100%; min-width: 80px; box-sizing: border-box; }

.items-table-wrapper { overflow-x: auto; }
.items-table { width: 100%; border-collapse: collapse; font-size: 0.875rem; min-width: 600px; }
.items-table th { text-align: left; padding: 0.5rem; border-bottom: 2px solid #e5e7eb; color: #6b7280; font-weight: 600; font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.05em; white-space: nowrap; }
.items-table td { padding: 0.5rem; border-bottom: 1px solid #f3f4f6; color: #374151; word-break: break-word; }
.items-table tr:hover td { background: #f9fafb; }

.cell-id { font-family: monospace; color: #9ca3af; width: 3rem; white-space: nowrap; }
.cell-muted { color: #9ca3af; font-size: 0.8rem; }
.badge { display: inline-block; padding: 0.125rem 0.5rem; border-radius: 9999px; font-size: 0.75rem; font-weight: 500; background: #f3f4f6; color: #6b7280; white-space: nowrap; }

.input { padding: 0.375rem 0.5rem; border: 1px solid #d1d5db; border-radius: 0.25rem; font-size: 0.875rem; color: #374151; background: white; width: auto; min-width: 60px; box-sizing: border-box; }
.input:focus { outline: none; border-color: #3b82f6; box-shadow: 0 0 0 2px rgba(59,130,246,0.15); }

.edit-row td { background: #f0f9ff !important; }
.inline-form { display: flex; gap: 0.375rem; align-items: center; flex-wrap: wrap; }
.empty-row { text-align: center; padding: 2rem !important; color: #9ca3af; }
.error-msg { color: #dc2626; font-size: 0.875rem; margin-bottom: 0.75rem; padding: 0.5rem; background: #fef2f2; border-radius: 0.25rem; }
.loading-bar { text-align: center; padding: 1rem; color: #6b7280; font-size: 0.875rem; }

.cat-group { display: inline-flex; gap: 0.25rem; flex-wrap: wrap; }
.cat-on { padding: 0.125rem 0.375rem; border-radius: 0.25rem; border: none; font-size: 0.75rem; background: #3b82f6; color: white; cursor: pointer; }
.cat-off { padding: 0.125rem 0.375rem; border-radius: 0.25rem; border: 1px solid #d1d5db; font-size: 0.75rem; background: white; color: #6b7280; cursor: pointer; }
.cat-off:hover { background: #f3f4f6; }

@media (max-width: 640px) {
  .items-page { padding: 0.75rem; }
  .items-table td, .items-table th { padding: 0.375rem 0.25rem; font-size: 0.75rem; }
  .filter-row { flex-direction: column; }
  .filter-field { min-width: 100%; }
  .filter-input { width: 100%; }
  .btn-sm { padding: 0.25rem 0.375rem; font-size: 0.7rem; }
  .inline-form { flex-direction: column; align-items: stretch; }
  .inline-form .input { width: 100%; }
}
</style>
