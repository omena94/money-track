"use client";
import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  CalendarDays,
  CircleCheck,
  ChevronRight,
  Repeat,
  Repeat2,
  X,
} from "lucide-react";
import { api, type Recurring, type SubscriptionPeriod } from "@/lib/api";
import {
  frequencies,
  money,
  relative,
  sourceNames,
  thisMonth,
  today,
} from "@/lib/format";
import { useApp } from "@/components/app-context";
import { BankLogo } from "@/components/bank-logo";
import {
  CategoryIcon,
  Empty,
  ErrorLine,
  Money,
  MonthSwitch,
  PageHead,
  Skeleton,
} from "@/components/ui";
import {
  periodDate,
  periodSpent,
  subscriptionStatus,
  SubscriptionIcon,
  useSubscriptionMonth,
} from "./subscriptions";

type Due = {
  id: string;
  connection_id: string;
  connector: string;
  account_name: string;
  period: string;
  date: string;
  currency: string;
  statement_amount: string | null;
  minimum_payment: string | null;
  paid_amount: string | null;
  is_paid: boolean | null;
  last_synced_at: string | null;
};
type CalendarData = { month: string; events: Due[]; unknown_due_count: number };
type Plan = Recurring & { kind?: string; category?: string };

/** 行事曆上的一件事：信用卡帳單、固定收支的某一次，或訂閱的某一期 */
type Item =
  | { type: "bill"; key: string; date: string; bill: Due }
  | { type: "plan"; key: string; date: string; plan: Plan }
  | { type: "sub"; key: string; date: string; sub: SubscriptionPeriod };
/** 訂閱這一期的金額：已扣款用實際金額，否則用估計 */
const subAmount = (p: SubscriptionPeriod) =>
  p.status === "paid" ? periodSpent(p) : p.estimate_twd;

const DAY = 86400000;
const daysBetween = (a: string, b: string) =>
  Math.round((Date.parse(b) - Date.parse(a)) / DAY);
const wan = (v: number) =>
  Math.abs(v) >= 1e4
    ? `${+(Math.abs(v) / 1e4).toFixed(1)}萬`
    : Math.abs(v).toLocaleString("zh-TW");
const bankName = (b: Due) => sourceNames[b.connector] ?? b.connector;

/** 固定收支在這個月會發生的日子：從下次日期往後推，只推到月底 */
function occurrences(p: Plan, month: string): string[] {
  if (!p.enabled || !p.next_on) return [];
  const end = `${month}-31`;
  const out: string[] = [];
  let d = new Date(`${p.next_on}T00:00:00Z`);
  for (let i = 0; i < 40; i++) {
    const iso = d.toISOString().slice(0, 10);
    if (iso > end) break;
    if (iso.startsWith(month)) out.push(iso);
    if (p.frequency === "daily") d = new Date(d.getTime() + DAY);
    else if (p.frequency === "weekly") d = new Date(d.getTime() + 7 * DAY);
    else if (p.frequency === "monthly") {
      const day = Number(p.next_on.slice(8, 10));
      const next = new Date(
        Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 1),
      );
      const last = new Date(
        Date.UTC(next.getUTCFullYear(), next.getUTCMonth() + 1, 0),
      ).getUTCDate();
      d = new Date(
        Date.UTC(
          next.getUTCFullYear(),
          next.getUTCMonth(),
          Math.min(day, last),
        ),
      );
    } else if (p.frequency === "yearly")
      d = new Date(
        Date.UTC(d.getUTCFullYear() + 1, d.getUTCMonth(), d.getUTCDate()),
      );
    else break;
  }
  return out;
}

/** 帳單狀態：已繳、逾期、今天、幾天後、待確認 */
function billState(b: Due, now: string) {
  if (b.is_paid === true) return { tone: "good", label: "已繳清" } as const;
  const left = daysBetween(now, b.date);
  if (b.is_paid === false || b.is_paid == null) {
    if (left < 0)
      return {
        tone: b.is_paid === false ? "bad" : "neutral",
        label: b.is_paid === false ? `逾期 ${-left} 天` : "已過期限・待確認",
      } as const;
    if (left === 0) return { tone: "warn", label: "今天到期" } as const;
    return {
      tone: left <= 5 ? "warn" : "info",
      label: `還有 ${left} 天`,
    } as const;
  }
  return { tone: "neutral", label: "待確認" } as const;
}

export function CalendarPage() {
  const { viewing } = useApp();
  const [month, setMonth] = useState(thisMonth);
  const [selected, setSelected] = useState<string | null>(null);
  const bills = useQuery({
    queryKey: ["calendar", month],
    queryFn: () => api<CalendarData>(`/calendar?month=${month}`),
    enabled: !viewing,
    refetchInterval: 30000,
  });
  const plans = useQuery({
    queryKey: ["recurring"],
    queryFn: () => api<Plan[]>("/recurring"),
    enabled: !viewing,
  });
  const subs = useSubscriptionMonth(month, !viewing);
  const change = (m: string) => {
    setMonth(m);
    setSelected(null);
  };

  const items = useMemo(() => {
    const list: Item[] = [
      ...(bills.data?.events ?? []).map((b): Item => ({
        type: "bill",
        key: b.id,
        date: b.date,
        bill: b,
      })),
      ...(plans.data ?? []).flatMap((p) =>
        occurrences(p, month).map((date): Item => ({
          type: "plan",
          key: `${p.id}-${date}`,
          date,
          plan: p,
        })),
      ),
      ...(subs.data?.periods ?? []).map((p): Item => ({
        type: "sub",
        key: `sub-${p.subscription.id}`,
        // 月底扣款跨到下個月入帳時，仍排在這一期的扣款日
        date: periodDate(p).startsWith(month)
          ? periodDate(p)
          : (p.due ?? periodDate(p)),
        sub: p,
      })),
    ];
    return list
      .filter((it) => it.date.startsWith(month))
      .sort((a, b) => a.date.localeCompare(b.date));
  }, [bills.data, plans.data, subs.data, month]);

  if (viewing)
    return (
      <Empty icon={CalendarDays} title="個人行事曆">
        請切回自己的帳本，查看信用卡繳款期限與固定收支。
      </Empty>
    );

  return (
    <>
      <PageHead
        title="行事曆"
        subtitle="信用卡繳款期限、固定收支與訂閱扣款，這個月要付什麼一次看清楚"
        actions={<MonthSwitch month={month} onChange={change} />}
      />
      {bills.isError && <ErrorLine>行事曆讀取失敗，請稍後重試</ErrorLine>}
      {bills.isPending ? (
        <Skeleton rows={3} h={120} />
      ) : (
        <>
          <Totals items={items} month={month} />
          <div className="cal-layout">
            <MonthGrid
              month={month}
              items={items}
              selected={selected}
              onSelect={setSelected}
            />
            <Agenda
              month={month}
              items={items}
              selected={selected}
              onClear={() => setSelected(null)}
            />
          </div>
          {Boolean(bills.data?.unknown_due_count) && (
            <p className="dim small cal-note">
              另有 {bills.data!.unknown_due_count}{" "}
              期歷史帳單銀行沒有提供繳款期限，因此沒有排進日期。
            </p>
          )}
          <p className="dim small cal-note">
            日期以台北時間顯示。帳單金額可能含溢繳款，不一定是目前剩餘應繳；繳款與自動扣款狀態以銀行最新資料為準。固定收支可在「計畫」頁管理；訂閱扣款依收支中的刷卡交易判斷，已含在信用卡帳單內，可在「訂閱」頁管理。
          </p>
        </>
      )}
    </>
  );
}

function Totals({ items, month }: { items: Item[]; month: string }) {
  const now = today();
  let due = 0,
    paid = 0,
    unpaid = 0,
    fixedOut = 0,
    fixedIn = 0,
    subTotal = 0,
    subPaid = 0;
  let next: Item | undefined;
  for (const it of items) {
    if (it.type === "bill") {
      const v = Number(it.bill.statement_amount ?? 0);
      due += v;
      if (it.bill.is_paid) paid += v;
      else {
        unpaid += v;
        if (it.date >= now && !next) next = it;
      }
    } else if (it.type === "sub") {
      const v = Number(subAmount(it.sub) ?? 0);
      subTotal += v;
      if (it.sub.status === "paid") subPaid += v;
      else if (it.date >= now && !next) next = it;
    } else {
      const v = Number(it.plan.amount);
      if (v < 0 || it.plan.kind === "expense") fixedOut += Math.abs(v);
      else fixedIn += v;
      if (it.date >= now && !next) next = it;
    }
  }
  const isNow = month === thisMonth();
  return (
    <div className="cal-totals">
      <div>
        <span>信用卡帳單</span>
        <Money value={String(due)} className="lg" />
        <small>
          已繳 <Money value={String(paid)} /> · 未繳{" "}
          <Money value={String(unpaid)} />
        </small>
      </div>
      <div>
        <span>固定支出</span>
        <Money value={String(fixedOut)} className="lg" />
        <small>
          固定收入 <Money value={String(fixedIn)} />
        </small>
        <small>
          訂閱約 <Money value={String(subTotal)} /> · 已扣{" "}
          <Money value={String(subPaid)} />
        </small>
      </div>
      <div className="cal-next">
        <span>{isNow ? "下一件要處理" : "本月第一件"}</span>
        {next ? (
          <>
            <strong>
              {Number(next.date.slice(5, 7))}/{Number(next.date.slice(8))}{" "}
              {next.type === "bill"
                ? bankName(next.bill)
                : next.type === "sub"
                  ? next.sub.subscription.name
                  : next.plan.description}
            </strong>
            <small>
              {daysBetween(now, next.date) === 0
                ? "今天"
                : daysBetween(now, next.date) > 0
                  ? `${daysBetween(now, next.date)} 天後`
                  : `${-daysBetween(now, next.date)} 天前`}
              {" · "}
              {next.type === "bill"
                ? money(next.bill.statement_amount, next.bill.currency)
                : next.type === "sub"
                  ? `約 ${money(subAmount(next.sub))}`
                  : money(next.plan.amount, next.plan.currency)}
            </small>
          </>
        ) : (
          <strong className="dim">沒有待處理的事</strong>
        )}
      </div>
    </div>
  );
}

function MonthGrid({
  month,
  items,
  selected,
  onSelect,
}: {
  month: string;
  items: Item[];
  selected: string | null;
  onSelect: (d: string | null) => void;
}) {
  const { hidden } = useApp();
  const now = today();
  const [year, m] = month.split("-").map(Number);
  const offset = new Date(Date.UTC(year, m - 1, 1)).getUTCDay();
  const days = new Date(Date.UTC(year, m, 0)).getUTCDate();
  const cells = Array.from(
    { length: Math.ceil((offset + days) / 7) * 7 },
    (_, i) => i - offset + 1,
  );
  const byDate = new Map<string, Item[]>();
  for (const it of items)
    byDate.set(it.date, [...(byDate.get(it.date) ?? []), it]);
  return (
    <section className="card cal-grid-card">
      <div className="cal-grid" role="grid" aria-label={`${year} 年 ${m} 月`}>
        {["日", "一", "二", "三", "四", "五", "六"].map((w, i) => (
          <div
            key={w}
            className={`cal-weekday ${i === 0 || i === 6 ? "weekend" : ""}`}
            role="columnheader"
          >
            {w}
          </div>
        ))}
        {cells.map((day, i) => {
          if (day < 1 || day > days)
            return <div key={i} className="cal-blank" />;
          const date = `${month}-${String(day).padStart(2, "0")}`;
          const list = byDate.get(date) ?? [];
          const cls = [
            "cal-day",
            date === now && "today",
            date < now && "past",
            date === selected && "selected",
            list.length && "has",
          ]
            .filter(Boolean)
            .join(" ");
          return (
            <button
              key={i}
              type="button"
              role="gridcell"
              className={cls}
              aria-pressed={date === selected}
              aria-label={`${m} 月 ${day} 日，${list.length} 件`}
              onClick={() => onSelect(date === selected ? null : date)}
            >
              <span className="cal-num">{day}</span>
              <span className="cal-events">
                {list.slice(0, 3).map((it) =>
                  it.type === "bill" ? (
                    <span
                      key={it.key}
                      className={`cal-pill bill ${billState(it.bill, now).tone}`}
                    >
                      <BankLogo names={[it.bill.connector]} size={16} />
                      <span className="cal-pill-text">
                        {hidden
                          ? bankName(it.bill)
                          : wan(Number(it.bill.statement_amount ?? 0))}
                      </span>
                      {it.bill.is_paid && <CircleCheck size={12} />}
                    </span>
                  ) : it.type === "sub" ? (
                    <span
                      key={it.key}
                      className={`cal-pill sub ${subscriptionStatus[it.sub.status].tone}`}
                    >
                      <Repeat size={12} />
                      <span className="cal-pill-text">
                        {it.sub.subscription.name}
                      </span>
                      {it.sub.status === "paid" && <CircleCheck size={12} />}
                    </span>
                  ) : (
                    <span
                      key={it.key}
                      className={`cal-pill plan ${Number(it.plan.amount) >= 0 && it.plan.kind !== "expense" ? "in" : ""}`}
                    >
                      <Repeat2 size={12} />
                      <span className="cal-pill-text">
                        {it.plan.description}
                      </span>
                    </span>
                  ),
                )}
                {list.length > 3 && (
                  <span className="cal-more">+{list.length - 3}</span>
                )}
              </span>
              {list.length > 0 && (
                <span className="cal-dots" aria-hidden="true">
                  {list.slice(0, 4).map((it) => (
                    <i
                      key={it.key}
                      className={
                        it.type === "bill"
                          ? billState(it.bill, now).tone
                          : it.type === "sub"
                            ? subscriptionStatus[it.sub.status].tone
                            : "plan"
                      }
                    />
                  ))}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </section>
  );
}

function Agenda({
  month,
  items,
  selected,
  onClear,
}: {
  month: string;
  items: Item[];
  selected: string | null;
  onClear: () => void;
}) {
  const now = today();
  const shown = selected ? items.filter((it) => it.date === selected) : items;
  const upcoming = shown.filter((it) => it.date >= now);
  const earlier = shown.filter((it) => it.date < now);
  const title = selected
    ? `${Number(selected.slice(5, 7))} 月 ${Number(selected.slice(8))} 日`
    : `${Number(month.slice(5))} 月全部`;
  return (
    <section className="card flush cal-agenda">
      <header className="card-head">
        <h2>{title}</h2>
        {selected ? (
          <button type="button" className="chip" onClick={onClear}>
            <X size={13} /> 看整個月
          </button>
        ) : (
          <span className="dim small">{items.length} 件</span>
        )}
      </header>
      {!shown.length ? (
        <p className="dim small cal-empty">
          {selected
            ? "這一天沒有帳單、固定收支或訂閱。"
            : "這個月沒有帳單、固定收支或訂閱。"}
        </p>
      ) : (
        <>
          {upcoming.length > 0 && (
            <AgendaGroup
              label={earlier.length ? "接下來" : undefined}
              items={upcoming}
            />
          )}
          {earlier.length > 0 && (
            <AgendaGroup label="已經過了" items={earlier} past />
          )}
        </>
      )}
    </section>
  );
}

function AgendaGroup({
  label,
  items,
  past = false,
}: {
  label?: string;
  items: Item[];
  past?: boolean;
}) {
  const { navigate } = useApp();
  const now = today();
  return (
    <div className={`cal-group ${past ? "past" : ""}`}>
      {label && <div className="cal-group-label">{label}</div>}
      <ul>
        {items.map((it) => {
          const d = new Date(`${it.date}T00:00:00Z`);
          const date = (
            <span className="cal-date">
              <strong>{d.getUTCDate()}</strong>
              <small>週{"日一二三四五六"[d.getUTCDay()]}</small>
            </span>
          );
          if (it.type === "bill") {
            const b = it.bill;
            const st = billState(b, now);
            return (
              <li key={it.key}>
                <button
                  type="button"
                  className="cal-item"
                  onClick={() => navigate("sources", { source: b.connector })}
                  title="查看資料來源"
                >
                  {date}
                  <BankLogo names={[b.connector]} size={36} />
                  <span className="cal-item-main">
                    <strong>{bankName(b)}</strong>
                    <small>
                      {b.account_name} · {b.period} 帳單
                      {b.minimum_payment != null &&
                        ` · 最低 ${money(b.minimum_payment, b.currency)}`}
                    </small>
                    <small>更新於 {relative(b.last_synced_at)}</small>
                  </span>
                  <span className="cal-item-side">
                    <Money value={b.statement_amount} currency={b.currency} />
                    <span className={`cal-state ${st.tone}`}>{st.label}</span>
                  </span>
                  <ChevronRight size={16} className="row-chevron" />
                </button>
              </li>
            );
          }
          if (it.type === "sub") {
            const p = it.sub;
            const st = subscriptionStatus[p.status];
            const charged = p.items.find(
              (i) => i.role === "charge" && i.match !== "pending",
            );
            return (
              <li key={it.key}>
                <button
                  type="button"
                  className="cal-item"
                  onClick={() => navigate("subscriptions")}
                  title="在訂閱頁查看扣款明細"
                >
                  {date}
                  <SubscriptionIcon subscription={p.subscription} size={36} />
                  <span className="cal-item-main">
                    <strong>{p.subscription.name}</strong>
                    <small>
                      {p.subscription.cycle === "yearly" ? "每年" : "每月"}
                      訂閱 · {p.subscription.category}
                      {charged?.account_name && ` · ${charged.account_name}`}
                    </small>
                  </span>
                  <span className="cal-item-side">
                    {p.status === "paid" ? (
                      <Money value={subAmount(p)} />
                    ) : (
                      <span className="dim small">
                        約 <Money value={subAmount(p)} />
                      </span>
                    )}
                    <span className={`cal-state ${st.tone}`}>
                      <Repeat size={11} /> {st.label}
                    </span>
                  </span>
                  <ChevronRight size={16} className="row-chevron" />
                </button>
              </li>
            );
          }
          const p = it.plan;
          return (
            <li key={it.key}>
              <button
                type="button"
                className="cal-item"
                onClick={() => navigate("plans")}
                title="在計畫頁管理固定收支"
              >
                {date}
                <CategoryIcon category={p.category ?? "未分類"} size={36} />
                <span className="cal-item-main">
                  <strong>{p.description}</strong>
                  <small>
                    {frequencies[p.frequency] ?? p.frequency} 固定 ·{" "}
                    {p.account_name}
                  </small>
                </span>
                <span className="cal-item-side">
                  <Money value={p.amount} currency={p.currency} sign tone />
                  <span className="cal-state neutral">
                    <Repeat2 size={11} /> 自動記帳
                  </span>
                </span>
                <ChevronRight size={16} className="row-chevron" />
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
