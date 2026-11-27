<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import type { Answer, Attachment, Question, QuestionItem } from "../types";
import { engine, files, showToast, snapshot } from "../bridge";
import { addBlobs, addPaths, IMAGE_FILE } from "../attachments";
import { imageMime } from "../attachmentFormats";
import Icon from "./Icon.vue";

const props = defineProps<{ question: Question }>();

/** Per question id: chosen option labels, free text and images attached to it. */
const picks = reactive<Record<string, { selected: string[]; custom: string; attachments: Attachment[] }>>({});
/** Images need a model that can see them; unknown (`null`) is allowed and the engine decides. */
const canAttach = computed(() => snapshot.value?.model?.acceptsImages !== false);
/** Set once an answer or skip went out, until the engine closes the question. */
const sent = ref(false);
/** With several questions, the one on screen; they are answered one at a time. */
const step = ref(0);
const item = computed(() => props.question.items[Math.min(step.value, props.question.items.length - 1)]);
const last = computed(() => step.value >= props.question.items.length - 1);

const answerable = computed(() => props.question.id !== "");

function reset() {
  for (const key of Object.keys(picks)) delete picks[key];
  for (const item of props.question.items) {
    const suggested = item.recommended === null ? null : item.options[item.recommended]?.label;
    picks[item.id] = { selected: suggested ? [suggested] : [], custom: "", attachments: [] };
  }
  sent.value = false;
  step.value = 0;
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

/** Whether `item` can be sent: a single choice needs a pick, text or an image; "any that apply" may be none. */
function done(item: QuestionItem) {
  const pick = picks[item.id];
  return !!pick && (item.multi || pick.selected.length > 0 || pick.custom.trim() !== "" || pick.attachments.length > 0);
}

const canSubmit = computed(() => answerable.value && !sent.value && props.question.items.every(done));

async function chooseImages(item: QuestionItem) {
  const pick = picks[item.id];
  if (!pick || sent.value) return;
  try {
    const paths = (await files.chooseFiles()).filter((path) => IMAGE_FILE.test(path));
    await addPaths(pick, paths);
  } catch (error) {
    showToast(`Could not attach the image: ${error}`);
  }
}

/** Pasting an image into the answer attaches it; text pastes stay normal. */
function onPaste(item: QuestionItem, event: ClipboardEvent) {
  const pick = picks[item.id];
  const images = Array.from(event.clipboardData?.files ?? []).filter((f) => imageMime(f));
  if (!pick || sent.value || !images.length || !canAttach.value) return;
  event.preventDefault();
  void addBlobs(pick, images);
}

function detach(item: QuestionItem, key: string) {
  const pick = picks[item.id];
  if (pick && !sent.value) pick.attachments = pick.attachments.filter((a) => a.key !== key);
}

async function submit() {
  if (!canSubmit.value) return;
  const answers: Answer[] = props.question.items.map((item) => {
    const pick = picks[item.id];
    const custom = pick.custom.trim();
    const images = pick.attachments.flatMap((a) => (a.path !== null ? [a.path] : []));
    const inlineImages = pick.attachments.flatMap((a) => (a.data !== null ? [a.data] : []));
    return { id: item.id, selected: pick.selected, custom: custom || null, images, inlineImages };
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

/** Enter moves to the next question, and on the last one sends every answer. */
function next() {
  if (!item.value || !done(item.value)) return;
  if (last.value) submit();
  else step.value++;
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    next();
  }
}
</script>

<template>
  <section class="card question" :class="{ sent }" aria-live="polite">
    <div class="card-head">
      <strong>{{ question.items.length > 1 ? "Dial has a few questions" : "Dial has a question" }}</strong>
      <span v-if="question.items.length > 1" class="count" aria-live="polite">{{ step + 1 }} of {{ question.items.length }}</span>
    </div>

    <!-- Only the question scrolls, so the actions stay in reach. -->
    <div v-if="item" :key="item.id" class="items">
      <div class="item">
        <div class="ask">
          <span v-if="item.header" class="pill header">{{ item.header }}</span>
          <p>{{ item.text }}</p>
        </div>
        <span v-if="answerable && item.options.length" class="hint">{{ item.multi ? "Choose any that apply" : "Choose one" }}</span>
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
          <div v-if="picks[item.id]" class="custom-row">
            <input
              v-model="picks[item.id].custom"
              class="custom"
              type="text"
              data-prose
              :disabled="sent"
              :aria-label="item.text"
              :placeholder="item.options.length ? 'Or type your own answer…' : 'Type your answer…'"
              @keydown="onKey"
              @paste="onPaste(item, $event)"
            />
            <button
              v-if="canAttach"
              class="btn btn-secondary attach"
              :disabled="sent"
              title="Attach an image to your answer (or paste one)"
              aria-label="Attach an image"
              @click="chooseImages(item)"
            >
              <Icon name="image" :size="14" />
            </button>
          </div>
          <div v-if="picks[item.id]?.attachments.length" class="thumbs">
            <span v-for="a in picks[item.id].attachments" :key="a.key" class="thumb" :title="a.name">
              <img v-if="a.url" :src="a.url" :alt="a.name" />
              <button class="remove" :disabled="sent" :aria-label="`Remove ${a.name}`" @click="detach(item, a.key)">
                <Icon name="cross" :size="10" />
              </button>
            </span>
          </div>
        </template>
      </div>
    </div>

    <div v-if="answerable" class="actions">
      <span class="hint">Enter {{ last ? "sends" : "continues" }}</span>
      <span class="grow" />
      <button class="btn btn-secondary" :disabled="sent" title="Let Dial continue without an answer" @click="skip">
        Skip
      </button>
      <button v-if="step > 0" class="btn btn-secondary" :disabled="sent" @click="step--">Back</button>
      <button v-if="!last" class="btn btn-primary" :disabled="sent || !item || !done(item)" @click="next">
        Next <Icon name="chevronRight" :size="13" />
      </button>
      <button v-else class="btn btn-primary" :disabled="!canSubmit" @click="submit">
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
.count {
  margin-left: auto;
  font-size: var(--text-sm);
  color: var(--text-faint);
  font-variant-numeric: tabular-nums;
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
.item > .hint {
  display: block;
  margin: 4px 0 0;
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
.custom-row {
  display: flex;
  gap: 6px;
  margin-top: 8px;
}
.custom {
  display: block;
  flex: 1;
  min-width: 0;
  padding: 7px 10px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border);
  background: var(--bg);
  color: var(--text);
  font: inherit;
  outline: none;
}
.attach {
  flex: none;
}
.thumbs {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 6px;
}
.thumb {
  position: relative;
  width: 48px;
  height: 48px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  overflow: hidden;
}
.thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.thumb .remove {
  position: absolute;
  top: 2px;
  right: 2px;
  display: grid;
  place-items: center;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--glass);
  color: var(--text);
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
