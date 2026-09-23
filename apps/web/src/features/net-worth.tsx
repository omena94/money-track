"use client";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  ArrowDownRight,
  ArrowUpRight,
  ChevronDown,
  CircleAlert,
  Globe,
  TrendingUp,
} from "lucide-react";
import Decimal from "decimal.js";
import type { AssetCategory, NetWorth, NetWorthPoint } from "@/lib/api";
import { dayLabel, money, shortDate } from "@/lib/format";
import { readStore, useApp, writeStore } from "@/components/app-context";
import { Empty, Money, Segmented } from "@/components/ui";

export const categoryLabels: Record<AssetCategory, string> = {
  bank: "銀行與現金",
  time_deposit: "定存",
  investment: "投資",
  other: "其他資產",
  debt: "負債",
};
const countUnit: Record<AssetCategory, string> = {
  bank: "個帳戶",
  time_deposit: "筆",
  investment: "項",
  other: "項",
  debt: "筆",
};
type Range = "1M" | "3M" | "6M" | "1Y" | "ALL";
const rangeDays: Record<Range, number> = {
  "1M": 30,
  "3M": 91,
  "6M": 182,
  "1Y": 365,
  ALL: Infinity,
};
type Pick = Record<AssetCategory, boolean>;
const allPicked: Pick = {
  bank: true,
  time_deposit: true,
  investment: true,
  other: true,
  debt: true,
};

/** 萬／億為單位的軸標籤 */
function wan(v: number) {
  const a = Math.abs(v);
  if (a >= 1e8) return `${(v / 1e8).toFixed(a >= 1e9 ? 0 : 1)}億`;
  if (a >= 1e4) return `${Math.round(v / 1e4).toLocaleString("zh-TW")}萬`;
  return v.toLocaleString("zh-TW");
}

/** 總覽的淨資產區塊：總額、類別、每日走勢 */
export function NetWorthPanel({
  data,
  onOpenAssets,
}: {
  data: NetWorth;
  onOpenAssets: () => void;
}) {
  const debt = new Decimal(data.liabilities);
  return (
    <section className="card worth">
      <div className="worth-head">
        <span>淨資產</span>
        <span className="dim">{dayLabel(data.as_of).split(" · ").pop()}</span>
      </div>
      <Money value={data.net} className="hero-amount" />
      <p className="worth-note">
        {debt.gt(0) ? (
          <>
            已扣除 <Money value={debt.toString()} /> 信用卡與貸款負債
          </>
        ) : (
          "目前沒有負債"
        )}
      </p>
      {data.unknown > 0 && (
        <p className="hero-warn">
          <CircleAlert size={15} />
          {data.unknown} 筆資產缺少金額或價格，未計入合計
        </p>
      )}
      <div className="worth-cats">
        {data.categories
          .filter((c) => c.key !== "debt")
          .map((c) => (
            <button type="button" key={c.key} onClick={onOpenAssets}>
              <span>{categoryLabels[c.key]}</span>
              <Money value={c.amount} className="lg" />
              <small>
                {c.count} {countUnit[c.key]}
              </small>
            </button>
          ))}
      </div>
      <ForeignBlock data={data} />
      <TrendChart history={data.history} />
    </section>
  );
}

const fxTime = (iso: string) =>
  new Date(iso).toLocaleString("zh-TW", {
    timeZone: "Asia/Taipei",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });

/** 外幣資產：原幣、匯率與等值台幣，與台幣淨資產分開列示 */
export function ForeignBlock({ data }: { data: NetWorth }) {
  const rows = data.foreign.filter((f) => !new Decimal(f.net).isZero());
  if (!rows.length) return null;
  const fx = data.fx;
  return (
    <div className="fx-block">
      <div className="fx-head">
        <span>
          <Globe size={15} />
          外幣資產
        </span>
        {fx && (
          <span className="fx-total">
            <small>約</small>
            <Money value={fx.foreign_twd} />
          </span>
        )}
      </div>
      <ul className="fx-rows">
        {rows.map((f) => (
          <li key={f.currency}>
            <span className="fx-code">{f.currency}</span>
            <Money value={f.net} currency={f.currency} className="fx-amt" />
            <span className="fx-rate dim">
              {f.rate ? `× ${new Decimal(f.rate).toString()}` : "尚無匯率"}
            </span>
            <span className="fx-twd">
              {f.twd != null ? (
                <>
                  ≈ <Money value={f.twd} />
                </>
              ) : (
                <span className="dim">—</span>
              )}
            </span>
          </li>
        ))}
      </ul>
      {fx ? (
        <p className="fx-note">
          含外幣淨資產約 <Money value={fx.net_with_foreign} />
          {!fx.complete && "（部分幣別無匯率，未計入）"}
          <br />
          {fx.source}
          {fx.source.startsWith("玉山") && "買入匯率"} · 報價{" "}
          {fxTime(fx.quoted_at)} · 約每 10 分鐘更新；外幣未計入上方淨資產與走勢
        </p>
      ) : (
        <p className="fx-note">目前取不到匯率，外幣未計入上方淨資產與走勢</p>
      )}
    </div>
  );
}

export function TrendChart({ history }: { history: NetWorthPoint[] }) {
  const { hidden } = useApp();
  const [range, setRange] = useState<Range>(() =>
    readStore("trend:range", "3M"),
  );
  const [pick, setPick] = useState<Pick>(() => ({
    ...allPicked,
    ...readStore<Partial<Pick>>("trend:pick", {}),
  }));
  const [menu, setMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) =>
      menuRef.current?.contains(e.target as Node) || setMenu(false);
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menu]);
  const everything = Object.values(pick).every(Boolean);

  const points = useMemo(() => {
    const n = rangeDays[range];
    const slice = Number.isFinite(n) ? history.slice(-n) : history;
    // 點太多時抽樣，保留最後一天
    const step = Math.max(1, Math.ceil(slice.length / 400));
    return slice
      .filter((_, i) => i % step === 0 || i === slice.length - 1)
      .map((p) => ({
        date: p.date,
        value:
          (pick.bank ? p.bank : 0) +
          (pick.time_deposit ? (p.time_deposit ?? 0) : 0) +
          (pick.investment ? p.investment : 0) +
          (pick.other ? p.other : 0) +
          (pick.debt ? p.debt : 0),
      }));
  }, [history, range, pick]);

  const first = points[0]?.value ?? 0;
  const last = points.at(-1)?.value ?? 0;
  const change = first ? ((last - first) / Math.abs(first)) * 100 : null;
  const values = points.map((p) => p.value);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const pad = Math.max((max - min) * 0.08, Math.abs(max) * 0.01, 1);

  function toggle(k: AssetCategory) {
    const next = { ...pick, [k]: !pick[k] };
    if (!next.bank && !next.time_deposit && !next.investment && !next.other)
      return;
    setPick(next);
    writeStore("trend:pick", next);
  }

  return (
    <div className="trend">
      <div className="trend-head">
        <h2>
          <TrendingUp size={17} />
          資產走勢
        </h2>
        {change != null && points.length > 1 && !hidden && (
          <span className={`trend-change ${change >= 0 ? "up" : "down"}`}>
            {change >= 0 ? (
              <ArrowUpRight size={14} />
            ) : (
              <ArrowDownRight size={14} />
            )}
            {Math.abs(change).toFixed(1)}%
          </span>
        )}
      </div>
      <div className="trend-controls">
        <Segmented<Range>
          label="期間"
          size="sm"
          value={range}
          onChange={(r) => {
            setRange(r);
            writeStore("trend:range", r);
          }}
          options={(["1M", "3M", "6M", "1Y", "ALL"] as Range[]).map((r) => ({
            value: r,
            label: r === "ALL" ? "全部" : r,
          }))}
        />
        <div className="menu-wrap" ref={menuRef}>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            aria-expanded={menu}
            onClick={() => setMenu(!menu)}
          >
            顯示設定
            <ChevronDown size={15} />
          </button>
          {menu && (
            <div className="menu" role="menu">
              {(Object.keys(categoryLabels) as AssetCategory[]).map((k) => (
                <label key={k} className="menu-item">
                  <input
                    type="checkbox"
                    checked={pick[k]}
                    onChange={() => toggle(k)}
                  />
                  {k === "debt" ? "扣除負債" : categoryLabels[k]}
                </label>
              ))}
            </div>
          )}
        </div>
      </div>
      <div className="trend-total">
        <div>
          <span>{everything ? "淨資產" : "已選資產合計"}</span>
          <Money value={String(last)} className="xl" />
        </div>
        {points.length > 0 && (
          <span className="dim small">
            {points.at(-1)!.date.replaceAll("-", "/")}
          </span>
        )}
      </div>
      {points.length < 2 ? (
        <Empty compact icon={TrendingUp} title="走勢會從今天開始累積">
          記帳或同步銀行後，這裡會畫出每日資產變化。
        </Empty>
      ) : (
        <div className="trend-chart" aria-label="資產走勢圖" role="img">
          <ResponsiveContainer width="100%" height={260}>
            <AreaChart
              data={points}
              margin={{ top: 8, right: 8, left: 0, bottom: 0 }}
            >
              <defs>
                <linearGradient id="worthFill" x1="0" y1="0" x2="0" y2="1">
                  <stop
                    offset="0%"
                    stopColor="var(--accent)"
                    stopOpacity={0.22}
                  />
                  <stop
                    offset="100%"
                    stopColor="var(--accent)"
                    stopOpacity={0.02}
                  />
                </linearGradient>
              </defs>
              <CartesianGrid vertical={false} stroke="var(--line)" />
              <XAxis
                dataKey="date"
                tickFormatter={shortDate}
                tickLine={false}
                axisLine={false}
                minTickGap={36}
                tick={{ fill: "var(--ink-3)", fontSize: 12 }}
              />
              <YAxis
                domain={[min - pad, max + pad]}
                tickFormatter={(v: number) => (hidden ? "" : wan(v))}
                tickLine={false}
                axisLine={false}
                width={hidden ? 8 : 60}
                tickCount={5}
                tick={{ fill: "var(--ink-3)", fontSize: 12 }}
              />
              <Tooltip
                cursor={{ stroke: "var(--line-2)" }}
                content={({ active, payload }) =>
                  active && payload?.length ? (
                    <div className="chart-tip">
                      <span>{dayLabel(String(payload[0].payload.date))}</span>
                      <strong>
                        {hidden ? "••••••" : money(String(payload[0].value))}
                      </strong>
                    </div>
                  ) : null
                }
              />
              <Area
                type="monotone"
                dataKey="value"
                stroke="var(--accent)"
                strokeWidth={2}
                fill="url(#worthFill)"
                isAnimationActive={false}
                dot={false}
                activeDot={{
                  r: 4,
                  fill: "var(--accent)",
                  stroke: "var(--surface)",
                }}
              />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      )}
      <p className="hint-line">
        手動帳戶依交易紀錄回推；同步帳戶以同步到的餘額為準，更早的日期依交割帳戶收支回推；集保持股在同步之前使用集保提供的資產歷史（股票＋基金）。手動投資的市值從開始使用此功能當天起每日記錄。
      </p>
    </div>
  );
}
