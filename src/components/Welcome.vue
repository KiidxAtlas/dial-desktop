<script setup lang="ts">
// Shown when no folder is open: pick a project, like starting a terminal agent in one.
import { computed, onMounted } from "vue";
import { folders } from "../bridge";
import { installEngine, updates } from "../updates";
import { openMenu, ui } from "../ui";
import { clearRecentProjects, forgetProject, loadRecentProjects, projectItems, recentProjects as recent } from "../projects";
import { welcomeSteps } from "../setupState";
import Icon from "./Icon.vue";
import Logo from "./Logo.vue";

onMounted(loadRecentProjects);
/** First run only: once there are recent projects the user has been through it. */
const steps = computed(() => welcomeSteps(updates.engine, updates.engineChecking));
const showSteps = computed(() => !recent.value.length || steps.value[0].state !== "done");
</script>

<template>
  <div class="welcome" data-tauri-drag-region>
    <div class="panel">
      <div class="hero">
        <Logo :size="56" />
        <h1>Welcome to Dial</h1>
        <p>Choose a project folder. Dial starts its engine there and works on that code.</p>
      </div>
      <button class="btn btn-primary open" @click="folders.choose()">
        <Icon name="folder" :size="16" /> Open folder…
      </button>
      <button class="btn btn-secondary settings" title="Appearance, notifications and updates (⌘,)" @click="ui.settingsOpen = true">
        <Icon name="gear" :size="14" /> Settings
      </button>
      <ol v-if="showSteps" class="steps" aria-label="Getting started">
        <li v-for="(s, i) in steps" :key="s.id" :class="s.state">
          <span class="num" aria-hidden="true">
            <Icon v-if="s.state === 'done'" name="check" :size="12" />
            <template v-else>{{ i + 1 }}</template>
          </span>
          <span class="step-text">
            <span class="step-title">{{ s.title }}</span>
            <span class="step-detail">{{ s.detail }}</span>
          </span>
        </li>
      </ol>
      <div v-if="updates.engine?.state === 'missing'" class="engine-missing">
        <p>
          Dial Desktop couldn’t find an engine. Install a signed copy for Desktop to use.
          On macOS and Linux, this copy stays private to Desktop and isn’t added to your terminal’s PATH.
        </p>
        <button class="btn btn-secondary" :disabled="!!updates.engineBusy || updates.appPhase !== 'idle'" @click="installEngine()">
          <Icon name="download" :size="14" /> {{ updates.engineBusy ? "Installing…" : "Install Desktop copy" }}
        </button>
        <button class="btn btn-secondary" title="Settings › Updates › Use a different engine…" @click="ui.settingsTab = 'updates'; ui.settingsOpen = true">
          Already installed? Choose it…
        </button>
        <p v-if="updates.engineError" class="error">{{ updates.engineError }}</p>
      </div>
      <div v-if="recent.length" class="recent">
        <div class="section-label head">
          <span>Recent</span>
          <button class="clear" title="Remove every project from this list (folders stay on disk)" @click="clearRecentProjects()">Clear</button>
        </div>
        <div v-for="w in recent" :key="w.root" class="item" @contextmenu="openMenu($event, projectItems(w))">
          <button class="row" :title="w.root" @click="folders.open(w.root)">
            <Icon name="folder" :size="15" />
            <span class="name truncate">{{ w.name }}</span>
            <span class="path truncate">{{ w.display }}</span>
          </button>
          <button class="forget" :aria-label="`Remove ${w.name} from recent projects`" title="Remove from this list (the folder stays on disk)" @click="forgetProject(w.root)">
            <Icon name="cross" :size="13" />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.welcome {
  height: 100%;
  display: grid;
  place-items: center;
  padding: var(--space-xxl);
}
.panel {
  width: min(460px, 100%);
  display: flex;
  flex-direction: column;
  gap: var(--space-lg);
}
.hero {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
}
h1 {
  margin: 18px 0 8px;
  font: 400 32px/1.2 ui-serif, "Iowan Old Style", "Palatino Linotype", Georgia, serif;
  letter-spacing: -0.01em;
}
.hero p {
  margin: 0;
  color: var(--text-muted);
}
.open {
  justify-content: center;
  height: 40px;
  font-size: 14px;
}
.settings {
  align-self: center;
}
.recent {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.recent .section-label {
  padding: 0 10px 6px;
}
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.clear {
  padding: 1px 6px;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  color: var(--text-faint);
  text-transform: none;
  letter-spacing: 0;
}
.clear:hover {
  background: var(--hover);
  color: var(--text);
}
.row {
  display: grid;
  grid-template-columns: auto minmax(0, auto) minmax(0, 1fr);
  align-items: center;
  gap: 10px;
  padding: 9px 10px;
  border-radius: var(--radius-md);
  text-align: left;
  color: var(--text-faint);
}
.row:hover {
  background: var(--hover);
}
.item {
  position: relative;
}
.item .row {
  width: 100%;
  padding-right: 40px;
}
.forget {
  position: absolute;
  top: 50%;
  right: 8px;
  display: grid;
  place-items: center;
  width: 26px;
  height: 26px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  opacity: 0;
  transform: translateY(-50%);
  transition: opacity 120ms var(--ease), background 120ms var(--ease), color 120ms var(--ease);
}
.item:hover .forget,
.forget:focus-visible {
  opacity: 1;
}
.forget:hover {
  background: color-mix(in srgb, var(--danger) 14%, transparent);
  color: var(--danger);
}
.name {
  color: var(--text);
}
.path {
  font: var(--text-sm) / 1.4 var(--font-mono);
}
.steps {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin: 0;
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  list-style: none;
}
.steps li {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  font-size: var(--text-sm);
}
.num {
  display: grid;
  flex: none;
  place-items: center;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  border: 1px solid var(--border-strong);
  font-size: 11px;
  color: var(--text-faint);
}
.steps li.done .num {
  border-color: transparent;
  background: color-mix(in srgb, var(--success) 18%, transparent);
  color: var(--success);
}
.steps li.next .num {
  border-color: var(--accent);
  color: var(--accent);
}
.steps li.problem .num {
  border-color: var(--danger);
  color: var(--danger);
}
.step-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}
.step-title {
  color: var(--text);
}
.steps li.later .step-title {
  color: var(--text-muted);
}
.step-detail {
  color: var(--text-faint);
  line-height: 1.45;
}
.engine-missing {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 10px;
  padding: 14px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.engine-missing p {
  margin: 0;
  line-height: 1.5;
}
.engine-missing code {
  font-family: var(--font-mono);
}
.engine-missing .error {
  color: var(--danger);
  white-space: pre-wrap;
}
</style>
