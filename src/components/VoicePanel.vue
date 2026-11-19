<script setup lang="ts">
import { computed } from "vue";
import { engine } from "../bridge";
import type { VoiceView } from "../types";
import Icon from "./Icon.vue";

const props = defineProps<{ voice: VoiceView; turnRunning: boolean }>();
const emit = defineEmits<{ edit: [text: string] }>();

/** Dial is working on what it heard, so a misheard request can still be stopped. */
const misheardChance = computed(() => props.voice.phase === "thinking" && props.turnRunning && !!props.voice.heard);

function editHeard() {
  const text = props.voice.heard;
  if (!text) return;
  void engine.voiceCancel();
  emit("edit", text);
}

const status = computed(() => {
  switch (props.voice.phase) {
    case "starting": return "Starting voice…";
    case "listening": return props.voice.hearing ? "Hearing you…" : "Listening…";
    case "thinking": return "Dial is working…";
    case "speaking": return props.voice.muted ? "Replies muted" : "Speaking";
    case "paused": return props.voice.problem ?? "Tap to talk";
  }
  return "";
});

/** What tapping the orb does right now, for its label; `null` when it does nothing. */
const action = computed(() => {
  switch (props.voice.phase) {
    case "listening": return "Stop listening";
    case "speaking": return props.turnRunning ? null : "Interrupt and talk";
    case "paused": return "Talk";
  }
  return null;
});
</script>

<template>
  <section class="voice" :class="voice.phase" :style="{ '--level': voice.level / 100 }" aria-label="Voice mode" aria-live="polite">
    <button
      class="orb"
      :disabled="!action"
      :aria-label="action ?? status"
      :title="action ?? status"
      @click="engine.voiceTap()"
    >
      <span class="halo" aria-hidden="true" />
      <span class="ring" />
      <span class="bars" aria-hidden="true"><i /><i /><i /><i /></span>
      <Icon v-if="voice.phase === 'listening' || voice.phase === 'paused' || voice.phase === 'starting'" name="mic" :size="20" />
    </button>
    <div class="text">
      <div class="status" :class="{ problem: voice.phase === 'paused' && voice.problem }">{{ status }}</div>
      <div v-if="voice.heard" class="heard truncate" :title="voice.heard">“{{ voice.heard }}”</div>
      <div v-else class="heard hint">Speak, then pause; Dial answers out loud.</div>
    </div>
    <div v-if="misheardChance" class="misheard">
      <button class="btn btn-secondary" title="Stop this request; tap the orb to talk again" @click="engine.voiceCancel()">Cancel</button>
      <button class="btn btn-secondary" title="Stop this request and fix the text in the message box" @click="editHeard">Edit</button>
    </div>
    <button
      class="mute"
      :aria-pressed="voice.muted"
      :title="voice.muted ? 'Read replies aloud' : 'Mute replies'"
      :aria-label="voice.muted ? 'Unmute replies' : 'Mute replies'"
      @click="engine.voiceMute(!voice.muted)"
    >
      <Icon :name="voice.muted ? 'volumeOff' : 'volume'" :size="16" />
    </button>
    <button class="end" title="End voice mode" aria-label="End voice mode" @click="engine.voiceStop()">
      <Icon name="cross" :size="15" />
    </button>
  </section>
</template>

<style scoped>
.voice {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 12px 14px 12px 12px;
  border-radius: var(--radius-xl);
  background: var(--glass);
  -webkit-backdrop-filter: var(--glass-blur);
  backdrop-filter: var(--glass-blur);
  border: 1px solid var(--border-strong);
}
.orb {
  position: relative;
  flex: none;
  display: grid;
  place-items: center;
  width: 48px;
  height: 48px;
  border-radius: 50%;
  background: var(--accent);
  color: var(--on-accent);
  transition: transform 160ms var(--ease), background 160ms var(--ease);
}
.orb:not(:disabled):hover { transform: scale(1.05); }
.orb:disabled { cursor: default; }
.paused .orb, .starting .orb { background: var(--raised); color: var(--text); border: 1px solid var(--border-strong); }
.ring {
  position: absolute;
  inset: -4px;
  border-radius: 50%;
  border: 2px solid var(--accent);
  opacity: 0;
  pointer-events: none;
}
.listening .ring { animation: pulse 1.4s var(--ease) infinite; }
/* The microphone level: a halo that grows with how loud the room is. */
.halo {
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: var(--accent);
  opacity: 0;
  pointer-events: none;
  z-index: -1;
  transition: transform 90ms linear, opacity 90ms linear;
}
.listening .halo {
  opacity: calc(0.15 + var(--level, 0) * 0.35);
  transform: scale(calc(1 + var(--level, 0) * 0.55));
}
.misheard { display: flex; gap: 6px; flex: none; }
.misheard .btn { padding: 4px 10px; font-size: var(--text-sm); }
.thinking .ring {
  opacity: 1;
  border-color: var(--accent) transparent transparent transparent;
  animation: spin 0.9s linear infinite;
}
.bars { display: none; gap: 3px; align-items: center; height: 18px; }
.speaking .bars, .thinking .bars { display: flex; }
.bars i {
  display: block;
  width: 3px;
  height: 6px;
  border-radius: 2px;
  background: currentColor;
}
.speaking .bars i { animation: wave 0.9s ease-in-out infinite; }
.speaking .bars i:nth-child(2) { animation-delay: -0.6s; }
.speaking .bars i:nth-child(3) { animation-delay: -0.3s; }
.speaking .bars i:nth-child(4) { animation-delay: -0.45s; }
.text { flex: 1; min-width: 0; }
.status { font-weight: 600; color: var(--text); }
.status.problem { font-weight: 500; color: var(--text-muted); }
.heard { margin-top: 2px; font-size: var(--text-sm); color: var(--text-muted); }
.heard.hint { color: var(--text-faint); }
.mute {
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  border-radius: var(--radius-md);
  color: var(--text-muted);
  transition: background 120ms var(--ease), color 120ms var(--ease);
}
.mute:hover { background: var(--hover); color: var(--text); }
.mute[aria-pressed="true"] { color: var(--accent); }
.end {
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  border-radius: 50%;
  background: var(--danger, #d9534f);
  color: #fff;
  transition: filter 120ms var(--ease);
}
.end:hover { filter: brightness(1.1); }
@keyframes pulse {
  0% { opacity: 0.7; transform: scale(0.95); }
  100% { opacity: 0; transform: scale(1.35); }
}
@keyframes spin { to { transform: rotate(360deg); } }
@keyframes wave {
  0%, 100% { height: 5px; }
  50% { height: 18px; }
}
@media (prefers-reduced-motion: reduce) {
  .ring, .bars i { animation: none !important; }
  .listening .ring { opacity: 0.6; }
}
</style>
