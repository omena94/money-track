"use client";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  AlertTriangle,
  ArrowDownRight,
  ArrowRight,
  ArrowUpRight,
  ChevronDown,
  CircleCheck,
  Globe,
  Lightbulb,
  PiggyBank,
  Receipt,
  Tags,
  Target,
  Wallet,
  type LucideIcon,
} from "lucide-react";
import {
  Bar,
  CartesianGrid,
  ComposedChart,
  Line,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { api, type Analysis } from "@/lib/api";
import { money, percent, shiftMonth, thisMonth } from "@/lib/format";
import { useApp } from "@/components/app-context";
import { CategoriesSheet } from "./category-manager";
import {
  Card,
  CategoryIcon,
  Empty,
  ErrorLine,
  Money,
  MonthSwitch,
  PageHead,
  Skeleton,
  categoryColor,
  Writable,
} from "@/components/ui";

type Entry = Analysis["entries"][number];
type Cat = {
  category: string;
  amount: number;
  count: number;
  avg: number;
  budget?: number;
};
type Merchant = {
  name: string;
  amount: number;
  count: number;
  category: string;
};
type Insight = {
  tone: "good" | "warn" | "bad" | "info";
  icon: LucideIcon;
  text: ReactNode;
  action?: { label: string; run: () => void };
};

const num = (v: string | number | null | undefined) => Number(v ?? 0) || 0;
/** 句中金額：幣別與數字間用不斷行空白，避免「NT$」與數字被拆到兩行 */
const cash = (v: number) =>
  money(String(Math.round(v))).replace(" ", "\u00a0");
const monthName = (m: string) => `${Number(m.slice(5))}月`;
const wan = (v: number) =>
  Math.abs(v) >= 1e4 ? `${+(v / 1e4).toFixed(1)}萬` : v.toLocaleString("zh-TW");
const isSpend = (e: Entry) => e.kind === "expense" || e.kind === "refund";

/** 刷卡描述轉成商家名稱：去掉支付前綴、末碼、交易序號與括號備註 */
export function merchantName(raw: string) {
  let s = raw.normalize("NFKC").replace(/\s+/g, " ").trim();
  s = s.replace(/[（(].*$/, "").trim();
  for (let i = 0; i < 4; i++) {
    const next = s
      .replace(/^(連加\*|連支\*|優步[-－]|電支交易 ?)/, "")
      .replace(/[ _]+[A-Za-z*#-]*\d[A-Za-z0-9*#-]*$/, "")
      .trim();
    if (next === s || !next) break;
    s = next;
  }
  return s || raw.trim();
}
/** 發票賣方去掉公司型態，讓名稱短一點 */
const sellerName = (s: string) =>
  s
    .replace(/(股份)?有限公司/g, " ")
    .replace(/\s+/g, " ")
    .trim();

export function AnalysisPage() {
  const { route, navigate } = useApp();
  const current = thisMonth();
  const month = /^\d{4}-\d{2}$/.test(route.params.get("month") ?? "")
    ? route.params.get("month")!
    : current;
  const setMonth = (m: string) =>
    navigate("analysis", m === current ? {} : { month: m });
  const q = useQuery({
    queryKey: ["analysis", month],
    queryFn: () => api<Analysis>(`/analysis?month=${month}`),
  });

  return (
    <>
      <PageHead
        title="分析"
        subtitle="錢花去哪裡、和平常比起來如何，每個月一次看完"
        actions={
          <MonthSwitch month={month} onChange={setMonth} max={current} />
        }
      />
      {q.isError && <ErrorLine>分析資料讀取失敗</ErrorLine>}
      {q.isPending && <Skeleton rows={4} h={120} />}
      {q.data && <Report data={q.data} onMonth={setMonth} />}
    </>
  );
}

function Report({
  data,
  onMonth,
}: {
  data: Analysis;
  onMonth: (m: string) => void;
}) {
  const { navigate, hidden } = useApp();
  const m = useMemo(() => derive(data), [data]);
  const [managing, setManaging] = useState(false);
  const insights = useMemo(
    () => review(data, m, navigate),
    [data, m, navigate],
  );

  if (!m.spend.length && !m.cur.income)
    return (
      <Empty icon={PiggyBank} title="這個月還沒有收支">
        同步銀行或記帳後，這裡會分析錢花去哪裡。
        {m.series.some((s) => s.expense > 0) && (
          <> 可以用右上角切換到有資料的月份。</>
        )}
      </Empty>
    );

  return (
    <div className="analysis">
      <Summary m={m} month={data.month} />

      <Card
        title={
          <>
            <Lightbulb size={17} /> {monthName(data.month)}總檢討
          </>
        }
        className="review"
      >
        <ul className="review-list">
          {insights.map((it, i) => (
            <li key={i} className={it.tone}>
              <span className="review-icon">
                <it.icon size={16} />
              </span>
              <span className="review-text">{it.text}</span>
              {it.action && (
                <button type="button" className="link" onClick={it.action.run}>
                  {it.action.label} <ArrowRight size={13} />
                </button>
              )}
            </li>
          ))}
        </ul>
      </Card>

      <div className="analysis-grid">
        <Card
          title="錢花去哪裡"
          pad={false}
          className="where"
          action={
            <Writable>
              <button
                type="button"
                className="link small"
                onClick={() => setManaging(true)}
              >
                管理分類
              </button>
            </Writable>
          }
        >
          <Categories m={m} />
        </Card>
        <div className="analysis-stack">
          <Card title="每日支出">
            <Daily m={m} hidden={hidden} />
          </Card>
          <Card title="常花錢的地方" pad={false}>
            <MerchantList items={m.merchants.slice(0, 8)} />
          </Card>
        </div>
      </div>

      <Card title="單筆最高" pad={false}>
        <ul className="big-list">
          {m.biggest.map((e) => (
            <li key={e.id}>
              <CategoryIcon category={e.category} size={34} />
              <span className="big-main">
                <strong>{e.description || e.category}</strong>
                <small>
                  {Number(e.date.slice(8))} 日 · {e.category} · {e.account_name}
                </small>
              </span>
              {-num(e.amount) >= data.large_threshold && (
                <span className="tag-large">大額</span>
              )}
              <Money value={String(-num(e.amount))} className="big-amount" />
            </li>
          ))}
        </ul>
      </Card>

      <Card
        title="近 12 個月"
        action={<span className="dim small">點長條切換月份</span>}
      >
        <Trend m={m} month={data.month} hidden={hidden} onMonth={onMonth} />
      </Card>

      <Card title="各分類每月支出" pad={false}>
        <Heatmap data={data} m={m} onMonth={onMonth} />
      </Card>

      {data.invoices.count > 0 && (
        <Card
          title={
            <>
              <Receipt size={17} /> 發票明細
            </>
          }
          action={
            <span className="dim small">
              {data.invoices.count} 張 · <Money value={data.invoices.amount} />
            </span>
          }
        >
          <p className="dim small invoice-note">
            電子發票和刷卡是同一筆錢的不同紀錄，這裡只看買了什麼，不另外加進支出。
          </p>
          <div className="invoice-cols">
            <div>
              <h3>商店</h3>
              <ol className="rank">
                {data.invoices.sellers.map((s) => (
                  <li key={s.name}>
                    <span title={s.name}>{sellerName(s.name)}</span>
                    <small>{s.count} 張</small>
                    <Money value={s.amount} />
                  </li>
                ))}
              </ol>
            </div>
            <div>
              <h3>品項</h3>
              <ol className="rank">
                {data.invoices.items.map((s) => (
                  <li key={s.name}>
                    <span title={s.name}>{s.name}</span>
                    <small>×{Number(s.quantity).toLocaleString("zh-TW")}</small>
                    <Money value={s.amount} />
                  </li>
                ))}
              </ol>
            </div>
          </div>
        </Card>
      )}

      {managing && <CategoriesSheet onClose={() => setManaging(false)} />}
      <p className="dim small analysis-foot">
        只計新臺幣；轉帳、信用卡繳款、買賣投資屬於資金移動，不算收入或支出；待入帳的刷卡要等入帳後才計入。單筆滿{" "}
        {cash(data.large_threshold)} 視為大額支出。
      </p>
    </div>
  );
}

type Derived = ReturnType<typeof derive>;
function derive(data: Analysis) {
  const byMonth = new Map(data.months.map((x) => [x.month, x]));
  const series = Array.from({ length: 12 }, (_, i) => {
    const key = shiftMonth(data.month, i - 11);
    const r = byMonth.get(key);
    return {
      month: key,
      income: num(r?.income),
      expense: num(r?.expense),
      large: num(r?.large),
      count: r?.count ?? 0,
    };
  });
  const cur = series[11];
  const prev = series[10];
  // 比較基準：前 6 個月中有支出的月份
  const past = series.slice(5, 11).filter((s) => s.count > 0);
  const avg = past.length
    ? past.reduce((a, s) => a + s.expense, 0) / past.length
    : 0;
  const avgDaily = past.length
    ? past.reduce((a, s) => a + s.expense - s.large, 0) / past.length
    : 0;

  const [y, mo] = data.month.split("-").map(Number);
  const days = new Date(y, mo, 0).getDate();
  const isCurrent = data.today.slice(0, 7) === data.month;
  const elapsed = isCurrent ? Number(data.today.slice(8, 10)) : days;
  const regular = cur.expense - cur.large;
  // 月底預估只外推日常支出，大額照實計
  const projected = isCurrent
    ? (regular / elapsed) * days + cur.large
    : cur.expense;

  const spend = data.entries.filter(isSpend);
  const catMap = new Map<string, Cat>();
  const pastMonths = new Set(past.map((p) => p.month));
  for (const r of data.matrix) {
    if (!pastMonths.has(r.month)) continue;
    const c = catMap.get(r.category) ?? {
      category: r.category,
      amount: 0,
      count: 0,
      avg: 0,
    };
    c.avg += num(r.amount) / past.length;
    catMap.set(r.category, c);
  }
  for (const e of spend) {
    const c = catMap.get(e.category) ?? {
      category: e.category,
      amount: 0,
      count: 0,
      avg: 0,
    };
    c.amount -= num(e.amount);
    c.count += 1;
    catMap.set(e.category, c);
  }
  for (const b of data.budgets) {
    const c = catMap.get(b.category);
    if (c) c.budget = num(b.amount);
  }
  const cats = [...catMap.values()]
    .filter((c) => c.amount > 0)
    .sort((a, b) => b.amount - a.amount);
  const quiet = [...catMap.values()].filter((c) => c.amount <= 0 && c.avg > 0);

  const merchMap = new Map<string, Merchant>();
  for (const e of spend) {
    const name = merchantName(e.description || e.category);
    const x = merchMap.get(name) ?? {
      name,
      amount: 0,
      count: 0,
      category: e.category,
    };
    x.amount -= num(e.amount);
    x.count += 1;
    merchMap.set(name, x);
  }
  const merchants = [...merchMap.values()]
    .filter((x) => x.amount > 0)
    .sort((a, b) => b.amount - a.amount);

  const daily = Array.from({ length: days }, (_, i) => ({
    day: i + 1,
    amount: 0,
    future: isCurrent && i + 1 > elapsed,
  }));
  for (const e of spend)
    daily[Number(e.date.slice(8, 10)) - 1].amount -= num(e.amount);

  const biggest = spend
    .filter((e) => num(e.amount) < 0)
    .sort((a, b) => num(a.amount) - num(b.amount))
    .slice(0, 8);

  return {
    series,
    cur,
    prev,
    past,
    avg,
    avgDaily,
    days,
    elapsed,
    isCurrent,
    regular,
    projected,
    spend,
    cats,
    quiet,
    merchants,
    daily,
    biggest,
  };
}

function Delta({
  now,
  base,
  label,
}: {
  now: number;
  base: number;
  label: string;
}) {
  if (!base) return null;
  const pct = ((now - base) / base) * 100;
  const up = pct > 0;
  const Icon = up ? ArrowUpRight : ArrowDownRight;
  return (
    <span
      className={`delta ${Math.abs(pct) < 5 ? "flat" : up ? "up" : "down"}`}
    >
      <Icon size={13} />
      {label} {percent(Math.abs(Math.round(pct)))}
    </span>
  );
}

function Summary({ m, month }: { m: Derived; month: string }) {
  const net = m.cur.income - m.cur.expense;
  const rate = m.cur.income > 0 ? (net / m.cur.income) * 100 : null;
  return (
    <section className="card analysis-hero">
      <div className="hero-main">
        <span className="dim">{monthName(month)}支出</span>
        <Money value={String(m.cur.expense)} className="xl" />
        <div className="delta-row">
          <Delta now={m.cur.expense} base={m.prev.expense} label="比上月" />
          <Delta
            now={m.isCurrent ? m.projected : m.cur.expense}
            base={m.avg}
            label={m.isCurrent ? "預估比平均" : "比平均"}
          />
        </div>
        {m.isCurrent && (
          <div className="pace">
            <div className="pace-bar">
              <i style={{ width: `${(m.elapsed / m.days) * 100}%` }} />
            </div>
            <span className="dim small">
              已過 {m.elapsed}/{m.days} 天 · 日常支出日均{" "}
              <Money value={String(Math.round(m.regular / m.elapsed))} /> ·
              照這速度月底約 <Money value={String(Math.round(m.projected))} />
            </span>
          </div>
        )}
      </div>
      <dl className="hero-stats">
        <div>
          <dt>收入</dt>
          <dd>
            <Money value={String(m.cur.income)} />
          </dd>
        </div>
        <div>
          <dt>結餘</dt>
          <dd>
            <Money value={String(net)} sign tone />
          </dd>
        </div>
        <div>
          <dt>儲蓄率</dt>
          <dd
            className={
              rate == null ? "" : rate < 0 ? "bad" : rate >= 20 ? "pos" : ""
            }
          >
            {rate == null ? "—" : percent(Math.round(rate))}
          </dd>
        </div>
        <div>
          <dt>近 6 月平均支出</dt>
          <dd>{m.avg ? <Money value={String(Math.round(m.avg))} /> : "—"}</dd>
        </div>
      </dl>
    </section>
  );
}

function Categories({ m }: { m: Derived }) {
  const { navigate } = useApp();
  const [open, setOpen] = useState<string>();
  const total = m.cats.reduce((a, c) => a + c.amount, 0);
  const max = m.cats[0]?.amount ?? 1;
  if (!m.cats.length)
    return <Empty compact icon={PiggyBank} title="這個月沒有支出" />;
  return (
    <>
      <div className="share-bar" aria-hidden="true">
        {m.cats.map((c) => (
          <i
            key={c.category}
            style={{
              flexGrow: c.amount,
              background: categoryColor(c.category),
            }}
            title={c.category}
          />
        ))}
      </div>
      <ul className="cat-list">
        {m.cats.map((c) => {
          const pct = total ? (c.amount / total) * 100 : 0;
          const diff = c.amount - c.avg;
          const expanded = open === c.category;
          const over = c.budget != null && c.amount > c.budget;
          return (
            <li key={c.category} className={expanded ? "open" : ""}>
              <button
                type="button"
                className="cat-row"
                aria-expanded={expanded}
                onClick={() => setOpen(expanded ? undefined : c.category)}
              >
                <CategoryIcon category={c.category} size={34} />
                <span className="cat-main">
                  <span className="cat-line">
                    <strong>{c.category}</strong>
                    <span className="dim small">
                      {percent(Math.round(pct))} · {c.count} 筆
                    </span>
                  </span>
                  <span className="cat-meter">
                    <i
                      style={{
                        width: `${(c.amount / max) * 100}%`,
                        background: categoryColor(c.category),
                      }}
                    />
                  </span>
                  {c.budget != null ? (
                    <span className={`small ${over ? "bad" : "dim"}`}>
                      預算 {cash(c.budget)}
                      {over
                        ? ` · 超出 ${cash(c.amount - c.budget)}`
                        : ` · 剩 ${cash(c.budget - c.amount)}`}
                    </span>
                  ) : c.avg > 0 ? (
                    <span
                      className={`small ${Math.abs(diff) < Math.max(500, c.avg * 0.15) ? "dim" : diff > 0 ? "bad" : "pos"}`}
                    >
                      平均 {cash(c.avg)}
                      {Math.abs(diff) >= Math.max(500, c.avg * 0.15) &&
                        ` · ${diff > 0 ? "多" : "少"} ${cash(Math.abs(diff))}`}
                    </span>
                  ) : (
                    <span className="small dim">前 6 個月沒有這類支出</span>
                  )}
                </span>
                <Money value={String(c.amount)} className="cat-amount" />
                <ChevronDown size={16} className="cat-chevron" />
              </button>
              {expanded && (
                <div className="cat-detail">
                  <MerchantList
                    items={m.merchants
                      .filter((x) => x.category === c.category)
                      .slice(0, 6)}
                    compact
                  />
                  <button
                    type="button"
                    className="link"
                    onClick={() => navigate("activity", { q: c.category })}
                  >
                    看{c.category}的全部明細 <ArrowRight size={13} />
                  </button>
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </>
  );
}

function MerchantList({
  items,
  compact = false,
}: {
  items: Merchant[];
  compact?: boolean;
}) {
  const { navigate } = useApp();
  if (!items.length) return <p className="dim small empty-line">沒有資料</p>;
  const max = items[0].amount;
  return (
    <ol className={`merchant-list ${compact ? "compact" : ""}`}>
      {items.map((x, i) => (
        <li key={x.name}>
          <button
            type="button"
            onClick={() => navigate("activity", { q: x.name })}
            title={`搜尋「${x.name}」`}
          >
            {!compact && <span className="rank-no">{i + 1}</span>}
            <span className="merchant-main">
              <span className="merchant-name">{x.name}</span>
              <span className="merchant-meter">
                <i
                  style={{
                    width: `${(x.amount / max) * 100}%`,
                    background: categoryColor(x.category),
                  }}
                />
              </span>
            </span>
            <small>{x.count} 次</small>
            <Money value={String(x.amount)} />
          </button>
        </li>
      ))}
    </ol>
  );
}

const tooltipStyle = {
  borderRadius: 10,
  border: "1px solid var(--line)",
  background: "var(--surface)",
  fontSize: 13,
};

function Daily({ m, hidden }: { m: Derived; hidden: boolean }) {
  const avg = m.regular / m.elapsed;
  return (
    <div className="daily-chart">
      <ResponsiveContainer width="100%" height={170}>
        <ComposedChart
          data={m.daily}
          margin={{ top: 6, right: 0, left: 0, bottom: 0 }}
        >
          <CartesianGrid vertical={false} stroke="var(--line)" />
          <XAxis
            dataKey="day"
            tickLine={false}
            axisLine={false}
            interval={4}
            tick={{ fontSize: 11, fill: "var(--ink-3)" }}
          />
          <YAxis
            hide={hidden}
            width={40}
            tickLine={false}
            axisLine={false}
            tickFormatter={wan}
            tick={{ fontSize: 11, fill: "var(--ink-3)" }}
          />
          <Tooltip
            cursor={{ fill: "var(--surface-2)" }}
            labelFormatter={(d) => `${d} 日`}
            formatter={(v) => [hidden ? "••••" : money(String(v)), "支出"]}
            contentStyle={tooltipStyle}
          />
          <ReferenceLine
            y={avg}
            stroke="var(--ink-3)"
            strokeDasharray="4 4"
            ifOverflow="extendDomain"
          />
          <Bar dataKey="amount" fill="var(--c2)" radius={[3, 3, 0, 0]} />
        </ComposedChart>
      </ResponsiveContainer>
      <p className="dim small">虛線是日常支出日均（不含大額）</p>
    </div>
  );
}

function Trend({
  m,
  month,
  hidden,
  onMonth,
}: {
  m: Derived;
  month: string;
  hidden: boolean;
  onMonth: (m: string) => void;
}) {
  const data = m.series.map((s) => ({
    key: s.month,
    label: monthName(s.month),
    日常支出: s.expense - s.large,
    大額支出: s.large,
    收入: s.income,
  }));
  return (
    <div className="month-trend">
      <ResponsiveContainer width="100%" height={240}>
        <ComposedChart
          data={data}
          margin={{ top: 8, right: 4, left: 0, bottom: 0 }}
          onClick={(e) => {
            const i = Number(e?.activeTooltipIndex);
            if (Number.isInteger(i) && data[i]) onMonth(data[i].key);
          }}
        >
          <CartesianGrid vertical={false} stroke="var(--line)" />
          <XAxis
            dataKey="label"
            tickLine={false}
            axisLine={false}
            tick={({ x, y, payload, index }) => (
              <text
                x={x}
                y={Number(y) + 12}
                textAnchor="middle"
                fontSize={12}
                fontWeight={data[index]?.key === month ? 700 : 400}
                fill={
                  data[index]?.key === month ? "var(--ink)" : "var(--ink-3)"
                }
              >
                {payload.value}
              </text>
            )}
          />
          <YAxis
            hide={hidden}
            width={48}
            tickLine={false}
            axisLine={false}
            tickFormatter={wan}
            tick={{ fontSize: 12, fill: "var(--ink-3)" }}
          />
          <Tooltip
            cursor={{ fill: "var(--surface-2)" }}
            formatter={(v, name) => [hidden ? "••••" : money(String(v)), name]}
            contentStyle={tooltipStyle}
          />
          <Bar dataKey="日常支出" stackId="s" fill="var(--bad)" />
          <Bar
            dataKey="大額支出"
            stackId="s"
            fill="color-mix(in srgb, var(--bad) 45%, transparent)"
            radius={[4, 4, 0, 0]}
          />
          <Line
            dataKey="收入"
            type="monotone"
            stroke="var(--pos)"
            strokeWidth={2}
            dot={{ r: 3 }}
          />
        </ComposedChart>
      </ResponsiveContainer>
      <div className="legend-row small dim">
        <span>
          <i style={{ background: "var(--bad)" }} />
          日常支出
        </span>
        <span>
          <i
            style={{
              background: "color-mix(in srgb, var(--bad) 45%, transparent)",
            }}
          />
          大額支出
        </span>
        <span>
          <i style={{ background: "var(--pos)" }} />
          收入
        </span>
      </div>
    </div>
  );
}

function Heatmap({
  data,
  m,
  onMonth,
}: {
  data: Analysis;
  m: Derived;
  onMonth: (m: string) => void;
}) {
  const { hidden } = useApp();
  const months = m.series.map((s) => s.month);
  const cell = new Map(
    data.matrix.map((r) => [`${r.month}|${r.category}`, num(r.amount)]),
  );
  const totals = new Map<string, number>();
  for (const r of data.matrix)
    totals.set(r.category, (totals.get(r.category) ?? 0) + num(r.amount));
  const rows = [...totals.entries()]
    .filter(([, v]) => v > 0)
    .sort((a, b) => b[1] - a[1])
    .map(([c]) => c);
  const wrap = useRef<HTMLDivElement>(null);
  // 窄螢幕需橫向捲動：把選取的月份捲進可視範圍，而不是停在最舊的月份
  useEffect(() => {
    const el = wrap.current;
    const on = el?.querySelector<HTMLElement>("thead th.on");
    if (!el || !on || el.scrollWidth <= el.clientWidth) return;
    el.scrollLeft = on.offsetLeft + on.offsetWidth - el.clientWidth + 72;
  }, [data.month, rows.length]);
  if (!rows.length) return <p className="dim small empty-line">沒有資料</p>;
  return (
    <div className="heat-wrap" ref={wrap}>
      <table className="heat">
        <thead>
          <tr>
            <th scope="col">分類</th>
            {months.map((mo) => (
              <th
                key={mo}
                scope="col"
                className={mo === data.month ? "on" : ""}
              >
                <button type="button" onClick={() => onMonth(mo)}>
                  {monthName(mo)}
                </button>
              </th>
            ))}
            <th scope="col">月均</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((c) => {
            const values = months.map((mo) => cell.get(`${mo}|${c}`) ?? 0);
            const max = Math.max(...values, 1);
            const active = values.filter((v) => v > 0).length || 1;
            return (
              <tr key={c}>
                <th scope="row">
                  <i style={{ background: categoryColor(c) }} />
                  {c}
                </th>
                {values.map((v, i) => (
                  <td
                    key={months[i]}
                    className={months[i] === data.month ? "on" : ""}
                    style={{
                      background:
                        v > 0
                          ? `color-mix(in srgb, ${categoryColor(c)} ${Math.round(12 + (v / max) * 58)}%, transparent)`
                          : undefined,
                    }}
                    title={`${monthName(months[i])} ${c}：${money(String(v))}`}
                  >
                    {v > 0 ? (hidden ? "••" : wan(Math.round(v))) : ""}
                  </td>
                ))}
                <td className="heat-avg">
                  {hidden
                    ? "••"
                    : wan(
                        Math.round(values.reduce((a, b) => a + b, 0) / active),
                      )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/** 自動總檢討：只根據資料可以確定的事，每條都附具體金額 */
function review(
  data: Analysis,
  m: Derived,
  navigate: ReturnType<typeof useApp>["navigate"],
): Insight[] {
  const out: Insight[] = [];
  const { cur } = m;

  // 1. 整體支出和平常比
  if (m.avg > 0) {
    const base = m.isCurrent ? m.projected : cur.expense;
    const pct = Math.round(((base - m.avg) / m.avg) * 100);
    out.push({
      tone: pct > 15 ? "warn" : pct < -10 ? "good" : "info",
      icon: pct > 15 ? AlertTriangle : CircleCheck,
      text: (
        <>
          {m.isCurrent ? "照目前速度，月底支出約 " : "這個月支出 "}
          <b>{cash(base)}</b>，
          {Math.abs(pct) < 5
            ? "和近 6 個月平均差不多"
            : `比近 6 個月平均${pct > 0 ? "多" : "少"} ${Math.abs(pct)}%（平均 ${cash(m.avg)}）`}
          。
        </>
      ),
    });
  }

  // 2. 最大宗
  const top = m.cats[0];
  const total = m.cats.reduce((a, c) => a + c.amount, 0);
  if (top && total > 0) {
    const top3 = m.cats.slice(0, 3);
    const share = Math.round(
      (top3.reduce((a, c) => a + c.amount, 0) / total) * 100,
    );
    out.push({
      tone: "info",
      icon: Wallet,
      text: (
        <>
          錢主要花在{" "}
          {top3.map((c, i) => (
            <span key={c.category}>
              {i > 0 && "、"}
              <b>{c.category}</b> {cash(c.amount)}
            </span>
          ))}
          ，前 {top3.length} 名佔了 {share}%。
        </>
      ),
    });
  }

  // 3. 和平常比變化最大的分類
  const moved = m.cats
    .filter((c) => c.avg > 0 && c.category !== "未分類")
    .map((c) => ({ ...c, diff: c.amount - c.avg }))
    .filter((c) => Math.abs(c.diff) >= Math.max(1000, c.avg * 0.3));
  const rise = [...moved].sort((a, b) => b.diff - a.diff)[0];
  if (rise && rise.diff > 0)
    out.push({
      tone: "warn",
      icon: ArrowUpRight,
      text: (
        <>
          <b>{rise.category}</b> 花了 {cash(rise.amount)}，比平常多{" "}
          <b>{cash(rise.diff)}</b>
          {(() => {
            const who = m.merchants.find((x) => x.category === rise.category);
            return who ? `，主要是「${who.name}」${cash(who.amount)}` : "";
          })()}
          。
        </>
      ),
      action: {
        label: "看明細",
        run: () => navigate("activity", { q: rise.category }),
      },
    });
  const drop = [...moved, ...m.quiet.map((c) => ({ ...c, diff: -c.avg }))]
    .filter((c) => c.diff < 0 && c.avg >= 1000)
    .sort((a, b) => a.diff - b.diff)[0];
  if (drop)
    out.push({
      tone: "good",
      icon: ArrowDownRight,
      text: (
        <>
          <b>{drop.category}</b> 比平常少了 {cash(-drop.diff)}（平均{" "}
          {cash(drop.avg)}）。
        </>
      ),
    });

  // 4. 大額支出
  const large = m.spend.filter((e) => -num(e.amount) >= data.large_threshold);
  if (large.length)
    out.push({
      tone: "info",
      icon: Target,
      text: (
        <>
          有 {large.length} 筆大額支出共 <b>{cash(cur.large)}</b>（
          {large
            .slice(0, 2)
            .map(
              (e) => `${e.description || e.category} ${cash(-num(e.amount))}`,
            )
            .join("、")}
          ），扣掉之後日常支出是 {cash(m.regular)}
          {m.avgDaily > 0 && `，平常約 ${cash(m.avgDaily)}`}。
        </>
      ),
    });

  // 5. 儲蓄率
  if (cur.income > 0) {
    const rate = Math.round(((cur.income - cur.expense) / cur.income) * 100);
    out.push({
      tone: rate < 0 ? "bad" : rate >= 20 ? "good" : "warn",
      icon: PiggyBank,
      text:
        rate < 0 ? (
          <>
            支出比收入多 <b>{cash(cur.expense - cur.income)}</b>
            ，這個月是動用存款。
          </>
        ) : (
          <>
            收入 {cash(cur.income)}，存下{" "}
            <b>{cash(cur.income - cur.expense)}</b>
            ，儲蓄率 {rate}%{rate >= 20 ? "，很健康" : "，建議至少 20%"}。
          </>
        ),
    });
  }

  // 6. 預算
  const over = m.cats.filter((c) => c.budget != null && c.amount > c.budget);
  if (over.length)
    out.push({
      tone: "bad",
      icon: AlertTriangle,
      text: (
        <>
          超出預算：
          {over.map((c, i) => (
            <span key={c.category}>
              {i > 0 && "、"}
              <b>{c.category}</b> 超 {cash(c.amount - (c.budget ?? 0))}
            </span>
          ))}
          。
        </>
      ),
      action: { label: "調整預算", run: () => navigate("plans") },
    });
  else if (!data.budgets.length && m.cats.length >= 3 && m.avg > 0)
    out.push({
      tone: "info",
      icon: Target,
      text: (
        <>
          還沒設定預算。可以用前 6 個月的平均當起點，例如{" "}
          <b>{m.cats[0].category}</b> 每月{" "}
          {cash(m.cats[0].avg || m.cats[0].amount)}。
        </>
      ),
      action: { label: "設定預算", run: () => navigate("plans") },
    });

  // 7. 海外刷卡手續費
  const fee = m.spend
    .filter((e) =>
      /國外交易服務費|海外交易手續費|國外交易手續費/.test(e.description),
    )
    .reduce((a, e) => a - num(e.amount), 0);
  if (fee >= 50)
    out.push({
      tone: "warn",
      icon: Globe,
      text: (
        <>
          海外刷卡手續費 <b>{cash(fee)}</b>
          ，訂閱或國外網站消費可以改用免海外手續費或高回饋的卡。
        </>
      ),
      action: {
        label: "看明細",
        run: () => navigate("activity", { q: "國外交易服務費" }),
      },
    });

  // 8. 電子支付儲值
  const wallet = m.cats.find((c) => c.category === "電子支付");
  if (wallet && wallet.amount >= 1000)
    out.push({
      tone: "info",
      icon: Wallet,
      text: (
        <>
          電子支付（街口、全支付等）儲值或付款 {cash(wallet.amount)}
          ，銀行只看得到儲值金額，實際買了什麼要看錢包 App 的紀錄。
        </>
      ),
    });

  // 9. 未分類：放最後，但標為需要處理
  const unc = m.cats.find((c) => c.category === "未分類");
  if (unc)
    out.push({
      tone: "warn",
      icon: Tags,
      text: (
        <>
          還有 {unc.count} 筆共 <b>{cash(unc.amount)}</b> 沒有分類（佔{" "}
          {Math.round((unc.amount / (total || 1)) * 100)}
          %），分好之後分析會更準。
        </>
      ),
      action: {
        label: "去分類",
        run: () => navigate("activity", { q: "未分類" }),
      },
    });

  if (!out.length)
    out.push({
      tone: "info",
      icon: Lightbulb,
      text: <>資料還不夠多，累積幾個月之後這裡會自動比較與提醒。</>,
    });
  return out;
}
