export function createCollectionsResource(ky: any) {
  return {
    list: (options?: any) => ky.get("collections", options).json(),
    get: (name: string, options?: any) => ky.get(`collections/${encodeURIComponent(name)}`, options).json(),
    create: (data: any, options?: any) => ky.post("collections", { json: data, ...options }).json(),
    update: (name: string, data: any, options?: any) => ky.put(`collections/${encodeURIComponent(name)}`, { json: data, ...options }).json(),
    delete: (name: string, options?: any) => ky.delete(`collections/${encodeURIComponent(name)}`, options).json(),

    // Layouts
    listLayouts: (name: string, options?: any) => ky.get(`collections/${encodeURIComponent(name)}/layouts`, options).json(),
    createLayout: (name: string, data: any, options?: any) => ky.post(`collections/${encodeURIComponent(name)}/layouts`, { json: data, ...options }).json(),
    updateLayout: (name: string, layoutId: string, data: any, options?: any) => ky.put(`collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}`, { json: data, ...options }).json(),
    deleteLayout: (name: string, layoutId: string, options?: any) => ky.delete(`collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}`, options).json(),
    getLayoutRoles: (name: string, layoutId: string, options?: any) => ky.get(`collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/roles`, options).json(),
    setLayoutRoles: (name: string, layoutId: string, roleIds: string[], options?: any) => ky.put(`collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/roles`, { json: { role_ids: roleIds }, ...options }).json(),
    getResolvedLayout: (name: string, options?: any) => ky.get(`collections/${encodeURIComponent(name)}/layout`, options).json(),

    // Layout-scoped sections
    listLayoutSections: (name: string, layoutId: string, options?: any) => ky.get(`collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/sections`, options).json(),
    createLayoutSection: (name: string, layoutId: string, data: any, options?: any) => ky.post(`collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/sections`, { json: data, ...options }).json(),
    updateLayoutSection: (name: string, layoutId: string, sectionId: string, data: any, options?: any) => ky.put(`collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/sections/${encodeURIComponent(sectionId)}`, { json: data, ...options }).json(),
    deleteLayoutSection: (name: string, layoutId: string, sectionId: string, options?: any) => ky.delete(`collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/sections/${encodeURIComponent(sectionId)}`, options).json(),
    batchReorderSections: (name: string, layoutId: string, sections: any[], options?: any) => ky.patch(`collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/sections`, { json: { sections }, ...options }).json(),

    // Saved views
    listViews: (name: string, options?: any) => ky.get(`collections/${encodeURIComponent(name)}/views`, options).json(),
    createView: (name: string, data: any, options?: any) => ky.post(`collections/${encodeURIComponent(name)}/views`, { json: data, ...options }).json(),
    updateView: (name: string, viewId: string, data: any, options?: any) => ky.put(`collections/${encodeURIComponent(name)}/views/${encodeURIComponent(viewId)}`, { json: data, ...options }).json(),
    deleteView: (name: string, viewId: string, options?: any) => ky.delete(`collections/${encodeURIComponent(name)}/views/${encodeURIComponent(viewId)}`, options).json(),
    setDefaultView: (name: string, viewId: string, options?: any) => ky.put(`collections/${encodeURIComponent(name)}/views/${encodeURIComponent(viewId)}/default`, options).json(),
    getCreatePolicy: (name: string, options?: any) => ky.get(`collections/${encodeURIComponent(name)}/$create`, options).json(),
  };
}
