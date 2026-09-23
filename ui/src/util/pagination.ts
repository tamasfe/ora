import { computed, ref } from "vue";
import type { DataTablePageEvent } from "primevue/datatable";

export const pageSizeOptions = [10, 20, 50, 100];

/**
 * Token-based pagination state that can be used with lazy PrimeVue DataTables.
 *
 * The API only supports moving forward with page tokens,
 * so tokens of previously visited pages are remembered to support moving back.
 */
export function useTokenPagination(initialRows = 20) {
  const rows = ref(initialRows);
  const page = ref(0);
  /** Page tokens for each visited page, the first page has no token. */
  const tokens = ref<(string | undefined)[]>([undefined]);
  /** The token for the page after the current one (if any). */
  const nextPageToken = ref<string>();

  const pageToken = computed(() => tokens.value[page.value]);
  const first = computed(() => page.value * rows.value);

  function reset() {
    page.value = 0;
    tokens.value = [undefined];
    nextPageToken.value = undefined;
  }

  /**
   * Updates the state after a page was loaded.
   *
   * The server might return a page token even if there are no more items,
   * so short pages and the total count are also taken into account.
   */
  function update(itemCount: number, token: string | undefined, count?: number) {
    const isLast =
      itemCount < rows.value || (count !== undefined && first.value + itemCount >= count);
    nextPageToken.value = isLast ? undefined : token;
  }

  function onPage(event: DataTablePageEvent) {
    if (event.rows !== rows.value) {
      rows.value = event.rows;
      reset();
      return;
    }

    if (event.page > page.value) {
      if (nextPageToken.value) {
        tokens.value = [...tokens.value.slice(0, page.value + 1), nextPageToken.value];
        page.value += 1;
      }
    } else if (event.page < page.value) {
      page.value = event.page;
    }
  }

  /**
   * Calculates the total record count for the paginator,
   * the actual count might not be known or can differ between requests.
   */
  function totalRecords(count: number | undefined, itemsOnPage: number) {
    const seen = first.value + itemsOnPage;

    if (nextPageToken.value) {
      return Math.max(count ?? 0, seen + 1);
    }

    return seen;
  }

  return {
    rows,
    first,
    pageToken,
    nextPageToken,
    onPage,
    update,
    reset,
    totalRecords,
    /** Paginator template, jumping to arbitrary pages is not supported. */
    paginatorTemplate:
      "FirstPageLink PrevPageLink CurrentPageReport NextPageLink RowsPerPageDropdown",
  };
}
