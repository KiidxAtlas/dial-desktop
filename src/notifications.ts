import { reactive } from "vue";
import { folders, notificationApi, showToast, snapshot } from "./bridge";
import { ui } from "./ui";

const KEY = "dial.desktop-notifications";
export const notifications = reactive({ enabled: false, busy: false, error: "" });

export async function setNotifications(enabled: boolean, interactive = true) {
  if (notifications.busy) return;
  notifications.busy = true;
  notifications.error = "";
  try {
    await notificationApi.setEnabled(enabled, interactive);
    notifications.enabled = enabled;
    try {
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
    if (localStorage.getItem(KEY) === "on") await setNotifications(true, false);
  } catch {
    showToast("Could not read the saved desktop notifications setting");
  }
}
