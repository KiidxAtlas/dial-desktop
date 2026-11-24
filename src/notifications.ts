import { reactive } from "vue";
import { folders, notificationApi, showToast, snapshot } from "./bridge";
import { ui } from "./ui";

const KEY = "dial.desktop-notifications";
const KINDS = `${KEY}.kinds`;
const defaults = { approval: true, question: true, finished: true };
type NoticeKind = keyof typeof defaults;
export const notifications = reactive({ enabled: false, busy: false, error: '', kinds: { ...defaults } });

export function setNotificationKind(kind: NoticeKind, value: boolean) {
  return setNotifications(notifications.enabled, false, { ...notifications.kinds, [kind]: value });
}

export async function setNotifications(enabled: boolean, interactive = true, kinds = notifications.kinds) {
  if (notifications.busy) return;
  notifications.busy = true;
  notifications.error = "";
  try {
    await notificationApi.setEnabled(enabled, interactive, kinds);
    notifications.kinds = kinds;
    notifications.enabled = enabled;
    try {
      localStorage.setItem(KINDS, JSON.stringify(kinds));
      if (enabled) localStorage.setItem(KEY, "on");
      else localStorage.removeItem(KEY);
    } catch {
      notifications.error = "This choice could not be saved for the next launch.";
    }
  } catch (error) {
    notifications.error = String(error);
  } finally {
    notifications.busy = false;
  }
}

export async function startNotifications() {
  let opening = false;
  await notificationApi.onOpen(async ({ workspaceRoot, sessionId }) => {
    if (opening) return;
    opening = true;
    try {
      if (snapshot.value?.workspace?.root !== workspaceRoot && !(await folders.open(workspaceRoot))) return;
      if (await notificationApi.resume({ workspaceRoot, sessionId })) ui.settingsOpen = false;
    } finally {
      opening = false;
    }
  });
  try {
    const saved = JSON.parse(localStorage.getItem(KINDS) ?? '{}');
    for (const key of Object.keys(defaults) as NoticeKind[]) {
      if (typeof saved?.[key] === 'boolean') notifications.kinds[key] = saved[key];
    }
    if (localStorage.getItem(KEY) === "on") await setNotifications(true, false);
  } catch {
    showToast("Could not read the saved desktop notifications setting");
  }
}
