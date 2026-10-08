import { useStore } from "./store";
import { applyEditsPreview } from "./backend";
import { report } from "./errors";
import { t } from "./i18n";

const timers = new Map<string, number>();
const nonces = new Map<string, number>();
const lastKey = new Map<string, string>();

export function schedulePreview(tabId: string) {
  window.clearTimeout(timers.get(tabId));
  timers.set(tabId, window.setTimeout(() => void run(tabId), 250));
}

async function run(tabId: string) {
  const st = useStore.getState();
  const tab = st.tabs.find((t) => t.id === tabId);
  if (!tab) return;
  const key = JSON.stringify(tab.edits);
  const settled = tab.edits.length === 0 ? tab.previewPath === null : tab.previewPath !== null;
  if (lastKey.get(tabId) === key && settled) return;
  const nonce = (nonces.get(tabId) ?? 0) + 1;
  nonces.set(tabId, nonce);
  if (tab.edits.length === 0) {
    lastKey.set(tabId, key);
    if (tab.previewPath !== null) {
      st.patchTab(tabId, { previewPath: null, docVersion: tab.docVersion + 1 });
    }
    return;
  }
  try {
    const p = await applyEditsPreview(tab.path, tab.edits);
    if (nonces.get(tabId) !== nonce) return;
    const cur = useStore.getState().tabs.find((t) => t.id === tabId);
    if (!cur || JSON.stringify(cur.edits) !== key) return;
    lastKey.set(tabId, key);
    if (p) {
      useStore.getState().patchTab(tabId, { previewPath: p, docVersion: cur.docVersion + 1 });
    }
  } catch (e) {
    report(t("stPreviewFailed"), e);
  }
}
