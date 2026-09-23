"use client";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import { CircleCheck, CircleAlert, X } from "lucide-react";
import { api, setViewOwner, type HouseholdPerson, type User } from "@/lib/api";

export type View =
  | "overview"
  | "activity"
  | "assets"
  | "analysis"
  | "plans"
  | "subscriptions"
  | "calendar"
  | "household"
  | "sources"
  | "members";
export const views: View[] = [
  "overview",
  "activity",
  "assets",
  "analysis",
  "plans",
  "subscriptions",
  "calendar",
  "household",
  "sources",
  "members",
];
export type Route = { view: View; params: URLSearchParams };
export type ComposeOptions = {
  kind?: "expense" | "income" | "transfer";
  accountId?: string;
};
/** 提示上的操作按鈕，例如按錯時的「復原」 */
export type ToastAction = { label: string; run: () => void };
type Toast = {
  id: number;
  text: string;
  tone: "good" | "bad";
  action?: ToastAction;
};
type Ctx = {
  user: User;
  hidden: boolean;
  setHidden: (v: boolean) => void;
  toast: (text: string, tone?: "good" | "bad", action?: ToastAction) => void;
  route: Route;
  navigate: (view: View, params?: Record<string, string>) => void;
  compose: (o?: ComposeOptions) => void;
  /** 正在唯讀查看的家庭成員；null 為自己的帳本 */
  viewing: HouseholdPerson | null;
  setViewing: (p: HouseholdPerson | null) => void;
};
const AppContext = createContext<Ctx | null>(null);
export function useApp() {
  const v = useContext(AppContext);
  if (!v) throw new Error("useApp outside provider");
  return v;
}

function parseHash(): Route {
  const raw =
    typeof location === "undefined" ? "" : location.hash.replace(/^#\/?/, "");
  const [path, query] = raw.split("?");
  // 舊的「分享」頁併入「家庭」；「投資」頁已移除，持股改在「資產」
  const name =
    path === "sharing" ? "household" : path === "investments" ? "assets" : path;
  const view = (views as string[]).includes(name) ? (name as View) : "overview";
  return { view, params: new URLSearchParams(query ?? "") };
}

export function readStore<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(`mt:${key}`);
    return v == null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}
export function writeStore(key: string, value: unknown) {
  try {
    localStorage.setItem(`mt:${key}`, JSON.stringify(value));
  } catch {}
}

export function AppProvider({
  user,
  children,
  onCompose,
}: {
  user: User;
  children: ReactNode;
  onCompose: (o?: ComposeOptions) => void;
}) {
  const [hidden, setHiddenState] = useState(false);
  const [route, setRoute] = useState<Route>({
    view: "overview",
    params: new URLSearchParams(),
  });
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [viewing, setViewingState] = useState<HouseholdPerson | null>(null);
  const client = useQueryClient();
  const seq = useRef(0);
  useEffect(() => {
    setHiddenState(readStore("hidden", false));
    const sync = () => setRoute(parseHash());
    sync();
    window.addEventListener("hashchange", sync);
    return () => window.removeEventListener("hashchange", sync);
  }, []);
  const setHidden = useCallback((v: boolean) => {
    setHiddenState(v);
    writeStore("hidden", v);
  }, []);
  const toast = useCallback(
    (text: string, tone: "good" | "bad" = "good", action?: ToastAction) => {
      const id = ++seq.current;
      setToasts([{ id, text, tone, action }]);
      // 有「復原」之類的操作時留久一點，來得及按
      setTimeout(
        () => setToasts((t) => t.filter((x) => x.id !== id)),
        tone === "bad" || action ? 8000 : 3200,
      );
    },
    [],
  );
  const setViewing = useCallback(
    (p: HouseholdPerson | null) => {
      setViewOwner(p?.id ?? null);
      setViewingState(p);
      writeStore("viewing", p?.id ?? null);
      // 換人檢視：清掉快取，重新讀取（自己的登入與家庭資料不受影響）
      client.resetQueries({
        predicate: (q) =>
          !["me", "health", "household"].includes(String(q.queryKey[0])),
      });
    },
    [client],
  );
  const guardedCompose = useCallback(
    (o?: ComposeOptions) => {
      if (viewing) toast(`正在查看 ${viewing.name} 的帳本，無法記帳`, "bad");
      else onCompose(o);
    },
    [viewing, toast, onCompose],
  );
  const navigate = useCallback(
    (view: View, params?: Record<string, string>) => {
      const q = params ? new URLSearchParams(params).toString() : "";
      const next = `#/${view}${q ? `?${q}` : ""}`;
      if (location.hash !== next) location.hash = next;
      window.scrollTo({ top: 0 });
    },
    [],
  );
  return (
    <AppContext.Provider
      value={{
        user,
        hidden,
        setHidden,
        toast,
        route,
        navigate,
        compose: guardedCompose,
        viewing,
        setViewing,
      }}
    >
      {children}
      <div className="toasts" aria-live="polite" aria-atomic="false">
        {toasts.map((t) => (
          <div key={t.id} className={`toast toast-${t.tone}`} role="status">
            {t.tone === "good" ? (
              <CircleCheck size={18} />
            ) : (
              <CircleAlert size={18} />
            )}
            <span>{t.text}</span>
            {t.action && (
              <button
                type="button"
                className="toast-action"
                onClick={() => {
                  setToasts((x) => x.filter((v) => v.id !== t.id));
                  t.action!.run();
                }}
              >
                {t.action.label}
              </button>
            )}
            <button
              type="button"
              aria-label="關閉提示"
              onClick={() => setToasts((x) => x.filter((v) => v.id !== t.id))}
            >
              <X size={15} />
            </button>
          </div>
        ))}
      </div>
    </AppContext.Provider>
  );
}

/** 寫入操作：自動帶 CSRF、錯誤訊息、完成後重新讀取資料 */
export function useAction() {
  const { user, toast, viewing } = useApp();
  const client = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const run = useCallback(
    async (
      path: string,
      opts: {
        method?: string;
        body?: unknown;
        success?: string;
        /** 錯誤改用 toast 顯示（用於列表上的快速操作） */
        toastError?: boolean;
      } = {},
    ) => {
      if (viewing) {
        toast(
          `正在查看 ${viewing.name} 的帳本（唯讀），請先切回自己的帳本`,
          "bad",
        );
        return false;
      }
      setBusy(true);
      setError("");
      try {
        await api(path, {
          method: opts.method ?? "POST",
          body: opts.body,
          csrf: user.csrf_token,
        });
        await client.invalidateQueries();
        if (opts.success) toast(opts.success);
        return true;
      } catch (e) {
        const msg = (e as Error).message;
        if (opts.toastError) toast(msg, "bad");
        else setError(msg);
        return false;
      } finally {
        setBusy(false);
      }
    },
    [client, toast, user.csrf_token, viewing],
  );
  return { run, busy, error, setError };
}

/** 全域快捷鍵，輸入框內不觸發 */
export function useHotkey(key: string, fn: () => void, enabled = true) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    if (!enabled) return;
    function onKey(e: KeyboardEvent) {
      const t = e.target as HTMLElement;
      if (
        e.metaKey ||
        e.ctrlKey ||
        e.altKey ||
        t.closest("input,textarea,select,[contenteditable],[role=dialog]")
      )
        return;
      if (e.key.toLowerCase() === key) {
        e.preventDefault();
        ref.current();
      }
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [key, enabled]);
}

export function useDebounced<T>(value: T, ms = 300) {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}
