"use client";
import { useMemo, useState, type FormEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  CalendarClock,
  ChevronLeft,
  ChevronRight,
  Copy,
  PiggyBank,
  Plus,
  Repeat2,
  Target,
} from "lucide-react";
import Decimal from "decimal.js";
import { api, type Budget, type Goal, type Recurring } from "@/lib/api";
import { useCategoryNames } from "@/lib/categories";
import {
  currencies,
  daysLeftInMonth,
  evaluate,
  frequencies,
  money,
  monthLabel,
  shiftMonth,
  shortDate,
  symbol,
  thisMonth,
  today,
} from "@/lib/format";
import { useAccounts, useBudgets, useGoals } from "@/lib/queries";
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
  PageHead,
  Progress,
  Segmented,
  Sheet,
  Skeleton,
  Switch,
  Writable,
} from "@/components/ui";
import { AccountPicker } from "./composer";

type Tab = "budgets" | "goals" | "recurring";

export function PlansPage() {
  const { route, navigate } = useApp();
  const tab = (
    ["budgets", "goals", "recurring"].includes(route.params.get("tab") ?? "")
      ? route.params.get("tab")
      : "budgets"
  ) as Tab;
  return (
    <>
      <PageHead title="計畫" subtitle="預算、儲蓄目標與固定收支" />
      <div className="tabs">
        <Segmented<Tab>
          label="計畫類型"
          value={tab}
          onChange={(t) => navigate("plans", t === "budgets" ? {} : { tab: t })}
          options={[
            { value: "budgets", label: "每月預算" },
            { value: "goals", label: "儲蓄目標" },
            { value: "recurring", label: "固定收支" },
          ]}
        />
      </div>
      {tab === "budgets" && <Budgets />}
      {tab === "goals" && <Goals />}
      {tab === "recurring" && <RecurringList />}
    </>
  );
}

/* ---------------- 預算 ---------------- */
function Budgets() {
  const budgets = useBudgets();
  const { run, busy } = useAction();
  const { viewing } = useApp();
  const [month, setMonth] = useState(thisMonth());
  const [editing, setEditing] = useState<Budget | "new">();
  const list = (budgets.data ?? []).filter((b) => b.month === month);
  const previous = (budgets.data ?? []).filter(
    (b) => b.month === shiftMonth(month, -1),
  );
  const current = month === thisMonth();
  const twd = list.filter((b) => b.currency === "TWD");
  const total = twd.reduce((a, b) => a.plus(b.amount), new Decimal(0));
  const spent = twd.reduce((a, b) => a.plus(b.spent), new Decimal(0));
  const left = total.minus(spent);
  const days = daysLeftInMonth();
  return (
    <div className="stack">
      <div className="month-nav">
        <button
          type="button"
          className="icon-btn"
          aria-label="上個月"
          onClick={() => setMonth(shiftMonth(month, -1))}
        >
          <ChevronLeft size={18} />
        </button>
        <strong>{monthLabel(month)}</strong>
        <button
          type="button"
          className="icon-btn"
          aria-label="下個月"
          disabled={month >= shiftMonth(thisMonth(), 1)}
          onClick={() => setMonth(shiftMonth(month, 1))}
        >
          <ChevronRight size={18} />
        </button>
        {!current && (
          <button
            type="button"
            className="link"
            onClick={() => setMonth(thisMonth())}
          >
            回到本月
          </button>
        )}
        <span className="grow" />
        <Writable>
          <Button
            variant="primary"
            icon={Plus}
            onClick={() => setEditing("new")}
          >
            設定預算
          </Button>
        </Writable>
      </div>
      {budgets.isPending ? (
        <Skeleton rows={3} h={84} />
      ) : budgets.isError ? (
        <ErrorLine retry={() => budgets.refetch()}>預算讀取失敗</ErrorLine>
      ) : !list.length ? (
        <section className="card">
          <Empty
            icon={PiggyBank}
            title={`${monthLabel(month)}還沒有預算`}
            action={
              <div className="inline-actions">
                {previous.length > 0 && (
                  <Writable>
                    <Button
                      icon={Copy}
                      busy={busy}
                      onClick={async () => {
                        for (const b of previous)
                          if (
                            !(await run("/budgets", {
                              body: {
                                month,
                                category: b.category,
                                amount: b.amount,
                                currency: b.currency,
                              },
                              toastError: true,
                            }))
                          )
                            return;
                      }}
                    >
                      沿用上個月的 {previous.length} 項預算
                    </Button>
                  </Writable>
                )}
                <Writable>
                  <Button
                    variant="primary"
                    icon={Plus}
                    onClick={() => setEditing("new")}
                  >
                    設定預算
                  </Button>
                </Writable>
              </div>
            }
          >
            替主要分類訂一個每月上限，花費接近時總覽會提醒你。
          </Empty>
        </section>
      ) : (
        <>
          {twd.length > 0 && (
            <section className="card budget-summary">
              <div>
                <span>{left.isNegative() ? "已超支" : "還可以花"}</span>
                <Money
                  value={left.abs().toString()}
                  className={`xl ${left.isNegative() ? "bad" : ""}`}
                />
                {current && left.isPositive() && (
                  <small>
                    平均每天{" "}
                    <Money
                      value={left.div(days).toDecimalPlaces(0).toString()}
                    />
                    （剩 {days} 天）
                  </small>
                )}
              </div>
              <div className="budget-summary-bar">
                <Progress
                  value={
                    total.isZero() ? 0 : spent.div(total).times(100).toNumber()
                  }
                  tone={tone(spent, total)}
                  label="預算總使用率"
                />
                <span>
                  已花 <Money value={spent.toString()} /> /{" "}
                  <Money value={total.toString()} />
                </span>
              </div>
            </section>
          )}
          <div className="budget-grid">
            {list.map((b) => {
              const s = new Decimal(b.spent),
                a = new Decimal(b.amount),
                rest = a.minus(s);
              return (
                <button
                  type="button"
                  key={b.id}
                  className="card budget"
                  onClick={() => !viewing && setEditing(b)}
                >
                  <div className="budget-top">
                    <CategoryIcon category={b.category} size={36} />
                    <strong>{b.category}</strong>
                    {b.currency !== "TWD" && <Badge>{b.currency}</Badge>}
                    <span
                      className={`budget-left ${rest.isNegative() ? "bad" : ""}`}
                    >
                      {rest.isNegative() ? "超支 " : "剩 "}
                      <Money
                        value={rest.abs().toString()}
                        currency={b.currency}
                      />
                    </span>
                  </div>
                  <Progress
                    value={a.isZero() ? 0 : s.div(a).times(100).toNumber()}
                    tone={tone(s, a)}
                    label={`${b.category}預算使用率`}
                  />
                  <div className="budget-foot">
                    <span>
                      <Money value={b.spent} currency={b.currency} /> /{" "}
                      <Money value={b.amount} currency={b.currency} />
                    </span>
                    {current && rest.isPositive() && (
                      <span>
                        每天{" "}
                        <Money
                          value={rest.div(days).toDecimalPlaces(0).toString()}
                          currency={b.currency}
                        />
                      </span>
                    )}
                  </div>
                </button>
              );
            })}
          </div>
        </>
      )}
      {editing && (
        <BudgetSheet
          budget={editing === "new" ? undefined : editing}
          month={month}
          existing={list.map((b) => b.category)}
          onClose={() => setEditing(undefined)}
        />
      )}
    </div>
  );
}
const tone = (spent: Decimal, total: Decimal) => {
  if (total.isZero()) return "good" as const;
  const r = spent.div(total).toNumber();
  return r >= 1
    ? ("bad" as const)
    : r >= 0.8
      ? ("warn" as const)
      : ("good" as const);
};

function BudgetSheet({
  budget,
  month,
  existing,
  onClose,
}: {
  budget?: Budget;
  month: string;
  existing: string[];
  onClose: () => void;
}) {
  const { run, busy, error, setError } = useAction();
  const { main: expenseNames } = useCategoryNames("expense");
  const [category, setCategory] = useState(
    budget?.category ?? expenseNames.find((c) => !existing.includes(c)) ?? "",
  );
  const [amount, setAmount] = useState(
    budget ? new Decimal(budget.amount).toString() : "",
  );
  const [currency, setCurrency] = useState(budget?.currency ?? "TWD");
  const options = [...new Set([...expenseNames, ...existing])];
  async function submit(e: FormEvent) {
    e.preventDefault();
    const v = evaluate(amount);
    if (!v || new Decimal(v).lt(1)) {
      setError("預算至少 1 元");
      return;
    }
    if (!category.trim()) {
      setError("請選擇分類");
      return;
    }
    if (
      await run("/budgets", {
        body: {
          month: budget?.month ?? month,
          category: category.trim(),
          amount: v,
          currency,
        },
        success: budget ? "預算已更新" : "預算已設定",
      })
    )
      onClose();
  }
  return (
    <Sheet
      title={budget ? `${budget.category}預算` : "設定預算"}
      description={monthLabel(budget?.month ?? month)}
      onClose={onClose}
      busy={busy}
    >
      <form className="form" onSubmit={submit}>
        <FormError>{error}</FormError>
        {!budget && (
          <fieldset className="field">
            <legend className="field-label">分類</legend>
            <div className="category-grid">
              {options.map((c) => (
                <button
                  type="button"
                  key={c}
                  className={`category-option ${category === c ? "on" : ""}`}
                  aria-pressed={category === c}
                  onClick={() => setCategory(c)}
                >
                  <CategoryIcon category={c} size={34} />
                  <span>{c}</span>
                  {existing.includes(c) && (
                    <small className="dot-note">已設定</small>
                  )}
                </button>
              ))}
            </div>
            <input
              className="mt-8"
              name="category"
              aria-label="或輸入其他分類"
              placeholder="或輸入其他分類"
              value={options.includes(category) ? "" : category}
              onChange={(e) => setCategory(e.target.value)}
            />
          </fieldset>
        )}
        <div className="grid-2">
          <Field label="每月上限">
            <div className="affix">
              <span>{symbol(currency)}</span>
              <input
                name="amount"
                inputMode="decimal"
                autoComplete="off"
                required
                data-autofocus
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
              />
            </div>
          </Field>
          <Field label="幣別">
            <select
              name="currency"
              value={currency}
              disabled={!!budget}
              onChange={(e) => setCurrency(e.target.value)}
            >
              {currencies.map((c) => (
                <option key={c}>{c}</option>
              ))}
            </select>
          </Field>
        </div>
        {!budget && existing.includes(category) && (
          <p className="hint-line">這個分類本月已有預算，儲存後會更新金額。</p>
        )}
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

/* ---------------- 目標 ---------------- */
function monthsUntil(date: string) {
  const [y, m] = today().split("-").map(Number);
  const [ty, tm] = date.split("-").map(Number);
  return (ty - y) * 12 + (tm - m);
}
function Goals() {
  const goals = useGoals();
  const [editing, setEditing] = useState<Goal | "new">();
  const [depositing, setDepositing] = useState<Goal>();
  return (
    <div className="stack">
      <div className="month-nav">
        <span className="muted">為想完成的事，一點一點存。</span>
        <span className="grow" />
        <Writable>
          <Button
            variant="primary"
            icon={Plus}
            onClick={() => setEditing("new")}
          >
            新增目標
          </Button>
        </Writable>
      </div>
      {goals.isPending ? (
        <Skeleton rows={2} h={120} />
      ) : goals.isError ? (
        <ErrorLine retry={() => goals.refetch()}>目標讀取失敗</ErrorLine>
      ) : !goals.data?.length ? (
        <section className="card">
          <Empty
            icon={Target}
            title="下一個想完成的是什麼？"
            action={
              <Writable>
                <Button
                  variant="primary"
                  icon={Plus}
                  onClick={() => setEditing("new")}
                >
                  新增目標
                </Button>
              </Writable>
            }
          >
            旅行、緊急預備金、換新電腦……設定金額與日期，我們會算出每個月要存多少。
          </Empty>
        </section>
      ) : (
        <div className="goal-grid">
          {goals.data.map((g) => {
            const saved = new Decimal(g.saved),
              target = new Decimal(g.target),
              rest = target.minus(saved);
            const done = rest.lte(0);
            const months = g.due_on ? monthsUntil(g.due_on) : null;
            const overdue = g.due_on && g.due_on < today() && !done;
            return (
              <section key={g.id} className={`card goal ${done ? "done" : ""}`}>
                <div className="goal-top">
                  <strong>{g.name}</strong>
                  {done ? (
                    <Badge tone="good">已達成</Badge>
                  ) : overdue ? (
                    <Badge tone="bad">已過期</Badge>
                  ) : (
                    g.due_on && <Badge>{g.due_on}</Badge>
                  )}
                </div>
                <div className="goal-amount">
                  <Money value={g.saved} currency={g.currency} className="lg" />
                  <span>
                    / <Money value={g.target} currency={g.currency} />
                  </span>
                </div>
                <Progress
                  value={saved.div(target).times(100).toNumber()}
                  tone={done ? "good" : "accent"}
                  label={`${g.name}進度`}
                />
                <p className="goal-note">
                  {done ? (
                    "恭喜，目標完成！"
                  ) : months != null && months > 0 ? (
                    <>
                      還差{" "}
                      <Money value={rest.toString()} currency={g.currency} />
                      ，每月存{" "}
                      <Money
                        value={rest
                          .div(months)
                          .toDecimalPlaces(0, Decimal.ROUND_UP)
                          .toString()}
                        currency={g.currency}
                      />{" "}
                      可以在 {months} 個月後達成
                    </>
                  ) : (
                    <>
                      還差{" "}
                      <Money value={rest.toString()} currency={g.currency} />
                    </>
                  )}
                </p>
                <div className="inline-actions">
                  {!done && (
                    <Writable>
                      <Button
                        size="sm"
                        variant="primary"
                        icon={Plus}
                        onClick={() => setDepositing(g)}
                      >
                        存入
                      </Button>
                    </Writable>
                  )}
                  <Writable>
                    <Button size="sm" onClick={() => setEditing(g)}>
                      編輯
                    </Button>
                  </Writable>
                </div>
              </section>
            );
          })}
        </div>
      )}
      {editing && (
        <GoalSheet
          goal={editing === "new" ? undefined : editing}
          onClose={() => setEditing(undefined)}
        />
      )}
      {depositing && (
        <DepositSheet
          goal={depositing}
          onClose={() => setDepositing(undefined)}
        />
      )}
    </div>
  );
}
const goalBody = (g: Goal, patch: Partial<Goal>) => ({
  id: g.id,
  name: g.name,
  target: g.target,
  saved: g.saved,
  currency: g.currency,
  due_on: g.due_on,
  ...patch,
});
function DepositSheet({ goal, onClose }: { goal: Goal; onClose: () => void }) {
  const { run, busy, error, setError } = useAction();
  const [amount, setAmount] = useState("");
  const [mode, setMode] = useState<"add" | "withdraw">("add");
  return (
    <Sheet
      title={goal.name}
      description={<>目前已存 {money(goal.saved, goal.currency)}</>}
      onClose={onClose}
      busy={busy}
      size="sm"
    >
      <form
        className="form"
        onSubmit={async (e) => {
          e.preventDefault();
          const v = evaluate(amount);
          if (!v || new Decimal(v).lte(0)) {
            setError("請輸入金額");
            return;
          }
          const next =
            mode === "add"
              ? new Decimal(goal.saved).plus(v)
              : Decimal.max(0, new Decimal(goal.saved).minus(v));
          if (
            await run("/goals", {
              body: goalBody(goal, { saved: next.toString() }),
              success:
                mode === "add" ? `已存入 ${money(v, goal.currency)}` : "已更新",
            })
          )
            onClose();
        }}
      >
        <Segmented
          label="存入或取出"
          value={mode}
          onChange={setMode}
          options={[
            { value: "add", label: "存入" },
            { value: "withdraw", label: "取出" },
          ]}
        />
        <FormError>{error}</FormError>
        <Field label="金額" hint="只更新目標進度，不會新增帳戶交易">
          <div className="affix">
            <span>{symbol(goal.currency)}</span>
            <input
              inputMode="decimal"
              autoComplete="off"
              data-autofocus
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
            />
          </div>
        </Field>
        <Button
          type="submit"
          variant="primary"
          size="lg"
          className="block"
          busy={busy}
        >
          {mode === "add" ? "存入" : "取出"}
        </Button>
      </form>
    </Sheet>
  );
}
function GoalSheet({ goal, onClose }: { goal?: Goal; onClose: () => void }) {
  const { run, busy, error, setError } = useAction();
  const [currency, setCurrency] = useState(goal?.currency ?? "TWD");
  return (
    <Sheet
      title={goal ? "編輯目標" : "新增儲蓄目標"}
      onClose={onClose}
      busy={busy}
    >
      <form
        className="form"
        onSubmit={async (e) => {
          e.preventDefault();
          const v = Object.fromEntries(new FormData(e.currentTarget)) as Record<
            string,
            string
          >;
          const target = evaluate(v.target),
            saved = evaluate(v.saved || "0");
          if (
            !target ||
            new Decimal(target).lte(0) ||
            saved == null ||
            new Decimal(saved).lt(0)
          ) {
            setError("請確認目標金額與已存金額");
            return;
          }
          if (
            await run("/goals", {
              body: {
                ...(goal ? { id: goal.id } : {}),
                name: v.name.trim(),
                target,
                saved,
                currency,
                due_on: v.due_on || null,
              },
              success: goal ? "目標已更新" : "目標已建立",
            })
          )
            onClose();
        }}
      >
        <FormError>{error}</FormError>
        <Field label="目標名稱">
          <input
            name="name"
            required
            maxLength={120}
            data-autofocus
            defaultValue={goal?.name}
            placeholder="例如：日本旅行基金"
          />
        </Field>
        <div className="grid-2">
          <Field label="目標金額">
            <div className="affix">
              <span>{symbol(currency)}</span>
              <input
                name="target"
                inputMode="decimal"
                required
                autoComplete="off"
                defaultValue={goal ? new Decimal(goal.target).toString() : ""}
              />
            </div>
          </Field>
          <Field label="目前已存">
            <div className="affix">
              <span>{symbol(currency)}</span>
              <input
                name="saved"
                inputMode="decimal"
                autoComplete="off"
                defaultValue={goal ? new Decimal(goal.saved).toString() : "0"}
              />
            </div>
          </Field>
        </div>
        <div className="grid-2">
          <Field label="預計達成日" optional hint="填了就會算出每月該存多少">
            <input
              name="due_on"
              type="date"
              defaultValue={goal?.due_on ?? ""}
              min={goal ? undefined : today()}
            />
          </Field>
          <Field label="幣別">
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

/* ---------------- 固定收支 ---------------- */
function RecurringList() {
  const accounts = useAccounts();
  const list = useQuery({
    queryKey: ["recurring"],
    queryFn: () => api<Recurring[]>("/recurring"),
  });
  const { run } = useAction();
  const { viewing } = useApp();
  const [adding, setAdding] = useState(false);
  const monthly = useMemo(() => {
    const factor: Record<string, number> = {
      daily: 30,
      weekly: 4.33,
      monthly: 1,
      yearly: 1 / 12,
    };
    return (list.data ?? [])
      .filter((r) => r.enabled && r.currency === "TWD")
      .reduce(
        (a, r) => a.plus(new Decimal(r.amount).times(factor[r.frequency] ?? 1)),
        new Decimal(0),
      );
  }, [list.data]);
  return (
    <div className="stack">
      <div className="month-nav">
        <span className="muted">
          {list.data?.length ? (
            <>
              每月固定淨額約{" "}
              <Money value={monthly.toDecimalPlaces(0).toString()} sign tone />
            </>
          ) : (
            "房租、薪水等沒有同步來源的收支，到期自動幫你記一筆。刷卡扣款的訂閱請用「訂閱」頁，避免重複記帳。"
          )}
        </span>
        <span className="grow" />
        <Writable>
          <Button
            variant="primary"
            icon={Plus}
            disabled={!accounts.data?.length}
            onClick={() => setAdding(true)}
          >
            新增固定收支
          </Button>
        </Writable>
      </div>
      <section className="card flush">
        {list.isPending ? (
          <div className="pad">
            <Skeleton rows={3} />
          </div>
        ) : list.isError ? (
          <div className="pad">
            <ErrorLine retry={() => list.refetch()}>讀取失敗</ErrorLine>
          </div>
        ) : !list.data?.length ? (
          <Empty icon={Repeat2} title="沒有固定收支">
            到期時系統會自動新增紀錄並通知你；服務停機期間錯過的也會補記。
          </Empty>
        ) : (
          <ul className="rows">
            {list.data.map((r) => (
              <li
                key={r.id}
                className={`row static ${r.enabled ? "" : "paused"}`}
              >
                <span
                  className="cat-icon"
                  style={{
                    width: 40,
                    height: 40,
                    ["--cat" as string]: "var(--accent)",
                  }}
                >
                  <CalendarClock size={18} />
                </span>
                <span className="row-main">
                  <span className="row-title">
                    <span className="row-text">{r.description}</span>
                    {!r.enabled && <Badge>已暫停</Badge>}
                  </span>
                  <span className="row-meta">
                    {frequencies[r.frequency]} · {r.account_name} ·{" "}
                    {r.enabled
                      ? `下次 ${shortDate(r.next_on)}`
                      : "不會自動記帳"}
                  </span>
                </span>
                <Money
                  value={r.amount}
                  currency={r.currency}
                  sign
                  tone
                  className="row-amount"
                />
                <Switch
                  disabled={!!viewing}
                  checked={r.enabled}
                  label={`${r.enabled ? "暫停" : "啟用"}「${r.description}」`}
                  onChange={(v) =>
                    run(`/recurring/${r.id}`, {
                      method: "PATCH",
                      body: { enabled: v },
                      success: v
                        ? `已啟用「${r.description}」`
                        : `已暫停「${r.description}」`,
                      toastError: true,
                    })
                  }
                />
              </li>
            ))}
          </ul>
        )}
      </section>
      {adding && accounts.data && (
        <RecurringSheet onClose={() => setAdding(false)} />
      )}
    </div>
  );
}
function RecurringSheet({ onClose }: { onClose: () => void }) {
  const accounts = useAccounts();
  const { run, busy, error, setError } = useAction();
  const list = accounts.data ?? [];
  const [kind, setKind] = useState<"expense" | "income">("expense");
  const [accountId, setAccountId] = useState(list[0]?.id ?? "");
  const [frequency, setFrequency] = useState("monthly");
  const account = list.find((a) => a.id === accountId);
  const cats = useCategoryNames(kind).main;
  return (
    <Sheet title="新增固定收支" onClose={onClose} busy={busy}>
      <form
        className="form"
        onSubmit={async (e) => {
          e.preventDefault();
          const v = Object.fromEntries(new FormData(e.currentTarget)) as Record<
            string,
            string
          >;
          const amount = evaluate(v.amount);
          if (!amount || new Decimal(amount).lte(0)) {
            setError("請輸入大於 0 的金額");
            return;
          }
          const n = new Decimal(amount).abs();
          if (
            await run("/recurring", {
              body: {
                account_id: accountId,
                kind,
                amount: (kind === "expense" ? n.negated() : n).toString(),
                description: v.description.trim(),
                category: v.category,
                frequency,
                next_on: v.next_on,
              },
              success: `已建立「${v.description.trim()}」`,
            })
          )
            onClose();
        }}
      >
        <Segmented
          label="類型"
          value={kind}
          onChange={setKind}
          options={[
            { value: "expense", label: "支出" },
            { value: "income", label: "收入" },
          ]}
        />
        <FormError>{error}</FormError>
        <div className="grid-2">
          <Field label="說明">
            <input
              name="description"
              required
              maxLength={500}
              data-autofocus
              placeholder={
                kind === "expense" ? "例如：房租、管理費" : "例如：薪水"
              }
            />
          </Field>
          <Field label="金額">
            <div className="affix">
              <span>{symbol(account?.currency ?? "TWD")}</span>
              <input
                name="amount"
                inputMode="decimal"
                autoComplete="off"
                required
              />
            </div>
          </Field>
        </div>
        <Field label="帳戶">
          <AccountPicker
            accounts={list}
            value={accountId}
            onChange={setAccountId}
          />
        </Field>
        <div className="grid-2">
          <Field label="分類">
            <select name="category" key={kind}>
              {cats.map((c) => (
                <option key={c}>{c}</option>
              ))}
            </select>
          </Field>
          <Field label="首次日期">
            <input
              type="date"
              name="next_on"
              defaultValue={today()}
              min={today()}
              required
            />
          </Field>
        </div>
        <fieldset className="field">
          <legend className="field-label">頻率</legend>
          <Segmented
            label="頻率"
            size="sm"
            value={frequency}
            onChange={setFrequency}
            options={Object.entries(frequencies).map(([value, label]) => ({
              value,
              label,
            }))}
          />
        </fieldset>
        <p className="hint-line">
          遇到月底不足的月份會記在最後一天（例如 1/31 → 2/28 →
          3/31）。只會新增本站紀錄，不會操作銀行扣款。
        </p>
        <div className="form-actions">
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button type="submit" variant="primary" busy={busy}>
            建立
          </Button>
        </div>
      </form>
    </Sheet>
  );
}
