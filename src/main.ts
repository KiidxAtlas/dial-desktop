import { createApp } from "vue";
import App from "./App.vue";
import { connect, openExternal, showToast, snapshot } from "./bridge";
import { loadHighlighter } from "./markdown";
import { startAppearance } from "./appearance";
import { openInBrowser, previewUrlsIn, showFile } from "./workspace";
import { IMAGE_EXT, viewImage } from "./imageViewer";
import { startNotifications } from "./notifications";
import { startTypes } from "./savedTypes";
import { installLegacyKeyGuard } from "./macKeys";
import "./styles.css";

installLegacyKeyGuard();
startAppearance();
createApp(App).mount("#app");

void connect();
startNotifications().catch((error) => showToast(`Could not initialize notifications: ${error}`));
startTypes().catch((error) => showToast(`Could not load your types: ${error}`));
loadHighlighter().catch((error) => console.warn("syntax highlighting unavailable", error));

// Web links open in the browser: this computer's (a dev server) in the Browser panel, others in
// the system browser. Image files open in the image viewer; other project file links in the Files
// panel. Nothing navigates the app window. Captured, and the same with ⌥/⌘/⇧ held: WebKit would
// otherwise download an ⌥-clicked link or open a ⇧-clicked one in a new window.
document.addEventListener(
  "click",
  (event) => {
    const link = (event.target as Element | null)?.closest?.("a[href]");
    if (!link) return;
    event.preventDefault();
    const href = link.getAttribute("href") ?? "";
    const image = link.getAttribute("data-image-path");
    if (image) return viewImage(image);
    if (previewUrlsIn(href).length && !(event.metaKey || event.ctrlKey)) openInBrowser(href);
    else if (/^(https?|mailto):/i.test(href)) openExternal(href);
    else if (!/^[a-z][a-z\d+.-]*:|^#/i.test(href)) {
      const path = href.replace(/[?#].*$/, "");
      if (IMAGE_EXT.test(path)) {
        try {
          return viewImage(decodeURIComponent(path));
        } catch {
          return showToast(`Could not open ${href}`);
        }
      }
      openProjectFile(href);
    }
  },
  true,
);

/** `src/app.ts#L12` or an absolute path inside the project; the backend refuses anything else. */
function openProjectFile(href: string) {
  const root = snapshot.value?.workspace?.root;
  if (!root) return;
  let path: string;
  try {
    path = decodeURIComponent(href.replace(/[?#].*$/, ""));
  } catch {
    return showToast(`Could not open ${href}`);
  }
  if (path.startsWith(`${root}/`)) path = path.slice(root.length + 1);
  path = path.replace(/^\.\//, "");
  if (!path) return;
  const match = /#L([1-9]\d*)(?:-L([1-9]\d*))?$/.exec(href);
  const start = Number(match?.[1]), end = Number(match?.[2] ?? start);
  showFile(path, Number.isSafeInteger(start) && Number.isSafeInteger(end) && end >= start ? { start, end } : null);
}

// Copy buttons in rendered code blocks.
document.addEventListener("click", (event) => {
  const button = (event.target as Element | null)?.closest?.("[data-copy]");
  if (!(button instanceof HTMLElement)) return;
  const code = button.closest(".code-block")?.querySelector("pre")?.textContent ?? "";
  navigator.clipboard.writeText(code).then(
    () => {
      button.textContent = "✓ Copied";
      button.classList.add("copied");
      setTimeout(() => {
        button.textContent = "Copy";
        button.classList.remove("copied");
      }, 1400);
    },
    () => showToast("Could not copy to the clipboard"),
  );
});
