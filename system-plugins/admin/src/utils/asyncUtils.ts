import { type Ref } from 'vue'

export async function withAsyncHandling<T>(
  loading: Ref<boolean>,
  error: Ref<string | null>,
  fn: () => Promise<T>,
  errorMessage?: string,
): Promise<T | null> {
  loading.value = true
  error.value = null
  try {
    return await fn()
  } catch (e) {
    error.value = errorMessage || (e instanceof Error ? e.message : 'An error occurred')
    return null
  } finally {
    loading.value = false
  }
}

export async function withAsyncHandlingVoid(
  loading: Ref<boolean>,
  error: Ref<string | null>,
  fn: () => Promise<void>,
  errorMessage?: string,
): Promise<void> {
  loading.value = true
  error.value = null
  try {
    await fn()
  } catch (e) {
    error.value = errorMessage || (e instanceof Error ? e.message : 'An error occurred')
  } finally {
    loading.value = false
  }
}
