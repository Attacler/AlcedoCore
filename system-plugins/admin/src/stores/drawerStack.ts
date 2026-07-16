import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

export interface DrawerEntry {
  id: string
  collectionName: string
  itemId: string
  itemData?: any
  label: string
}

const MAX_DEPTH = 4

export const useDrawerStackStore = defineStore('drawerStack', () => {
  const stack = ref<DrawerEntry[]>([])
  const hasUnsavedChanges = ref(false)

  const depth = computed(() => stack.value.length)
  const isOpen = computed(() => stack.value.length > 0)
  const currentDrawer = computed(() => stack.value.length > 0 ? stack.value[stack.value.length - 1] : null)
  const breadcrumbs = computed(() => stack.value.map(d => ({ id: d.id, label: d.label })))

  function push(entry: DrawerEntry) {
    if (stack.value.length >= MAX_DEPTH) return
    stack.value.push(entry)
  }

  function pop(): DrawerEntry | undefined {
    return stack.value.pop()
  }

  function peek(): DrawerEntry | undefined {
    return stack.value[stack.value.length - 1]
  }

  function clear() {
    stack.value = []
    hasUnsavedChanges.value = false
  }

  function setUnsaved(v: boolean) {
    hasUnsavedChanges.value = v
  }

  function removeById(id: string) {
    const idx = stack.value.findIndex(e => e.id === id)
    if (idx !== -1) {
      stack.value.splice(idx)
    }
  }

  return {
    stack, depth, isOpen, currentDrawer, breadcrumbs, hasUnsavedChanges,
    push, pop, peek, clear, setUnsaved, removeById,
  }
})
