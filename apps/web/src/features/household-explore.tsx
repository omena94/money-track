"use client";
import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Line,
  LineChart,
  Pie,
  PieChart,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  ArrowDownRight,
  ArrowUpRight,
  ChevronRight,
  Info,
  Search,
  X,
} from "lucide-react";
import {
  api,
  type Household,
  type HouseholdExplore,
  type HouseholdPerson,
} from "@/lib/api";
import { money, shiftMonth, shortDate, thisMonth, today } from "@/lib/format";
import { readStore, useApp, writeStore } from "@/components/app-context";
import {
  Badge,
  Card,
  CategoryIcon,
  Chip,
  Empty,
  ErrorLine,
  MonthSwitch,
  Segmented,
  Skeleton,
  categoryColor,
  palette,
} from "@/components/ui";
import { merchantName } from "./analysis";

/* ---------- 日期工具（YYYY-MM-DD，UTC 計算避免時區位移） ---------- */
const dayNo = (s: string) =>
  Date.UTC(+s.slice(0, 4), +s.slice(5, 7) - 1, +s.slice(8, 10)) / 864e5;
const fromNo = (n: number) => new Date(n * 864e5).toISOString().slice(0, 10);
const addDays = (s: string, k: number) => fromNo(dayNo(s) + k);
const monthEnd = (m: string) => addDays(`${shiftMonth(m, 1)}-01`, -1);
const monthDays = (m: string) => Number(monthEnd(m).slice(8));
const span = (r: Range) => dayNo(r.to) - dayNo(r.from) + 1;
const clampDay = (m: string, d: number) =>
  `${m}-${String(Math.min(d, monthDays(m))).padStart(2, "0")}`;
const rangeLabel = (r: Range) =>
  r.from === r.to
    ? shortDate(r.from)
    : `${r.from.slice(0, 4) !== today().slice(0, 4) ? `${r.from.slice(0, 4)}/` : ""}${shortDate(r.from)}–${shortDate(r.to)}`;

type Range = { from: string; to: string };
type Mode = "period" | "member";
type Preset = "prev_same" | "prev" | "last_year" | "avg3" | "before" | "custom";
type Dim = "category" | "merchant" | "member" | "account";
type Drill = Partial<Record<Dim, string>>;

type Config = {
  mode: Mode;
  aKind: "month" | "custom";
  aMonth: string;
  aRange: Range;
  preset: Preset;
  bRange: Range;
  /** 期間比較：兩邊共用的成員；成員比較：A、B 各自的成員 */
  who: string[];
  aWho: string[];
  bWho: string[];
};

const presetLabel: Record<Preset, string> = {
  prev_same: "上月同期",
  prev: "上個月",
  last_year: "去年同期",
  avg3: "前 3 個月平均",
  before: "前一段",
  custom: "自訂",
};
const dimLabel: Record<Dim, string> = {
  category: "分類",
  merchant: "商家",
  member: "成員",
  account: "帳戶／卡",
};
/** 點選某一組後，下一層改依什麼拆 */
const nextDim: Record<Dim, Dim> = {
  category: "merchant",
  merchant: "member",
  member: "category",
  account: "merchant",
};

function aRangeOf(c: Config): Range {
  if (c.aKind === "custom") return c.aRange;
  const now = today();
  return {
    from: `${c.aMonth}-01`,
    to: c.aMonth === thisMonth() ? now : monthEnd(c.aMonth),
  };
}
function presetsFor(c: Config, a: Range): Preset[] {
  if (c.aKind === "custom") return ["before", "last_year", "custom"];
  const partial = a.to !== monthEnd(c.aMonth);
  return [
    ...(partial ? (["prev_same"] as Preset[]) : []),
    "prev",
    "last_year",
    "avg3",
    "custom",
  ];
}
function bRangeOf(c: Config, a: Range, preset: Preset): Range {
  const lastDay = Number(a.to.slice(8));
  switch (preset) {
    case "prev_same": {
      const m = shiftMonth(c.aMonth, -1);
      return { from: `${m}-01`, to: clampDay(m, lastDay) };
    }
    case "prev": {
      const m = shiftMonth(c.aMonth, -1);
      return { from: `${m}-01`, to: monthEnd(m) };
    }
    case "last_year": {
      if (c.aKind === "month") {
        const m = shiftMonth(c.aMonth, -12);
        return { from: `${m}-01`, to: clampDay(m, lastDay) };
      }
      const back = (s: string) =>
        clampDay(`${Number(s.slice(0, 4)) - 1}-${s.slice(5, 7)}`, +s.slice(8));
      return { from: back(a.from), to: back(a.to) };
    }
    case "avg3":
      return {
        from: `${shiftMonth(c.aMonth, -3)}-01`,
        to: monthEnd(shiftMonth(c.aMonth, -1)),
      };
    case "before":
      return { from: addDays(a.from, -span(a)), to: addDays(a.from, -1) };
    case "custom":
      return c.bRange;
  }
}

type Row = HouseholdExplore["entries"][number] & {
  n: number;
  merchant: string;
  side: "a" | "b" | "ab";
};
type Group = {
  key: string;
  label: string;
  a: number;
  /** 已依天數換算成 A 的長度 */
  b: number;
  na: number;
  nb: number;
};

const cash = (v: number) => money(String(Math.round(v)));
const signedCash = (v: number) =>
  `${v > 0 ? "+" : v < 0 ? "−" : ""}${money(String(Math.abs(Math.round(v))))}`;
const wan = (v: number) =>
  Math.abs(v) >= 1e4 ? `${+(v / 1e4).toFixed(1)}萬` : v.toLocaleString("zh-TW");
const pct = (now: number, prev: number) =>
  prev > 0
    ? `${now >= prev ? "+" : "−"}${Math.round((Math.abs(now - prev) / prev) * 100)}%`
    : null;

export function Explore({ home }: { home: Household }) {
  const { route, hidden } = useApp();
  const people: HouseholdPerson[] = useMemo(
    () => [home.me, ...home.shared_with_me],
    [home],
  );
  const nameOf = (id: string) =>
    id === home.me.id
      ? "我"
      : (people.find((p) => p.id === id)?.name ?? "成員");

  const [c, setC] = useState<Config>(() => {
    const saved = readStore<Partial<Config>>("household:explore", {});
    const month = /^\d{4}-\d{2}$/.test(route.params.get("month") ?? "")
      ? route.params.get("month")!
      : thisMonth();
    const ids = people.map((p) => p.id);
    return {
      mode: saved.mode === "member" && people.length > 1 ? "member" : "period",
      aKind: "month",
      aMonth: month,
      aRange: { from: `${month}-01`, to: today() },
      preset: saved.preset ?? "prev_same",
      bRange: saved.bRange ?? {
        from: `${shiftMonth(month, -1)}-01`,
        to: monthEnd(shiftMonth(month, -1)),
      },
      who: (saved.who ?? []).filter((x) => ids.includes(x)),
      aWho: (saved.aWho ?? []).filter((x) => ids.includes(x)),
      bWho: (saved.bWho ?? []).filter((x) => ids.includes(x)),
    };
  });
  const update = (patch: Partial<Config>) =>
    setC((prev) => {
      const next = { ...prev, ...patch };
      const { mode, preset, bRange, who, aWho, bWho } = next;
      writeStore("household:explore", {
        mode,
        preset,
        bRange,
        who,
        aWho,
        bWho,
      });
      return next;
    });

  const [drill, setDrill] = useState<Drill>(() => {
    const cat = route.params.get("cat");
    return cat ? { category: cat } : {};
  });
  const [by, setBy] = useState<Dim>(() =>
    route.params.get("cat") ? "merchant" : "category",
  );

  // 條件整理：成員與期間
  const ids = people.map((p) => p.id);
  const who = c.who.length ? c.who : ids;
  const aWho = c.aWho.length ? c.aWho : [home.me.id];
  const bWho = c.bWho.length
    ? c.bWho
    : [ids.find((x) => x !== home.me.id) ?? home.me.id];
  const a = aRangeOf(c);
  const presets = presetsFor(c, a);
  const preset =
    c.mode === "member"
      ? "custom"
      : presets.includes(c.preset)
        ? c.preset
        : presets[0];
  const b = c.mode === "member" ? a : bRangeOf(c, a, preset);
  const sideA = c.mode === "member" ? aWho : who;
  const sideB = c.mode === "member" ? bWho : who;
  const loaded = [...new Set([...sideA, ...sideB])];
  const valid = a.from <= a.to && b.from <= b.to;

  const q = useQuery({
    queryKey: [
      "household",
      "explore",
      loaded.join(","),
      sideA.join(","),
      sideB.join(","),
      a.from,
      a.to,
      b.from,
      b.to,
    ],
    queryFn: () =>
      api<HouseholdExplore>(
        `/household/explore?members=${loaded.join(",")}&a_members=${sideA.join(",")}&b_members=${sideB.join(",")}&a_from=${a.from}&a_to=${a.to}&b_from=${b.from}&b_to=${b.to}`,
      ),
    enabled: valid,
    placeholderData: (prev) => prev,
  });

  const bName =
    c.mode === "member"
      ? bWho.map(nameOf).join("、")
      : preset === "custom"
        ? rangeLabel(b)
        : presetLabel[preset];
  const aName = c.mode === "member" ? aWho.map(nameOf).join("、") : "本期";

  return (
    <div className="stack">
      <section className="card hx-controls" aria-label="比較條件">
        <div className="hx-control-row">
          <Segmented
            label="比較方式"
            value={c.mode}
            onChange={(mode) => update({ mode })}
            options={[
              { value: "period", label: "期間 vs 期間" },
              {
                value: "member",
                label: "人 vs 人",
                disabled: people.length < 2,
              },
            ]}
          />
          <Segmented
            size="sm"
            label="期間單位"
            value={c.aKind}
            onChange={(aKind) =>
              update(
                aKind === "custom"
                  ? { aKind, aRange: a, preset: "before" }
                  : { aKind, preset: "prev_same" },
              )
            }
            options={[
              { value: "month", label: "月" },
              { value: "custom", label: "自訂期間" },
            ]}
          />
        </div>
        <div className="hx-sides">
          <div className="hx-side">
            <span className="hx-tag hx-tag-a">A</span>
            {c.aKind === "month" ? (
              <MonthSwitch
                month={c.aMonth}
                max={thisMonth()}
                onChange={(aMonth) => update({ aMonth })}
              />
            ) : (
              <DateRange
                value={c.aRange}
                onChange={(aRange) => update({ aRange })}
              />
            )}
            {c.mode === "member" && (
              <People
                people={people}
                me={home.me.id}
                value={aWho}
                onChange={(aWho) => update({ aWho })}
                label="A 的成員"
              />
            )}
          </div>
          <span className="hx-vs">vs</span>
          <div className="hx-side">
            <span className="hx-tag hx-tag-b">B</span>
            {c.mode === "member" ? (
              <>
                <span className="dim small">同一期間</span>
                <People
                  people={people}
                  me={home.me.id}
                  value={bWho}
                  onChange={(bWho) => update({ bWho })}
                  label="B 的成員"
                />
              </>
            ) : (
              <>
                <div className="chip-row" role="radiogroup" aria-label="B 期間">
                  {presets.map((p) => (
                    <Chip
                      key={p}
                      active={preset === p}
                      onClick={() =>
                        update(
                          p === "custom" && preset !== "custom"
                            ? { preset: p, bRange: bRangeOf(c, a, preset) }
                            : { preset: p },
                        )
                      }
                    >
                      {presetLabel[p]}
                    </Chip>
                  ))}
                </div>
                {preset === "custom" ? (
                  <DateRange
                    value={c.bRange}
                    onChange={(bRange) => update({ bRange })}
                  />
                ) : (
                  <span className="dim small">{rangeLabel(b)}</span>
                )}
              </>
            )}
          </div>
        </div>
        {c.mode === "period" && people.length > 1 && (
          <People
            people={people}
            me={home.me.id}
            value={who}
            onChange={(who) => update({ who })}
            label="要合併的成員"
          />
        )}
      </section>

      {!valid ? (
        <ErrorLine>結束日不能早於開始日</ErrorLine>
      ) : q.isError ? (
        <ErrorLine retry={() => q.refetch()}>
          {(q.error as Error)?.message || "追查資料讀取失敗"}
        </ErrorLine>
      ) : !q.data ? (
        <Skeleton rows={3} h={140} />
      ) : (
        <Report
          data={q.data}
          aName={aName}
          bName={bName}
          scaled={c.mode === "period" && span(a) !== span(b)}
          nameOf={nameOf}
          people={people}
          drill={drill}
          setDrill={setDrill}
          by={by}
          setBy={setBy}
          hidden={hidden}
          busy={q.isFetching}
          monthMode={c.aKind === "month"}
        />
      )}
    </div>
  );
}

function DateRange({
  value,
  onChange,
}: {
  value: Range;
  onChange: (r: Range) => void;
}) {
  return (
    <span className="hx-dates">
      <input
        type="date"
        aria-label="開始日"
        value={value.from}
        max={today()}
        onChange={(e) =>
          e.target.value && onChange({ ...value, from: e.target.value })
        }
      />
      <span className="dim">–</span>
      <input
        type="date"
        aria-label="結束日"
        value={value.to}
        max={today()}
        onChange={(e) =>
          e.target.value && onChange({ ...value, to: e.target.value })
        }
      />
    </span>
  );
}

function People({
  people,
  me,
  value,
  onChange,
  label,
}: {
  people: HouseholdPerson[];
  me: string;
  value: string[];
  onChange: (v: string[]) => void;
  label: string;
}) {
  return (
    <div className="chip-row" role="group" aria-label={label}>
      {people.map((p) => {
        const on = value.includes(p.id);
        return (
          <Chip
            key={p.id}
            active={on}
            title={on && value.length === 1 ? "至少要選一位" : p.email}
            onClick={() => {
              const next = on
                ? value.filter((x) => x !== p.id)
                : [...value, p.id];
              if (next.length) onChange(next);
            }}
          >
            <span className="hh-dot">{p.name.slice(0, 1)}</span>
            {p.id === me ? "我" : p.name}
          </Chip>
        );
      })}
    </div>
  );
}

function Report({
  data,
  aName,
  bName,
  scaled,
  nameOf,
  people,
  drill,
  setDrill,
  by,
  setBy,
  hidden,
  busy,
  monthMode,
}: {
  data: HouseholdExplore;
  aName: string;
  bName: string;
  scaled: boolean;
  nameOf: (id: string) => string;
  people: HouseholdPerson[];
  drill: Drill;
  setDrill: (d: Drill) => void;
  by: Dim;
  setBy: (d: Dim) => void;
  hidden: boolean;
  busy: boolean;
  monthMode: boolean;
}) {
  const A = data.a.period;
  const B = data.b.period;
  const factor = scaled ? A.days / B.days : 1;
  const multi = data.members.length > 1;

  const rows: Row[] = useMemo(() => {
    const inA = (r: HouseholdExplore["entries"][number]) =>
      r.date >= A.from &&
      r.date <= A.to &&
      data.a.members.includes(r.member_id);
    const inB = (r: HouseholdExplore["entries"][number]) =>
      r.date >= B.from &&
      r.date <= B.to &&
      data.b.members.includes(r.member_id);
    return data.entries.flatMap((r) => {
      const x = inA(r);
      const y = inB(r);
      if (!x && !y) return [];
      return [
        {
          ...r,
          n: Number(r.amount),
          merchant: merchantName(r.description || r.category),
          side: x && y ? "ab" : x ? "a" : "b",
        } as Row,
      ];
    });
  }, [data, A, B]);

  const keyOf = (r: Row, d: Dim) =>
    d === "category"
      ? r.category
      : d === "merchant"
        ? r.merchant
        : d === "member"
          ? r.member_id
          : (r.account ?? "手動記帳");
  const labelOf = (d: Dim, k: string) => (d === "member" ? nameOf(k) : k);
  const filtered = useMemo(
    () =>
      rows.filter((r) =>
        (Object.keys(drill) as Dim[]).every((d) => keyOf(r, d) === drill[d]),
      ),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [rows, drill],
  );
  const isA = (r: Row) => r.side !== "b";
  const isB = (r: Row) => r.side !== "a";

  // 分組
  const dims = (["category", "merchant", "member", "account"] as Dim[]).filter(
    (d) => !(d in drill) && (d !== "member" || multi),
  );
  const dim = dims.includes(by) ? by : (dims[0] ?? "merchant");
  const groups: Group[] = useMemo(() => {
    const m = new Map<string, Group>();
    for (const r of filtered) {
      const k = keyOf(r, dim);
      const g = m.get(k) ?? {
        key: k,
        label: labelOf(dim, k),
        a: 0,
        b: 0,
        na: 0,
        nb: 0,
      };
      if (isA(r)) {
        g.a += r.n;
        g.na += 1;
      }
      if (isB(r)) {
        g.b += r.n * factor;
        g.nb += 1;
      }
      m.set(k, g);
    }
    return [...m.values()];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filtered, dim, factor]);

  const totalA = filtered.filter(isA).reduce((s, r) => s + r.n, 0);
  const totalB = filtered.filter(isB).reduce((s, r) => s + r.n, 0) * factor;
  const nA = filtered.filter(isA).length;
  const nBraw = filtered.filter(isB).length;
  const nB = nBraw * factor;

  function pick(d: Dim, key: string) {
    setDrill({ ...drill, [d]: key });
    const next = nextDim[d];
    setBy(next === "member" && !multi ? "account" : next);
  }
  const crumbs = (Object.keys(drill) as Dim[]).map((d) => ({
    d,
    label: labelOf(d, drill[d]!),
  }));

  return (
    <>
      <div className="hx-scope">
        <Info size={15} aria-hidden />
        <span>
          只計新台幣支出，不含轉帳、信用卡繳款、投資
          {data.excluded.transfers > 0 &&
            ` · 已排除 ${data.excluded.transfers} 筆家庭內互轉`}
          {data.excluded.duplicates > 0 &&
            ` · 共同帳戶重複 ${data.excluded.duplicates} 筆只算一次`}
          {scaled &&
            ` · B（${B.days} 天）已依天數換算成 A 的 ${A.days} 天，方便直接比較`}
          {busy && " · 更新中…"}
        </span>
      </div>
      {data.truncated && (
        <ErrorLine>資料筆數太多，只載入前 20,000 筆；請縮短期間。</ErrorLine>
      )}

      <nav className="hx-crumbs" aria-label="追查路徑">
        <button
          type="button"
          className={crumbs.length ? "link" : "on"}
          onClick={() => {
            setDrill({});
            setBy("category");
          }}
        >
          全部支出
        </button>
        {crumbs.map((c, i) => (
          <span key={c.d} className="hx-crumb">
            <ChevronRight size={14} aria-hidden />
            <button
              type="button"
              className={i === crumbs.length - 1 ? "on" : "link"}
              onClick={() => {
                const keep: Drill = {};
                crumbs.slice(0, i + 1).forEach((x) => (keep[x.d] = drill[x.d]));
                setDrill(keep);
                setBy(nextDim[c.d]);
              }}
            >
              {dimLabel[c.d]}：{c.label}
            </button>
            <button
              type="button"
              className="icon-btn sm"
              aria-label={`移除${dimLabel[c.d]}篩選`}
              onClick={() => {
                const next = { ...drill };
                delete next[c.d];
                setDrill(next);
              }}
            >
              <X size={13} />
            </button>
          </span>
        ))}
      </nav>

      <Headline
        title={crumbs.length ? crumbs.map((c) => c.label).join(" › ") : "支出"}
        aName={aName}
        bName={bName}
        A={A}
        B={B}
        totalA={totalA}
        totalB={totalB}
        nA={nA}
        nB={nB}
        scaled={scaled}
        groups={groups}
        dim={dim}
        onPick={(k) => pick(dim, k)}
      />

      <div className="hx-grid">
        <Card title="累積支出曲線">
          <PaceChart
            rows={filtered}
            A={A}
            B={B}
            factor={factor}
            aName={aName}
            bName={bName}
            today={data.today}
            hidden={hidden}
          />
        </Card>
        <Card
          title={`${aName}花在哪`}
          action={<DimSwitch dims={dims} value={dim} onChange={setBy} />}
        >
          <Donut
            groups={groups}
            dim={dim}
            total={totalA}
            hidden={hidden}
            people={people}
            onPick={(k) => pick(dim, k)}
          />
        </Card>
      </div>

      <Card
        title={`哪裡變多、變少（依${dimLabel[dim]}）`}
        pad={false}
        action={<DimSwitch dims={dims} value={dim} onChange={setBy} />}
      >
        <DeltaList
          groups={groups}
          dim={dim}
          aName={aName}
          bName={bName}
          onPick={(k) => pick(dim, k)}
        />
      </Card>

      <div className="hx-grid">
        <Card
          title={`近 12 個月${drill.category ? `「${drill.category}」` : "支出"}走勢`}
        >
          <Trend
            data={data}
            drill={drill}
            monthMode={monthMode}
            hidden={hidden}
            aName={aName}
          />
        </Card>
        <Card title="具體花費" pad={false}>
          <Entries
            rows={filtered}
            aName={aName}
            bName={bName}
            nameOf={nameOf}
            multi={multi}
          />
        </Card>
      </div>
    </>
  );
}

function DimSwitch({
  dims,
  value,
  onChange,
}: {
  dims: Dim[];
  value: Dim;
  onChange: (d: Dim) => void;
}) {
  if (dims.length < 2) return null;
  return (
    <Segmented
      size="sm"
      label="拆解方式"
      value={value}
      onChange={onChange}
      options={dims.map((d) => ({ value: d, label: dimLabel[d] }))}
    />
  );
}

/** 結論：總差額，拆成「次數」與「平均單價」兩個原因，加上最大的變化來源 */
function Headline({
  title,
  aName,
  bName,
  A,
  B,
  totalA,
  totalB,
  nA,
  nB,
  scaled,
  groups,
  dim,
  onPick,
}: {
  title: string;
  aName: string;
  bName: string;
  A: HouseholdExplore["a"]["period"];
  B: HouseholdExplore["b"]["period"];
  totalA: number;
  totalB: number;
  nA: number;
  nB: number;
  scaled: boolean;
  groups: Group[];
  dim: Dim;
  onPick: (k: string) => void;
}) {
  const { hidden } = useApp();
  const delta = totalA - totalB;
  const avgA = nA ? totalA / nA : 0;
  const avgB = nB ? totalB / nB : 0;
  const countEffect = (nA - nB) * avgB;
  const priceEffect = nA * (avgA - avgB);
  const flat = totalB > 0 && Math.abs(delta) / totalB < 0.03;
  const up = delta > 0;
  const drivers = [...groups]
    .map((g) => ({ ...g, d: g.a - g.b }))
    .filter((g) => (up ? g.d > 0 : g.d < 0))
    .sort((x, y) => Math.abs(y.d) - Math.abs(x.d))
    .slice(0, 3);
  const show = (v: number) => (hidden ? "••••" : cash(v));
  const showSigned = (v: number) => (hidden ? "••••" : signedCash(v));
  return (
    <section className="card hx-head">
      <div className="hx-head-nums">
        <div>
          <span className="hx-tag hx-tag-a">A</span>
          <small>
            {aName} · {rangeLabel(A)}
          </small>
          <strong className="num hx-big">{show(totalA)}</strong>
          <small>{nA} 筆</small>
        </div>
        <div>
          <span className="hx-tag hx-tag-b">B</span>
          <small>
            {bName} · {rangeLabel(B)}
            {scaled && "（換算後）"}
          </small>
          <strong className="num hx-big">{show(totalB)}</strong>
          <small>{Math.round(nB)} 筆</small>
        </div>
        <div className={`hx-delta ${flat ? "" : up ? "up" : "down"}`}>
          <small>差額（A − B）</small>
          <strong className="num hx-big">
            {!flat &&
              (up ? (
                <ArrowUpRight size={22} aria-hidden />
              ) : (
                <ArrowDownRight size={22} aria-hidden />
              ))}
            {showSigned(delta)}
          </strong>
          <small>
            {flat ? "大致持平" : (pct(totalA, totalB) ?? "B 沒有支出")}
          </small>
        </div>
      </div>
      {totalA === 0 && totalB === 0 ? (
        <p className="hh-sentence">兩邊都沒有支出紀錄。</p>
      ) : (
        <>
          <p className="hh-sentence">
            <strong>{title}</strong>
            {flat ? (
              <>與 {bName} 大致持平。</>
            ) : (
              <>
                比 {bName}
                {up ? "多" : "少"}{" "}
                <strong className="num">{show(Math.abs(delta))}</strong>
                {nA > 0 && nB > 0 && (
                  <>
                    ：次數 {Math.round(nB)} → {nA} 筆（{showSigned(countEffect)}
                    ），平均每筆 {show(avgB)} → {show(avgA)}（
                    {showSigned(priceEffect)}）
                  </>
                )}
                。
              </>
            )}
          </p>
          {!flat && drivers.length > 0 && (
            <div className="hx-drivers">
              <span className="dim small">主要{up ? "增加" : "減少"}來自</span>
              {drivers.map((g) => (
                <button
                  key={g.key}
                  type="button"
                  className="hx-driver"
                  onClick={() => onPick(g.key)}
                >
                  {dim === "category" && (
                    <CategoryIcon category={g.key} kind="expense" size={22} />
                  )}
                  <span>{g.label}</span>
                  <strong className="num">{showSigned(g.d)}</strong>
                  {g.b === 0 && <Badge tone="warn">新出現</Badge>}
                  <ChevronRight size={14} aria-hidden />
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </section>
  );
}

/** 兩條累積曲線：同樣過了第幾天，A 已經花了多少、B 當時花了多少 */
function PaceChart({
  rows,
  A,
  B,
  factor,
  aName,
  bName,
  today: now,
  hidden,
}: {
  rows: Row[];
  A: HouseholdExplore["a"]["period"];
  B: HouseholdExplore["b"]["period"];
  factor: number;
  aName: string;
  bName: string;
  today: string;
  hidden: boolean;
}) {
  const points = useMemo(() => {
    const cum = (from: string, days: number, side: (r: Row) => boolean) => {
      const daily = new Array(days).fill(0);
      for (const r of rows) {
        if (!side(r)) continue;
        const i = dayNo(r.date) - dayNo(from);
        if (i >= 0 && i < days) daily[i] += r.n;
      }
      for (let i = 1; i < days; i++) daily[i] += daily[i - 1];
      return daily;
    };
    const a = cum(A.from, A.days, (r) => r.side !== "b");
    const b = cum(B.from, B.days, (r) => r.side !== "a");
    const lastA = Math.min(A.days, dayNo(now) - dayNo(A.from) + 1);
    return Array.from({ length: A.days }, (_, i) => {
      // B 對齊到 A 的時間軸：第 i 天對應 B 同比例位置
      const j = Math.min(
        B.days - 1,
        Math.floor(((i + 1) * B.days) / A.days) - 1,
      );
      return {
        x: i + 1,
        aDate: addDays(A.from, i),
        bDate: addDays(B.from, Math.max(j, 0)),
        A: i < lastA ? Math.round(a[i]) : null,
        B: j >= 0 ? Math.round(b[j] * factor) : 0,
      };
    });
  }, [rows, A, B, factor, now]);
  if (!rows.length)
    return <Empty compact icon={Search} title="沒有符合的支出" />;
  return (
    <div
      className="hh-chart"
      role="img"
      aria-label={`${aName}與${bName}的累積支出曲線`}
    >
      <div className="hx-legend">
        <span>
          <i className="hx-line-a" />A {aName}
        </span>
        <span>
          <i className="hx-line-b" />B {bName}
        </span>
      </div>
      <ResponsiveContainer width="100%" height={240}>
        <LineChart
          data={points}
          margin={{ top: 8, right: 8, left: 0, bottom: 0 }}
        >
          <CartesianGrid vertical={false} stroke="var(--line)" />
          <XAxis
            dataKey="x"
            tickLine={false}
            axisLine={false}
            minTickGap={24}
            tickFormatter={(x: number) => shortDate(addDays(A.from, x - 1))}
            tick={{ fontSize: 12, fill: "var(--ink-3)" }}
          />
          <YAxis
            hide={hidden}
            width={52}
            tickLine={false}
            axisLine={false}
            tickFormatter={wan}
            tick={{ fontSize: 12, fill: "var(--ink-3)" }}
          />
          <Tooltip
            cursor={{ stroke: "var(--line-2)" }}
            content={({ active, payload }) => {
              if (!active || !payload?.length) return null;
              const p = payload[0].payload as (typeof points)[number];
              return (
                <div className="chart-tip">
                  <span>第 {p.x} 天</span>
                  {p.A != null && (
                    <span>
                      A {shortDate(p.aDate)}{" "}
                      <strong>{hidden ? "••••" : cash(p.A)}</strong>
                    </span>
                  )}
                  <span>
                    B {shortDate(p.bDate)}{" "}
                    <strong>{hidden ? "••••" : cash(p.B)}</strong>
                  </span>
                  {p.A != null && !hidden && (
                    <span>差 {signedCash(p.A - p.B)}</span>
                  )}
                </div>
              );
            }}
          />
          <Line
            type="monotone"
            dataKey="B"
            stroke="var(--ink-3)"
            strokeWidth={2}
            strokeDasharray="5 4"
            dot={false}
            isAnimationActive={false}
          />
          <Line
            type="monotone"
            dataKey="A"
            stroke="var(--accent)"
            strokeWidth={2}
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
            activeDot={{
              r: 4,
              fill: "var(--accent)",
              stroke: "var(--surface)",
            }}
          />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}

const OTHER = "__other__";

/** A 期間的組成；點一塊就往下查 */
function Donut({
  groups,
  dim,
  total,
  hidden,
  people,
  onPick,
}: {
  groups: Group[];
  dim: Dim;
  total: number;
  hidden: boolean;
  people: HouseholdPerson[];
  onPick: (k: string) => void;
}) {
  const [active, setActive] = useState<number>();
  const slices = useMemo(() => {
    const pos = groups.filter((g) => g.a > 0).sort((x, y) => y.a - x.a);
    const top = pos.slice(0, 7);
    const rest = pos.slice(7);
    const color = (g: Group, i: number) =>
      dim === "category"
        ? categoryColor(g.key)
        : dim === "member"
          ? palette[
              Math.max(
                0,
                people.findIndex((p) => p.id === g.key),
              ) % palette.length
            ]
          : palette[i % palette.length];
    const out = top.map((g, i) => ({
      key: g.key,
      label: g.label,
      value: g.a,
      b: g.b,
      color: color(g, i),
    }));
    if (rest.length)
      out.push({
        key: OTHER,
        label: `其他 ${rest.length} 項`,
        value: rest.reduce((s, g) => s + g.a, 0),
        b: rest.reduce((s, g) => s + g.b, 0),
        color: "var(--ink-3)",
      });
    return out;
  }, [groups, dim, people]);
  useEffect(() => setActive(undefined), [slices]);
  if (!slices.length)
    return <Empty compact icon={Search} title="這段期間沒有支出" />;
  const focus = active != null ? slices[active] : undefined;
  return (
    <div className="hx-donut">
      <div className="hx-donut-chart">
        <ResponsiveContainer width="100%" height={220}>
          <PieChart>
            <Pie
              data={slices}
              dataKey="value"
              nameKey="label"
              innerRadius="62%"
              outerRadius="95%"
              paddingAngle={slices.length > 1 ? 1 : 0}
              stroke="var(--surface)"
              strokeWidth={2}
              isAnimationActive={false}
              onMouseEnter={(_, i) => setActive(i)}
              onMouseLeave={() => setActive(undefined)}
              onClick={(_, i) =>
                slices[i].key !== OTHER && onPick(slices[i].key)
              }
            >
              {slices.map((s, i) => (
                <Cell
                  key={s.key}
                  fill={s.color}
                  opacity={active == null || active === i ? 1 : 0.45}
                  style={{ cursor: s.key === OTHER ? "default" : "pointer" }}
                />
              ))}
            </Pie>
          </PieChart>
        </ResponsiveContainer>
        <div className="hx-donut-center" aria-hidden>
          <small>{focus ? focus.label : "合計"}</small>
          <strong className="num">
            {hidden ? "••••" : cash(focus ? focus.value : total)}
          </strong>
          {focus && total > 0 && (
            <small>{Math.round((focus.value / total) * 100)}%</small>
          )}
        </div>
      </div>
      <ul className="hx-donut-legend">
        {slices.map((s, i) => (
          <li key={s.key}>
            <button
              type="button"
              disabled={s.key === OTHER}
              onMouseEnter={() => setActive(i)}
              onMouseLeave={() => setActive(undefined)}
              onFocus={() => setActive(i)}
              onBlur={() => setActive(undefined)}
              onClick={() => onPick(s.key)}
            >
              <i style={{ background: s.color }} />
              <span className="hx-donut-name">{s.label}</span>
              <span className="num">
                {total > 0 ? Math.round((s.value / total) * 100) : 0}%
              </span>
              <span className="num hx-donut-amt">
                {hidden ? "••••" : cash(s.value)}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** 差額排行：依變化多寡排序，條往右是變多、往左是變少 */
function DeltaList({
  groups,
  dim,
  aName,
  bName,
  onPick,
}: {
  groups: Group[];
  dim: Dim;
  aName: string;
  bName: string;
  onPick: (k: string) => void;
}) {
  const { hidden } = useApp();
  const [all, setAll] = useState(false);
  const sorted = [...groups]
    .filter((g) => Math.round(g.a) !== Math.round(g.b))
    .sort((x, y) => Math.abs(y.a - y.b) - Math.abs(x.a - x.b));
  if (!sorted.length)
    return (
      <Empty compact icon={Search} title="沒有差異">
        兩邊的支出完全一樣。
      </Empty>
    );
  const max = Math.max(...sorted.map((g) => Math.abs(g.a - g.b)));
  const shown = all ? sorted : sorted.slice(0, 10);
  const show = (v: number) => (hidden ? "••••" : cash(v));
  return (
    <>
      <div className="hx-delta-head" aria-hidden>
        <span>{dimLabel[dim]}</span>
        <span>A {aName}</span>
        <span>B {bName}</span>
        <span className="hx-delta-axis">
          <span>變少</span>
          <span>變多</span>
        </span>
      </div>
      <ul className="hx-delta-list">
        {shown.map((g) => {
          const d = g.a - g.b;
          const w = max ? (Math.abs(d) / max) * 50 : 0;
          return (
            <li key={g.key}>
              <button type="button" onClick={() => onPick(g.key)}>
                <span className="hx-delta-name">
                  {dim === "category" && (
                    <CategoryIcon category={g.key} kind="expense" size={24} />
                  )}
                  <span>{g.label}</span>
                  {g.b === 0 && <Badge tone="warn">新出現</Badge>}
                  {g.a === 0 && <Badge>A 沒有</Badge>}
                </span>
                <span className="num">{show(g.a)}</span>
                <span className="num dim">{show(g.b)}</span>
                <span className="hx-delta-bar">
                  <i
                    className={d > 0 ? "up" : "down"}
                    style={
                      d > 0
                        ? { left: "50%", width: `${w}%` }
                        : { right: "50%", width: `${w}%` }
                    }
                  />
                  <em className={`num ${d > 0 ? "up" : "down"}`}>
                    {hidden ? "••••" : signedCash(d)}
                  </em>
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      {sorted.length > 10 && (
        <button
          type="button"
          className="link hx-more"
          onClick={() => setAll(!all)}
        >
          {all ? "收起" : `顯示全部 ${sorted.length} 項`}
        </button>
      )}
    </>
  );
}

/** 12 個月走勢：判斷這次是真的變多，還是回到平常水準 */
function Trend({
  data,
  drill,
  monthMode,
  hidden,
  aName,
}: {
  data: HouseholdExplore;
  drill: Drill;
  monthMode: boolean;
  hidden: boolean;
  aName: string;
}) {
  const end = data.a.period.to.slice(0, 7);
  const months = Array.from({ length: 12 }, (_, i) => shiftMonth(end, i - 11));
  const bars = months.map((m) => ({
    month: m,
    label: `${Number(m.slice(5))}月`,
    value: Math.round(
      data.trend
        .filter(
          (t) =>
            t.month === m &&
            (!drill.category || t.category === drill.category) &&
            (!drill.member || t.member_id === drill.member),
        )
        .reduce((s, t) => s + Number(t.amount), 0),
    ),
  }));
  const prior = bars.slice(5, 11);
  const avg = prior.reduce((s, b) => s + b.value, 0) / prior.length;
  const last = bars[11];
  const partial = end === thisMonth();
  const inA = (m: string) => m >= data.a.period.from.slice(0, 7) && m <= end;
  return (
    <>
      <div className="hh-chart">
        <ResponsiveContainer width="100%" height={220}>
          <BarChart
            data={bars}
            margin={{ top: 8, right: 4, left: 0, bottom: 0 }}
          >
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
              content={({ active, payload }) =>
                active && payload?.length ? (
                  <div className="chart-tip">
                    <span>
                      {String(payload[0].payload.month).replace("-", "/")}
                    </span>
                    <strong>
                      {hidden ? "••••" : cash(Number(payload[0].value))}
                    </strong>
                  </div>
                ) : null
              }
            />
            {avg > 0 && (
              <ReferenceLine
                y={avg}
                stroke="var(--ink-3)"
                strokeDasharray="4 4"
                ifOverflow="extendDomain"
              />
            )}
            <Bar
              dataKey="value"
              radius={[4, 4, 0, 0]}
              isAnimationActive={false}
            >
              {bars.map((b) => (
                <Cell
                  key={b.month}
                  fill={
                    inA(b.month)
                      ? "var(--accent)"
                      : "var(--line-2, var(--line))"
                  }
                />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </div>
      <p className="dim small hx-note">
        虛線是前 6 個月平均
        {avg > 0 && !hidden && `（${cash(avg)}）`}
        {monthMode && avg > 0 && !hidden && (
          <>
            ；{aName}所在月{partial ? "目前" : ""} {cash(last.value)}，
            {pct(last.value, avg) ?? ""}
            {partial && "（本月尚未結束）"}
          </>
        )}
        。走勢依{drill.category ? "分類" : "全部支出"}
        {drill.member ? "與成員" : ""}計算
        {(drill.merchant || drill.account) && "，不含商家／帳戶篩選"}
        ，只含 A 的成員。
      </p>
    </>
  );
}

/** 具體花費：符合目前追查條件的每一筆 */
function Entries({
  rows,
  aName,
  bName,
  nameOf,
  multi,
}: {
  rows: Row[];
  aName: string;
  bName: string;
  nameOf: (id: string) => string;
  multi: boolean;
}) {
  const [side, setSide] = useState<"a" | "b">("a");
  const [sort, setSort] = useState<"amount" | "date">("amount");
  const [limit, setLimit] = useState(30);
  useEffect(() => setLimit(30), [rows.length, side, sort]);
  const list = rows
    .filter((r) => (side === "a" ? r.side !== "b" : r.side !== "a"))
    .sort((x, y) =>
      sort === "amount" ? y.n - x.n : y.date.localeCompare(x.date) || y.n - x.n,
    );
  return (
    <>
      <div className="hx-entries-bar">
        <Segmented
          size="sm"
          label="哪一邊"
          value={side}
          onChange={setSide}
          options={[
            { value: "a", label: `A ${aName}` },
            { value: "b", label: `B ${bName}` },
          ]}
        />
        <Segmented
          size="sm"
          label="排序"
          value={sort}
          onChange={setSort}
          options={[
            { value: "amount", label: "金額大到小" },
            { value: "date", label: "日期" },
          ]}
        />
      </div>
      {!list.length ? (
        <Empty compact icon={Search} title="沒有符合的交易" />
      ) : (
        <>
          <ul className="rows hx-entries">
            {list.slice(0, limit).map((r) => (
              <li key={`${r.member_id}:${r.id}`} className="row static">
                <CategoryIcon category={r.category} kind={r.kind} size={34} />
                <span className="row-main">
                  <span className="row-title">
                    <span className="row-text">{r.merchant}</span>
                    {r.kind === "refund" && <Badge tone="good">退款</Badge>}
                  </span>
                  <span className="row-meta">
                    {shortDate(r.date)}
                    {multi && ` · ${nameOf(r.member_id)}`} · {r.category}
                    {r.account && ` · ${r.account}`}
                    {r.description && r.description !== r.merchant && (
                      <> · {r.description}</>
                    )}
                  </span>
                </span>
                <MoneyCell value={r.n} />
              </li>
            ))}
          </ul>
          {list.length > limit && (
            <button
              type="button"
              className="link hx-more"
              onClick={() => setLimit(limit + 50)}
            >
              再顯示 {Math.min(50, list.length - limit)} 筆（共 {list.length}{" "}
              筆）
            </button>
          )}
        </>
      )}
    </>
  );
}

function MoneyCell({ value }: { value: number }) {
  const { hidden } = useApp();
  return (
    <span className="num row-amount">{hidden ? "••••••" : cash(value)}</span>
  );
}
