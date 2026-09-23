"use client";
import { useMemo, useRef, useState, type FormEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Check,
  ChevronDown,
  ChevronRight,
  ImageUp,
  Plus,
  Repeat,
  Sparkles,
  Trash2,
  Undo2,
  X,
} from "lucide-react";
import Decimal from "decimal.js";
import {
  api,
  type Subscription,
  type SubscriptionItem,
  type SubscriptionMonth,
  type SubscriptionPeriod,
  type SubscriptionStatus,
  type SubscriptionSuggestion,
  type SubscriptionSummary,
} from "@/lib/api";
import { useCategoryNames } from "@/lib/categories";
import {
  currencies,
  evaluate,
  money,
  symbol,
  thisMonth,
  today,
} from "@/lib/format";
import { useAction, useApp } from "@/components/app-context";
import {
  Badge,
  Button,
  CategoryIcon,
  Empty,
  ErrorLine,
  Field,
  FormError,
  Money,
  MonthSwitch,
  PageHead,
  Segmented,
  Sheet,
  Skeleton,
  Switch,
  Writable,
} from "@/components/ui";

/** 訂閱圖示：有自訂圖示就用，沒有就用分類圖示 */
export function SubscriptionIcon({
  subscription: s,
  size = 40,
}: {
  subscription: Pick<Subscription, "category" | "icon">;
  size?: number;
}) {
  if (!s.icon) return <CategoryIcon category={s.category} size={size} />;
  return (
    // eslint-disable-next-line @next/next/no-img-element
    <img
      className="sub-icon"
      src={s.icon}
      alt=""
      width={size}
      height={size}
      aria-hidden="true"
    />
  );
}

const ICON_SIZE = 128;

/** 圖示縮進 128px 正方形：完整保留圖片（不裁切），透明底，轉成 WebP（不支援時用 PNG） */
async function toIcon(file: File): Promise<string> {
  const bitmap = await createImageBitmap(file);
  const scale = Math.min(ICON_SIZE / bitmap.width, ICON_SIZE / bitmap.height);
  const w = Math.max(1, Math.round(bitmap.width * scale));
  const h = Math.max(1, Math.round(bitmap.height * scale));
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = ICON_SIZE;
  const ctx = canvas.getContext("2d")!;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(bitmap, (ICON_SIZE - w) / 2, (ICON_SIZE - h) / 2, w, h);
  bitmap.close();
  const webp = canvas.toDataURL("image/webp", 0.9);
  return webp.startsWith("data:image/webp")
    ? webp
    : canvas.toDataURL("image/png");
}

export const subscriptionStatus: Record<
  SubscriptionStatus,
  { label: string; tone: string }
> = {
  paid: { label: "已扣款", tone: "good" },
  upcoming: { label: "即將扣款", tone: "info" },
  missed: { label: "未扣款", tone: "bad" },
  review: { label: "待確認", tone: "warn" },
  inactive: { label: "已停用", tone: "neutral" },
  none: { label: "非扣款月", tone: "neutral" },
};
const flagLabels: Record<string, { label: string; tone: string }> = {
  amount_changed: { label: "金額異動", tone: "warn" },
  after_end: { label: "取消後仍扣款", tone: "bad" },
  refunded: { label: "有退款", tone: "info" },
  multiple: { label: "本期多筆", tone: "warn" },
  posting: { label: "待入帳", tone: "neutral" },
};
const roleLabels = { charge: "扣款", fee: "手續費", refund: "退款" };
const md = (d: string) => `${Number(d.slice(5, 7))}/${Number(d.slice(8, 10))}`;
const cycleText = (s: Subscription) =>
  s.cycle === "yearly"
    ? `每年 ${s.anchor_month} 月 ${s.anchor_day} 日`
    : `每月 ${s.anchor_day} 日`;

/** 行事曆與訂閱頁共用：這一期要顯示在哪一天 */
export function periodDate(p: SubscriptionPeriod) {
  const charge = p.items.find(
    (i) => i.role === "charge" && i.match !== "pending",
  );
  return charge?.date ?? p.due ?? p.items[0]?.date ?? `${p.month}-01`;
}
/** 這一期實際扣了多少台幣（沒扣款時為 null） */
export function periodSpent(p: SubscriptionPeriod) {
  return p.spent.find((s) => s.currency === "TWD")?.amount ?? null;
}

export function useSubscriptionMonth(month: string, enabled = true) {
  return useQuery({
    queryKey: ["subscriptions", "month", month],
    queryFn: () =>
      api<SubscriptionMonth>(`/subscriptions/month?month=${month}`),
    enabled,
  });
}

export type SubscriptionMatch = {
  transaction_id: string;
  subscription_id: string;
  name: string;
  month: string;
  role: "charge" | "fee" | "refund";
};

/** 收支清單用：交易 id → 歸到哪個訂閱（近 13 個月，不含待確認） */
export function useSubscriptionMatches() {
  const q = useQuery({
    queryKey: ["subscriptions", "matches"],
    queryFn: () => api<SubscriptionMatch[]>("/subscriptions/matches"),
    staleTime: 60_000,
  });
  return useMemo(
    () => new Map(q.data?.map((m) => [m.transaction_id, m])),
    [q.data],
  );
}

type Draft = Partial<Subscription> & { from?: SubscriptionSuggestion };
type DismissedSuggestion = {
  key: string;
  name: string;
  amount: string | null;
  created_at: string;
};

export function SubscriptionsPage() {
  const { viewing, route } = useApp();
  const [month, setMonth] = useState(
    () => route.params.get("month") ?? thisMonth(),
  );
  const [editing, setEditing] = useState<Draft | null>(null);
  const data = useSubscriptionMonth(month);
  const list = useQuery({
    queryKey: ["subscriptions", "list"],
    queryFn: () => api<SubscriptionSummary[]>("/subscriptions"),
  });
  const suggestions = useQuery({
    queryKey: ["subscriptions", "suggestions"],
    queryFn: () => api<SubscriptionSuggestion[]>("/subscriptions/suggestions"),
    enabled: !viewing,
  });
  const dismissed = useQuery({
    queryKey: ["subscriptions", "dismissed"],
    queryFn: () =>
      api<DismissedSuggestion[]>("/subscriptions/suggestions/dismissed"),
    enabled: !viewing,
  });
  return (
    <>
      <PageHead
        title="訂閱"
        subtitle="預期扣款對照實際刷卡；金額只算收支裡的交易，不會重複記帳"
        actions={
          <>
            <MonthSwitch month={month} onChange={setMonth} />
            <Writable>
              <Button
                variant="primary"
                icon={Plus}
                onClick={() => setEditing({})}
              >
                新增訂閱
              </Button>
            </Writable>
          </>
        }
      />
      {data.isError && (
        <ErrorLine retry={() => data.refetch()}>訂閱讀取失敗</ErrorLine>
      )}
      {data.isPending ? (
        <Skeleton rows={3} h={100} />
      ) : data.data ? (
        <div className="stack">
          <Totals data={data.data} />
          <MonthList
            data={data.data}
            empty={!list.data?.length}
            onEdit={(s) => setEditing(s)}
          />
          {!!(suggestions.data?.length || dismissed.data?.length) && (
            <Suggestions
              list={suggestions.data ?? []}
              dismissed={dismissed.data ?? []}
              onCreate={(s) => setEditing(fromSuggestion(s))}
            />
          )}
          {!!list.data?.length && (
            <AllSubscriptions list={list.data} onEdit={setEditing} />
          )}
          <p className="dim small cal-note">
            訂閱只是扣款清單：系統依「比對字串」在收支裡找對應的刷卡交易，找到就算已扣款，金額以銀行交易為準。
            代收商（例如 Google
            Payment）底下可能有多種服務：每個服務各建一個訂閱、用同樣的比對字串並開啟「只配對相同金額」，交易會依金額自動歸到對的訂閱。
          </p>
        </div>
      ) : null}
      {editing && (
        <SubscriptionSheet draft={editing} onClose={() => setEditing(null)} />
      )}
    </>
  );
}

function fromSuggestion(s: SubscriptionSuggestion): Draft {
  const oldest = s.recent[s.recent.length - 1]?.date ?? s.last_date;
  return {
    name: (s.name ?? s.pattern).slice(0, 80),
    category: s.category,
    amount: s.original_amount ?? s.amount,
    currency: s.original_currency ?? s.currency ?? "TWD",
    cycle: "monthly",
    anchor_day: s.anchor_day,
    started_on: oldest,
    patterns: [s.pattern],
    require_amount: s.require_amount,
    from: s,
  };
}

function Totals({ data }: { data: SubscriptionMonth }) {
  const s = data.summary;
  const actual = s.actual.find((a) => a.currency === "TWD")?.amount ?? "0";
  const other = s.actual.filter((a) => a.currency !== "TWD");
  return (
    <div className="cal-totals">
      <div>
        <span>本月實際扣款</span>
        <Money value={actual} className="lg" />
        <small>
          {s.paid} 項已扣
          {other.map((o) => ` · ${money(o.amount, o.currency)}`).join("")}
        </small>
      </div>
      <div>
        <span>本月預期</span>
        <Money value={s.expected_twd} className="lg" />
        <small>
          {s.upcoming} 項即將扣款
          {s.unknown_estimate > 0 && ` · ${s.unknown_estimate} 項無法換算`}
        </small>
      </div>
      <div className={s.missed || s.review ? "cal-next" : ""}>
        <span>平均每月</span>
        <Money value={s.monthly_equivalent_twd} className="lg" />
        <small>
          {s.missed || s.review
            ? `${s.missed} 項未扣款 · ${s.review} 項待確認`
            : "年繳已攤成每月"}
        </small>
      </div>
    </div>
  );
}

function MonthList({
  data,
  empty,
  onEdit,
}: {
  data: SubscriptionMonth;
  empty: boolean;
  onEdit: (s: Subscription) => void;
}) {
  const [open, setOpen] = useState<string | null>(null);
  return (
    <section className="card flush">
      <header className="card-head">
        <h2>{Number(data.month.slice(5))} 月扣款</h2>
        <span className="dim small">{data.periods.length} 項</span>
      </header>
      {!data.periods.length ? (
        <Empty icon={Repeat} title={empty ? "還沒有訂閱" : "這個月沒有扣款"}>
          {empty
            ? "新增訂閱，或從下方「疑似訂閱」一鍵建立。刷卡交易會自動對上，不會重複記帳。"
            : "這個月沒有預期或實際的訂閱扣款。"}
        </Empty>
      ) : (
        <ul className="rows">
          {data.periods.map((p) => (
            <PeriodRow
              key={p.subscription.id}
              p={p}
              open={open === p.subscription.id}
              onToggle={() =>
                setOpen(open === p.subscription.id ? null : p.subscription.id)
              }
              onEdit={() => onEdit(p.subscription)}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

function PeriodRow({
  p,
  open,
  onToggle,
  onEdit,
}: {
  p: SubscriptionPeriod;
  open: boolean;
  onToggle: () => void;
  onEdit: () => void;
}) {
  const s = p.subscription;
  const st = subscriptionStatus[p.status];
  const spent = periodSpent(p);
  const date = periodDate(p);
  const meta =
    p.status === "paid" ? `${md(date)} 扣款` : p.due ? `預計 ${md(p.due)}` : "";
  return (
    <li>
      <button
        type="button"
        className="row"
        aria-expanded={open}
        onClick={onToggle}
      >
        <SubscriptionIcon subscription={s} />
        <span className="row-main">
          <span className="row-title">
            <span className="row-text">{s.name}</span>
            {p.flags.map((f) => (
              <Badge key={f} tone={flagLabels[f]?.tone}>
                {flagLabels[f]?.label ?? f}
              </Badge>
            ))}
          </span>
          <span className="row-meta">
            {[meta, s.category, `預期 ${money(s.amount, s.currency)}`]
              .filter(Boolean)
              .join(" · ")}
          </span>
        </span>
        <span className="cal-item-side">
          {spent != null && p.status === "paid" ? (
            <Money value={spent} className="row-amount" />
          ) : (
            <span className="dim small">
              約 <Money value={p.estimate_twd} />
            </span>
          )}
          <span className={`cal-state ${st.tone}`}>{st.label}</span>
        </span>
        {open ? (
          <ChevronDown size={16} className="row-chevron" />
        ) : (
          <ChevronRight size={16} className="row-chevron" />
        )}
      </button>
      {open && <PeriodDetail p={p} onEdit={onEdit} />}
    </li>
  );
}

function PeriodDetail({
  p,
  onEdit,
}: {
  p: SubscriptionPeriod;
  onEdit: () => void;
}) {
  const s = p.subscription;
  const { toast } = useApp();
  const { run, busy } = useAction();
  const reset = (id: string) =>
    run(`/subscriptions/${s.id}/links/${id}`, {
      method: "DELETE",
      success: "已還原為自動配對",
      toastError: true,
    });
  const decide = async (
    i: SubscriptionItem,
    state: "confirmed" | "rejected",
  ) => {
    const ok = await run(`/subscriptions/${s.id}/links`, {
      method: "PUT",
      body: { transaction_id: i.id, period: p.month, state },
      toastError: true,
    });
    if (ok)
      toast(
        state === "confirmed" ? "已確認為這一期的扣款" : "已排除這筆交易",
        "good",
        { label: "復原", run: () => void reset(i.id) },
      );
  };
  return (
    <div className="sub-detail">
      {!p.items.length ? (
        <p className="dim small">
          {p.status === "missed"
            ? `預計 ${p.due ? md(p.due) : ""} 扣款，但收支裡找不到符合「${s.patterns.join("」、「")}」的交易。可能已取消、扣款失敗，或交易還沒同步。`
            : `還沒有符合「${s.patterns.join("」、「")}」的交易。`}
        </p>
      ) : (
        <ul className="sub-items">
          {p.items.map((i) => (
            <li key={i.id} className={i.match === "pending" ? "pending" : ""}>
              <span className="sub-item-date">{md(i.date)}</span>
              <span className="sub-item-main">
                <strong>{i.description}</strong>
                <small>
                  {[
                    roleLabels[i.role],
                    i.account_name,
                    i.original_currency &&
                      i.original_amount &&
                      `${i.original_currency} ${i.original_amount}`,
                    i.match === "auto"
                      ? "自動配對"
                      : i.match === "confirmed"
                        ? "已確認"
                        : "待確認",
                    i.amount_ok === false &&
                      i.role === "charge" &&
                      "金額與預期不同",
                    i.status === "pending" && "待入帳",
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </small>
              </span>
              <Money
                value={i.amount}
                currency={i.currency ?? "TWD"}
                sign
                tone
                className="row-amount"
              />
              <Writable>
                <span className="sub-item-actions">
                  {i.match === "pending" && (
                    <Button
                      size="sm"
                      icon={Check}
                      disabled={busy}
                      onClick={() => decide(i, "confirmed")}
                    >
                      是這期
                    </Button>
                  )}
                  {i.match !== "confirmed" ? (
                    <Button
                      size="sm"
                      variant="ghost"
                      icon={X}
                      disabled={busy}
                      onClick={() => decide(i, "rejected")}
                    >
                      不是
                    </Button>
                  ) : (
                    <Button
                      size="sm"
                      variant="ghost"
                      icon={Undo2}
                      disabled={busy}
                      onClick={() => reset(i.id)}
                    >
                      還原
                    </Button>
                  )}
                </span>
              </Writable>
            </li>
          ))}
        </ul>
      )}
      {!!p.excluded?.length && (
        <details className="sub-excluded">
          <summary>已排除 {p.excluded.length} 筆（按過「不是」）</summary>
          <ul className="sub-items">
            {p.excluded.map((i) => (
              <li key={i.id}>
                <span className="sub-item-date">{md(i.date)}</span>
                <span className="sub-item-main">
                  <strong>{i.description}</strong>
                  <small>
                    {[i.account_name, "已排除，不算這個訂閱"]
                      .filter(Boolean)
                      .join(" · ")}
                  </small>
                </span>
                <Money
                  value={i.amount}
                  currency={i.currency ?? "TWD"}
                  sign
                  tone
                  className="row-amount"
                />
                <Writable>
                  <span className="sub-item-actions">
                    <Button
                      size="sm"
                      variant="ghost"
                      icon={Undo2}
                      disabled={busy}
                      onClick={() => reset(i.id)}
                    >
                      還原
                    </Button>
                  </span>
                </Writable>
              </li>
            ))}
          </ul>
        </details>
      )}
      <div className="sub-detail-foot">
        <span className="dim small">
          {cycleText(s)} · 比對「{s.patterns.join("」、「")}」
          {s.require_amount && " · 只配對相同金額"}
        </span>
        <Writable>
          <button type="button" className="link" onClick={onEdit}>
            編輯訂閱
          </button>
        </Writable>
      </div>
    </div>
  );
}

function Suggestions({
  list,
  dismissed,
  onCreate,
}: {
  list: SubscriptionSuggestion[];
  dismissed: DismissedSuggestion[];
  onCreate: (s: SubscriptionSuggestion) => void;
}) {
  const { toast } = useApp();
  const { run, busy } = useAction();
  const restore = (key: string, name: string) =>
    run("/subscriptions/suggestions/restore", {
      body: { key },
      success: `已恢復建議「${name}」`,
      toastError: true,
    });
  const dismiss = async (s: SubscriptionSuggestion) => {
    const name = s.name ?? s.pattern;
    const ok = await run("/subscriptions/suggestions/dismiss", {
      body: { key: s.key },
      toastError: true,
    });
    if (ok)
      toast(`之後不再建議「${name}」`, "good", {
        label: "復原",
        run: () => void restore(s.key, name),
      });
  };
  return (
    <section className="card flush">
      <header className="card-head">
        <h2>
          <Sparkles size={16} /> 疑似訂閱
        </h2>
        <span className="dim small">近半年每月固定出現的扣款</span>
      </header>
      <ul className="rows">
        {list.map((s) => (
          <li key={s.key} className="row static">
            <CategoryIcon category={s.category} size={40} />
            <span className="row-main">
              <span className="row-title">
                <span className="row-text">{s.name ?? s.pattern}</span>
                {s.require_amount && (
                  <Badge tone="info">代收商・固定金額</Badge>
                )}
              </span>
              <span className="row-meta">
                連續 {s.months} 個月 · 每月 {s.anchor_day} 日前後 · 最近{" "}
                {md(s.last_date)}
                {s.original_currency &&
                  ` · ${s.original_currency} ${s.original_amount}`}
              </span>
            </span>
            <Money
              value={s.amount}
              currency={s.currency ?? "TWD"}
              className="row-amount"
            />
            <Writable>
              <span className="sub-item-actions">
                <Button size="sm" icon={Plus} onClick={() => onCreate(s)}>
                  建立
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => void dismiss(s)}
                >
                  不是訂閱
                </Button>
              </span>
            </Writable>
          </li>
        ))}
      </ul>
      {!list.length && (
        <p className="dim small sub-hidden-empty">目前沒有新的疑似訂閱。</p>
      )}
      {!!dismissed.length && (
        <details className="sub-excluded sub-hidden">
          <summary>
            已隱藏 {dismissed.length} 個商家（按過「不是訂閱」）
          </summary>
          <ul className="sub-items">
            {dismissed.map((d) => (
              <li key={d.key}>
                <span className="sub-item-main">
                  <strong>{d.name}</strong>
                  <small>
                    {d.amount ? `金額 ${d.amount} · ` : ""}
                    {md(d.created_at.slice(0, 10))} 隱藏
                  </small>
                </span>
                <Writable>
                  <span className="sub-item-actions">
                    <Button
                      size="sm"
                      variant="ghost"
                      icon={Undo2}
                      disabled={busy}
                      onClick={() => void restore(d.key, d.name)}
                    >
                      恢復建議
                    </Button>
                  </span>
                </Writable>
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}

function AllSubscriptions({
  list,
  onEdit,
}: {
  list: SubscriptionSummary[];
  onEdit: (s: Subscription) => void;
}) {
  const now = today();
  const active = (s: SubscriptionSummary) =>
    !s.paused && (!s.ended_on || s.ended_on >= now);
  const sorted = [...list].sort(
    (a, b) => Number(active(b)) - Number(active(a)),
  );
  return (
    <section className="card flush">
      <header className="card-head">
        <h2>全部訂閱</h2>
        <span className="dim small">{list.filter(active).length} 項使用中</span>
      </header>
      <ul className="rows">
        {sorted.map((s) => (
          <li key={s.id}>
            <button
              type="button"
              className={`row ${active(s) ? "" : "paused"}`}
              onClick={() => onEdit(s)}
            >
              <SubscriptionIcon subscription={s} />
              <span className="row-main">
                <span className="row-title">
                  <span className="row-text">{s.name}</span>
                  {s.paused && <Badge>已暫停</Badge>}
                  {s.ended_on && s.ended_on < now && <Badge>已取消</Badge>}
                  {s.review > 0 && (
                    <Badge tone="warn">{s.review} 期待確認</Badge>
                  )}
                </span>
                <span className="row-meta">
                  {cycleText(s)} · {money(s.amount, s.currency)}
                  {s.last_charge && ` · 上次 ${md(s.last_charge.date)} 扣款`}
                </span>
                <span className="sub-history" aria-label="近 12 個月扣款狀態">
                  {s.history.slice(-12).map((h) => (
                    <i
                      key={h.month}
                      className={subscriptionStatus[h.status].tone}
                      title={`${h.month} ${subscriptionStatus[h.status].label}`}
                    />
                  ))}
                </span>
              </span>
              <span className="cal-item-side">
                <span className="dim small">每期約</span>
                <Money value={s.estimate_twd} className="row-amount" />
              </span>
              <ChevronRight size={16} className="row-chevron" />
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** 自訂圖示：選圖後立即上傳；新增中的訂閱要先建立才有地方存 */
function IconField({ draft }: { draft: Draft }) {
  const { run, busy, error, setError } = useAction();
  const input = useRef<HTMLInputElement>(null);
  const list = useQuery({
    queryKey: ["subscriptions", "list"],
    queryFn: () => api<SubscriptionSummary[]>("/subscriptions"),
    enabled: Boolean(draft.id),
  });
  const icon = draft.id
    ? (list.data?.find((x) => x.id === draft.id)?.icon ?? null)
    : null;
  const category = draft.category ?? "未分類";
  async function pick(file: File | undefined) {
    if (!file || !draft.id) return;
    if (!/^image\/(png|jpeg|webp|gif|bmp|avif)$/.test(file.type)) {
      setError("請選擇 PNG、JPEG 或 WebP 圖片");
      return;
    }
    let data: string;
    try {
      data = await toIcon(file);
    } catch {
      setError("無法讀取這張圖片");
      return;
    }
    await run(`/subscriptions/${draft.id}/icon`, {
      method: "PUT",
      body: { data },
      success: "已更新圖示",
    });
  }
  return (
    <div className="sub-icon-field">
      <SubscriptionIcon subscription={{ category, icon }} size={48} />
      <div className="sub-icon-main">
        <strong>圖示</strong>
        <span className="dim small">
          {draft.id
            ? "上傳服務的 logo，圖片會縮成 128px 正方形；沒有上傳時使用分類圖示"
            : "建立訂閱後即可上傳自己的圖示"}
        </span>
        {error && <FormError>{error}</FormError>}
      </div>
      {draft.id && (
        <div className="sub-icon-actions">
          <input
            ref={input}
            type="file"
            accept="image/png,image/jpeg,image/webp"
            hidden
            onChange={(e) => {
              void pick(e.target.files?.[0]);
              e.target.value = "";
            }}
          />
          <Button
            size="sm"
            icon={ImageUp}
            busy={busy}
            onClick={() => input.current?.click()}
          >
            {icon ? "更換" : "上傳"}
          </Button>
          {icon && (
            <Button
              size="sm"
              variant="ghost"
              icon={Trash2}
              aria-label="移除圖示"
              disabled={busy}
              onClick={() =>
                run(`/subscriptions/${draft.id}/icon`, {
                  method: "DELETE",
                  success: "已改回分類圖示",
                })
              }
            />
          )}
        </div>
      )}
    </div>
  );
}

function SubscriptionSheet({
  draft,
  onClose,
}: {
  draft: Draft;
  onClose: () => void;
}) {
  const editing = Boolean(draft.id);
  const { run, busy, error, setError } = useAction();
  const cats = useCategoryNames("expense").main;
  const [cycle, setCycle] = useState<"monthly" | "yearly">(
    draft.cycle ?? "monthly",
  );
  const [currency, setCurrency] = useState(draft.currency ?? "TWD");
  const [paused, setPaused] = useState(draft.paused ?? false);
  const [requireAmount, setRequireAmount] = useState(
    draft.require_amount ?? false,
  );
  const [confirmDelete, setConfirmDelete] = useState(false);
  const category =
    draft.category && cats.includes(draft.category) ? draft.category : cats[0];
  const submit = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const v = Object.fromEntries(new FormData(e.currentTarget)) as Record<
      string,
      string
    >;
    const amount = evaluate(v.amount);
    if (!amount || new Decimal(amount).lte(0)) {
      setError("請輸入大於 0 的預期金額");
      return;
    }
    const patterns = v.patterns
      .split(/[\n,，]/)
      .map((p) => p.trim())
      .filter(Boolean);
    if (!patterns.length) {
      setError("至少需要一個比對字串");
      return;
    }
    const body = {
      name: v.name.trim(),
      category: v.category,
      amount: new Decimal(amount).toString(),
      currency,
      cycle,
      anchor_day: Number(v.anchor_day),
      anchor_month: cycle === "yearly" ? Number(v.anchor_month) : null,
      started_on: v.started_on,
      ended_on: v.ended_on || null,
      paused,
      patterns,
      tolerance: v.tolerance || "15",
      window_days: Number(v.window_days || 5),
      require_amount: requireAmount,
      note: v.note ?? "",
    };
    if (
      await run(editing ? `/subscriptions/${draft.id}` : "/subscriptions", {
        method: editing ? "PUT" : "POST",
        body,
        success: editing ? `已更新「${body.name}」` : `已建立「${body.name}」`,
      })
    )
      onClose();
  };
  return (
    <Sheet
      title={editing ? "編輯訂閱" : "新增訂閱"}
      description={
        draft.from
          ? `依「${draft.from.name ?? draft.from.pattern}」最近 ${draft.from.months} 個月的扣款帶入`
          : "只記預期扣款，實際金額仍以刷卡交易為準"
      }
      onClose={onClose}
      busy={busy}
    >
      <form className="form" onSubmit={submit}>
        <FormError>{error}</FormError>
        <IconField draft={draft} />
        <div className="grid-2">
          <Field label="名稱">
            <input
              name="name"
              required
              maxLength={80}
              defaultValue={draft.name}
              data-autofocus
              placeholder="例如：Claude Max、YouTube 家庭"
            />
          </Field>
          <Field label="分類">
            <select name="category" defaultValue={category}>
              {cats.map((c) => (
                <option key={c}>{c}</option>
              ))}
            </select>
          </Field>
        </div>
        <div className="grid-2">
          <Field label="預期金額">
            <div className="affix">
              <span>{symbol(currency)}</span>
              <input
                name="amount"
                inputMode="decimal"
                autoComplete="off"
                required
                defaultValue={draft.amount}
              />
            </div>
          </Field>
          <Field label="扣款幣別">
            <select
              value={currency}
              onChange={(e) => setCurrency(e.target.value)}
            >
              {currencies.map((c) => (
                <option key={c}>{c}</option>
              ))}
            </select>
          </Field>
        </div>
        <fieldset className="field">
          <legend className="field-label">週期</legend>
          <Segmented
            label="週期"
            size="sm"
            value={cycle}
            onChange={setCycle}
            options={[
              { value: "monthly", label: "每月" },
              { value: "yearly", label: "每年" },
            ]}
          />
        </fieldset>
        <div className="grid-2">
          {cycle === "yearly" && (
            <Field label="扣款月份">
              <select
                name="anchor_month"
                defaultValue={draft.anchor_month ?? Number(today().slice(5, 7))}
              >
                {Array.from({ length: 12 }, (_, i) => (
                  <option key={i} value={i + 1}>
                    {i + 1} 月
                  </option>
                ))}
              </select>
            </Field>
          )}
          <Field label="扣款日">
            <input
              name="anchor_day"
              type="number"
              min={1}
              max={31}
              required
              defaultValue={draft.anchor_day ?? Number(today().slice(8, 10))}
            />
          </Field>
          <Field label="開始日期">
            <input
              name="started_on"
              type="date"
              required
              defaultValue={draft.started_on ?? today()}
            />
          </Field>
        </div>
        <Field label="比對字串（每行一個，對應帳單上的商家名稱）">
          <textarea
            name="patterns"
            rows={3}
            required
            defaultValue={(draft.patterns ?? []).join("\n")}
            placeholder={"例如：\nanthropic*claude\nclaude.ai"}
          />
        </Field>
        <p className="hint-line">
          不分大小寫、忽略空白。商家名稱改過時可以多寫幾個；外幣手續費若是另一行（例如「國外交易手續費
          -CLAUDE」），加上「手續費-claude」就會一起算進這個訂閱。
        </p>
        <div className="sub-switch">
          <span>
            <strong>只配對相同金額</strong>
            <small>代收商（Google Payment、Apple）底下有多種服務時開啟</small>
          </span>
          <Switch
            checked={requireAmount}
            onChange={setRequireAmount}
            label="只配對相同金額"
          />
        </div>
        <div className="grid-2">
          <Field label="金額誤差（%）">
            <input
              name="tolerance"
              inputMode="decimal"
              defaultValue={draft.tolerance ?? "15"}
            />
          </Field>
          <Field label="扣款日前後（天）">
            <input
              name="window_days"
              type="number"
              min={0}
              max={15}
              defaultValue={draft.window_days ?? 5}
            />
          </Field>
        </div>
        {editing && (
          <>
            <div className="sub-switch">
              <span>
                <strong>暫停</strong>
                <small>暫停期間不預期扣款，也不會顯示未扣款</small>
              </span>
              <Switch checked={paused} onChange={setPaused} label="暫停訂閱" />
            </div>
            <Field label="取消日期（已退訂時填寫）">
              <input
                name="ended_on"
                type="date"
                defaultValue={draft.ended_on ?? ""}
              />
            </Field>
          </>
        )}
        <Field label="備註">
          <input name="note" maxLength={500} defaultValue={draft.note} />
        </Field>
        <div className="form-actions">
          {editing && (
            <Button
              variant="danger-ghost"
              disabled={busy}
              onClick={async () => {
                if (!confirmDelete) {
                  setConfirmDelete(true);
                  return;
                }
                if (
                  await run(`/subscriptions/${draft.id}`, {
                    method: "DELETE",
                    success: `已刪除「${draft.name}」`,
                  })
                )
                  onClose();
              }}
            >
              {confirmDelete ? "確定刪除？交易不受影響" : "刪除"}
            </Button>
          )}
          <span className="grow" />
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button type="submit" variant="primary" busy={busy}>
            {editing ? "儲存" : "建立"}
          </Button>
        </div>
      </form>
    </Sheet>
  );
}
