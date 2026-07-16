<template>
  <div class="min-h-screen bg-gray-50">
    <header class="bg-white shadow-sm border-b">
      <div class="max-w-4xl mx-auto px-4 py-4 flex items-center gap-4">
        <NuxtLink to="/" class="text-lg font-semibold text-gray-900 hover:text-blue-600 no-underline">
          Nuxt Plugin
        </NuxtLink>
        <nav class="flex gap-4 text-sm">
          <NuxtLink to="/" class="text-gray-600 hover:text-blue-600 no-underline">Hello</NuxtLink>
          <NuxtLink to="/kv-demo" class="text-gray-600 hover:text-blue-600 no-underline">KV Demo</NuxtLink>
          <NuxtLink to="/settings" class="text-gray-600 hover:text-blue-600 no-underline">Settings</NuxtLink>
        </nav>
      </div>
    </header>
    <main class="max-w-4xl mx-auto px-4 py-8">
      <NuxtPage />
    </main>
  </div>
</template>
