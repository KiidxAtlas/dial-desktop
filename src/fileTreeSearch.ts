import { computed, ref, watch } from "vue";

/** Search independently of the expanded tree. Invalidate even in-flight lookups on clear or retry. */
export function useFileTreeSearch(search: (query: string) => Promise<string[]>) {
  const filter = ref("");
  const query = computed(() => filter.value.trim());
  const revision = ref(0);
  const paths = ref<string[]>([]);
  const status = ref("");
  watch([query, revision], ([q], _, cleanup) => {
    paths.value = [];
    status.value = q ? "Searching…" : "";
    if (!q) return;
    let current = true;
    const timer = setTimeout(async () => {
      try {
        const found = await search(q);
        if (current) {
          paths.value = found;
          status.value = found.length ? "" : "No matching files";
        }
      } catch (error) {
        if (current) status.value = `File search failed: ${error}`;
      }
    }, 100);
    cleanup(() => { current = false; clearTimeout(timer); });
  }, { flush: "sync" });
  return { filter, query, paths, status, refreshSearch: () => { if (query.value) revision.value++; } };
}
