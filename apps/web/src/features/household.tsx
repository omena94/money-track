"use client";
import { useMemo, useState, type FormEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  AlertTriangle,
  CalendarClock,
  ChevronLeft,
  ChevronRight,
  Eye,
  Info,
  HeartHandshake,
  LogOut,
  Mail,
  UserPlus,
  Users,
  BookOpen,
} from "lucide-react";
import Decimal from "decimal.js";
import {
  api,
  type Household,
  type HouseholdPerson,
  type HouseholdSummary,
  type HouseholdUpcoming,
  type UpcomingPayment,
} from "@/lib/api";
import {
  money,
  monthLabel,
  percent,
  relative,
  shortDate,
  shiftMonth,
  thisMonth,
} from "@/lib/format";
import {
  readStore,
  useAction,
  useApp,
  useDebounced,
  writeStore,
} from "@/components/app-context";
import {
  Badge,
  Button,
  Card,
  CategoryIcon,
  Chip,
  Empty,
  ErrorLine,
  FormError,
  Money,
  PageHead,
  Progress,
  Segmented,
  Skeleton,
  categoryColor,
} from "@/components/ui";
import { TrendChart } from "./net-worth";
import { Avatar, type AvatarValue } from "@/components/avatar";
import { Explore } from "./household-explore";

export const useHousehold = () =>
  useQuery({
    queryKey: ["household"],
    queryFn: () => api<Household>("/household"),
  });

type Tab = "summary" | "explore" | "settings";

export function HouseholdPage() {
  const { route, navigate } = useApp();
  const param = route.params.get("tab");
  const tab: Tab =
    param === "settings" || param === "explore" ? param : "summary";
  const home = useHousehold();
  return (
    <>
      <PageHead
        title="家庭"
        subtitle="把家人的帳本合併起來看總收入、總支出與淨資產；分享都是唯讀的"
        actions={
          <Segmented
            label="家庭頁面"
            value={tab}
            onChange={(t) =>
              navigate("household", t === "summary" ? undefined : { tab: t })
            }
            options={[
              { value: "summary", label: "合併總覽" },
              { value: "explore", label: "支出追查" },
              { value: "settings", label: "分享設定" },
            ]}
          />
        }
      />
      {home.isError ? (
        <ErrorLine retry={() => home.refetch()}>家庭資料讀取失敗</ErrorLine>
      ) : home.isPending ? (
        <Skeleton rows={4} h={72} />
      ) : tab === "summary" ? (
        <Summary home={home.data} />
      ) : tab === "explore" ? (
        <Explore key={route.params.toString()} home={home.data} />
      ) : (
        <Settings home={home.data} />
      )}
    </>
  );
}

function everyone(home: Household): HouseholdPerson[] {
  return [home.me, ...home.shared_with_me];
}

function Summary({ home }: { home: Household }) {
  const { navigate, setViewing } = useApp();
  const people = everyone(home);
  const [picked, setPicked] = useState<string[]>(() =>
    readStore<string[]>("household:members", []),
  );
  // 預設全選；記住的成員若已不再分享就略過
  const chosen = useMemo(() => {
    const valid = picked.filter((id) => people.some((p) => p.id === id));
    return valid.length ? valid : people.map((p) => p.id);
  }, [picked, people]);
  const [month, setMonth] = useState(thisMonth());
  const summary = useQuery({
    queryKey: ["household", "summary", chosen.join(","), month],
    queryFn: () =>
      api<HouseholdSummary>(
        `/household/summary?members=${chosen.join(",")}&month=${month}`,
      ),
    placeholderData: (prev) => prev,
  });
  function toggle(id: string) {
    const next = chosen.includes(id)
      ? chosen.filter((x) => x !== id)
      : [...chosen, id];
    if (!next.length) return;
    setPicked(next);
    writeStore("household:members", next);
  }
  const d = summary.data;
  const totalExpense = d?.categories.reduce(
    (s, c) => s.plus(c.amount),
    new Decimal(0),
  );

  return (
    <div className="stack">
      {!home.shared_with_me.length && (
        <div className="hh-hint">
          <HeartHandshake size={20} />
          <span>
            目前只有你自己的帳本。請家人在他的 Money Track「家庭 ›
            分享設定」加入你的 Google 信箱 <strong>{home.me.email}</strong>
            ，就能在這裡合併加總。
          </span>
          <Button
            size="sm"
            onClick={() => navigate("household", { tab: "settings" })}
          >
            分享設定
          </Button>
        </div>
      )}
      <div className="hh-toolbar">
        <div className="chip-row" role="group" aria-label="要合併的成員">
          {people.map((p) => (
            <Chip
              key={p.id}
              active={chosen.includes(p.id)}
              onClick={() => toggle(p.id)}
              title={
                chosen.includes(p.id) && chosen.length === 1
                  ? "至少要選一位"
                  : p.email
              }
            >
              <span className="hh-dot">{p.name.slice(0, 1)}</span>
              {p.id === home.me.id ? "我" : p.name}
            </Chip>
          ))}
        </div>
        <div className="hh-month">
          <button
            type="button"
            className="icon-btn sm"
            aria-label="上個月"
            onClick={() => setMonth(shiftMonth(month, -1))}
          >
            <ChevronLeft size={17} />
          </button>
          <strong>{monthLabel(month)}</strong>
          <button
            type="button"
            className="icon-btn sm"
            aria-label="下個月"
            disabled={month >= thisMonth()}
            onClick={() => setMonth(shiftMonth(month, 1))}
          >
            <ChevronRight size={17} />
          </button>
        </div>
      </div>

      {summary.isError ? (
        <ErrorLine retry={() => summary.refetch()}>合併資料讀取失敗</ErrorLine>
      ) : !d ? (
        <Skeleton rows={3} h={96} />
      ) : (
        <>
          <DataScope data={d} people={chosen.length} />
          <MonthDigest
            data={d}
            onExplore={(cat) =>
              navigate("household", {
                tab: "explore",
                month: d.month,
                ...(cat ? { cat } : {}),
              })
            }
          />
          <UpcomingPayments members={chosen} multi={chosen.length > 1} />

          <section className="card worth">
            <div className="worth-head">
              <span>
                {chosen.length > 1
                  ? `合併淨資產 · ${chosen.length} 人`
                  : "淨資產"}
              </span>
              <span className="dim">只計新台幣</span>
            </div>
            <Money value={d.net} className="hero-amount" />
            <p className="worth-note">
              資產 <Money value={d.assets} />
              ，負債 <Money value={d.liabilities} />
            </p>
            <TrendChart history={d.history} />
          </section>

          <Card title="成員" pad={false}>
            <ul className="rows">
              {d.members.map((m) => (
                <li key={m.id}>
                  <button
                    type="button"
                    className="row"
                    onClick={() => {
                      if (!m.me) setViewing(m);
                      navigate("overview");
                    }}
                  >
                    <Avatar person={m} />
                    <span className="row-main">
                      <span className="row-title">
                        <span className="row-text">
                          {m.me ? `${m.name}（我）` : m.name}
                        </span>
                        {!m.me && <Badge>唯讀</Badge>}
                      </span>
                      <span className="row-meta">
                        收入 {money(m.income)} · 支出 {money(m.expense)} ·{" "}
                        {syncNote(m)}
                      </span>
                    </span>
                    <span className="hh-member-worth">
                      <small>淨資產</small>
                      <Money value={m.net} className="row-amount" />
                    </span>
                    <Eye size={16} className="dim" aria-label="查看帳本" />
                  </button>
                </li>
              ))}
            </ul>
          </Card>

          <div className="hh-grid">
            <Card title="近 12 個月收支">
              <MonthBars months={d.months} />
            </Card>
            <Card title={`${monthLabel(d.month)}支出分類`} pad={false}>
              {!d.categories.length ? (
                <Empty compact icon={Users} title="這個月還沒有支出" />
              ) : (
                <ul className="hh-cats">
                  {d.categories.slice(0, 10).map((c) => {
                    const pct =
                      totalExpense && !totalExpense.isZero()
                        ? new Decimal(c.amount)
                            .div(totalExpense)
                            .times(100)
                            .toNumber()
                        : 0;
                    return (
                      <li key={c.category}>
                        <button
                          type="button"
                          className="hh-cat-btn"
                          aria-label={`追查${c.category}`}
                          onClick={() =>
                            navigate("household", {
                              tab: "explore",
                              month: d.month,
                              cat: c.category,
                            })
                          }
                        >
                          <CategoryIcon
                            category={c.category}
                            kind="expense"
                            size={32}
                          />
                          <span className="hh-cat-main">
                            <span className="hh-cat-line">
                              <strong>{c.category}</strong>
                              <Money value={c.amount} />
                            </span>
                            <span className="hh-bar">
                              <i
                                style={{
                                  width: `${Math.max(pct, 1)}%`,
                                  background: categoryColor(c.category),
                                }}
                              />
                            </span>
                          </span>
                          <small className="dim">{pct.toFixed(0)}%</small>
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </Card>
          </div>
        </>
      )}
    </div>
  );
}

type Member = HouseholdSummary["members"][number];

/** 成員同步狀態：沒同步來源就不談更新時間 */
function syncNote(m: Member) {
  if (!m.connections) return "手動記帳";
  if (m.failing) return `${m.failing} 個來源登入失敗`;
  if (m.stale) return `${m.stale} 個來源超過 2 天未同步`;
  return `同步於${relative(m.last_synced_at)}`;
}

/** 合併數字的範圍與可信度：涵蓋誰、更新到何時、排除了什麼 */
function DataScope({
  data,
  people,
}: {
  data: HouseholdSummary;
  people: number;
}) {
  const behind = data.members.filter((m) => m.stale || m.failing);
  const t = data.internal_transfers;
  return (
    <div className="hh-scope">
      <p>
        <Info size={15} aria-hidden />
        <span>
          涵蓋 {people} 位成員 · 只計新台幣
          {data.foreign_count > 0 &&
            `（另有 ${data.foreign_count} 筆外幣交易未計入）`}
          {" · "}
          {relative(data.generated_at)}計算
          {t.count > 0 && (
            <>
              {" · "}已排除 {t.count} 筆家庭內互轉 <Money value={t.amount} />
            </>
          )}
          {data.duplicates.count > 0 &&
            ` · 共同帳戶重複的 ${data.duplicates.count} 筆交易只算一次`}
        </span>
      </p>
      {behind.length > 0 && (
        <p className="warn">
          <AlertTriangle size={15} aria-hidden />
          <span>
            {behind
              .map((m) => `${m.me ? "我" : m.name}：${syncNote(m)}`)
              .join("；")}
            。未同步的期間不代表沒有收支。
          </span>
        </p>
      )}
      {data.shared_accounts.length > 0 && (
        <p className="warn">
          <AlertTriangle size={15} aria-hidden />
          <span>
            {data.shared_accounts
              .map((a) => `${a.name}（${a.members.join("、")}）`)
              .join("、")}
            同時出現在多人帳本，收支已去重，但合併淨資產可能重複計算。
          </span>
        </p>
      )}
      {t.items.length > 0 && (
        <details className="disclosure">
          <summary>查看被排除的家庭內互轉</summary>
          <ul className="hh-basis">
            {t.items.map((i, n) => (
              <li key={n}>
                <span>
                  {shortDate(i.date)} {i.from} → {i.to}
                  <small className="dim">
                    {i.from_description} ／ {i.to_description}
                  </small>
                </span>
                <Money value={i.amount} />
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}

/** 本月摘要：收支、預算消耗與上月同期比較，數字附上依據 */
function MonthDigest({
  data,
  onExplore,
}: {
  data: HouseholdSummary;
  onExplore: (category?: string) => void;
}) {
  const c = data.compare;
  const row = data.months.find((m) => m.month === data.month);
  const income = row?.income ?? "0";
  const expense = row?.expense ?? "0";
  const net = new Decimal(income).minus(expense).toString();
  const ongoing = c.through_day < c.days_in_month;
  const multi = data.members.length > 1;
  return (
    <section className="card hh-digest">
      <div className="stat-strip">
        <div>
          <span>{multi ? "家庭總收入" : "收入"}</span>
          <Money value={income} className="lg pos" />
        </div>
        <div>
          <span>{multi ? "家庭總支出" : "支出"}</span>
          <Money value={expense} className="lg" />
        </div>
        <div>
          <span>結餘</span>
          <Money value={net} sign tone className="lg" />
        </div>
      </div>
      <div className="hh-digest-body">
        <p className="dim small">
          {ongoing
            ? `本月已過 ${c.through_day} / ${c.days_in_month} 天`
            : `${monthLabel(data.month)}已結束`}
        </p>
        {data.budget && (
          <BudgetPace budget={data.budget} compare={c} ongoing={ongoing} />
        )}
        <Comparison compare={c} ongoing={ongoing} />
        <button
          type="button"
          className="link hh-explore-link"
          onClick={() => onExplore()}
        >
          用圖表追查錢花去哪、哪裡變多 →
        </button>
      </div>
    </section>
  );
}

function BudgetPace({
  budget,
  compare,
  ongoing,
}: {
  budget: NonNullable<HouseholdSummary["budget"]>;
  compare: HouseholdSummary["compare"];
  ongoing: boolean;
}) {
  const amount = new Decimal(budget.amount);
  const left = amount.minus(budget.spent);
  const used = new Decimal(budget.spent).div(amount).times(100).toNumber();
  const elapsed = (compare.through_day / compare.days_in_month) * 100;
  // 消耗速度：已用比例明顯超過已過天數比例才提醒
  const tone =
    used > 100 ? "bad" : ongoing && used > elapsed + 10 ? "warn" : "good";
  const over = budget.categories.filter((b) =>
    new Decimal(b.spent).gt(b.amount),
  );
  return (
    <div className="hh-budget">
      <div className="hh-cat-line">
        <strong>
          預算{left.isNegative() ? "已超出" : "剩餘"}{" "}
          <Money value={left.abs().toString()} />
        </strong>
        <span className="dim small">
          已用 {percent(used)}
          {ongoing && ` · 時間過了 ${percent(elapsed)}`}
        </span>
      </div>
      <Progress value={used} tone={tone} label="預算使用率" />
      <p className="dim small">
        {budget.categories.length} 個分類有預算（各成員自己設定的本月預算加總）
        {over.length > 0 && `；${over.map((b) => b.category).join("、")}已超出`}
        {tone === "warn" && "；花費速度比天數快"}
      </p>
    </div>
  );
}

function Comparison({
  compare: c,
  ongoing,
}: {
  compare: HouseholdSummary["compare"];
  ongoing: boolean;
}) {
  const now = new Decimal(c.expense);
  const prev = new Decimal(c.prev_expense);
  const delta = now.minus(prev);
  const range = ongoing
    ? `上月同期（${Number(c.prev_month.slice(5))}/1–${c.prev_through_day}）`
    : "上月";
  if (now.isZero() && prev.isZero())
    return <p className="hh-sentence">{range}與本期都沒有支出紀錄。</p>;
  const flat = !prev.isZero() && delta.abs().div(prev).lt(0.05);
  const drivers = c.categories
    .filter((x) => {
      const d = new Decimal(x.amount).minus(x.prev);
      return !d.isZero() && d.isPositive() === delta.isPositive();
    })
    .slice(0, 2);
  return (
    <details className="hh-compare">
      <summary className="hh-sentence">
        {flat ? (
          <>支出與{range}大致持平。</>
        ) : (
          <>
            支出比{range}
            {delta.isPositive() ? "增加" : "減少"}{" "}
            <Money value={delta.abs().toString()} />
            {!prev.isZero() &&
              `（${percent(delta.abs().div(prev).times(100).toNumber())}）`}
            {drivers.length > 0 && (
              <>
                ，主要來自
                {drivers.map((x, i) => (
                  <span key={x.category}>
                    {i > 0 && "、"}
                    {x.category}{" "}
                    <Money
                      value={new Decimal(x.amount).minus(x.prev).toString()}
                      sign
                    />
                  </span>
                ))}
              </>
            )}
            。
          </>
        )}
        <span className="link small">查看依據</span>
      </summary>
      <div className="hh-table-wrap">
        <table className="hh-compare-table">
          <thead>
            <tr>
              <th>分類</th>
              <th>本期</th>
              <th>{ongoing ? "上月同期" : "上月"}</th>
              <th>差額</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td>全部支出</td>
              <td>
                <Money value={c.expense} />
              </td>
              <td>
                <Money value={c.prev_expense} />
              </td>
              <td>
                <Money value={delta.toString()} sign />
              </td>
            </tr>
            {c.categories.map((x) => (
              <tr key={x.category}>
                <td>{x.category}</td>
                <td>
                  <Money value={x.amount} />
                </td>
                <td>
                  <Money value={x.prev} />
                </td>
                <td>
                  <Money
                    value={new Decimal(x.amount).minus(x.prev).toString()}
                    sign
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="dim small">
        已排除轉帳、信用卡繳款、投資及家庭內互轉；只計新台幣。
      </p>
    </details>
  );
}

const paymentStatus: Record<
  UpcomingPayment["status"],
  { label: string; tone: string }
> = {
  scheduled: { label: "預計", tone: "neutral" },
  paid: { label: "來源顯示已繳", tone: "good" },
  unpaid: { label: "未繳", tone: "warn" },
  overdue: { label: "未見繳款紀錄", tone: "bad" },
};

/** 近期要付的固定支出與信用卡帳單；狀態只依來源資料，不因日期推測 */
function UpcomingPayments({
  members,
  multi,
}: {
  members: string[];
  multi: boolean;
}) {
  const [days, setDays] = useState<"7" | "30">(() =>
    readStore<string>("household:upcoming", "30") === "7" ? "7" : "30",
  );
  const q = useQuery({
    queryKey: ["household", "upcoming", members.join(","), days],
    queryFn: () =>
      api<HouseholdUpcoming>(
        `/household/upcoming?members=${members.join(",")}&days=${days}`,
      ),
    placeholderData: (prev) => prev,
  });
  const d = q.data;
  return (
    <Card
      title="近期要付"
      pad={false}
      action={
        <Segmented
          size="sm"
          label="期間"
          value={days}
          onChange={(v) => {
            setDays(v);
            writeStore("household:upcoming", v);
          }}
          options={[
            { value: "7", label: "7 天" },
            { value: "30", label: "30 天" },
          ]}
        />
      }
    >
      {q.isError ? (
        <ErrorLine retry={() => q.refetch()}>近期付款讀取失敗</ErrorLine>
      ) : !d ? (
        <Skeleton rows={2} h={52} />
      ) : !d.items.length ? (
        <Empty
          compact
          icon={CalendarClock}
          title={`未來 ${d.days} 天沒有待付項目`}
        >
          固定支出與已同步的信用卡帳單會出現在這裡。
        </Empty>
      ) : (
        <>
          <div className="hh-due">
            <span>
              本週待付 <Money value={d.week_due} className="lg" />
            </span>
            {d.days > 7 && (
              <span className="dim small">
                {d.days} 天內共 <Money value={d.period_due} />
              </span>
            )}
            {d.overdue_count > 0 && (
              <Badge tone="bad">{d.overdue_count} 筆過期未見繳款</Badge>
            )}
          </div>
          <ul className="rows">
            {d.items.map((i) => {
              const st = paymentStatus[i.status];
              return (
                <li key={i.id} className="row static">
                  <span className="hh-due-date">
                    <strong>{shortDate(i.date)}</strong>
                    <small>{i.source === "bill" ? "帳單" : "固定"}</small>
                  </span>
                  <span className="row-main">
                    <span className="row-title">
                      <span className="row-text">{i.title}</span>
                      <Badge tone={st.tone}>{st.label}</Badge>
                    </span>
                    <span className="row-meta">
                      {multi && `${i.member} · `}
                      {i.source === "bill"
                        ? `帳單資料${relative(i.last_synced_at ?? null)}同步`
                        : `${i.category} · ${i.account_name ?? ""} · 到期自動記帳`}
                    </span>
                  </span>
                  <Money
                    value={i.amount}
                    currency={i.currency}
                    className="row-amount"
                  />
                </li>
              );
            })}
          </ul>
          <p className="hh-foot dim small">
            「預計」是固定支出的排程，不代表已付款；帳單狀態依銀行同步資料，未同步前不會自動變成已繳。
          </p>
        </>
      )}
    </Card>
  );
}

function MonthBars({ months }: { months: HouseholdSummary["months"] }) {
  const { hidden } = useApp();
  const data = months.map((m) => ({
    label: `${Number(m.month.slice(5))}月`,
    收入: Number(m.income),
    支出: Number(m.expense),
  }));
  const wan = (v: number) =>
    Math.abs(v) >= 1e4 ? `${Math.round(v / 1e4)}萬` : v.toLocaleString("zh-TW");
  return (
    <div className="hh-chart">
      <ResponsiveContainer width="100%" height={220}>
        <BarChart data={data} margin={{ top: 8, right: 4, left: 0, bottom: 0 }}>
          <CartesianGrid vertical={false} stroke="var(--line)" />
          <XAxis
            dataKey="label"
            tickLine={false}
            axisLine={false}
            tick={{ fontSize: 12, fill: "var(--ink-3)" }}
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
            formatter={(v) => (hidden ? "••••" : money(String(v)))}
            contentStyle={{
              borderRadius: 10,
              border: "1px solid var(--line)",
              background: "var(--surface)",
              fontSize: 13,
            }}
          />
          <Bar dataKey="收入" fill="var(--pos)" radius={[4, 4, 0, 0]} />
          <Bar dataKey="支出" fill="var(--bad)" radius={[4, 4, 0, 0]} />
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}

/** 分享對象搜尋結果：Email 已由伺服器遮蔽 */
type Person = {
  id: string;
  name: string;
  email: string;
  added: boolean;
  avatar?: AvatarValue | null;
};

function Settings({ home }: { home: Household }) {
  const { viewing } = useApp();
  if (viewing) return <ReadOnlyNotice what="分享設定" />;
  return <SettingsForm home={home} />;
}
function SettingsForm({ home }: { home: Household }) {
  const { navigate, setViewing } = useApp();
  const { run, busy, error, setError } = useAction();
  const [email, setEmail] = useState("");
  const [confirming, setConfirming] = useState<string>();
  const needle = useDebounced(email.trim(), 250);
  const people = useQuery({
    queryKey: ["household", "people", needle],
    queryFn: () =>
      api<Person[]>(`/household/people?q=${encodeURIComponent(needle)}`),
    enabled: needle.length >= 2,
    staleTime: 30_000,
  });
  const looksLikeEmail = /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.trim());
  async function add(e: FormEvent) {
    e.preventDefault();
    const v = email.trim();
    if (!v) return setError("請輸入姓名或 Email");
    // 不是完整 Email 時，剛好只找到一位就直接加入那一位
    const only = people.data?.filter((p) => !p.added);
    if (!looksLikeEmail) {
      if (only?.length === 1) return addPerson(only[0]);
      return setError("請從下方清單選擇成員，或輸入完整的 Email");
    }
    if (
      await run("/household/viewers", {
        body: { email: v },
        success: `${v} 現在可以查看你的帳本`,
      })
    )
      setEmail("");
  }
  async function addPerson(p: Person) {
    if (
      await run("/household/viewers", {
        body: { user_id: p.id },
        success: `${p.name} 現在可以查看你的帳本`,
      })
    )
      setEmail("");
  }
  return (
    <div className="stack">
      <Card title="誰可以看我的帳本">
        <p className="muted hh-explain">
          對方用這個 Google 信箱登入 Money Track 後，就能<strong>唯讀</strong>
          查看你所有的帳戶、收支、投資與資料來源，也能把你合併進家庭總覽計算總收入與總支出。對方不能修改任何資料，也看不到你的銀行密碼。
        </p>
        <form className="hh-invite" onSubmit={add}>
          <label className="sr-only" htmlFor="hh-email">
            家人的 Google 信箱
          </label>
          <input
            id="hh-email"
            type="search"
            autoComplete="off"
            placeholder="搜尋姓名或 Email，或輸入家人的 Google 信箱"
            value={email}
            onChange={(e) => {
              setEmail(e.target.value);
              setError("");
            }}
          />
          <Button type="submit" variant="primary" icon={UserPlus} busy={busy}>
            加入
          </Button>
        </form>
        <FormError>{error}</FormError>
        {needle.length >= 2 && (
          <div className="hh-people" role="listbox" aria-label="符合的成員">
            {people.isPending ? (
              <p className="dim small">搜尋中…</p>
            ) : !people.data?.length ? (
              <p className="dim small">
                {looksLikeEmail
                  ? "這個 Email 還沒登入過 Money Track；按「加入」後，對方登入就能查看。"
                  : "找不到符合的成員。對方需要先用 Google 登入過一次，或直接輸入完整 Email。"}
              </p>
            ) : (
              <ul>
                {people.data.map((p) => (
                  <li key={p.id}>
                    <Avatar person={p} />
                    <span className="hh-person">
                      <strong>{p.name}</strong>
                      <small>{p.email}</small>
                    </span>
                    {p.added ? (
                      <span className="dim small">已可查看</span>
                    ) : (
                      <Button
                        size="sm"
                        icon={UserPlus}
                        busy={busy}
                        onClick={() => addPerson(p)}
                      >
                        加入
                      </Button>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
        {home.viewers.length > 0 && (
          <ul className="rows flush-x hh-viewers">
            {home.viewers.map((v) => (
              <li key={v.id} className="row static">
                <span className="acct-icon">
                  <Mail size={18} />
                </span>
                <span className="row-main">
                  <span className="row-title">
                    <span className="row-text">{v.email}</span>
                  </span>
                  <span className="row-meta">
                    可查看全部 · {relative(v.created_at)}加入
                  </span>
                </span>
                {confirming === v.id ? (
                  <>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => setConfirming(undefined)}
                    >
                      取消
                    </Button>
                    <Button
                      size="sm"
                      variant="danger"
                      busy={busy}
                      onClick={() =>
                        run(`/household/viewers/${v.id}`, {
                          method: "DELETE",
                          success: "已停止分享",
                          toastError: true,
                        })
                      }
                    >
                      確定停止
                    </Button>
                  </>
                ) : (
                  <Button
                    size="sm"
                    variant="danger-ghost"
                    onClick={() => setConfirming(v.id)}
                  >
                    停止分享
                  </Button>
                )}
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Card title="分享給我的帳本" pad={false}>
        {!home.shared_with_me.length ? (
          <Empty compact icon={Users} title="還沒有人分享給你">
            請家人在他的「分享設定」加入 {home.me.email}。
          </Empty>
        ) : (
          <ul className="rows">
            {home.shared_with_me.map((p) => (
              <li key={p.id} className="row static">
                <Avatar person={p} />
                <span className="row-main">
                  <span className="row-title">
                    <span className="row-text">{p.name}</span>
                  </span>
                  <span className="row-meta">{p.email}</span>
                </span>
                <Button
                  size="sm"
                  icon={Eye}
                  onClick={() => {
                    setViewing(p);
                    navigate("overview");
                  }}
                >
                  查看
                </Button>
                {confirming === p.grant_id ? (
                  <Button
                    size="sm"
                    variant="danger"
                    busy={busy}
                    onClick={() =>
                      run(`/household/viewers/${p.grant_id}`, {
                        method: "DELETE",
                        success: `已退出 ${p.name} 的帳本`,
                        toastError: true,
                      })
                    }
                  >
                    確定退出
                  </Button>
                ) : (
                  <Button
                    size="sm"
                    variant="ghost"
                    icon={LogOut}
                    aria-label={`退出 ${p.name} 的帳本`}
                    onClick={() => setConfirming(p.grant_id)}
                  />
                )}
              </li>
            ))}
          </ul>
        )}
      </Card>
    </div>
  );
}

/** 檢視家人帳本時只能看；這頁的操作都屬於帳本本人 */
function ReadOnlyNotice({ what }: { what: string }) {
  const { viewing, setViewing } = useApp();
  return (
    <Empty icon={BookOpen} title={`${what}只有帳本本人可以管理`}>
      正在唯讀查看 {viewing?.name} 的帳本。
      <button type="button" className="link" onClick={() => setViewing(null)}>
        切回我的帳本
      </button>
    </Empty>
  );
}
