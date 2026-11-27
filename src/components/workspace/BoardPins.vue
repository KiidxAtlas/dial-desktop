<script setup lang="ts">
// A board's pins as it is laid out, what each can do, and step-by-step wiring for first
// projects: choosing a project lights up the pins it uses.
import { computed, ref, watch } from 'vue';
import { dualRows, pinCommand, recipeRoles, type Pin, type Profile, type Recipe } from '../../boardProfiles';
import { addToPrompt } from '../../workspace';

const props = defineProps<{ profile: Profile; device: string; live: boolean; reply: string }>();
const emit = defineEmits<{ send: [lines: string[]] }>();
const picked = ref<Pin | null>(null), recipe = ref<Recipe | null>(null);
/** What was last sent from here; the board's answer shows under it. */
const sent = ref('');
watch(() => props.profile.id, () => { picked.value = null; recipe.value = null; sent.value = ''; });
watch(() => props.live, on => { if (!on) sent.value = ''; });
const answer = computed(() => sent.value ? props.reply.split('\n').map(l => l.trimEnd()).filter(Boolean).slice(-5).join('\n') : '');
function transmit(lines: string[]) { sent.value = lines.join(' · '); emit('send', lines); }

const roles = computed(() => recipeRoles(recipe.value));
const supply = computed(() => props.profile.pins.filter(p => p.number === undefined));
const numbered = computed(() => props.profile.pins.filter(p => p.number !== undefined));
const rows = computed(() => dualRows(props.profile.pins));
const byId = (id: string) => props.profile.pins.find(p => p.id === id);
const KIND = { power: 'Power', ground: 'Ground', io: 'Free to use', caution: 'Use with care', reserved: 'Taken by the board' } as const;
const canDrive = computed(() => !!props.profile.pinCommands && props.live && !!picked.value && (picked.value.kind === 'io' || picked.value.kind === 'caution'));
const board = computed(() => props.device && props.device !== props.profile.name ? `${props.profile.name} (${props.device})` : props.profile.name);

function pick(pin: Pin) { picked.value = picked.value?.id === pin.id ? null : pin; }
function choose(r: Recipe) { recipe.value = recipe.value?.id === r.id ? null : r; picked.value = null; }
function drive(template: string) { if (picked.value) transmit(pinCommand(template, picked.value)); }
const pinName = (id: string) => { const p = byId(id); return p ? (p.number !== undefined && props.profile.layout !== 'list' ? `${p.label} (pin ${p.number})` : p.label === p.id ? p.label : `${p.label} · ${p.id}`) : id; };
function askPin() {
  const p = picked.value;
  if (p) addToPrompt(`On my ${board.value}, what can I connect to ${pinName(p.id)} (${p.functions.join(', ')}) and how? Logic level: ${props.profile.logic}.`);
}
function askRecipe(r: Recipe) {
  const wiring = r.wires.map(w => `- ${w.part} → ${pinName(w.pin)}${w.via ? ` through ${w.via}` : ''}`).join('\n');
  addToPrompt(`Write code for my ${board.value} to ${r.goal}. It is wired like this:\n${wiring}\nThen build it, put it on the board with the board tools, and check the console.`);
}
</script>

<template>
  <section class="pins" aria-label="Pins and wiring">
    <div class="legend" aria-hidden="true">
      <span v-for="(label, kind) in KIND" :key="kind" :class="kind"><i />{{ label }}</span>
    </div>
    <p class="note">{{ profile.pinNote }} Logic level: <strong>{{ profile.logic }}</strong>.</p>

    <ol v-if="profile.layout === 'row'" class="header-row" aria-label="Pin header">
      <li v-for="p in profile.pins" :key="p.id">
        <button class="pin" :class="[p.kind, { on: picked?.id === p.id, used: roles.has(p.id) }]" :aria-pressed="picked?.id === p.id" :title="p.functions.join(', ')" @click="pick(p)">
          <small>{{ p.number }}</small><strong>{{ p.label }}</strong>
        </button>
      </li>
    </ol>

    <div v-else-if="profile.layout === 'dual'" class="dual" role="group" aria-label="Pin header">
      <div class="usb" aria-hidden="true">USB</div>
      <template v-for="[left, right] in rows" :key="left.id">
        <small class="num">{{ left.number }}</small>
        <button class="pin wide" :class="[left.kind, { on: picked?.id === left.id, used: roles.has(left.id) }]" :aria-pressed="picked?.id === left.id" @click="pick(left)"><strong>{{ left.label }}</strong><em v-if="roles.has(left.id)">{{ roles.get(left.id) }}</em></button>
        <span class="pcb" aria-hidden="true" />
        <button v-if="right" class="pin wide" :class="[right.kind, { on: picked?.id === right.id, used: roles.has(right.id) }]" :aria-pressed="picked?.id === right.id" @click="pick(right)"><strong>{{ right.label }}</strong><em v-if="roles.has(right.id)">{{ roles.get(right.id) }}</em></button>
        <span v-else />
        <small class="num">{{ right?.number }}</small>
      </template>
    </div>

    <template v-else>
      <div class="group" role="group" aria-label="Power and control pins">
        <button v-for="p in supply" :key="p.id" class="pin" :class="[p.kind, { on: picked?.id === p.id, used: roles.has(p.id) }]" :aria-pressed="picked?.id === p.id" :title="p.functions.join(', ')" @click="pick(p)"><strong>{{ p.label }}</strong></button>
      </div>
      <div v-if="numbered.length" class="group gpio" role="group" aria-label="GPIO pins">
        <button v-for="p in numbered" :key="p.id" class="pin" :class="[p.kind, { on: picked?.id === p.id, used: roles.has(p.id) }]" :aria-pressed="picked?.id === p.id" :title="p.functions.join(', ')" @click="pick(p)"><strong>{{ p.label }}</strong></button>
      </div>
    </template>

    <article v-if="picked" class="detail" aria-live="polite">
      <header><strong>{{ pinName(picked.id) }}</strong><span class="tag" :class="picked.kind">{{ KIND[picked.kind] }}</span></header>
      <ul class="chips"><li v-for="f in picked.functions" :key="f">{{ f }}</li></ul>
      <p v-if="picked.note">{{ picked.note }}</p>
      <p v-if="roles.has(picked.id)">In this project: <strong>{{ roles.get(picked.id) }}</strong></p>
      <div class="actions">
        <template v-if="canDrive && profile.pinCommands">
          <button @click="drive(profile.pinCommands.read)">Read level</button>
          <button @click="drive(profile.pinCommands.high)">Set high</button>
          <button @click="drive(profile.pinCommands.low)">Set low</button>
        </template>
        <button @click="askPin">Ask Dial about this pin</button>
      </div>
      <small v-if="profile.pinCommands && !live">Connect the console to read and set this pin from here.</small>
    </article>
    <small v-else class="hint">Click a pin for what it can do.</small>
    <div v-if="answer" class="reply" aria-live="polite"><small>Sent <code>{{ sent }}</code> · the board answered:</small><pre>{{ answer }}</pre></div>

    <h4>What to connect</h4>
    <ul class="recipes">
      <li v-for="r in profile.recipes" :key="r.id" :class="{ open: recipe?.id === r.id }">
        <button class="recipe" :aria-expanded="recipe?.id === r.id" @click="choose(r)"><strong>{{ r.title }}</strong><small>{{ r.parts }}</small></button>
        <div v-if="recipe?.id === r.id" class="steps">
          <table>
            <thead><tr><th>Part</th><th>Board pin</th></tr></thead>
            <tbody>
              <tr v-for="w in r.wires" :key="w.part + w.pin">
                <td>{{ w.part }}<small v-if="w.via"> via {{ w.via }}</small></td>
                <td><button class="pin inline" :class="byId(w.pin)?.kind" @click="picked = byId(w.pin) ?? null">{{ pinName(w.pin) }}</button></td>
              </tr>
            </tbody>
          </table>
          <ol><li v-for="s in r.steps" :key="s">{{ s }}</li></ol>
          <div class="actions">
            <template v-if="r.try">
              <button v-for="(t, i) in r.try" :key="t" :disabled="!live" :title="live ? t.replace(/\r/g, ' · ') : 'Connect the console first'" @click="transmit(t.split('\r'))">{{ r.try.length > 1 ? (i ? 'Undo' : 'Try it') : 'Try it' }}</button>
            </template>
            <button class="primary" @click="askRecipe(r)">Ask Dial to write it</button>
          </div>
        </div>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.pins { display: grid; gap: 10px; min-width: 0; }
h4, p { margin: 0; } h4 { font-size: 13px; margin-top: 4px; }
p, small { color: var(--text-muted); font-size: 12px; line-height: 1.45; overflow-wrap: anywhere; }
button { background: var(--surface); border: 1px solid var(--border-strong); border-radius: 6px; color: var(--text); padding: 5px 9px; font: inherit; font-size: 12px; cursor: pointer; }
button:disabled { opacity: .55; cursor: default; } .primary { background: var(--active); }
.legend { display: flex; flex-wrap: wrap; gap: 4px 10px; font-size: 11px; color: var(--text-muted); }
.legend span { display: inline-flex; align-items: center; gap: 4px; } .legend i { width: 8px; height: 8px; border-radius: 2px; background: var(--k); }
.power { --k: var(--danger); } .ground { --k: var(--text-faint); } .io { --k: var(--teal); } .caution { --k: var(--warning); } .reserved { --k: var(--border-strong); }
.pin { display: grid; justify-items: center; gap: 0; padding: 4px 2px; min-width: 0; border-color: var(--border); border-bottom: 3px solid var(--k); font-family: var(--font-mono, monospace); }
.pin strong { font-size: 11.5px; font-weight: 600; } .pin small { font-size: 10px; line-height: 1.1; color: var(--text-faint); }
.pin.reserved strong { color: var(--text-faint); }
.pin.on { outline: 2px solid var(--accent); outline-offset: 1px; }
.pin.used { background: color-mix(in srgb, var(--k) 22%, var(--surface)); border-color: var(--k); }
.header-row { list-style: none; margin: 0; padding: 6px; display: grid; grid-template-columns: repeat(auto-fill, minmax(40px, 1fr)); gap: 4px; background: var(--bubble); border-radius: 8px; }
.header-row .pin { width: 100%; }
.group { display: grid; grid-template-columns: repeat(auto-fill, minmax(46px, 1fr)); gap: 4px; }
.dual { display: grid; grid-template-columns: 20px minmax(0, 1fr) 26px minmax(0, 1fr) 20px; gap: 3px 4px; align-items: center; }
.dual .usb { grid-column: 1 / -1; justify-self: center; font-size: 10px; color: var(--text-faint); border: 1px solid var(--border-strong); border-bottom: 0; border-radius: 4px 4px 0 0; padding: 1px 10px; }
.dual .pcb { align-self: stretch; background: var(--bubble); margin: -2px 0; }
.dual .num { font-size: 10px; text-align: center; color: var(--text-faint); }
.pin.wide { display: flex; justify-content: space-between; gap: 6px; padding: 3px 7px; min-height: 24px; align-items: center; }
.pin.wide em { font: 10px/1.2 var(--font-ui); font-style: normal; color: var(--text); text-align: right; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.detail { display: grid; gap: 7px; padding: 10px; border: 1px solid var(--border-strong); border-radius: 8px; background: var(--surface); }
.detail header { display: flex; justify-content: space-between; align-items: center; gap: 8px; } .detail strong { font-size: 13px; }
.tag { font-size: 11px; color: var(--text-muted); display: inline-flex; align-items: center; gap: 5px; } .tag::before { content: ''; width: 8px; height: 8px; border-radius: 2px; background: var(--k); }
.chips { display: flex; flex-wrap: wrap; gap: 4px; list-style: none; margin: 0; padding: 0; }
.chips li { font-size: 11px; padding: 2px 7px; border-radius: 999px; background: var(--bubble); color: var(--text-muted); }
.actions { display: flex; flex-wrap: wrap; gap: 6px; }
.hint { font-style: italic; }
.recipes { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
.recipes > li { border: 1px solid var(--border); border-radius: 8px; overflow: hidden; } .recipes > li.open { border-color: var(--border-strong); }
.recipe { display: grid; gap: 1px; width: 100%; text-align: left; border: 0; border-radius: 0; padding: 8px 10px; background: transparent; }
.recipe strong { font-size: 12.5px; } .recipe:hover { background: var(--hover); }
.steps { display: grid; gap: 8px; padding: 0 10px 10px; }
table { border-collapse: collapse; font-size: 12px; width: 100%; }
th { text-align: left; font-weight: 500; color: var(--text-faint); font-size: 11px; padding: 2px 0; }
td { padding: 3px 6px 3px 0; border-top: 1px solid var(--border); vertical-align: middle; overflow-wrap: anywhere; }
.pin.inline { display: inline-block; padding: 1px 6px; font-size: 11px; }
ol { margin: 0; padding-left: 18px; font-size: 12px; color: var(--text-muted); display: grid; gap: 3px; line-height: 1.45; }
.reply { display: grid; gap: 4px; } .reply code { font-family: var(--font-mono, monospace); }
.reply pre { margin: 0; padding: 8px; border-radius: 6px; background: var(--bubble); font: 11.5px/1.45 var(--font-mono, monospace); white-space: pre-wrap; overflow-wrap: anywhere; max-height: 120px; overflow: auto; }
</style>
