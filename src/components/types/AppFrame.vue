<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { openExternal } from "../../bridge";
import { loadAsset, typeApi, type Fields, type Full } from "../../savedTypes";
import * as typeHost from "../../typeHost";

/**
 * A type's own app: the agent's HTML, served by the `dialtype:` scheme with the `window.dial`
 * runtime (`src-tauri/src/types/runtime.js`). The frame is sandboxed without same-origin access,
 * so it cannot reach this window, its files or its IPC. It talks to this host by message only,
 * and every request it makes is checked here and again by the Rust side.
 */
const props = defineProps<{ full: Full; platform: string }>();

const frame = ref<HTMLIFrameElement | null>(null);
const id = computed(() => props.full.def.id);
const src = computed(() => {
  const base = props.platform === "windows" ? "http://dialtype.localhost" : "dialtype://localhost";
  return `${base}/${id.value}?v=${props.full.appVersion}`;
});

/** Asset names look like this; nothing else from the frame is ever read as a file. */
const ASSET = /^[0-9a-f]{32}\.(png|jpg|gif|webp)$/;
/** Images saved by the app since the last reload of the type, until the next one lists them. */
const saved: Record<string, string> = {};
let ready = false;

function assetNames(): Set<string> {
  const names = new Set<string>();
  const visit = (value: unknown) => {
    if (typeof value === "string") {
      if (ASSET.test(value)) names.add(value);
    } else if (Array.isArray(value)) value.forEach(visit);
  };
  for (const item of props.full.items) Object.values(item.fields).forEach(visit);
  return names;
}

async function images(): Promise<Record<string, string>> {
  const out: Record<string, string> = { ...saved };
  const root = props.full.assets.replace(/[\\/]+$/, "");
  await Promise.all(
    [...assetNames()].slice(0, 200).map(async (name) => {
      try {
        out[name] = (await loadAsset(`${root}/${name}`)).url;
      } catch {
        // A missing image is left out; the app shows what it can.
      }
    }),
  );
  return out;
}

function post(message: object) {
  frame.value?.contentWindow?.postMessage({ dial: 1, ...message }, "*");
}

async function sendData() {
  if (!ready) return;
  const { def, items, state } = props.full;
  const type = { id: def.id, name: def.name, description: def.description, icon: def.icon, accent: def.accent, fields: def.fields };
  const data = JSON.parse(JSON.stringify({ kind: "data", type, items, state }));
  data.images = await images();
  post(data);
}

watch(() => props.full, () => void sendData());
// A new version of the app reloads the frame: it is not loaded until it says so.
watch(src, () => {
  ready = false;
  typeHost.update(id.value, { loaded: false, version: props.full.appVersion });
});

/** Only plain values for item fields; image values must be asset names the app already holds. */
function fieldsFrom(value: unknown): Fields | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  return JSON.parse(JSON.stringify(value)) as Fields;
}

async function answer(op: string, args: Record<string, unknown>): Promise<unknown> {
  const typeId = id.value;
  switch (op) {
    case "add": {
      const fields = fieldsFrom(args.fields);
      if (!fields) throw new Error("dial.add needs an object of fields");
      return typeApi.addItem(typeId, fields, null, false);
    }
    case "update": {
      if (typeof args.itemId !== "string") throw new Error("dial.update needs an item id");
      const fields = fieldsFrom(args.fields);
      return typeApi.updateItem(typeId, args.itemId, {
        ...(fields ? { fields } : {}),
        ...(typeof args.done === "boolean" ? { done: args.done } : {}),
      }, false);
    }
    case "remove":
      if (typeof args.itemId !== "string") throw new Error("dial.remove needs an item id");
      return typeApi.removeItem(typeId, args.itemId);
    case "move":
      if (typeof args.itemId !== "string" || typeof args.to !== "number") throw new Error("dial.move needs an item id and an index");
      return typeApi.moveItem(typeId, args.itemId, Math.max(0, Math.floor(args.to)));
    case "setState":
      return typeApi.setState(typeId, args.state ?? {});
    case "saveImage": {
      if (typeof args.data !== "string") throw new Error("dial.saveImage needs an image");
      const name = await typeApi.importImageData(typeId, args.data);
      try {
        saved[name] = (await loadAsset(`${props.full.assets.replace(/[\\/]+$/, "")}/${name}`)).url;
        void sendData();
      } catch {
        // The name still works once the type reloads.
      }
      return name;
    }
    case "notify":
      await typeApi.allowNotifications().catch(() => {});
      return typeApi.notify(typeId, String(args.title ?? props.full.def.name).slice(0, 200), String(args.body ?? "").slice(0, 1000));
    case "open": {
      const url = String(args.url ?? "");
      if (!/^(https?:\/\/|mailto:)/i.test(url)) throw new Error("dial.open takes an http(s) or mailto link");
      openExternal(url);
      return null;
    }
    default:
      throw new Error(`Unknown request ${op}`);
  }
}

interface FrameMessage { dial?: number; kind?: string; id?: number; op?: string; args?: Record<string, unknown>; level?: string; text?: string; ok?: boolean; value?: unknown; error?: string }

async function onMessage(event: MessageEvent) {
  if (event.source !== frame.value?.contentWindow) return;
  const message = event.data as FrameMessage;
  if (!message || typeof message !== "object" || message.dial !== 1) return;
  switch (message.kind) {
    case "ready":
      ready = true;
      return void sendData();
    case "loaded":
      return typeHost.update(id.value, { loaded: true, version: props.full.appVersion });
    case "log":
      return typeHost.log(id.value, { level: String(message.level ?? "log"), text: String(message.text ?? "") });
    case "eval-result":
      if (typeof message.id !== "number") return;
      return typeHost.settle(message.id, message.ok ? { ok: true, value: message.value ?? null } : { ok: false, error: String(message.error ?? "The script failed") });
    case "call": {
      if (typeof message.id !== "number" || typeof message.op !== "string") return;
      try {
        const value = await answer(message.op, message.args ?? {});
        post({ kind: "reply", id: message.id, ok: true, value: value === undefined ? null : JSON.parse(JSON.stringify(value)) });
      } catch (error) {
        const text = error instanceof Error ? error.message : String(error);
        typeHost.log(id.value, { level: "error", text: `dial.${message.op} failed: ${text}` });
        post({ kind: "reply", id: message.id, ok: false, error: text });
      }
    }
  }
}

onMounted(() => {
  window.addEventListener("message", onMessage);
  typeHost.update(id.value, { loaded: false, version: props.full.appVersion, post });
});
onBeforeUnmount(() => {
  window.removeEventListener("message", onMessage);
  typeHost.update(id.value, { loaded: false, post: null });
});
</script>

<template>
  <iframe
    ref="frame"
    class="app-frame"
    :src="src"
    sandbox="allow-scripts allow-forms allow-pointer-lock"
    allow="fullscreen; clipboard-write; geolocation"
    referrerpolicy="no-referrer"
    :title="full.def.name"
  />
</template>

<style scoped>
.app-frame {
  display: block;
  flex: 1;
  width: 100%;
  min-height: 0;
  border: 0;
  background: transparent;
  color-scheme: normal;
}
</style>
