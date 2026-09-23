"use client";
import { useMemo, useState } from "react";
import {
  ArrowRight,
  BellRing,
  CircleAlert,
  CreditCard,
  Landmark,
  PiggyBank,
  Plus,
  ShieldAlert,
  Target,
  type LucideIcon,
} from "lucide-react";
import Decimal from "decimal.js";
import { type Entry } from "@/lib/api";
import {
  daysLeftInMonth,
  greeting,
  money,
  percent,
  sourceNames,
  thisMonth,
  today,
} from "@/lib/format";
import {
  useAccounts,
  useBudgets,
  useDashboard,
  useGoals,
  useRecent,
  useJobsContext,
  useNetWorth,
  useSourceAssets,
} from "@/lib/queries";
import { useApp, type View } from "@/components/app-context";
import {
  Button,
  Card,
  categoryColor,
  Empty,
  ErrorLine,
  Money,
  Skeleton,
  Writable,
} from "@/components/ui";
import { EntryDetail, RecentList } from "./activity";
import { NetWorthPanel } from "./net-worth";
import { scheduledPaymentAmount } from "@/lib/payment-reminders";

export function OverviewPage() {
  const { user, navigate, compose, hidden } = useApp();
  const accounts = useAccounts();
  const dashboard = useDashboard();
  const recent = useRecent();
  const budgets = useBudgets();
  const worth = useNetWorth();
  const [selected, setSelected] = useState<Entry>();
  const month = dashboard.data?.monthly.find((m) => m.currency === "TWD");
  const spend = (dashboard.data?.categories ?? []).filter(
    (c) => c.currency === "TWD",
  );
  const income = new Decimal(month?.income ?? 0);
  const expense = new Decimal(month?.expense ?? 0);
  const balance = income.minus(expense);
  const monthBudgets = (budgets.data ?? []).filter(
    (b) => b.month === thisMonth() && b.currency === "TWD",
  );

  if (accounts.isPending || dashboard.isPending)
    return (
      <div className="stack">
        <Skeleton rows={1} h={180} />
        <Skeleton rows={4} />
      </div>
    );
  if (accounts.isError || dashboard.isError)
    return (
      <ErrorLine
        retry={() => {
          void accounts.refetch();
          void dashboard.refetch();
        }}
      >
        總覽資料讀取失敗
      </ErrorLine>
    );

  const hasAccounts = !!accounts.data?.length;
  const hasEntries = !!recent.data?.length;
  return (
    <div className="stack">
      <div className="greet">
        <h1>
          {greeting()}，{user.name}
        </h1>
        <p>
          {today().replaceAll("-", "/")} · 本月還有 {daysLeftInMonth()} 天
        </p>
      </div>

      <Attention />

      {worth.data ? (
        <NetWorthPanel
          data={worth.data}
          onOpenAssets={() => navigate("assets")}
        />
      ) : worth.isError ? (
        <ErrorLine retry={() => worth.refetch()}>淨資產讀取失敗</ErrorLine>
      ) : (
        <Skeleton rows={1} h={420} />
      )}

      <div className="overview-grid">
        <section className="card month">
          <div className="card-head">
            <h2>本月收支</h2>
            <button
              type="button"
              className="link"
              onClick={() => navigate("activity")}
            >
              全部 <ArrowRight size={14} />
            </button>
          </div>
          <div className="month-rows">
            <div>
              <span>收入</span>
              <Money value={income.toString()} tone className="lg" />
            </div>
            <div>
              <span>支出</span>
              <Money value={expense.toString()} className="lg" />
            </div>
          </div>
          <MonthBar income={income} expense={expense} />
          <div className="month-balance">
            <span>結餘</span>
            <Money value={balance.toString()} sign tone />
          </div>
        </section>
        <Card
          title="本月花在哪裡"
          action={
            spend.length > 0 && (
              <button
                type="button"
                className="link"
                onClick={() => navigate("analysis")}
              >
                完整分析 <ArrowRight size={14} />
              </button>
            )
          }
        >
          {spend.length ? (
            <SpendBreakdown
              items={spend}
              total={expense}
              hidden={hidden}
              budgets={Object.fromEntries(
                monthBudgets.map((b) => [b.category, b]),
              )}
              onPick={(c) => navigate("activity", { q: c })}
            />
          ) : (
            <Empty compact icon={PiggyBank} title="這個月還沒有支出">
              記帳後，這裡會依分類畫出花費比例。
            </Empty>
          )}
        </Card>
      </div>
      <div>
        <Card
          title="最近收支"
          pad={false}
          action={
            hasEntries && (
              <button
                type="button"
                className="link"
                onClick={() => navigate("activity")}
              >
                查看全部 <ArrowRight size={14} />
              </button>
            )
          }
        >
          {recent.isPending ? (
            <div className="pad">
              <Skeleton rows={4} />
            </div>
          ) : hasEntries ? (
            <RecentList
              entries={recent.data!.slice(0, 6)}
              onOpen={setSelected}
            />
          ) : (
            <Empty
              compact
              title="還沒有紀錄"
              action={
                hasAccounts && (
                  <Writable>
                    <Button
                      variant="primary"
                      size="sm"
                      icon={Plus}
                      onClick={() => compose()}
                    >
                      記一筆
                    </Button>
                  </Writable>
                )
              }
            />
          )}
        </Card>
      </div>
      {selected && (
        <EntryDetail entry={selected} onClose={() => setSelected(undefined)} />
      )}
    </div>
  );
}

function MonthBar({ income, expense }: { income: Decimal; expense: Decimal }) {
  const max = Decimal.max(income, expense, 1);
  return (
    <div className="month-bar" aria-hidden="true">
      <span
        className="in"
        style={{ width: `${income.div(max).times(100).toNumber()}%` }}
      />
      <span
        className="out"
        style={{ width: `${expense.div(max).times(100).toNumber()}%` }}
      />
    </div>
  );
}

function SpendBreakdown({
  items,
  total,
  hidden,
  budgets,
  onPick,
}: {
  items: { category: string; amount: string }[];
  total: Decimal;
  hidden: boolean;
  budgets: Record<string, { amount: string }>;
  onPick: (c: string) => void;
}) {
  const sum = items.reduce(
    (a, c) => a.plus(Decimal.max(0, c.amount)),
    new Decimal(0),
  );
  const shown = items.filter((c) => new Decimal(c.amount).gt(0));
  let acc = 0;
  return (
    <div className="spend">
      <svg
        viewBox="0 0 42 42"
        className="donut"
        role="img"
        aria-label="支出分類比例"
      >
        <circle cx="21" cy="21" r="15.915" className="donut-track" />
        {shown.map((c) => {
          const pct = sum.isZero()
            ? 0
            : new Decimal(c.amount).div(sum).times(100).toNumber();
          const seg = (
            <circle
              key={c.category}
              cx="21"
              cy="21"
              r="15.915"
              className="donut-seg"
              style={{ stroke: categoryColor(c.category) }}
              strokeDasharray={`${Math.max(0, pct - 0.6)} ${100 - Math.max(0, pct - 0.6)}`}
              strokeDashoffset={25 - acc}
            />
          );
          acc += pct;
          return seg;
        })}
        <text x="21" y="20" className="donut-label">
          本月支出
        </text>
        <text x="21" y="25.5" className="donut-value">
          {hidden ? "••••" : money(total.toString()).replace("NT$ ", "$")}
        </text>
      </svg>
      <ul className="legend">
        {shown.map((c) => {
          const pct = sum.isZero()
            ? 0
            : new Decimal(c.amount).div(sum).times(100).toNumber();
          const b = budgets[c.category];
          const over = b && new Decimal(c.amount).gt(b.amount);
          return (
            <li key={c.category}>
              <button type="button" onClick={() => onPick(c.category)}>
                <i style={{ background: categoryColor(c.category) }} />
                <span className="legend-name">{c.category}</span>
                <span className="legend-pct">{percent(pct)}</span>
                <Money value={c.amount} className={over ? "bad" : ""} />
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** 需要注意：把跨頁面、需要使用者行動的事集中在總覽 */
function Attention() {
  const { navigate } = useApp();
  const { jobs } = useJobsContext();
  const budgets = useBudgets();
  const goals = useGoals();
  const dashboard = useDashboard();
  const sources = useSourceAssets();
  const items = useMemo(() => {
    const list: {
      key: string;
      icon: LucideIcon;
      tone: "warn" | "bad" | "info";
      text: string;
      action: string;
      view: View;
      params?: Record<string, string>;
    }[] = [];
    for (const j of jobs)
      if (j.status === "waiting_user" && !j.answer_pending)
        list.push({
          key: `job-${j.id}`,
          icon: ShieldAlert,
          tone: "bad",
          text: `${sourceNames[j.connector ?? ""] ?? "資料來源"}正在等你完成驗證（約 10 分鐘內有效）`,
          action: "去驗證",
          view: "sources",
        });
    for (const b of budgets.data ?? []) {
      if (b.month !== thisMonth()) continue;
      const ratio = Number(b.spent) / Number(b.amount);
      if (ratio >= 1)
        list.push({
          key: `budget-${b.id}`,
          icon: CircleAlert,
          tone: "bad",
          text: `「${b.category}」已超出預算 ${money(new Decimal(b.spent).minus(b.amount).toString(), b.currency)}`,
          action: "查看預算",
          view: "plans",
        });
      else if (ratio >= 0.8)
        list.push({
          key: `budget-${b.id}`,
          icon: BellRing,
          tone: "warn",
          text: `「${b.category}」預算已用 ${Math.round(ratio * 100)}%，剩 ${money(new Decimal(b.amount).minus(b.spent).toString(), b.currency)}`,
          action: "查看預算",
          view: "plans",
        });
    }
    const unknown =
      dashboard.data?.totals.reduce((a, t) => a + t.unknown_accounts, 0) ?? 0;
    if (unknown)
      list.push({
        key: "valuation",
        icon: Landmark,
        tone: "warn",
        text: `${unknown} 個投資帳戶的持倉缺少價格，淨資產暫時無法加總`,
        action: "查看",
        view: "assets",
      });
    const week = new Date(Date.now() + 7 * 86400000).toISOString().slice(0, 10);
    for (const a of sources.data ?? []) {
      const due = a.payment_due?.slice(0, 10);
      if (a.category !== "debt" || !due || due < today() || due > week)
        continue;
      const payment = scheduledPaymentAmount(a);
      if (payment === "0") continue;
      const loan = a.account_type === "loan" || a.account_type === "mortgage";
      const when = due.slice(5).replace("-", "/");
      list.push({
        key: `due-${a.id}`,
        icon: loan ? Landmark : CreditCard,
        tone: "warn",
        text: `${a.name} ${loan ? `將於 ${when} 還款` : `繳款期限 ${when}`}，${payment === null ? "應繳金額待確認" : `${loan ? "下期" : "本期"}應繳 ${money(payment, a.currency)}`}`,
        action: "查看",
        view: "assets",
      });
    }
    const soon = new Date(Date.now() + 30 * 86400000)
      .toISOString()
      .slice(0, 10);
    for (const g of goals.data ?? [])
      if (g.due_on && g.due_on <= soon && new Decimal(g.saved).lt(g.target))
        list.push({
          key: `goal-${g.id}`,
          icon: Target,
          tone: "info",
          text: `目標「${g.name}」將在 ${g.due_on} 到期，還差 ${money(new Decimal(g.target).minus(g.saved).toString(), g.currency)}`,
          action: "查看",
          view: "plans",
          params: { tab: "goals" },
        });
    return list;
  }, [jobs, budgets.data, goals.data, dashboard.data, sources.data]);
  if (!items.length) return null;
  return (
    <section className="attention" aria-label="需要注意">
      {items.map((i) => (
        <div key={i.key} className={`attention-item ${i.tone}`}>
          <i.icon size={18} />
          <span>{i.text}</span>
          <button
            type="button"
            className="link"
            onClick={() => navigate(i.view, i.params)}
          >
            {i.action} <ArrowRight size={14} />
          </button>
        </div>
      ))}
    </section>
  );
}
