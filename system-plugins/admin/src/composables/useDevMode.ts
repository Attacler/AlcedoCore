import { ref } from 'vue'

const DEV_MODE_KEY = 'show_dev_mode_indicators'
const devMode = ref(localStorage.getItem(DEV_MODE_KEY) === 'true')

export function useDevMode() {
  function toggle(value: boolean) {
    devMode.value = value
    localStorage.setItem(DEV_MODE_KEY, String(value))
  }

  return { devMode, toggle }
}
