export function createLogsResource(ky: any) {
  return {
    list: (
      slug: string,
      options?: any & {
        limit?: number;
        offset?: number;
        start_date?: string;
        end_date?: string;
        target?: string;
        operation_type?: string;
        item_id?: string;
      },
    ) => ky.get(`plugins/${slug}/logs`, options).json(),
  };
}
