// Recent projects: the list the welcome screen, the sidebar's project menu and the palette share,
// and what can be done to one project.

import { ask } from "@tauri-apps/plugin-dialog";
import { ref } from "vue";
import { files, folders } from "./bridge";
import { copyText } from "./sessionActions";
import type { Workspace } from "./types";
import type { MenuItem } from "./ui";

/** Recently opened folders that still exist, most recent first, without the open one. */
export const recentProjects = ref<Workspace[]>([]);

export async function loadRecentProjects() {
  recentProjects.value = await folders.recent();
  return recentProjects.value;
}

/** Take a project off the list; its folder and sessions stay on disk. */
export async function forgetProject(root: string) {
  if (await folders.forget(root)) recentProjects.value = recentProjects.value.filter((w) => w.root !== root);
}

/** Empty the recent list after the user confirms. */
export async function clearRecentProjects() {
  const list = recentProjects.value;
  if (!list.length) return;
  const confirmed = await ask(
    `Remove ${list.length === 1 ? "1 project" : `${list.length} projects`} from the recent list? The folders and their sessions stay on disk.`,
    { title: "Clear recent projects?", kind: "warning", okLabel: "Clear", cancelLabel: "Cancel" },
  ).catch(() => false);
  if (!confirmed) return;
  for (const w of list) await folders.forget(w.root);
  await loadRecentProjects();
}

/** Actions on one project. `current` is the open one: it cannot be opened again or forgotten. */
export function projectItems(w: Workspace, current = false): MenuItem[] {
  const items: MenuItem[] = [];
  if (!current) items.push({ label: "Open", icon: "folder", run: () => void folders.open(w.root) });
  items.push(
    { label: "Open in new window", icon: "external", run: () => void folders.openInWindow(w.root) },
    { label: "Reveal in Finder", icon: "folder", run: () => void files.reveal(w.root) },
    { label: "Copy path", icon: "copy", run: () => void copyText(w.root, "Project path copied") },
  );
  if (!current) items.push({ divider: true }, { label: "Remove from list", icon: "cross", danger: true, run: () => void forgetProject(w.root) });
  return items;
}
