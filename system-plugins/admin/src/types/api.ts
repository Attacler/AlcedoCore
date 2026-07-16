export interface ApiListResponse<T> {
  data: T[]
  total: number
  limit: number
  offset: number
}

export interface ApiResponse<T> {
  data: T
}

export interface BackendPlugin {
  slug: string
  display_name: string | null
  description: string | null
  image: string
  plugin_type: string
  version: string
  status: string
  tags: string[]
}

export interface BackendPluginDetail extends BackendPlugin {
  env: Record<string, string>
  resources: Record<string, any>
  endpoints: any[]
  documentation: any
  settings_schema: any
  settings: any
  requested_scopes: string[]
  granted_scopes: string[]
  created_at: string | null
  updated_at: string | null
}

export interface PaginationParams {
  limit?: number
  offset?: number
  sort?: string
  order?: 'asc' | 'desc'
}

export interface PermissionCheck {
  create: boolean
  read: boolean
  update: boolean
  delete: boolean
  fields?: string[] | null
}
