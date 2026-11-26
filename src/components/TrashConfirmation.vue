<script setup lang="ts">
import { ref, watch } from 'vue';
import { trashPrompt } from '../bridge';
const dialog = ref<HTMLDialogElement>();
watch(trashPrompt, prompt => { if (prompt) dialog.value?.showModal(); else dialog.value?.close(); }, { flush: 'post' });
</script>
<template>
  <dialog ref="dialog" aria-labelledby="trash-title" aria-describedby="trash-message" @cancel.prevent="trashPrompt?.finish(false)" @keydown.stop>
    <template v-if="trashPrompt">
      <h2 id="trash-title">{{ trashPrompt.title }}</h2>
      <p id="trash-message">{{ trashPrompt.message }}</p>
      <footer><button autofocus @click="trashPrompt.finish(false)">Cancel</button><button class="confirm" @click="trashPrompt.finish(true)">{{ trashPrompt.action }}</button></footer>
    </template>
  </dialog>
</template>
<style scoped>
dialog { margin: auto; width: min(380px, 90vw); padding: 20px; background: var(--surface); color: var(--text); border: 1px solid var(--border); border-radius: 12px; box-shadow: 0 12px 48px #0004; }
dialog::backdrop { background: #0006; }
h2 { margin: 0; font-size: 17px; }
p { line-height: 1.5; overflow-wrap: anywhere; }
footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 18px; }
button { border: 1px solid var(--border); padding: 6px 12px; border-radius: 6px; }
.confirm { color: var(--danger); }
</style>
