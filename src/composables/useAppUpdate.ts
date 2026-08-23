import { ref } from "vue";
import { checkAppUpdate } from "../ipc/api";
import type { AppUpdateInfo } from "../types/models";

// Module-level state → single source of truth shared by App/Sidebar/PageHome.
// Not reactive across pages unless they all use this composable.
const info = ref<AppUpdateInfo | null>(null);
const checking = ref(false);
const checkedOnce = ref(false);

const DISMISS_KEY = "ccgate.dismissedUpdate";

export function useAppUpdate() {
  /** Silent check: fills `info` only when an update exists and isn't dismissed. */
  async function refresh(): Promise<AppUpdateInfo | null> {
    checking.value = true;
    try {
      const r = await checkAppUpdate();
      checkedOnce.value = true;
      if (r.has_update && localStorage.getItem(DISMISS_KEY) !== r.latest_version) {
        info.value = r;
      } else {
        info.value = null;
      }
      return r;
    } catch (e) {
      checkedOnce.value = true;
      info.value = null;
      return null;
    } finally {
      checking.value = false;
    }
  }

  /** Manual check with feedback: returns the result (or null on failure). */
  async function checkNow(): Promise<AppUpdateInfo | null> {
    const r = await refresh();
    return r;
  }

  /** "Ignore this version" — remember so it doesn't nag again. */
  function dismiss() {
    if (info.value) localStorage.setItem(DISMISS_KEY, info.value.latest_version);
    info.value = null;
  }

  return { info, checking, checkedOnce, refresh, checkNow, dismiss };
}
