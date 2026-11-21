<script setup lang="ts">
import { computed } from "vue";
import type { BookStyle } from "./bookPages";

/**
 * A drawing for a page with no photo of its own: the dish itself for a recipe (dumplings for
 * gyoza, a bowl for ramen), a sky for a journal entry, an emblem for a chapter. Ink lines over
 * washes of the book's colours, varied per item so no two pages are the same.
 */
const props = defineProps<{ title: string; words: string; kind: BookStyle; seed: string }>();

type Motif =
  | "dumpling" | "bowl" | "soup" | "bread" | "cake" | "pie" | "salad" | "pasta" | "fish" | "cup" | "egg" | "taco" | "plate"
  | "sun" | "moon" | "leaf" | "mountain" | "wave"
  | "compass" | "lamp" | "key";

const DISHES: [RegExp, Motif][] = [
  [/gyoza|dumpling|dim ?sum|wonton|potsticker|bao|pierogi|momo|empanada|ravioli|xiao ?long|mandu|samosa/i, "dumpling"],
  [/taco|burrito|quesadilla|wrap|tortilla|fajita/i, "taco"],
  [/pasta|spaghetti|linguine|lasagn|penne|carbonara|bolognese|macaroni|gnocchi|risotto/i, "pasta"],
  [/ramen|pho\b|noodle|udon|soba|laksa|congee|bibimbap|poke bowl|donburi/i, "bowl"],
  [/soup|curry|stew|chili|broth|chowder|bisque|gumbo|dal\b|tagine|goulash|minestrone|gazpacho|bowl/i, "soup"],
  [/\bpie\b|quiche|pot ?pie/i, "pie"],
  [/cake|cookie|brownie|dessert|tart|muffin|cupcake|pudding|cheesecake|tiramisu|biscuit|sweet|chocolate|ice cream/i, "cake"],
  [/bread|loaf|toast|sourdough|focaccia|bagel|brioche|baguette|bun|pizza|flatbread|naan/i, "bread"],
  [/salad|greens|slaw|vegetable|veggie|bowl of greens|herb/i, "salad"],
  [/fish|salmon|tuna|sushi|shrimp|prawn|seafood|cod|crab|mussel|ceviche|poke/i, "fish"],
  [/coffee|tea|latte|matcha|drink|smoothie|cocktail|juice|lemonade|chai|cocoa|shake/i, "cup"],
  [/egg|omelet|breakfast|pancake|waffle|frittata|shakshuka|benedict/i, "egg"],
];

function hash(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 16777619);
  return h >>> 0;
}

const motif = computed<Motif>(() => {
  const h = hash(props.seed);
  if (props.kind === "cookbook") {
    const words = `${props.title} ${props.words}`;
    // The title decides first: "gyoza with a dipping sauce" is dumplings, not a sauce.
    for (const [re, m] of DISHES) if (re.test(props.title)) return m;
    for (const [re, m] of DISHES) if (re.test(words)) return m;
    return "plate";
  }
  if (props.kind === "journal") return (["sun", "moon", "leaf", "mountain", "wave"] as const)[h % 5];
  return (["compass", "lamp", "key"] as const)[h % 3];
});

const BROTHS: [RegExp, string][] = [
  [/tomato|red pepper|chil[ie]|paprika|beet|borscht|gazpacho|harissa|goulash/i, "#d4573c"],
  [/pumpkin|squash|carrot|sweet potato|curry|lentil|dal\b|turmeric|corn/i, "#e49a3a"],
  [/pea|spinach|green|pesto|broccoli|kale|herb|leek|watercress/i, "#8db26a"],
  [/mushroom|miso|onion|beef|stew|chowder|potato|bean/i, "#b8916a"],
];
const broth = computed(() => {
  const words = `${props.title} ${props.words}`;
  return BROTHS.find(([re]) => re.test(words))?.[1] ?? "var(--wash-c)";
});

/** Each item's own turn of the palette: the washes drift in hue, the drawing tilts a little. */
const vary = computed(() => {
  const h = hash(props.seed + props.title);
  return { hue: (h % 50) - 25, tilt: ((h >> 8) % 5) - 2, flip: (h >> 12) % 2 === 1, specks: [0, 1, 2, 3, 4].map((i) => (h >> (i * 5)) % 31) };
});
</script>

<template>
  <svg class="art" viewBox="0 0 240 240" role="img" :aria-label="title" :style="{ '--hue': `${vary.hue}deg` }">
    <defs>
      <filter :id="`rough-${seed}`" x="-5%" y="-5%" width="110%" height="110%">
        <feTurbulence type="fractalNoise" baseFrequency="0.035" numOctaves="2" :seed="vary.specks[0]" />
        <feDisplacementMap in="SourceGraphic" scale="2.2" />
      </filter>
    </defs>

    <!-- The wash behind the drawing -->
    <path class="wash" d="M38 128c-6-52 34-90 86-88 50 2 86 34 82 86-4 50-44 80-90 78-46-2-74-30-78-76z" />
    <g class="specks">
      <circle v-for="(s, i) in vary.specks" :key="i" :cx="30 + s * 6" :cy="40 + ((s * 7 + i * 37) % 160)" :r="1.4 + (i % 2)" />
    </g>

    <g :filter="`url(#rough-${seed})`" :transform="`rotate(${kind === 'cookbook' ? vary.tilt : 0} 120 120)${vary.flip && kind !== 'cookbook' ? ' translate(240 0) scale(-1 1)' : ''}`">
      <!-- Cookbook -->
      <template v-if="motif === 'dumpling'">
        <ellipse class="plate" cx="120" cy="150" rx="92" ry="34" />
        <ellipse class="plate-rim" cx="120" cy="146" rx="74" ry="24" />
        <g v-for="(d, i) in [[78, 128, -8], [120, 120, 0], [162, 128, 8]]" :key="i" :transform="`translate(${d[0]} ${d[1]}) rotate(${d[2]})`">
          <path class="fill-a" d="M-30 14c0-26 14-40 30-40s30 14 30 40c-10 6-50 6-60 0z" />
          <path class="ink" d="M-30 14c0-26 14-40 30-40s30 14 30 40c-10 6-50 6-60 0z" />
          <path class="ink thin" d="M-16 -18c2 6 2 10 0 14M-4 -25c2 7 2 12 0 17M8 -24c2 7 2 11 0 16M19 -16c1 5 1 9 -1 13" />
          <path class="sear" d="M-27 13c10 4 44 4 54 0" />
        </g>
        <path class="fill-b" d="M186 92c10-4 22 2 22 12s-12 14-22 10c-8-4-8-18 0-22z" />
        <path class="ink" d="M186 92c10-4 22 2 22 12s-12 14-22 10c-8-4-8-18 0-22z" />
        <path class="ink" d="M30 70l70 40M38 62l70 40" />
      </template>

      <template v-else-if="motif === 'bowl'">
        <path class="steam" d="M96 72c-8-12 8-18 0-32M122 66c-8-12 8-18 0-32M148 72c-8-12 8-18 0-32" />
        <path class="fill-b" d="M48 112h144c0 44-32 74-72 74s-72-30-72-74z" />
        <ellipse class="fill-c" cx="120" cy="112" rx="72" ry="16" />
        <path class="ink thin" d="M70 110c14-8 22 6 36-2s22 6 36-2 22 6 30 0" />
        <circle class="fill-egg" cx="100" cy="108" r="11" /><circle class="fill-yolk" cx="100" cy="108" r="5" />
        <path class="fill-a" d="M132 100l22 4-4 10-20-4z" />
        <path class="ink" d="M48 112h144c0 44-32 74-72 74s-72-30-72-74zM48 112c0-9 32-16 72-16s72 7 72 16" />
        <path class="ink" d="M92 186h56" />
        <path class="ink" d="M168 50l-26 58M182 56l-30 54" />
      </template>

      <template v-else-if="motif === 'soup'">
        <path class="steam" d="M92 74c-8-12 8-18 0-30M120 68c-8-12 8-18 0-30M148 74c-8-12 8-18 0-30" />
        <ellipse class="plate" cx="120" cy="184" rx="88" ry="16" />
        <path class="fill-b" d="M44 112h152c0 44-34 72-76 72s-76-28-76-72z" />
        <ellipse :style="{ fill: broth }" cx="120" cy="112" rx="76" ry="17" />
        <path class="fill-cream" d="M100 110c8-8 26-8 34-2s-6 10-16 8-12 4-18-6z" />
        <path class="ink thin" d="M104 110c8-5 20-5 26 0" />
        <path class="fill-leaf" d="M146 104c6-7 15-6 17 0-5 5-12 5-17 0zM78 114c5-6 13-5 15 0-5 5-11 5-15 0z" />
        <circle class="ink-dot" cx="130" cy="118" r="1.4" /><circle class="ink-dot" cx="92" cy="106" r="1.4" /><circle class="ink-dot" cx="160" cy="114" r="1.4" />
        <path class="ink" d="M44 112h152c0 44-34 72-76 72s-76-28-76-72z" />
        <path class="ink" d="M44 112c0-10 34-18 76-18s76 8 76 18" />
        <path class="fill-c" d="M170 116l40-58c3-5 10-2 8 3l-36 60z" />
        <path class="ink" d="M170 116l40-58c3-5 10-2 8 3l-36 60z" />
      </template>

      <template v-else-if="motif === 'pasta'">
        <ellipse class="plate" cx="120" cy="146" rx="96" ry="40" />
        <ellipse class="plate-rim" cx="120" cy="142" rx="74" ry="28" />
        <path class="fill-c" d="M64 138c4-30 108-30 112 0-12 18-100 18-112 0z" />
        <path class="ink thin" d="M70 136c20-16 30 10 50-4s30 10 50-4M74 146c20-12 34 8 52-4s28 8 44-2M84 124c14-6 24 8 40 0s22 6 34 0" />
        <circle class="fill-a" cx="104" cy="128" r="9" /><circle class="fill-a" cx="136" cy="132" r="8" /><circle class="fill-a" cx="120" cy="142" r="7" />
        <path class="fill-leaf" d="M146 116c8-8 18-6 20 0-6 6-14 6-20 0z" />
        <path class="ink" d="M64 138c4-30 108-30 112 0" />
        <path class="ink" d="M178 60l-34 70M186 62l-6 14M194 66l-6 14M202 70l-6 14M178 60l24 10" />
      </template>

      <template v-else-if="motif === 'pie'">
        <ellipse class="plate" cx="120" cy="150" rx="94" ry="36" />
        <path class="fill-a" d="M42 138c0-30 36-50 78-50s78 20 78 50c0 12-36 22-78 22s-78-10-78-22z" />
        <path class="ink" d="M42 138c0-30 36-50 78-50s78 20 78 50c0 12-36 22-78 22s-78-10-78-22z" />
        <path class="ink thin" d="M60 122l120 0M68 110l104 0M90 94l-10 56M120 90v68M150 94l10 56" />
        <path class="ink" d="M46 136c8 6 14-4 22 2s14-4 22 2 14-4 22 2 14-4 22 2 14-4 22 2 14-4 22 2" />
      </template>

      <template v-else-if="motif === 'cake'">
        <ellipse class="plate" cx="120" cy="176" rx="84" ry="22" />
        <path class="fill-c" d="M60 120l100-34 22 16v58l-122 14z" />
        <path class="fill-a" d="M60 120l122-18v20l-122 18z" />
        <path class="fill-a" d="M60 152l122-14v12l-122 14z" />
        <path class="fill-cream" d="M60 120l100-34 22 16-122 18z" />
        <path class="ink" d="M60 120l100-34 22 16v58l-122 14zM60 120l122-18" />
        <path class="ink thin" d="M70 140l112-16M70 152l112-14" />
        <circle class="fill-berry" cx="150" cy="86" r="10" />
        <path class="ink" d="M150 76c2-8 8-12 14-12" />
      </template>

      <template v-else-if="motif === 'bread'">
        <path class="fill-a" d="M40 150c-6-40 30-70 80-70s86 30 80 70c-2 14-40 22-80 22s-78-8-80-22z" />
        <path class="ink" d="M40 150c-6-40 30-70 80-70s86 30 80 70c-2 14-40 22-80 22s-78-8-80-22z" />
        <path class="fill-cream" d="M76 104c10 10 22 12 30 8M110 96c10 10 22 12 30 8M144 100c8 10 18 12 26 8" />
        <path class="ink" d="M72 102c12 14 26 14 36 8M108 94c12 14 26 14 36 8M142 98c10 12 22 14 30 8" />
        <path class="ink thin" d="M48 168h144" />
        <g class="wheat"><path class="ink thin" d="M188 64l-30 42" /><path class="fill-b" d="M186 66c6-2 8 4 4 8-6 2-8-4-4-8zM178 76c6-2 8 4 4 8-6 2-8-4-4-8zM170 86c6-2 8 4 4 8-6 2-8-4-4-8z" /></g>
      </template>

      <template v-else-if="motif === 'salad'">
        <path class="fill-b" d="M44 120h152c0 40-34 66-76 66s-76-26-76-66z" />
        <path class="fill-leaf" d="M58 120c-4-22 18-34 34-22 6-18 34-22 42-2 14-14 40-6 40 14 12 0 20 4 22 10z" />
        <circle class="fill-berry" cx="98" cy="108" r="9" /><circle class="fill-berry" cx="146" cy="104" r="8" />
        <circle class="fill-cream" cx="122" cy="112" r="7" />
        <path class="ink" d="M44 120h152c0 40-34 66-76 66s-76-26-76-66z" />
        <path class="ink thin" d="M58 120c-4-22 18-34 34-22 6-18 34-22 42-2 14-14 40-6 40 14 12 0 20 4 22 10M80 108c6 4 10 8 12 12M118 96c2 8 2 14 0 22M156 104c-4 6-6 10-6 16" />
      </template>

      <template v-else-if="motif === 'fish'">
        <ellipse class="plate" cx="120" cy="150" rx="98" ry="36" />
        <path class="fill-a" d="M48 140c24-34 86-40 124-8l26-20v52l-26-20c-38 30-100 26-124-4z" />
        <path class="ink" d="M48 140c24-34 86-40 124-8l26-20v52l-26-20c-38 30-100 26-124-4z" />
        <circle class="ink-dot" cx="72" cy="134" r="4" />
        <path class="ink thin" d="M96 120c6 10 6 30 0 40M118 118c6 12 6 32 0 44M140 122c5 10 5 26 0 36" />
        <path class="fill-yolk" d="M160 92c10-6 22 0 20 10-10 6-22 0-20-10z" />
        <path class="fill-leaf" d="M178 104c6-8 16-8 18-2-6 6-12 6-18 2z" />
      </template>

      <template v-else-if="motif === 'cup'">
        <path class="steam" d="M100 66c-8-12 8-18 0-30M126 62c-8-12 8-18 0-30" />
        <ellipse class="plate" cx="116" cy="178" rx="80" ry="18" />
        <path class="fill-b" d="M62 92h108l-10 72c-2 10-12 16-22 16h-44c-10 0-20-6-22-16z" />
        <ellipse class="fill-c" cx="116" cy="92" rx="54" ry="10" />
        <path class="ink" d="M62 92h108l-10 72c-2 10-12 16-22 16h-44c-10 0-20-6-22-16zM62 92c0-6 24-10 54-10s54 4 54 10" />
        <path class="ink" d="M166 108c22-6 32 6 28 20s-18 18-34 14" />
        <path class="fill-cream" d="M100 92c6-6 26-6 32 0-8 4-24 4-32 0z" />
      </template>

      <template v-else-if="motif === 'egg'">
        <circle class="plate" cx="120" cy="134" r="86" />
        <circle class="plate-rim" cx="120" cy="134" r="64" />
        <path class="fill-egg" d="M76 130c-8-26 20-44 42-32 22-14 54 2 46 30 10 20-10 40-34 34-18 12-46 4-50-12-12-4-12-16-4-20z" />
        <path class="ink" d="M76 130c-8-26 20-44 42-32 22-14 54 2 46 30 10 20-10 40-34 34-18 12-46 4-50-12-12-4-12-16-4-20z" />
        <circle class="fill-yolk" cx="120" cy="130" r="18" />
        <circle class="ink" cx="120" cy="130" r="18" />
        <path class="ink-white" d="M112 122c3-4 7-5 10-4" />
        <circle class="ink-dot" cx="150" cy="112" r="1.6" /><circle class="ink-dot" cx="92" cy="150" r="1.6" /><circle class="ink-dot" cx="144" cy="156" r="1.6" />
      </template>

      <template v-else-if="motif === 'taco'">
        <ellipse class="plate" cx="120" cy="166" rx="92" ry="26" />
        <path class="fill-leaf" d="M54 132c10-22 30-28 34-16 8-18 34-20 38-6 10-14 34-12 38 4 10-6 26 0 26 14z" />
        <circle class="fill-berry" cx="92" cy="120" r="8" /><circle class="fill-berry" cx="140" cy="116" r="7" />
        <path class="fill-a" d="M42 160c0-46 36-78 78-78s78 32 78 78z" />
        <path class="ink" d="M42 160c0-46 36-78 78-78s78 32 78 78zM42 160h156" />
        <path class="ink thin" d="M66 140c10-30 34-44 54-44" />
        <circle class="ink-dot" cx="90" cy="140" r="2" /><circle class="ink-dot" cx="130" cy="132" r="2" /><circle class="ink-dot" cx="160" cy="146" r="2" />
      </template>

      <template v-else-if="motif === 'plate'">
        <circle class="plate" cx="120" cy="126" r="82" />
        <circle class="plate-rim" cx="120" cy="126" r="60" />
        <path class="fill-a" d="M88 132c0-20 14-32 32-32s32 12 32 32-14 24-32 24-32-4-32-24z" />
        <path class="ink" d="M88 132c0-20 14-32 32-32s32 12 32 32-14 24-32 24-32-4-32-24z" />
        <path class="fill-leaf" d="M112 98c-4-12 4-20 12-20 0 10-4 16-12 20zM124 100c6-10 16-12 22-8-6 8-14 10-22 8z" />
        <path class="ink" d="M24 60v58c0 6 4 8 8 8v70M16 60v34c0 8 6 12 12 12M40 60v34c0 8-6 12-12 12" />
        <path class="ink" d="M216 60c-12 10-14 40-8 62h8v74" />
      </template>

      <!-- Journal -->
      <template v-else-if="motif === 'sun'">
        <circle class="fill-yolk" cx="120" cy="112" r="40" />
        <circle class="ink" cx="120" cy="112" r="40" />
        <path class="ink" d="M120 50v-18M120 192v-18M58 112h-18M200 112h-18M76 68l-12-12M176 168l-12-12M164 68l12-12M64 168l12-12" />
        <path class="fill-c" d="M30 190c30-18 60-18 90 0s60 18 90 0v20H30z" />
        <path class="ink thin" d="M30 190c30-18 60-18 90 0s60 18 90 0" />
      </template>
      <template v-else-if="motif === 'moon'">
        <path class="fill-cream" d="M140 52c-40 4-66 40-58 78s46 58 82 46c-30-8-48-40-42-72 4-22 12-38 18-52z" />
        <path class="ink" d="M140 52c-40 4-66 40-58 78s46 58 82 46c-30-8-48-40-42-72 4-22 12-38 18-52z" />
        <path class="star" d="M176 74l3 8 8 3-8 3-3 8-3-8-8-3 8-3zM62 70l2 5 5 2-5 2-2 5-2-5-5-2 5-2zM184 150l2 5 5 2-5 2-2 5-2-5-5-2 5-2z" />
      </template>
      <template v-else-if="motif === 'leaf'">
        <path class="fill-leaf" d="M58 186c-10-70 40-128 128-132 4 88-50 138-128 132z" />
        <path class="ink" d="M58 186c-10-70 40-128 128-132 4 88-50 138-128 132zM58 186c34-40 70-84 128-132" />
        <path class="ink thin" d="M90 150l-6-34M110 128l-4-38M132 106l0-34M96 144l34 4M118 120l38 0M140 98l30-6" />
      </template>
      <template v-else-if="motif === 'mountain'">
        <circle class="fill-yolk" cx="168" cy="70" r="18" />
        <path class="fill-b" d="M24 186l66-104 40 60 26-34 60 78z" />
        <path class="fill-cream" d="M90 82l-18 28 12 -6 8 8 10-10 8 6z" />
        <path class="ink" d="M24 186l66-104 40 60 26-34 60 78zM24 186h192" />
      </template>
      <template v-else-if="motif === 'wave'">
        <path class="fill-b" d="M24 150c24-50 70-70 106-50-24 4-34 26-24 40 14 18 46 10 50-8 24 26 50 30 60 18V188c-20 8-40 8-60 0s-40-8-60 0-40 8-62 2z" />
        <path class="ink" d="M24 150c24-50 70-70 106-50-24 4-34 26-24 40 14 18 46 10 50-8 24 26 50 30 60 18" />
        <path class="ink thin" d="M40 182c20-10 40-10 60 0s40 10 60 0 30-8 44-2" />
        <circle class="fill-yolk" cx="70" cy="62" r="14" />
      </template>

      <!-- Guide -->
      <template v-else-if="motif === 'compass'">
        <circle class="fill-cream" cx="120" cy="120" r="74" />
        <circle class="ink" cx="120" cy="120" r="74" /><circle class="ink thin" cx="120" cy="120" r="60" />
        <path class="fill-a" d="M120 54l14 66-14 66-14-66z" />
        <path class="fill-b" d="M54 120l66-14 66 14-66 14z" />
        <path class="ink" d="M120 54l14 66-14 66-14-66zM54 120l66-14 66 14-66 14z" />
        <circle class="ink-dot" cx="120" cy="120" r="4" />
      </template>
      <template v-else-if="motif === 'lamp'">
        <path class="fill-yolk" d="M120 40c-30 0-50 22-50 50 0 22 14 34 22 46 6 10 6 20 6 26h44c0-6 0-16 6-26 8-12 22-24 22-46 0-28-20-50-50-50z" />
        <path class="ink" d="M120 40c-30 0-50 22-50 50 0 22 14 34 22 46 6 10 6 20 6 26h44c0-6 0-16 6-26 8-12 22-24 22-46 0-28-20-50-50-50zM100 176h40M104 190h32" />
        <path class="ink thin" d="M108 140l12-40 12 40M40 90H22M218 90h-18M58 40L46 28M182 40l12-12" />
      </template>
      <template v-else>
        <circle class="fill-a" cx="84" cy="96" r="38" />
        <circle class="fill-cream" cx="84" cy="96" r="14" />
        <path class="ink" d="M84 58a38 38 0 1 1 0 76 38 38 0 0 1 0-76zM84 82a14 14 0 1 1 0 28 14 14 0 0 1 0-28zM112 122l78 78M160 170l14-14M176 186l14-14" />
      </template>
    </g>
  </svg>
</template>

<style scoped>
.art {
  width: 100%;
  height: 100%;
  overflow: visible;
  --wash-a: color-mix(in oklch, var(--ty-accent) 70%, var(--ty-paper));
  /* Two companion washes: the accent drawn toward a cool and a warm neighbour. */
  --wash-b: color-mix(in oklch, color-mix(in oklch, var(--ty-accent) 30%, #5f8f9c) 60%, var(--ty-paper));
  --wash-c: color-mix(in oklch, color-mix(in oklch, var(--ty-accent) 25%, #f1d49a) 85%, var(--ty-paper));
  filter: hue-rotate(calc(var(--hue) * 0.4));
}
.wash {
  fill: color-mix(in oklch, var(--ty-accent) 13%, transparent);
}
.specks circle {
  fill: color-mix(in oklch, var(--ty-accent) 35%, transparent);
}
.ink,
.ink.thin,
.steam,
.ink-white {
  fill: none;
  stroke: var(--ty-ink-line, color-mix(in srgb, var(--ty-ink) 86%, transparent));
  stroke-width: 2.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.ink.thin {
  stroke-width: 1.6;
}
.ink-white {
  stroke: #fff;
  stroke-width: 2.4;
}
.ink-dot {
  fill: var(--ty-ink);
}
.steam {
  stroke: color-mix(in srgb, var(--ty-ink) 40%, transparent);
  stroke-dasharray: 2 7;
  stroke-width: 2.4;
}
.sear {
  fill: none;
  stroke: color-mix(in oklch, var(--ty-accent) 60%, #5a3510);
  stroke-width: 4;
  stroke-linecap: round;
  opacity: 0.55;
}
.plate {
  fill: color-mix(in srgb, var(--ty-paper) 55%, white);
  stroke: color-mix(in srgb, var(--ty-ink) 35%, transparent);
  stroke-width: 1.6;
}
.plate-rim {
  fill: none;
  stroke: color-mix(in srgb, var(--ty-ink) 18%, transparent);
  stroke-width: 1.4;
}
.fill-a { fill: var(--wash-a); }
.fill-b { fill: var(--wash-b); }
.fill-c { fill: var(--wash-c); }
.fill-cream { fill: color-mix(in srgb, #fff6e4 85%, var(--ty-paper)); }
.fill-egg { fill: #fffdf6; }
.fill-yolk { fill: #f4b73e; }
.fill-berry { fill: #d2524b; }
.fill-leaf { fill: #7fae6a; }
.star { fill: #f4c95d; }
</style>
