"use client";
import { useState, type FormEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, Clock, Plus, ShieldCheck, X } from "lucide-react";
import { api, type SyncSettings } from "@/lib/api";
import { stamp } from "@/lib/format";
import { useAction } from "@/components/app-context";
import { Button, FormError, Segmented, Sheet } from "@/components/ui";

export const useSyncSettings = () =>
  useQuery({
    queryKey: ["sync-settings"],
    queryFn: () => api<SyncSettings>("/sync-settings"),
  });

const presets: { label: string; times: string[] }[] = [
  { label: "每天一次", times: ["07:00"] },
  { label: "早晚各一次", times: ["07:00", "21:00"] },
  { label: "一天三次", times: ["07:00", "13:00", "21:00"] },
];
const MAX_TIMES = 6;

/** 所有來源共用的同步時間與登入保護 */
export function SyncSettingsSheet({
  settings,
  onClose,
}: {
  settings?: SyncSettings;
  onClose: () => void;
}) {
  const { run, busy, error, setError } = useAction();
  const [times, setTimes] = useState<string[]>(settings?.times ?? ["07:00"]);
  const [attempts, setAttempts] = useState(settings?.attempt_limit ?? 1);
  const [failures, setFailures] = useState(settings?.failure_limit ?? 2);
  const maxAttempts = settings?.max_attempts ?? 3;
  const maxFailures = settings?.max_failures ?? 5;

  const unique = [...new Set(times.filter(Boolean))].sort();
  const setAt = (i: number, v: string) =>
    setTimes(times.map((t, j) => (j === i ? v : t)));
  const addTime = () => {
    // 預設加在最後一個時間的 6 小時後
    const last = unique.at(-1) ?? "07:00";
    const h = (Number(last.slice(0, 2)) + 6) % 24;
    setTimes([...times, `${String(h).padStart(2, "0")}:${last.slice(3)}`]);
  };

  async function save(e: FormEvent) {
    e.preventDefault();
    if (!unique.length) return setError("至少要設定一個同步時間");
    const ok = await run("/sync-settings", {
      method: "PUT",
      body: { times: unique, attempt_limit: attempts, failure_limit: failures },
      success: `已設定每天 ${unique.join("、")} 自動同步`,
    });
    if (ok) onClose();
  }

  return (
    <Sheet
      title="同步設定"
      description="所有資料來源共用同一組時間；登入保護避免密碼錯誤時被銀行鎖帳號"
      onClose={onClose}
      busy={busy}
    >
      <form className="form sync-form" onSubmit={save}>
        <FormError>{error}</FormError>

        <section className="sync-section">
          <h3>
            <Clock size={16} /> 每天自動同步的時間
          </h3>
          <div className="sync-presets">
            {presets.map((p) => (
              <button
                type="button"
                key={p.label}
                className={
                  p.times.join() === unique.join() ? "chip on" : "chip"
                }
                onClick={() => setTimes(p.times)}
              >
                {p.label}
              </button>
            ))}
          </div>
          <ul className="sync-times">
            {times.map((t, i) => (
              <li key={i}>
                <input
                  type="time"
                  aria-label={`第 ${i + 1} 個同步時間`}
                  value={t}
                  required
                  onChange={(e) => {
                    setAt(i, e.target.value);
                    setError("");
                  }}
                />
                <button
                  type="button"
                  className="icon-btn sm"
                  aria-label="移除這個時間"
                  disabled={times.length <= 1}
                  onClick={() => setTimes(times.filter((_, j) => j !== i))}
                >
                  <X size={15} />
                </button>
              </li>
            ))}
          </ul>
          {times.length < MAX_TIMES && (
            <Button size="sm" icon={Plus} onClick={addTime}>
              新增時間
            </Button>
          )}
          <p className="hint-line">
            台北時間。到了時間會依序同步所有開啟「自動同步」的來源，一次一家，不會同時登入。
            {settings && ` 目前下次同步：${stamp(settings.next_run_at)}。`}
          </p>
        </section>

        <section className="sync-section">
          <h3>
            <ShieldCheck size={16} /> 登入保護
          </h3>
          <div className="sync-guard">
            <div>
              <strong>單次同步最多嘗試登入</strong>
              <small>
                例如驗證碼辨識錯誤時會重試；超過就停止這次同步，等下一個時間再試。
              </small>
            </div>
            <Segmented
              label="單次同步最多嘗試登入"
              size="sm"
              value={String(attempts)}
              onChange={(v) => setAttempts(Number(v))}
              options={Array.from({ length: maxAttempts }, (_, i) => ({
                value: String(i + 1),
                label: `${i + 1} 次`,
              }))}
            />
          </div>
          <div className="sync-guard">
            <div>
              <strong>連續失敗幾次就暫停</strong>
              <small>
                跨多次同步累計。達到後這個來源停止自動與手動同步，等你確認修好。
              </small>
            </div>
            <Segmented
              label="連續失敗幾次就暫停"
              size="sm"
              value={String(failures)}
              onChange={(v) => setFailures(Number(v))}
              options={Array.from({ length: maxFailures }, (_, i) => ({
                value: String(i + 1),
                label: `${i + 1} 次`,
              }))}
            />
          </div>
          {failures >= 3 && (
            <div className="callout warn">
              <AlertTriangle size={16} />
              <span>
                多數銀行密碼連續錯誤 3 到 5
                次就會鎖住網銀帳號，需要臨櫃或客服解鎖。建議設定 2 次以下。
              </span>
            </div>
          )}
          <p className="hint-line">
            只要登入成功，錯誤次數就歸零。暫停後，到該來源按「已修好，恢復同步」，或更新登入設定，就會恢復。
          </p>
        </section>

        <div className="form-actions">
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button type="submit" variant="primary" busy={busy}>
            儲存
          </Button>
        </div>
      </form>
    </Sheet>
  );
}
