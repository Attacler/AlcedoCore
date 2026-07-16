declare namespace alcedocore {
  /** Collection item operations */
  var items: {
    /** Get a single item by ID */
    get(collection: string, id: string): Promise<Record<string, any>>
  }

  /** Key-value store operations */
  var kv: {
    /** Get a value by key */
    get(key: string): Promise<any>
    /** Set a value with optional TTL (seconds) */
    set(key: string, value: any, ttl?: number): Promise<any>
    /** Delete a key */
    delete(key: string): Promise<any>
  }

  /** Execute a SQL query against the plugin schema */
  var db: {
    query(sql: string, params?: string[]): Promise<any>
  }

  /** Health check */
  function health(): Promise<any>
}

/** The event object passed to the function handler */
declare var event: {
  type: 'ItemCreated' | 'ItemUpdated' | 'ItemDeleted'
  data: {
    collection_name: string
    item_id: string
    old_values?: Record<string, any>
    new_values?: Record<string, any>
    diff?: Record<string, { old: any; new: any }>
    values?: Record<string, any>
  }
}
