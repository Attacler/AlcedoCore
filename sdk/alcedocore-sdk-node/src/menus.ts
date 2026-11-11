export interface MenuItem {
  id: string;
  label: string;
  icon: string;
  visible: boolean;
  route?: string | null;
  url?: string | null;
  external?: boolean;
  linkType?: string | null;
  children?: MenuItem[];
}

export interface MenuSection {
  id: string;
  label: string;
  icon: string;
  visible: boolean;
  items: MenuItem[];
}

export interface Menu {
  id: string;
  name: string;
  icon: string;
  sections: MenuSection[];
}

export interface MenuListEntry {
  id: string;
  name: string;
  icon: string;
  role_count: number;
  item_count: number;
  created_at: string;
}

export function createMenusResource(ky: any) {
  return {
    list: (options?: any) => ky.get("app/menus", options).json(),
    get: (id: string, options?: any) =>
      ky.get(`app/menus/${encodeURIComponent(id)}`, options).json(),
    create: (data: any, options?: any) =>
      ky.post("app/menus", { json: data, ...options }).json(),
    update: (id: string, data: any, options?: any) =>
      ky
        .put(`app/menus/${encodeURIComponent(id)}`, { json: data, ...options })
        .json(),
    delete: (id: string, options?: any) =>
      ky.delete(`app/menus/${encodeURIComponent(id)}`, options).json(),
    my: (options?: any) => ky.get("app/menus/my", options).json(),
    getRoles: (id: string, options?: any) =>
      ky.get(`app/menus/${encodeURIComponent(id)}/roles`, options).json(),
    setRoles: (id: string, roleIds: string[], options?: any) =>
      ky
        .put(`app/menus/${encodeURIComponent(id)}/roles`, {
          json: { role_ids: roleIds },
          ...options,
        })
        .json(),
    copy: (id: string, sourceMenuId: string, options?: any) =>
      ky
        .post(`app/menus/${encodeURIComponent(id)}/copy`, {
          json: { source_menu_id: sourceMenuId },
          ...options,
        })
        .json(),
  };
}
