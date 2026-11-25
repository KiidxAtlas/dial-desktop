import { computed, ref, watch, type Ref } from "vue";
import type { TranscriptResults } from "./types";

const FIRST = 40;
const MAX = 1000; // search_sessions expands the ranked window; there is no offset or stable cursor.

type Search = (root: string, query: string, limit: number) => Promise<TranscriptResults>;

export function useTranscriptSearch(root: () => string, filter: () => string, enabled: Ref<boolean>, search: Search) {
  const results = ref<TranscriptResults | null>(null);
  const searching = ref(false);
  const searchError = ref("");
  const limit = ref(FIRST);
  const canExpand = computed(() => !!results.value?.has_more && limit.value < MAX);
  let revision = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function run(size: number) {
    clearTimeout(timer);
    const query = filter().trim();
    if (!query || !enabled.value || searching.value) return;
    const current = revision;
    searching.value = true;
    searchError.value = "";
    try {
      const found = await search(root(), query, size);
      if (current === revision) {
        results.value = found;
        limit.value = size;
      }
    } catch (error) {
      if (current === revision) searchError.value = String(error);
    } finally {
      if (current === revision) searching.value = false;
    }
  }

  watch([root, () => filter().trim(), enabled], ([, query, on], _, cleanup) => {
    results.value = null;
    searching.value = false;
    searchError.value = "";
    limit.value = FIRST;
    if (on && query) timer = setTimeout(() => void run(FIRST), 300);
    cleanup(() => { clearTimeout(timer); revision++; });
  }, { flush: "sync", immediate: true });

  return { results, searching, searchError, canExpand,
    searchTranscripts: () => run(limit.value),
    loadMore: () => { if (canExpand.value) void run(Math.min(MAX, limit.value * 2)); },
  };
}
