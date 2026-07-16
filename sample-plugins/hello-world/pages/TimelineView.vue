<template>
  <div class="timeline-view">
    <div v-if="items.length === 0" class="empty">No items to display</div>
    <div v-else class="timeline">
      <div v-for="(item, i) in items" :key="item.id || i" class="timeline-item">
        <div class="timeline-dot" />
        <div class="timeline-content">
          <strong>{{ item.name || item.title || 'Item ' + (i+1) }}</strong>
          <p v-if="item.description" class="desc">{{ item.description }}</p>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
defineProps({
  items: { type: Array, default: () => [] },
  fields: { type: Array, default: () => [] },
  loading: { type: Boolean, default: false },
  error: { type: String, default: null },
  total: { type: Number, default: 0 },
  page: { type: Number, default: 1 },
  perPage: { type: Number, default: 20 },
  sortField: { type: String, default: '' },
  sortOrder: { type: String, default: 'asc' },
  filters: { type: Object, default: () => ({}) },
  systemFields: { type: Array, default: () => [] },
})
defineEmits(['update:sort', 'update:page', 'update:filters', 'edit-item', 'delete-item'])
</script>

<style scoped>
.timeline-view { padding: 16px; }
.timeline { position: relative; padding-left: 24px; }
.timeline::before { content: ''; position: absolute; left: 8px; top: 0; bottom: 0; width: 2px; background: #e2e8f0; }
.timeline-item { position: relative; padding-bottom: 16px; }
.timeline-dot { position: absolute; left: -20px; top: 4px; width: 12px; height: 12px; border-radius: 50%; background: #3b82f6; border: 2px solid white; box-shadow: 0 0 0 2px #3b82f6; }
.timeline-content { padding: 8px 12px; background: #f8fafc; border-radius: 6px; border: 1px solid #e2e8f0; }
.timeline-content strong { font-size: 14px; color: #1e293b; }
.desc { font-size: 12px; color: #64748b; margin: 4px 0 0; }
.empty { text-align: center; padding: 40px; color: #94a3b8; }
</style>
