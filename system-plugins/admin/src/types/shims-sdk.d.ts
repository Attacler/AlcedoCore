import type { AlcedoSDK } from '../sdk/types'

declare global {
  interface Window {
    __alcedo_sdk?: AlcedoSDK
  }
}

export {}
