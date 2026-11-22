<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { showToast } from "../../bridge";
import { assetPath, typeApi, type Full } from "../../savedTypes";
import { loadAsset } from "./TypeImage.vue";

/**
 * A type's own page, from its blueprint's `customHtml`, served by the `dialtype:` protocol with a
 * policy that allows only its inline code. The frame is sandboxed without same-origin access, so
 * it cannot reach the app; it receives the items by message and may ask to change or add one.
 */
const props = defineProps<{ full: Full; platform: string }>();

const frame = ref<HTMLIFrameElement | null>(null);
const src = computed(() => {
  const base = props.platform === "windows" ? "http://dialtype.localhost" : "dialtype://localhost";
  return `${base}/${props.full.def.id}?v=${props.full.def.updated}`;
});

async function images(): Promise<Record<string, string>> {
  const out: Record<string, string> = {};
  const kinds = new Set(props.full.def.fields.filter((f) => f.kind === "image" || f.kind === "images").map((f) => f.key));
  const names = new Set<string>();
  for (const item of props.full.items) {
    for (const key of kinds) {
      const value = item.fields[key];
      for (const name of Array.isArray(value) ? value : [value]) if (typeof name === "string") names.add(name);
    }
  }
  await Promise.all(
    [...names].slice(0, 120).map(async (name) => {
      try {
        out[name] = (await loadAsset(assetPath(props.full, name)!)).url;
      } catch {
        // A missing image is left out; the page shows what it can.
      }
    }),
  );
  return out;
}

async function post() {
  const target = frame.value?.contentWindow;
  if (!target) return;
  const data = JSON.parse(JSON.stringify({ type: "dial-type", def: props.full.def, items: props.full.items }));
  data.images = await images();
  target.postMessage(data, "*");
}

watch(() => props.full, () => void post());

const ASSET_RE = /^[0-9a-f]{32}\.(png|jpg|gif|webp)$/;

/** Drop non-asset image values from a sandboxed frame so it can never import arbitrary host files. */
function sanitizeFromFrame(fields: Record<string, unknown>): Record<string, unknown> {
  const imageKinds = new Map(
    props.full.def.fields
      .filter((f) => f.kind === "image" || f.kind === "images")
      .map((f) => [f.key, f.kind]),
  );
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(fields)) {
    const kind = imageKinds.get(key);
    if (!kind) {
      out[key] = value;
    } else if (value === null) {
      out[key] = null;
    } else if (kind === "image" && typeof value === "string" && ASSET_RE.test(value.trim())) {
      out[key] = value.trim();
    } else if (kind === "images" && Array.isArray(value)) {
      out[key] = value.filter((v): v is string => typeof v === "string" && ASSET_RE.test(v.trim())).map((v) => v.trim());
    }
  }
  return out;
}

async function onMessage(event: MessageEvent) {
  if (event.source !== frame.value?.contentWindow || !event.data || typeof event.data !== "object") return;
  const message = event.data as { type?: string; itemId?: string; fields?: Record<string, unknown>; done?: boolean };
  try {
    if (message.type === "dial-type-update" && typeof message.itemId === "string") {
      const fields = message.fields && typeof message.fields === "object" ? sanitizeFromFrame(message.fields) : undefined;
      await typeApi.updateItem(
        props.full.def.id,
        message.itemId,
        {
          ...(fields && Object.keys(fields).length ? { fields } : {}),
          ...(typeof message.done === "boolean" ? { done: message.done } : {}),
        },
        false,
      );
    } else if (message.type === "dial-type-add" && message.fields && typeof message.fields === "object") {
      await typeApi.addItem(props.full.def.id, sanitizeFromFrame(message.fields), null, false);
    } else if (message.type === "dial-type-ready") {
      await post();
    }
  } catch (error) {
    showToast(String(error));
  }
}
onMounted(() => window.addEventListener("message", onMessage));
onBeforeUnmount(() => window.removeEventListener("message", onMessage));
</script>

<template>
  <iframe
    ref="frame"
    class="custom"
    :src="src"
    sandbox="allow-scripts"
    referrerpolicy="no-referrer"
    :title="full.def.name"
    @load="post"
  />
</template>

<style scoped>
.custom {
  display: block;
  flex: 1;
  width: 100%;
  min-height: 0;
  border: 0;
  background: transparent;
}
</style>
