import { createApp } from "vue";
import App from "./App.vue";
import { connect, openExternal, showToast } from "./bridge";
import { loadHighlighter } from "./markdown";
import { startAppearance } from "./appearance";
import { openInBrowser, previewUrlsIn } from "./workspace";
import "./styles.css";

startAppearance();
createApp(App).mount("#app");

connect().catch((error) => showToast(`Could not reach the app: ${error}`));
loadHighlighter().catch((error) => console.warn("syntax highlighting unavailable", error));

// Web links open in the browser: this computer's (a dev server) in the Browser panel, others in
// the system browser. Nothing navigates the app window away.
document.addEventListener("click", (event) => {
  const link = (event.target as Element | null)?.closest?.("a[href]");
  if (!link) return;
  event.preventDefault();
  const href = link.getAttribute("href") ?? "";
  if (previewUrlsIn(href).length && !(event.metaKey || event.ctrlKey)) openInBrowser(href);
  else if (/^(https?|mailto):/i.test(href)) openExternal(href);
});

// Copy buttons in rendered code blocks.
document.addEventListener("click", (event) => {
  const button = (event.target as Element | null)?.closest?.("[data-copy]");
  if (!(button instanceof HTMLElement)) return;
  const code = button.closest(".code-block")?.querySelector("pre")?.textContent ?? "";
  navigator.clipboard.writeText(code).then(
    () => {
      button.textContent = "Copied";
      setTimeout(() => (button.textContent = "Copy"), 1400);
    },
    () => showToast("Could not copy to the clipboard"),
  );
});
