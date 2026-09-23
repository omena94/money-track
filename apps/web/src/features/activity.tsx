"use client";
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useInfiniteQuery } from "@tanstack/react-query";
import {
  Download,
  FileUp,
  Pencil,
  Plus,
  Scissors,
  RotateCcw,
  Search,
  Sparkles,
  Trash2,
  Undo2,
  Upload,
  Tags,
  X,
} from "lucide-react";
import Decimal from "decimal.js";
import { api, type Account, type Entry } from "@/lib/api";
import {
  dayLabel,
  entryKinds,
  evaluate,
  money,
  symbol,
  today,
} from "@/lib/format";
import { useAccounts, useDashboard, useSourceAssets } from "@/lib/queries";
import {
  useAction,
  useApp,
  useDebounced,
  useHotkey,
} from "@/components/app-context";
import {
  Badge,
  Button,
  CategoryIcon,
  Chip,
  Empty,
  ErrorLine,
  Field,
  FormError,
  Money,
  PageHead,
  Sheet,
  Skeleton,
} from "@/components/ui";
import { CategoryPicker, SourceEntryForm } from "./categorize";
import { CategoriesSheet } from "./category-manager";
import { AccountPicker } from "./account-picker";
import { useCategories } from "@/lib/categories";
import { useSubscriptionMatches } from "./subscriptions";

const PAGE = 200;
type TypeFilter = "all" | "expense" | "income" | "transfer" | "uncategorized";
const uncategorized = (e: Entry) =>
  e.kind !== "transfer" && e.category === "未分類";

export function ActivityPage() {
  const { route, navigate, compose, viewing } = useApp();
  const accounts = useAccounts();
  const sourceAccounts = useSourceAssets();
  const dashboard = useDashboard();
  const accountId = route.params.get("account") ?? "";
  const [q, setQ] = useState(route.params.get("q") ?? "");
  const [type, setType] = useState<TypeFilter>("all");
  const [selected, setSelected] = useState<Entry>();
  const [importing, setImporting] = useState(false);
  const [rules, setRules] = useState(false);
  const query = useDebounced(q.trim(), 300);
  const searchRef = useRef<HTMLInputElement>(null);
  useHotkey("/", () => searchRef.current?.focus());
  useEffect(() => setQ(route.params.get("q") ?? ""), [route.params]);

  const entries = useInfiniteQuery({
    queryKey: ["transactions", "feed", query, accountId],
    initialPageParam: 0,
    queryFn: ({ pageParam }) => {
      const p = new URLSearchParams({ offset: String(pageParam) });
      if (query) p.set("q", query);
      if (accountId) p.set("account_id", accountId);
      return api<Entry[]>(`/activity-feed?${p}`);
    },
    getNextPageParam: (last, all) =>
      last.length === PAGE ? all.length * PAGE : undefined,
  });
  const all = useMemo(() => entries.data?.pages.flat() ?? [], [entries.data]);
  const rows = useMemo(
    () =>
      all.filter((e) =>
        type === "all"
          ? true
          : type === "uncategorized"
            ? uncategorized(e)
            : type === "expense"
              ? e.kind === "expense" || e.kind === "refund"
              : e.kind === type,
      ),
    [all, type],
  );
  const groups = useMemo(() => {
    const map = new Map<string, Entry[]>();
    for (const e of rows)
      map.set(e.occurred_on, [...(map.get(e.occurred_on) ?? []), e]);
    return [...map.entries()];
  }, [rows]);
  const totals = useMemo(() => {
    const currencies = new Set(rows.map((e) => e.currency));
    if (currencies.size !== 1) return null;
    let income = new Decimal(0),
      expense = new Decimal(0);
    for (const e of rows) {
      if (e.kind === "income") income = income.plus(e.amount);
      if (e.kind === "expense" || e.kind === "refund")
        expense = expense.minus(e.amount);
    }
    return { currency: [...currencies][0], income, expense };
  }, [rows]);
  const account =
    accounts.data?.find((a) => a.id === accountId) ??
    sourceAccounts.data?.find((a) => a.id === accountId);
  const pending = useMemo(() => all.filter(uncategorized).length, [all]);
  const month = dashboard.data?.monthly.find((m) => m.currency === "TWD");
  const filtered = !!(query || accountId || type !== "all");

  return (
    <>
      <PageHead
        title="收支"
        subtitle={
          month && !filtered ? (
            <>
              本月收入 <Money value={month.income} /> · 支出{" "}
              <Money value={month.expense} />
            </>
          ) : undefined
        }
        actions={
          viewing ? undefined : (
            <>
              <Button icon={Tags} onClick={() => setRules(true)}>
                分類
              </Button>
              <Button
                icon={Upload}
                onClick={() => setImporting(true)}
                disabled={!accounts.data?.length}
              >
                匯入
              </Button>
              <a
                className="btn btn-secondary"
                href="/api/v1/export/transactions.csv"
                download
              >
                <Download size={17} />
                匯出
              </a>
              <Button
                variant="primary"
                icon={Plus}
                onClick={() => compose({ accountId: accountId || undefined })}
                className="hide-mobile"
              >
                記一筆
              </Button>
            </>
          )
        }
      />
      <div className="filters">
        <label className="search">
          <Search size={17} />
          <input
            ref={searchRef}
            type="search"
            placeholder="搜尋說明、分類或帳戶"
            aria-label="搜尋交易"
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
          {q ? (
            <button
              type="button"
              aria-label="清除搜尋"
              onClick={() => setQ("")}
            >
              <X size={15} />
            </button>
          ) : (
            <kbd className="hide-mobile">/</kbd>
          )}
        </label>
        <AccountPicker
          accounts={accounts.data ?? []}
          sources={sourceAccounts.data ?? []}
          value={accountId}
          onChange={(id) => navigate("activity", id ? { account: id } : {})}
        />
        <div className="chip-row" role="group" aria-label="類型">
          {(
            [
              ["all", "全部"],
              ["expense", "支出"],
              ["income", "收入"],
              ["transfer", "轉帳"],
            ] as const
          ).map(([v, l]) => (
            <Chip key={v} active={type === v} onClick={() => setType(v)}>
              {l}
            </Chip>
          ))}
          {(pending > 0 || type === "uncategorized") && (
            <Chip
              active={type === "uncategorized"}
              onClick={() =>
                setType(type === "uncategorized" ? "all" : "uncategorized")
              }
            >
              未分類 {pending}
            </Chip>
          )}
        </div>
      </div>
      {filtered && (
        <div className="filter-summary">
          <span>
            {account && <strong>{account.name}</strong>}
            {query && <> 搜尋「{query}」</>} 共 {rows.length}
            {entries.hasNextPage ? "+" : ""} 筆
          </span>
          {totals && (
            <span>
              收入{" "}
              <Money
                value={totals.income.toString()}
                currency={totals.currency}
              />{" "}
              · 支出{" "}
              <Money
                value={totals.expense.toString()}
                currency={totals.currency}
              />
            </span>
          )}
          <button
            type="button"
            className="link"
            onClick={() => {
              setQ("");
              setType("all");
              navigate("activity");
            }}
          >
            清除篩選
          </button>
        </div>
      )}
      {!filtered && pending > 0 && !viewing && (
        <button
          type="button"
          className="sort-hint"
          onClick={() => setType("uncategorized")}
        >
          <Sparkles size={17} />
          <span>
            同步交易大多已自動分類，還有 <strong>{pending}</strong>{" "}
            筆認不出來。點一筆選好分類並勾選「以後自動分類」，相同商家之後就不用再分。
          </span>
          <span className="link">整理未分類</span>
        </button>
      )}
      <section className="card flush">
        {entries.isPending ? (
          <div className="pad">
            <Skeleton rows={6} />
          </div>
        ) : entries.isError ? (
          <div className="pad">
            <ErrorLine retry={() => entries.refetch()}>交易讀取失敗</ErrorLine>
          </div>
        ) : !rows.length ? (
          filtered ? (
            <Empty title="找不到符合的紀錄" icon={Search}>
              換個關鍵字，或清除篩選看看。
            </Empty>
          ) : (
            <Empty
              title="還沒有任何收支"
              action={
                !viewing && (
                  <Button
                    variant="primary"
                    icon={Plus}
                    onClick={() => compose()}
                  >
                    記下第一筆
                  </Button>
                )
              }
            >
              從今天的一筆開始，慢慢就能看見自己的消費習慣。
            </Empty>
          )
        ) : (
          groups.map(([day, list]) => (
            <DayGroup key={day} day={day} list={list} onOpen={setSelected} />
          ))
        )}
        {entries.hasNextPage && (
          <div className="load-more">
            <Button
              busy={entries.isFetchingNextPage}
              onClick={() => entries.fetchNextPage()}
            >
              載入更早的紀錄
            </Button>
          </div>
        )}
      </section>
      {selected && (
        <EntryDetail entry={selected} onClose={() => setSelected(undefined)} />
      )}
      {rules && <CategoriesSheet onClose={() => setRules(false)} />}
      {importing && accounts.data && (
        <ImportSheet
          accounts={accounts.data}
          initial={accountId}
          onClose={() => setImporting(false)}
        />
      )}
    </>
  );
}

function DayGroup({
  day,
  list,
  onOpen,
}: {
  day: string;
  list: Entry[];
  onOpen: (e: Entry) => void;
}) {
  const sameCurrency = new Set(list.map((e) => e.currency)).size === 1;
  const net = list
    .filter((e) => e.kind !== "transfer")
    .reduce((a, e) => a.plus(e.amount), new Decimal(0));
  return (
    <div className="day">
      <div className="day-head">
        <span>{dayLabel(day)}</span>
        {sameCurrency && !net.isZero() && (
          <Money value={net.toString()} currency={list[0].currency} sign />
        )}
      </div>
      <ul className="rows">
        {list.map((e) => (
          <li key={e.id}>
            <EntryRow entry={e} onOpen={onOpen} />
          </li>
        ))}
      </ul>
    </div>
  );
}

export function EntryRow({
  entry: e,
  onOpen,
  showDate,
}: {
  entry: Entry;
  onOpen: (e: Entry) => void;
  showDate?: boolean;
}) {
  const sub = useSubscriptionMatches().get(e.id);
  return (
    <button type="button" className="row" onClick={() => onOpen(e)}>
      <CategoryIcon category={e.category} kind={e.kind} />
      <span className="row-main">
        <span className="row-title">
          <span className="row-text">{e.description}</span>
          {e.split_parent_id && <Badge>拆帳</Badge>}
          {e.kind === "refund" && <Badge tone="info">退款</Badge>}
          {e.status === "pending" && <Badge>未出帳</Badge>}
          {sub && <Badge tone="accent">{sub.name}</Badge>}
        </span>
        <span className="row-meta">
          {showDate && <>{dayLabel(e.occurred_on).split(" · ").pop()} · </>}
          {e.kind === "transfer" ? (
            "轉帳"
          ) : e.category === "未分類" ? (
            <span className="uncat">未分類</span>
          ) : (
            e.category
          )}
          {(e.category_source === "auto" || e.category_source === "rule") && (
            <Sparkles size={11} className="auto-mark" aria-label="自動分類" />
          )}{" "}
          · {e.account_name}
          {e.tags.length > 0 && <> · {e.tags.map((t) => `#${t}`).join(" ")}</>}
        </span>
      </span>
      <Money
        value={e.amount}
        currency={e.currency}
        sign
        tone={e.kind === "income" || e.kind === "refund"}
        className="row-amount"
      />
    </button>
  );
}

/** 銀行回傳的交易分類代碼 */
const sourceStatusText: Record<string, string> = {
  income: "收入",
  expense: "支出",
  posted: "已入帳",
  pending: "未入帳",
  transfer: "轉帳",
};

/* ---------- 明細抽屜：所有單筆操作集中在這裡 ---------- */
type Mode = "view" | "edit" | "label" | "split" | "refund" | "delete";
export function EntryDetail({
  entry,
  onClose,
}: {
  entry: Entry;
  onClose: () => void;
}) {
  const { navigate, viewing } = useApp();
  const sub = useSubscriptionMatches().get(entry.id);
  const [mode, setMode] = useState<Mode>("view");
  const [cat, setCat] = useState(entry.category);
  const { run, busy, error, setError } = useAction();
  const canSplit =
    ["income", "expense"].includes(entry.kind) &&
    !entry.split_parent_id &&
    !entry.investment_trade_id;
  const canRefund = entry.kind === "expense" && !entry.investment_trade_id;
  const canDelete = !entry.investment_trade_id;
  const go = (m: Mode) => {
    setError("");
    setMode(m);
  };
  const titles: Record<Mode, string> = {
    view: "交易明細",
    edit: "編輯分類與備註",
    label: "修改分類",
    split: "拆帳",
    refund: "記錄退款",
    delete: "刪除這筆紀錄？",
  };
  return (
    <Sheet
      title={titles[mode]}
      side="right"
      onClose={mode === "view" ? onClose : () => go("view")}
      busy={busy}
    >
      <div className="detail-hero">
        <CategoryIcon category={entry.category} kind={entry.kind} size={48} />
        <Money
          value={entry.amount}
          currency={entry.currency}
          sign
          tone={entry.kind === "income" || entry.kind === "refund"}
          className="detail-amount"
        />
        <strong>{entry.description}</strong>
      </div>
      {mode === "view" && (
        <>
          <dl className="facts">
            <div>
              <dt>日期</dt>
              <dd>{dayLabel(entry.occurred_on)}</dd>
            </div>
            <div>
              <dt>類型</dt>
              <dd>
                {entryKinds[entry.kind] ?? entry.kind}
                {entry.split_parent_id && "（拆帳後的一部分）"}
              </dd>
            </div>
            <div>
              <dt>帳戶</dt>
              <dd>
                <button
                  type="button"
                  className="link"
                  onClick={() => {
                    onClose();
                    navigate("activity", { account: entry.account_id });
                  }}
                >
                  {entry.account_name}
                </button>
              </dd>
            </div>
            {entry.kind !== "transfer" && (
              <div>
                <dt>分類</dt>
                <dd>
                  {entry.category}
                  {entry.category_source === "auto" && (
                    <small className="dim">（依關鍵字自動分類）</small>
                  )}
                  {entry.category_source === "rule" && (
                    <small className="dim">（依你的規則自動分類）</small>
                  )}
                </dd>
              </div>
            )}
            {sub && (
              <div>
                <dt>訂閱</dt>
                <dd>
                  <button
                    type="button"
                    className="link"
                    onClick={() => {
                      onClose();
                      navigate("subscriptions", { month: sub.month });
                    }}
                  >
                    {sub.name}
                  </button>
                  <small className="dim">
                    （{Number(sub.month.slice(5))} 月
                    {sub.role === "fee"
                      ? "的服務費"
                      : sub.role === "refund"
                        ? "的退款"
                        : "扣款"}
                    ）
                  </small>
                </dd>
              </div>
            )}
            {entry.tags.length > 0 && (
              <div>
                <dt>標籤</dt>
                <dd className="tags">
                  {entry.tags.map((t) => (
                    <Badge key={t}>#{t}</Badge>
                  ))}
                </dd>
              </div>
            )}
            {entry.note && (
              <div>
                <dt>備註</dt>
                <dd className="pre">{entry.note}</dd>
              </div>
            )}
          </dl>
          {entry.investment_trade_id && (
            <p className="callout">
              這筆由舊版投資紀錄自動產生，無法在這裡修改。
            </p>
          )}
          {entry.read_only && (
            <>
              <p className="callout">
                {viewing
                  ? `銀行同步紀錄 · 正在查看 ${viewing.name} 的帳本，只能看不能改`
                  : "銀行同步紀錄 · 金額與日期由來源更新，分類、標籤與備註可以自己改"}
              </p>
              <dl className="facts source-facts-list">
                {Object.entries({
                  card_mask: "卡片末四碼",
                  statement_period: "帳單期別",
                  original_currency: "原幣",
                  original_amount: "原幣金額",
                  fee: "手續費",
                  balance: "交易後餘額",
                  posted_date: "入帳日",
                  source_date: "來源時間",
                  source_status: "來源分類",
                }).map(([key, label]) => {
                  const raw = entry.source_details?.[key];
                  if (!raw) return null;
                  const d = entry.source_details ?? {};
                  // 金額類加上幣別與千分位；英文代碼翻成中文
                  const value =
                    (key === "balance" || key === "fee") &&
                    !Number.isNaN(Number(raw))
                      ? money(raw, entry.currency)
                      : key === "original_amount" && !Number.isNaN(Number(raw))
                        ? money(raw, d.original_currency ?? entry.currency)
                        : key === "source_status"
                          ? (sourceStatusText[raw] ?? raw)
                          : raw;
                  return (
                    <div key={key}>
                      <dt>{label}</dt>
                      <dd>{value}</dd>
                    </div>
                  );
                })}
              </dl>
              {!viewing && (
                <div className="action-list">
                  <button type="button" onClick={() => go("label")}>
                    <Pencil size={18} />
                    <span>
                      修改分類、標籤與備註
                      <small>可以順便建立規則，相同商家之後自動分類</small>
                    </span>
                  </button>
                  {entry.category_source === "user" && (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={async () => {
                        if (
                          await run(`/source-transactions/${entry.id}/label`, {
                            method: "DELETE",
                            success: "已改回自動分類",
                          })
                        )
                          onClose();
                      }}
                    >
                      <RotateCcw size={18} />
                      <span>
                        改回自動分類
                        <small>清除你對這一筆設定的分類、標籤與備註</small>
                      </span>
                    </button>
                  )}
                </div>
              )}
            </>
          )}
          {!entry.read_only && viewing && (
            <p className="callout">
              正在查看 {viewing.name} 的帳本，只能看不能改
            </p>
          )}
          {!entry.read_only && !viewing && (
            <div className="action-list">
              <button type="button" onClick={() => go("edit")}>
                <Pencil size={18} />
                <span>
                  編輯分類、標籤與備註
                  <small>金額與日期無法修改，需要時請刪除後重記</small>
                </span>
              </button>
              {canSplit && (
                <button type="button" onClick={() => go("split")}>
                  <Scissors size={18} />
                  <span>
                    拆帳
                    <small>
                      一筆消費拆成多個分類，例如大賣場的食物與日用品
                    </small>
                  </span>
                </button>
              )}
              {canRefund && (
                <button type="button" onClick={() => go("refund")}>
                  <Undo2 size={18} />
                  <span>
                    記錄退款
                    <small>退款會扣回這個分類的支出，不算收入</small>
                  </span>
                </button>
              )}
              {canDelete && (
                <button
                  type="button"
                  className="danger"
                  onClick={() => go("delete")}
                >
                  <Trash2 size={18} />
                  <span>刪除</span>
                </button>
              )}
            </div>
          )}
        </>
      )}
      {mode === "edit" && (
        <form
          className="form"
          onSubmit={async (e) => {
            e.preventDefault();
            const v = Object.fromEntries(
              new FormData(e.currentTarget),
            ) as Record<string, string>;
            if (
              await run(`/transactions/${entry.id}`, {
                method: "PATCH",
                body: {
                  category: v.category.trim(),
                  note: v.note,
                  tags: v.tags
                    .split(/[,，]/)
                    .map((t) => t.trim())
                    .filter(Boolean),
                },
                success: "已更新",
              })
            )
              onClose();
          }}
        >
          <FormError>{error}</FormError>
          <Field label="分類">
            <CategoryPicker value={cat} onChange={setCat} kind={entry.kind} />
          </Field>
          <input type="hidden" name="category" value={cat} />
          <Field label="標籤" hint="用逗號分隔">
            <input name="tags" defaultValue={entry.tags.join(", ")} />
          </Field>
          <Field label="備註">
            <textarea
              name="note"
              rows={3}
              maxLength={2000}
              defaultValue={entry.note ?? ""}
            />
          </Field>
          <FormActions busy={busy} onCancel={() => go("view")} />
        </form>
      )}
      {mode === "label" && (
        <SourceEntryForm
          entry={entry}
          onCancel={() => go("view")}
          onDone={onClose}
        />
      )}
      {mode === "split" && (
        <SplitForm entry={entry} onCancel={() => go("view")} onDone={onClose} />
      )}
      {mode === "refund" && (
        <RefundForm
          entry={entry}
          onCancel={() => go("view")}
          onDone={onClose}
        />
      )}
      {mode === "delete" && (
        <div className="form">
          <FormError>{error}</FormError>
          <p>
            {entry.kind === "transfer"
              ? "轉帳的另一端也會一起刪除。"
              : entry.split_parent_id
                ? "這是拆帳的一部分，整組拆帳會一起移除，並恢復為原本那一筆。"
                : "帳戶餘額會一併更新。"}
            刪除後無法復原。
          </p>
          <div className="form-actions">
            <Button onClick={() => go("view")} disabled={busy}>
              取消
            </Button>
            <Button
              variant="danger"
              busy={busy}
              onClick={async () => {
                if (
                  await run(`/transactions/${entry.id}`, {
                    method: "DELETE",
                    success: "已刪除",
                  })
                )
                  onClose();
              }}
            >
              確認刪除
            </Button>
          </div>
        </div>
      )}
    </Sheet>
  );
}

function FormActions({
  busy,
  onCancel,
  label = "儲存",
  disabled,
}: {
  busy: boolean;
  onCancel: () => void;
  label?: string;
  disabled?: boolean;
}) {
  return (
    <div className="form-actions">
      <Button onClick={onCancel} disabled={busy}>
        取消
      </Button>
      <Button type="submit" variant="primary" busy={busy} disabled={disabled}>
        {label}
      </Button>
    </div>
  );
}
export function CategoryList() {
  const all = useCategories();
  return (
    <datalist id="category-list">
      {all.map(({ name: c }) => (
        <option key={c} value={c} />
      ))}
    </datalist>
  );
}

function SplitForm({
  entry,
  onCancel,
  onDone,
}: {
  entry: Entry;
  onCancel: () => void;
  onDone: () => void;
}) {
  const { run, busy, error, setError } = useAction();
  const total = new Decimal(entry.amount).abs();
  const [parts, setParts] = useState([
    { amount: "", category: entry.category, description: entry.description },
    { amount: "", category: "未分類", description: entry.description },
  ]);
  const sum = parts.reduce(
    (a, p) => a.plus(evaluate(p.amount) ?? 0),
    new Decimal(0),
  );
  const remaining = total.minus(sum);
  const set = (i: number, k: keyof (typeof parts)[0], v: string) =>
    setParts(parts.map((p, j) => (j === i ? { ...p, [k]: v } : p)));
  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!remaining.isZero()) {
      setError(
        remaining.isPositive()
          ? `還有 ${money(remaining.toString(), entry.currency)} 沒有分配`
          : `超出原金額 ${money(remaining.abs().toString(), entry.currency)}`,
      );
      return;
    }
    const neg = new Decimal(entry.amount).isNegative();
    if (
      await run(`/transactions/${entry.id}/split`, {
        body: {
          parts: parts.map((p) => {
            const n = new Decimal(evaluate(p.amount) ?? 0).abs();
            return { ...p, amount: (neg ? n.negated() : n).toString() };
          }),
        },
        success: `已拆成 ${parts.length} 筆`,
      })
    )
      onDone();
  }
  return (
    <form className="form" onSubmit={submit}>
      <FormError>{error}</FormError>
      <CategoryList />
      {parts.map((p, i) => (
        <div className="split-part" key={i}>
          <div className="split-head">
            <strong>第 {i + 1} 筆</strong>
            {parts.length > 2 && (
              <button
                type="button"
                className="icon-btn sm"
                aria-label={`移除第 ${i + 1} 筆`}
                onClick={() => setParts(parts.filter((_, j) => j !== i))}
              >
                <X size={15} />
              </button>
            )}
          </div>
          <div className="grid-2">
            <Field label="金額">
              <div className="affix">
                <span>{symbol(entry.currency)}</span>
                <input
                  inputMode="decimal"
                  autoComplete="off"
                  value={p.amount}
                  required
                  data-autofocus={i === 0 ? true : undefined}
                  onChange={(e) => set(i, "amount", e.target.value)}
                />
                {!p.amount && remaining.isPositive() && (
                  <button
                    type="button"
                    className="affix-action"
                    onClick={() => set(i, "amount", remaining.toString())}
                  >
                    填入剩餘
                  </button>
                )}
              </div>
            </Field>
            <Field label="分類">
              <input
                list="category-list"
                value={p.category}
                required
                onChange={(e) => set(i, "category", e.target.value)}
              />
            </Field>
          </div>
          <Field label="說明">
            <input
              value={p.description}
              required
              onChange={(e) => set(i, "description", e.target.value)}
            />
          </Field>
        </div>
      ))}
      <Button
        icon={Plus}
        variant="ghost"
        disabled={parts.length >= 30}
        onClick={() =>
          setParts([
            ...parts,
            { amount: "", category: "未分類", description: entry.description },
          ])
        }
      >
        再加一筆
      </Button>
      <div className={`split-status ${remaining.isZero() ? "ok" : ""}`}>
        <span>原金額 {money(total.toString(), entry.currency)}</span>
        <strong>
          {remaining.isZero()
            ? "已全部分配 ✓"
            : remaining.isPositive()
              ? `尚餘 ${money(remaining.toString(), entry.currency)}`
              : `超出 ${money(remaining.abs().toString(), entry.currency)}`}
        </strong>
      </div>
      <FormActions
        busy={busy}
        onCancel={onCancel}
        label="確認拆帳"
        disabled={!remaining.isZero()}
      />
    </form>
  );
}

function RefundForm({
  entry,
  onCancel,
  onDone,
}: {
  entry: Entry;
  onCancel: () => void;
  onDone: () => void;
}) {
  const { run, busy, error, setError } = useAction();
  const [requestId] = useState(() => crypto.randomUUID());
  const max = new Decimal(entry.amount).abs();
  const [amount, setAmount] = useState("");
  const [date, setDate] = useState(today());
  return (
    <form
      className="form"
      onSubmit={async (e) => {
        e.preventDefault();
        const v = evaluate(amount);
        if (!v || new Decimal(v).lte(0) || new Decimal(v).gt(max)) {
          setError(
            `退款金額需介於 0 與 ${money(max.toString(), entry.currency)} 之間`,
          );
          return;
        }
        if (
          await run(`/transactions/${entry.id}/refund`, {
            body: { request_id: requestId, amount: v, occurred_on: date },
            success: "已記錄退款",
          })
        )
          onDone();
      }}
    >
      <FormError>{error}</FormError>
      <Field
        label="退款金額"
        hint={`最多 ${money(max.toString(), entry.currency)}（含先前已退）`}
      >
        <div className="affix">
          <span>{symbol(entry.currency)}</span>
          <input
            name="amount"
            inputMode="decimal"
            autoComplete="off"
            data-autofocus
            required
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
          />
          <button
            type="button"
            className="affix-action"
            onClick={() => setAmount(max.toString())}
          >
            全額
          </button>
        </div>
      </Field>
      <Field label="退款日期">
        <input
          name="occurred_on"
          type="date"
          value={date}
          min={entry.occurred_on}
          max={today()}
          required
          onChange={(e) => setDate(e.target.value)}
        />
      </Field>
      <FormActions busy={busy} onCancel={onCancel} label="記錄退款" />
    </form>
  );
}

function ImportSheet({
  accounts,
  initial,
  onClose,
}: {
  accounts: Account[];
  initial: string;
  onClose: () => void;
}) {
  const { user, toast } = useApp();
  const { run } = useAction();
  const [account, setAccount] = useState(initial || accounts[0].id);
  const [csv, setCsv] = useState("");
  const [file, setFile] = useState("");
  const [drag, setDrag] = useState(false);
  const [preview, setPreview] = useState<{
    rows: number;
    deduplication: string;
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function load(f?: File) {
    if (!f) return;
    setError("");
    setPreview(null);
    if (f.size > 1_500_000) {
      setError("檔案需小於 1.5 MB");
      return;
    }
    setFile(f.name);
    const text = await f.text();
    setCsv(text);
    await check(text, account);
  }
  async function check(text: string, acc: string) {
    setBusy(true);
    try {
      setPreview(
        await api("/import/transactions", {
          method: "POST",
          csrf: user.csrf_token,
          body: { account_id: acc, csv: text, commit: false },
        }),
      );
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  async function commit() {
    setBusy(true);
    const ok = await run("/import/transactions", {
      body: { account_id: account, csv, commit: true },
      success: `已匯入 ${preview?.rows ?? ""} 筆`,
      toastError: true,
    });
    setBusy(false);
    if (ok) onClose();
  }
  return (
    <Sheet
      title="匯入 CSV"
      description="先檢查、確認筆數後才會真正寫入。重複的紀錄會自動略過。"
      onClose={onClose}
      busy={busy}
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button
            variant="primary"
            busy={busy}
            disabled={!preview}
            onClick={commit}
            className="grow"
          >
            {preview ? `確認匯入 ${preview.rows} 筆` : "選擇檔案後即可匯入"}
          </Button>
        </>
      }
    >
      <div className="form">
        <Field label="匯入到哪個帳戶">
          <select
            value={account}
            onChange={(e) => {
              setAccount(e.target.value);
              if (csv) void check(csv, e.target.value);
            }}
          >
            {accounts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}（{a.currency}）
              </option>
            ))}
          </select>
        </Field>
        <label
          className={`dropzone ${drag ? "drag" : ""}`}
          onDragOver={(e) => {
            e.preventDefault();
            setDrag(true);
          }}
          onDragLeave={() => setDrag(false)}
          onDrop={(e) => {
            e.preventDefault();
            setDrag(false);
            void load(e.dataTransfer.files[0]);
          }}
        >
          <FileUp size={26} />
          <strong>{file || "拖曳 CSV 到這裡，或點擊選擇"}</strong>
          <span>{file ? "點擊可更換檔案" : "最大 1.5 MB"}</span>
          <input
            type="file"
            accept=".csv,text/csv"
            className="sr-only"
            onChange={(e) => void load(e.target.files?.[0])}
          />
        </label>
        <FormError>{error}</FormError>
        {preview && (
          <div className="callout good">
            檢查通過：共 <strong>{preview.rows}</strong> 筆。
            {preview.deduplication}
          </div>
        )}
        <details className="disclosure">
          <summary>CSV 格式說明</summary>
          <p>
            必要欄位：<code>date, amount, kind, description</code>
            。支出金額為負數、kind 填<code>expense</code>；收入 kind 填{" "}
            <code>income</code>。可選 <code>category</code>、<code>id</code>
            （用來避免重複匯入）。
          </p>
          <pre className="code">
            {
              "date,amount,kind,description,category,id\n2026-09-23,-120,expense,午餐,餐飲,tx-001"
            }
          </pre>
        </details>
      </div>
    </Sheet>
  );
}

export function RecentList({
  entries,
  onOpen,
}: {
  entries: Entry[];
  onOpen: (e: Entry) => void;
}) {
  return (
    <ul className="rows">
      {entries.map((e) => (
        <li key={e.id}>
          <EntryRow entry={e} onOpen={onOpen} showDate />
        </li>
      ))}
    </ul>
  );
}
