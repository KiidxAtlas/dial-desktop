<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import type { Answer, Question, QuestionItem } from "../types";
import { engine } from "../bridge";
import Icon from "./Icon.vue";

const props = defineProps<{ question: Question }>();

/** Per question id: chosen option labels and free text. */
const picks = reactive<Record<string, { selected: string[]; custom: string }>>({});
/** Set once an answer or skip went out, until the engine closes the question. */
const sent = ref(false);

const answerable = computed(() => props.question.id !== "");

function reset() {
  for (const key of Object.keys(picks)) delete picks[key];
  for (const item of props.question.items) {
    const suggested = item.recommended === null ? null : item.options[item.recommended]?.label;
    picks[item.id] = { selected: suggested ? [suggested] : [], custom: "" };
  }
  sent.value = false;
}

// A new question resets the form and tells the engine it is on screen (this starts its timeout).
watch(
  () => props.question.id,
  (id) => {
    reset();
    if (id) engine.questionPresented(id);
  },
  { immediate: true },
);

function toggle(item: QuestionItem, label: string) {
  const pick = picks[item.id];
  if (!pick || sent.value) return;
  if (item.multi) {
    pick.selected = pick.selected.includes(label)
      ? pick.selected.filter((l) => l !== label)
      : [...pick.selected, label];
  } else {
    pick.selected = pick.selected[0] === label ? [] : [label];
  }
}

/** Whether `item` can be sent: a single choice needs a pick or text; "any that apply" may be none. */
function done(item: QuestionItem) {
  const pick = picks[item.id];
  return !!pick && (item.multi || pick.selected.length > 0 || pick.custom.trim() !== "");
}

const canSubmit = computed(() => answerable.value && !sent.value && props.question.items.every(done));

async function submit() {
  if (!canSubmit.value) return;
  const answers: Answer[] = props.question.items.map((item) => {
    const pick = picks[item.id];
    const custom = pick.custom.trim();
    return { id: item.id, selected: pick.selected, custom: custom || null };
  });
  sent.value = true;
  const id = props.question.id;
  if (!(await engine.answerQuestion(id, answers)) && props.question.id === id) sent.value = false;
}

async function skip() {
  if (!answerable.value || sent.value) return;
  sent.value = true;
  const id = props.question.id;
  if (!(await engine.skipQuestion(id)) && props.question.id === id) sent.value = false;
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    submit();
  }
}
</script>

<template>
  <section class="card question" :class="{ sent }" aria-live="polite">
    <div class="card-head">
      <strong>{{ question.items.length > 1 ? `Dial has ${question.items.length} questions` : "Dial has a question" }}</strong>
    </div>

    <!-- Only the questions scroll, so the actions stay in reach. -->
    <div class="items">
      <div v-for="item in question.items" :key="item.id" class="item">
        <div class="ask">
          <span v-if="item.header" class="pill header">{{ item.header }}</span>
          <p>{{ item.text }}</p>
        </div>
        <template v-if="answerable">
          <div v-if="item.options.length" class="options" :role="item.multi ? 'group' : 'radiogroup'">
            <button
              v-for="(option, index) in item.options"
              :key="option.label"
              class="option"
              :class="{ on: picks[item.id]?.selected.includes(option.label) }"
              :role="item.multi ? 'checkbox' : 'radio'"
              :aria-checked="picks[item.id]?.selected.includes(option.label) ?? false"
              :disabled="sent"
              @click="toggle(item, option.label)"
            >
              <span class="mark" :class="item.multi ? 'box' : 'circle'">
                <Icon v-if="picks[item.id]?.selected.includes(option.label)" name="check" :size="11" />
              </span>
              <span class="body">
                <span class="label">
                  {{ option.label }}
                  <span v-if="index === item.recommended" class="suggested">Suggested</span>
                </span>
                <span v-if="option.description" class="description">{{ option.description }}</span>
                <pre v-if="option.preview" class="preview">{{ option.preview }}</pre>
              </span>
            </button>
          </div>
          <input
            v-if="picks[item.id]"
            v-model="picks[item.id].custom"
            class="custom"
            type="text"
            data-prose
            :disabled="sent"
            :aria-label="item.text"
            :placeholder="item.options.length ? 'Or type your own answer…' : 'Type your answer…'"
            @keydown="onKey"
          />
        </template>
      </div>
    </div>

    <div v-if="answerable" class="actions">
      <span class="hint">{{ question.items.some((i) => i.multi) ? "Choose any that apply" : "Enter submits" }}</span>
      <span class="grow" />
      <button class="btn btn-secondary" :disabled="sent" title="Let Dial continue without an answer" @click="skip">
        Skip
      </button>
      <button class="btn btn-primary" :disabled="!canSubmit" @click="submit">
        <Icon name="check" :size="14" /> {{ sent ? "Sent" : "Answer" }}
      </button>
    </div>
    <p v-else class="hint">This question can't be answered from the desktop app.</p>
  </section>
</template>

<style scoped>
.card {
  display: flex;
  flex-direction: column;
  max-height: 55vh;
  padding: 14px 16px;
  border-radius: var(--radius-lg);
  background: var(--glass);
  -webkit-backdrop-filter: var(--glass-blur);
  backdrop-filter: var(--glass-blur);
  border: 1px solid var(--border-strong);
  --tone: var(--accent);
}
.card-head {
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
}
.card-head strong {
  font-weight: 600;
}
.items {
  min-height: 0;
  overflow: auto;
  margin-right: -6px;
  padding-right: 6px;
}
.item {
  margin-top: 12px;
}
.item + .item {
  padding-top: 12px;
  border-top: 1px solid var(--border);
}
.ask {
  display: flex;
  align-items: baseline;
  gap: 8px;
}
.ask p {
  margin: 0;
  user-select: text;
  -webkit-user-select: text;
}
.header {
  flex: none;
  color: var(--text-muted);
  background: var(--hover);
}
.options {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-top: 8px;
}
.option {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  width: 100%;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border);
  text-align: left;
  transition:
    background 120ms var(--ease),
    border-color 120ms var(--ease);
}
.option:hover:not(:disabled) {
  background: var(--hover);
}
.option.on {
  border-color: var(--border-strong);
  background: var(--active);
}
.mark {
  flex: none;
  display: grid;
  place-items: center;
  width: 16px;
  height: 16px;
  margin-top: 2px;
  border: 1.5px solid var(--border-strong);
  color: var(--bg);
}
.mark.circle {
  border-radius: 50%;
}
.mark.box {
  border-radius: 4px;
}
.option.on .mark {
  border-color: var(--text);
  background: var(--text);
}
.body {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.label {
  font-weight: 500;
}
.suggested {
  margin-left: 6px;
  font-size: var(--text-sm);
  font-weight: 400;
  color: var(--accent);
}
.description {
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.preview {
  margin: 4px 0 0;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  background: var(--bg);
  font: var(--text-mono) / 1.5 var(--font-mono);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 120px;
  overflow: auto;
}
.custom {
  display: block;
  width: 100%;
  margin-top: 8px;
  padding: 7px 10px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border);
  background: var(--bg);
  color: var(--text);
  font: inherit;
  outline: none;
}
.custom:focus {
  border-color: var(--border-strong);
}
.custom::placeholder {
  color: var(--text-faint);
}
.actions {
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 14px;
}
.grow {
  flex: 1;
}
.hint {
  margin: 8px 0 0;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.actions .hint {
  margin: 0;
}
.sent {
  opacity: 0.75;
}
</style>
