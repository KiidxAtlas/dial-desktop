<script setup lang="ts">
// Where new Types are saved: with the app, so every project has them, or in the open project.
import { computed } from "vue";
import { setTypeLocation, typeLocation, type TypeScope } from "../savedTypes";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";
import SSegmented from "./settings/SSegmented.vue";

const PLACES: { value: TypeScope; label: string }[] = [
  { value: "app", label: "In the app" },
  { value: "project", label: "In this project" },
];

const note = computed(() => {
  if (typeLocation.saveIn === "app") return "New types are saved in the app and show up in every project.";
  if (typeLocation.project) return `New types are saved in ${typeLocation.project} and show up only in this project. Commit the folder to share them.`;
  return "No project is open, so new types are saved in the app until you open one.";
});
</script>

<template>
  <SGroup title="Save new types">
    <SRow label="Where" description="Types you already have stay where they are; both places are always listed.">
      <SSegmented
        :model-value="typeLocation.saveIn"
        :options="PLACES"
        label="Where new types are saved"
        @update:model-value="setTypeLocation($event as TypeScope)"
      />
    </SRow>
  </SGroup>
  <p class="s-note">{{ note }}</p>
</template>
