"use client";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDown,
  CircleX,
  ClipboardCopy,
  KeyRound,
  RefreshCw,
  ScrollText,
} from "lucide-react";
import type { Job, SyncEvent } from "@/lib/api";
import { activeJob, sourceNames, stamp } from "@/lib/format";
import { useJobLog } from "@/lib/queries";
import { useApp } from "@/components/app-context";
import { Button } from "@/components/ui";

type Action = "retry" | "edit" | "report" | "wait" | "none";
/** 錯誤碼 → 使用者看得懂的標題、建議與主要動作 */
const HINTS: Record<string, { title: string; hint: string; action: Action }> = {
  login_failed: {
    title: "銀行拒絕登入",
    hint: "請確認帳號密碼是否變更，或到銀行網站確認帳號狀態。",
    action: "edit",
  },
  login_locked: {
    title: "連續登入失敗，已暫停自動同步",
    hint: "確認銀行帳號沒有被鎖定後，更新登入設定或恢復同步。",
    action: "edit",
  },
  attempt_limit: {
    title: "本次登入嘗試次數已用完",
    hint: "下次排程會再試；若一直失敗，請確認帳號密碼。",
    action: "retry",
  },
  captcha_uncertain: {
    title: "圖形驗證碼辨識失敗",
    hint: "不會計入帳密錯誤，可以直接重試。",
    action: "retry",
  },
  captcha_retry_exhausted: {
    title: "圖形驗證碼連續未通過",
    hint: "不會計入帳密錯誤，可以直接重試。",
    action: "retry",
  },
  duplicate_login: {
    title: "銀行顯示已有其他登入",
    hint: "請先在銀行網站或 App 登出，約 5–10 分鐘後再同步。",
    action: "wait",
  },
  verification_failed: {
    title: "驗證沒有完成",
    hint: "驗證碼逾時或被拒絕；重新同步後請在 10 分鐘內完成驗證。",
    action: "retry",
  },
  timeout: {
    title: "銀行回應逾時",
    hint: "通常是銀行端忙碌，稍後再試即可。",
    action: "retry",
  },
  page_changed: {
    title: "銀行網頁格式已變更",
    hint: "需要更新同步程式，重試通常無法解決。請複製診斷資訊回報。",
    action: "report",
  },
  empty: {
    title: "登入成功但沒有讀到資料",
    hint: "請確認這個帳號在銀行網站看得到帳戶。",
    action: "retry",
  },
  validation_failed: {
    title: "資料未通過檢查",
    hint: "為避免寫入錯誤資料，本次結果已全部略過。請複製診斷資訊回報。",
    action: "report",
  },
  protocol_error: {
    title: "同步程序回應異常",
    hint: "請稍後重試；持續發生請複製診斷資訊回報。",
    action: "retry",
  },
  worker_exited: {
    title: "同步程序意外結束",
    hint: "請稍後重試；持續發生請複製診斷資訊回報。",
    action: "retry",
  },
  worker_start_failed: {
    title: "同步程序無法啟動",
    hint: "本機同步模組可能尚未建置，請通知管理者。",
    action: "report",
  },
  lease_expired: {
    title: "同步程序中斷",
    hint: "可能是服務重新啟動，重新同步即可。",
    action: "retry",
  },
  not_approved: {
    title: "同步資格已撤銷",
    hint: "請聯絡管理員。",
    action: "none",
  },
};
const FALLBACK = {
  title: "同步失敗",
  hint: "請再試一次；若持續失敗，請更新登入設定或複製診斷資訊回報。",
  action: "retry" as Action,
};

const offset = (start: number, at: string) => {
  const ms = Math.max(0, Date.parse(at) - start);
  const m = Math.floor(ms / 60000);
  const s = ((ms % 60000) / 1000).toFixed(1).padStart(4, "0");
  return `+${String(m).padStart(2, "0")}:${s}`;
};
const LEVEL = { info: "INFO", warn: "WARN", error: "ERROR" } as const;

function diagnostics(job: Job, events: SyncEvent[]) {
  const start = Date.parse(job.created_at);
  const lines = events
    .slice(-80)
    .map(
      (e) =>
        `${offset(start, e.at)} ${LEVEL[e.level].padEnd(5)} ${e.code}: ${e.message}${e.detail ? ` | ${e.detail}` : ""}`,
    );
  return [
    "Money Track 同步診斷",
    `工作：${job.id}`,
    `來源：${job.connector ?? "-"}`,
    `狀態：${job.status}　錯誤碼：${job.error_code ?? "-"}　失敗步驟：${job.failed_step ?? "-"}`,
    `開始：${job.created_at}`,
    "---",
    ...lines,
  ].join("\n");
}

/** 失敗摘要卡：先講結論與建議動作，細節留給 log */
export function SyncErrorCard({
  job,
  onRetry,
  onEdit,
  onShowLog,
  retryDisabled,
}: {
  job: Job;
  onRetry?: () => void;
  onEdit?: () => void;
  onShowLog: () => void;
  retryDisabled?: boolean;
}) {
  const info = HINTS[job.error_code ?? ""] ?? FALLBACK;
  const step = job.progress?.find((p) => p.code === job.failed_step)?.label;
  return (
    <section className="sync-error" role="alert">
      <header>
        <CircleX size={18} />
        <strong>{info.title}</strong>
      </header>
      <p>{job.message}</p>
      <dl>
        {step && (
          <div>
            <dt>失敗步驟</dt>
            <dd>{step}</dd>
          </div>
        )}
        <div>
          <dt>錯誤碼</dt>
          <dd>
            <code>{job.error_code ?? "unknown"}</code>
          </dd>
        </div>
        <div>
          <dt>時間</dt>
          <dd>{stamp(job.created_at)}</dd>
        </div>
      </dl>
      <p className="sync-error-hint">
        {info.hint}本次沒有寫入新資料，先前的資料都保留。
      </p>
      <div className="sync-error-actions">
        {info.action === "edit" && onEdit && (
          <Button size="sm" variant="primary" icon={KeyRound} onClick={onEdit}>
            更新登入設定
          </Button>
        )}
        {onRetry && info.action !== "none" && (
          <Button
            size="sm"
            variant={info.action === "retry" ? "primary" : "secondary"}
            icon={RefreshCw}
            disabled={retryDisabled}
            onClick={onRetry}
          >
            重試
          </Button>
        )}
        <Button size="sm" variant="ghost" icon={ScrollText} onClick={onShowLog}>
          查看錯誤 log
        </Button>
      </div>
    </section>
  );
}

type Filter = "all" | "warn" | "error";
/** 同步 log 面板：進行中即時跟隨最新一行；往上捲時暫停跟隨並提示新行數 */
export function SyncLog({
  job,
  focusError,
}: {
  job: Job;
  /** 開啟時捲到最後一筆錯誤並標亮 */
  focusError?: boolean;
}) {
  const { toast } = useApp();
  const active = activeJob(job.status);
  const { events, live, error } = useJobLog(job.id, active);
  const [filter, setFilter] = useState<Filter>("all");
  const [follow, setFollow] = useState(!focusError);
  const [unseen, setUnseen] = useState(0);
  const box = useRef<HTMLDivElement>(null);
  const seen = useRef(0);
  const start = Date.parse(job.created_at);
  const warns = events.filter((e) => e.level === "warn").length;
  const errors = events.filter((e) => e.level === "error").length;
  const shown = useMemo(
    () =>
      filter === "all"
        ? events
        : events.filter((e) =>
            filter === "error" ? e.level === "error" : e.level !== "info",
          ),
    [events, filter],
  );

  // 新事件進來：跟隨中就捲到底，否則累計未讀
  useLayoutEffect(() => {
    const added = shown.length - seen.current;
    seen.current = shown.length;
    const el = box.current;
    if (!el || added <= 0) return;
    if (follow) el.scrollTop = el.scrollHeight;
    else setUnseen((n) => n + added);
  }, [shown.length, follow]);

  const focused = useRef(false);
  useEffect(() => {
    if (!focusError || focused.current || !errors) return;
    const all = box.current?.querySelectorAll<HTMLElement>("li.lvl-error");
    const target = all?.[all.length - 1];
    if (target) {
      target.scrollIntoView({ block: "center" });
      target.classList.add("flash");
      focused.current = true;
    }
  }, [focusError, errors]);

  const onScroll = () => {
    const el = box.current;
    if (!el) return;
    const bottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    setFollow(bottom);
    if (bottom) setUnseen(0);
  };
  const jump = () => {
    const el = box.current;
    if (el) el.scrollTop = el.scrollHeight;
    setFollow(true);
    setUnseen(0);
  };
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(diagnostics(job, events));
      toast("已複製診斷資訊（已遮罩個資）");
    } catch {
      toast("無法存取剪貼簿", "bad");
    }
  };

  const state = !active
    ? null
    : live
      ? { cls: "live", text: "即時" }
      : error
        ? { cls: "bad", text: "連線中斷，重試中" }
        : { cls: "wait", text: "連線中" };

  return (
    <div className="sync-log">
      <div className="sync-log-bar">
        <div className="sync-log-filters" role="group" aria-label="篩選 log">
          {(
            [
              ["all", `全部 ${events.length}`],
              ["warn", `警告 ${warns + errors}`],
              ["error", `錯誤 ${errors}`],
            ] as const
          ).map(([k, label]) => (
            <button
              key={k}
              type="button"
              className={`chip ${filter === k ? "on" : ""} ${k === "error" && errors ? "has-error" : ""}`}
              aria-pressed={filter === k}
              onClick={() => setFilter(k)}
            >
              {label}
            </button>
          ))}
        </div>
        {state && (
          <span className={`sync-log-state ${state.cls}`}>
            <i aria-hidden /> {state.text}
          </span>
        )}
        <Button size="sm" variant="ghost" icon={ClipboardCopy} onClick={copy}>
          複製診斷
        </Button>
      </div>
      <div
        className="sync-log-body"
        ref={box}
        onScroll={onScroll}
        role="log"
        aria-live="off"
        aria-label={`${sourceNames[job.connector ?? ""] ?? job.connector ?? ""} 同步 log`}
        tabIndex={0}
      >
        {shown.length ? (
          <ol>
            {shown.map((e) => (
              <li
                key={e.id}
                className={`lvl-${e.level} ${e.step && e.step === e.code ? "is-step" : ""}`}
              >
                <time dateTime={e.at} title={stamp(e.at)}>
                  {offset(start, e.at)}
                </time>
                <b>{LEVEL[e.level]}</b>
                <span>
                  {e.message}
                  {e.detail && <small>{e.detail}</small>}
                </span>
              </li>
            ))}
          </ol>
        ) : (
          <p className="sync-log-empty">
            {error
              ? "暫時無法讀取 log，稍後會自動重試。"
              : active
                ? "等待同步程序回報…"
                : filter === "all"
                  ? "這次同步沒有 log（可能在此功能上線前執行）。"
                  : "沒有符合篩選的紀錄。"}
          </p>
        )}
      </div>
      {unseen > 0 && (
        <button type="button" className="sync-log-jump" onClick={jump}>
          <ArrowDown size={13} /> 有 {unseen} 行新 log
        </button>
      )}
      {/* 螢幕報讀只播報錯誤，不逐行播報 */}
      <span className="sr-only" aria-live="assertive">
        {errors ? `同步發生 ${errors} 個錯誤` : ""}
      </span>
    </div>
  );
}
