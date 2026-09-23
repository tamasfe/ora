import { computed, shallowRef, type Ref } from "vue";
import type { DataTablePageEvent } from "primevue/datatable";
import { formatCount } from "./format";

export const pageSizeOptions = [10, 20, 50, 100];

interface PageState {
  /** The query (filters, ordering, page size) the pages belong to. */
  key: string;
  page: number;
  /** Page tokens of the visited pages, the first page has no token. */
  tokens: (string | undefined)[];
  /** The last page, known once the page after it turned out to be empty. */
  lastPage?: number;
}

interface PageResult {
  key: string;
  page: number;
  items: number;
  nextPageToken?: string;
}

/** The positions of lists, e.g. to restore them when navigating back from a detail page. */
const savedStates = new Map<string, PageState>();

function firstPage(key: string): PageState {
  return { key, page: 0, tokens: [undefined] };
}

/**
 * Token-based pagination state for lazy PrimeVue DataTables.
 *
 * The API only supports moving forward with page tokens, so the tokens of visited pages are
 * remembered to support moving back. The state belongs to a query `key` (filters, ordering, page
 * size), any other key starts at the first page right away, so that no request is sent with a
 * token of a different query.
 *
 * The total count is optional and can arrive later, the server returns a page token for full
 * pages even if there are no more items, which is detected once an empty page is loaded.
 */
export function useTokenPagination(options: {
  key: Readonly<Ref<string>>;
  rows: Ref<number>;
  /** The total number of items, if known. */
  count: Readonly<Ref<number | undefined>>;
  /** Remembers the position for the session under this ID. */
  cacheId?: string;
}) {
  const { key, rows, count, cacheId } = options;

  const saved = cacheId ? savedStates.get(cacheId) : undefined;
  const state = shallowRef<PageState>(saved?.key === key.value ? saved : firstPage(key.value));
  const result = shallowRef<PageResult>();

  const current = computed(() =>
    state.value.key === key.value ? state.value : firstPage(key.value),
  );
  const page = computed(() => current.value.page);
  const first = computed(() => page.value * rows.value);
  const pageToken = computed(() => current.value.tokens[page.value]);

  /** The result for the current page, if it has been loaded. */
  const loaded = computed(() => {
    const r = result.value;
    return r && r.key === key.value && r.page === page.value ? r : undefined;
  });

  const hasNext = computed(() => {
    const r = loaded.value;
    const lastPage = current.value.lastPage;

    if (!r?.nextPageToken || r.items < rows.value) {
      return false;
    }

    if (lastPage !== undefined && page.value >= lastPage) {
      return false;
    }

    return count.value === undefined || first.value + r.items < count.value;
  });

  /**
   * The total records for the paginator, only enabling moving to the next page if possible.
   *
   * It is never less than the items seen, as the paginator jumps back otherwise.
   */
  const totalRecords = computed(() => {
    const r = loaded.value;

    if (!r) {
      // Loading, moving forward is not possible yet.
      return first.value + 1;
    }

    const seen = first.value + r.items;
    return hasNext.value ? Math.max(count.value ?? 0, seen + 1) : seen;
  });

  /** The current page report, e.g. "21–40 of 1,234", kept while the next page is loading. */
  const report = computed<string>(previous => {
    const r = loaded.value;

    if (!r) {
      return previous ?? "";
    }

    const range = r.items === 0 ? "0" : `${first.value + 1}–${first.value + r.items}`;
    return count.value === undefined ? range : `${range} of ${formatCount(count.value)}`;
  });

  function save(next: PageState) {
    state.value = next;
    if (cacheId) {
      savedStates.set(cacheId, next);
    }
  }

  /** Updates the state after a page was loaded. */
  function update(forKey: string, forPage: number, items: number, nextPageToken?: string) {
    if (forKey !== key.value || forPage !== page.value) {
      return;
    }

    result.value = { key: forKey, page: forPage, items, nextPageToken };
    const s = current.value;

    if (items === 0 && forPage > 0) {
      // The previous page was the last one, even though it had a page token.
      save({ ...s, page: forPage - 1, lastPage: forPage - 1 });
    } else if (s.lastPage === forPage && nextPageToken !== s.tokens[forPage + 1]) {
      // The last page changed (e.g. new items were added), there might be more pages now.
      save({ ...s, lastPage: undefined });
    }
  }

  function onPage(event: DataTablePageEvent) {
    if (event.rows !== rows.value) {
      // The page size is part of the key, the first page is shown for the new size.
      rows.value = event.rows;
      return;
    }

    const s = current.value;
    const next = loaded.value?.nextPageToken;

    if (event.page === s.page + 1 && next) {
      save({ ...s, page: event.page, tokens: [...s.tokens.slice(0, event.page), next] });
    } else if (event.page < s.page) {
      save({ ...s, page: event.page });
    }
    // Other pages can't be selected, only the next page is ever enabled.
  }

  return {
    key,
    page,
    first,
    pageToken,
    hasNext,
    totalRecords,
    report,
    onPage,
    update,
    /** Paginator template, jumping to arbitrary pages is not supported. */
    paginatorTemplate:
      "FirstPageLink PrevPageLink CurrentPageReport NextPageLink RowsPerPageDropdown",
  };
}
