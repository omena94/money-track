"use client";
import { useMemo, useState, type FormEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Area,
  AreaChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  ArrowLeftRight,
  Check,
  Link2,
  Loader2,
  Search,
  Sparkles,
  ListOrdered,
  Pencil,
  PlugZap,
  Plus,
  Share2,
  Trash2,
  TrendingUp,
  Wallet,
} from "lucide-react";
import Decimal from "decimal.js";
import { api, type Account, type Entry, type SourceAsset } from "@/lib/api";
import { useAccounts, useSourceAssets } from "@/lib/queries";
import {
  dayLabel,
  evaluate,
  kinds,
  liabilityKinds,
  money,
  quantity,
  relative,
  shortDate,
  sourceNames,
} from "@/lib/format";
import {
  readStore,
  useAction,
  useApp,
  writeStore,
} from "@/components/app-context";
import { BankLogo, HoldingLogo, bankKey } from "@/components/bank-logo";
import {
  Badge,
  Button,
  CategoryIcon,
  FormError,
  Field,
  Money,
  Segmented,
  Sheet,
  Switch,
  Writable,
} from "@/components/ui";
import { kindIcons, kindTones } from "./account-form";

export const accountTypes: Record<string, string> = {
  savings: "活期存款",
  checking: "活期存款",
  time_deposit: "定期存款",
  settlement_cash: "交割帳戶",
  stored_value: "儲值帳戶",
  credit_card: "信用卡",
  loan: "貸款",
};

export type AssetRef =
  { type: "manual"; account: Account } | { type: "sync"; asset: SourceAsset };

/** 帳戶詳情：金額、每日走勢、基本資料、最近收支與操作 */
export function AssetSheet({
  item,
  onClose,
}: {
  item: AssetRef;
  onClose: () => void;
}) {
  const accounts = useAccounts();
  const manual =
    item.type === "manual"
      ? (accounts.data?.find((a) => a.id === item.account.id) ?? item.account)
      : undefined;
  const sync = item.type === "sync" ? item.asset : undefined;
  const debt = manual
    ? liabilityKinds.has(manual.kind)
    : sync!.category === "debt";
  const holding = sync?.kind === "holding";
  const name = manual?.name ?? sync!.name;
  const institution = manual?.institution ?? sync?.institution ?? null;
  const source = sync ? (sourceNames[sync.connector] ?? sync.connector) : null;
  const currency = manual?.currency ?? sync!.currency;
  const amount = manual
    ? manual.balance
    : sync!.amount == null
      ? null
      : debt
        ? new Decimal(sync!.amount).abs().negated().toString()
        : sync!.amount;
  const typeLabel = manual
    ? (kinds[manual.kind] ?? manual.kind)
    : holding
      ? sync?.asset_type === "fund"
        ? "基金"
        : "持股"
      : (accountTypes[sync!.account_type ?? ""] ?? "帳戶");
  const FallbackIcon = manual
    ? (kindIcons[manual.kind] ?? Wallet)
    : holding
      ? TrendingUp
      : undefined;
  const meta = [institution ?? source, sync?.mask, typeLabel]
    .filter(Boolean)
    .join(" · ");
  const accountId = manual?.id ?? (sync && !holding ? sync.id : undefined);

  return (
    <Sheet title={name} description={meta} side="right" onClose={onClose}>
      <div className="asset-hero">
        {holding ? (
          <HoldingLogo name={name} size={52} />
        ) : (
          <BankLogo
            names={[institution, source, sync?.connector]}
            size={52}
            fallback={FallbackIcon}
            tone={manual ? kindTones[manual.kind] : undefined}
          />
        )}
        <div>
          <span className="muted">
            {holding ? "市值" : debt ? "目前欠款" : "目前餘額"}
          </span>
          <Money
            value={amount}
            currency={currency}
            className={`detail-amount ${debt ? "bad" : ""}`}
          />
          {sync && !sync.included && <Badge>未計入淨資產</Badge>}
        </div>
      </div>

      {currency === "TWD" ? (
        <AssetTrend
          path={
            manual
              ? `/asset-history/manual/${manual.id}`
              : `/asset-history/source/${sync!.id}`
          }
          debt={debt}
        />
      ) : (
        <p className="muted small asset-note">
          外幣帳戶尚無匯率換算，暫不畫走勢。
        </p>
      )}

      <dl className="facts">
        {sync ? <SyncFacts a={sync} /> : <ManualFacts a={manual!} />}
      </dl>

      {accountId && !sync?.reference_only && (
        <RecentEntries accountId={accountId} onClose={onClose} />
      )}

      {sync ? (
        <SyncActions a={sync} onClose={onClose} />
      ) : (
        <ManualActions a={manual!} onClose={onClose} />
      )}
    </Sheet>
  );
}

type Range = "1M" | "3M" | "6M" | "1Y" | "ALL";
const rangeDays: Record<Range, number> = {
  "1M": 30,
  "3M": 91,
  "6M": 182,
  "1Y": 365,
  ALL: Infinity,
};

function AssetTrend({ path, debt }: { path: string; debt: boolean }) {
  const { hidden } = useApp();
  const [range, setRange] = useState<Range>(() =>
    readStore("asset-trend:range", "3M"),
  );
  const q = useQuery({
    queryKey: ["asset-history", path],
    queryFn: () => api<{ history: { date: string; value: number }[] }>(path),
  });
  const points = useMemo(() => {
    const all = q.data?.history ?? [];
    // 帳戶開始有資料之前都是 0，略過
    const first = all.findIndex((p) => p.value !== 0);
    const trimmed = first < 0 ? all.slice(-1) : all.slice(first);
    const days = rangeDays[range];
    const cut = Number.isFinite(days) ? trimmed.slice(-days - 1) : trimmed;
    // 負債以正數顯示欠款
    return cut.map((p) => ({ date: p.date, value: debt ? -p.value : p.value }));
  }, [q.data, range, debt]);
  if (q.isPending) return <div className="asset-chart skeleton" />;
  if (q.isError)
    return <p className="muted small asset-note">走勢讀取失敗。</p>;
  const first = points[0]?.value ?? 0;
  const last = points.at(-1)?.value ?? 0;
  const diff = last - first;
  const pct = first !== 0 ? (diff / Math.abs(first)) * 100 : null;
  const good = debt ? diff <= 0 : diff >= 0;
  return (
    <section className="asset-trend">
      <header>
        <span
          className={`asset-change ${diff === 0 ? "" : good ? "pos" : "bad"}`}
        >
          {hidden
            ? "••••"
            : `${diff > 0 ? "+" : diff < 0 ? "−" : "±"}${money(String(Math.abs(diff)))}`}
          {pct != null && !hidden && diff !== 0 && (
            <small>
              （{pct > 0 ? "+" : ""}
              {pct.toFixed(1)}%）
            </small>
          )}
          <small className="dim">
            {points.length > 1
              ? `${shortDate(points[0].date)} 起`
              : "資料累積中"}
          </small>
        </span>
        <Segmented
          size="sm"
          label="走勢區間"
          value={range}
          onChange={(r) => {
            setRange(r);
            writeStore("asset-trend:range", r);
          }}
          options={[
            { value: "1M", label: "1月" },
            { value: "3M", label: "3月" },
            { value: "6M", label: "半年" },
            { value: "1Y", label: "1年" },
            { value: "ALL", label: "全部" },
          ]}
        />
      </header>
      <div className="asset-chart">
        <ResponsiveContainer width="100%" height={180}>
          <AreaChart
            data={points}
            margin={{ top: 8, right: 0, left: 0, bottom: 0 }}
          >
            <defs>
              <linearGradient id="assetFill" x1="0" y1="0" x2="0" y2="1">
                <stop
                  offset="0%"
                  stopColor={debt ? "var(--bad)" : "var(--accent)"}
                  stopOpacity={0.28}
                />
                <stop
                  offset="100%"
                  stopColor={debt ? "var(--bad)" : "var(--accent)"}
                  stopOpacity={0}
                />
              </linearGradient>
            </defs>
            <XAxis dataKey="date" hide />
            <YAxis hide domain={["auto", "auto"]} />
            <Tooltip
              cursor={{ stroke: "var(--line-strong, var(--line))" }}
              labelFormatter={(d) => dayLabel(String(d))}
              formatter={(v) => [
                hidden ? "••••" : money(String(v)),
                debt ? "欠款" : "餘額",
              ]}
              contentStyle={{
                borderRadius: 10,
                border: "1px solid var(--line)",
                background: "var(--surface)",
                fontSize: 13,
              }}
            />
            <Area
              type="stepAfter"
              dataKey="value"
              stroke={debt ? "var(--bad)" : "var(--accent)"}
              strokeWidth={2}
              fill="url(#assetFill)"
              isAnimationActive={false}
            />
          </AreaChart>
        </ResponsiveContainer>
      </div>
    </section>
  );
}

function SyncFacts({ a }: { a: SourceAsset }) {
  const rate =
    a.interest_rate != null
      ? `${new Decimal(a.interest_rate).times(100).toDecimalPlaces(3).toString()}%`
      : null;
  const rows: [string, React.ReactNode][] = [
    ["資料來源", sourceNames[a.connector] ?? a.connector],
  ];
  if (a.institution) rows.push(["銀行", a.institution]);
  if (a.mask) rows.push(["帳號", a.mask]);
  if (a.symbol) rows.push(["代號", a.symbol]);
  if (a.quantity)
    rows.push([
      "持有",
      `${quantity(a.quantity)} ${a.asset_type === "fund" ? "單位" : "股"}`,
    ]);
  if (a.asset_type === "fund") {
    for (const [label, value] of [
      ["投資本金", a.cost_basis],
      ["參考淨值", a.unit_price],
      ["未實現損益", a.unrealized_gain],
      ["累積配息", a.accumulated_dividend],
      ["每次申購金額", a.recurring_amount],
    ] as const) {
      if (value != null)
        rows.push([
          label,
          <Money key={label} value={value} currency={a.currency} />,
        ]);
    }
    if (a.price_date) rows.push(["淨值日期", a.price_date]);
    if (a.recurring_days) rows.push(["每月扣款日", a.recurring_days]);
    if (a.source_status) rows.push(["銀行狀態", a.source_status]);
  }
  if (a.available_balance != null)
    rows.push([
      "可用餘額",
      <Money key="ab" value={a.available_balance} currency={a.currency} />,
    ]);
  if (a.credit_limit != null)
    rows.push([
      "信用額度",
      <Money key="cl" value={a.credit_limit} currency={a.currency} />,
    ]);
  if (a.payment_due)
    rows.push(["繳款截止", a.payment_due.slice(0, 10).replaceAll("-", "/")]);
  if (a.balance_from === "bill")
    rows.push(["欠款計算", "以最新一期帳單（應繳 − 已繳）"]);
  if (rate) rows.push(["年利率", rate]);
  if (a.next_payment_amount != null)
    rows.push([
      "下次還款金額",
      <Money key="np" value={a.next_payment_amount} currency={a.currency} />,
    ]);
  if (a.remaining_installments != null)
    rows.push(["剩餘還款期數", a.remaining_installments]);
  if (a.last_payment_on)
    rows.push([
      "前期付款日",
      a.last_payment_on.slice(0, 10).replaceAll("-", "/"),
    ]);
  if (a.last_payment_amount != null)
    rows.push([
      "前期付款金額",
      <Money key="lp" value={a.last_payment_amount} currency={a.currency} />,
    ]);
  if (a.term) rows.push(["期別", a.term]);
  if (a.opened_on)
    rows.push(["起存日", a.opened_on.slice(0, 10).replaceAll("-", "/")]);
  if (a.maturity_on)
    rows.push(["到期日", a.maturity_on.slice(0, 10).replaceAll("-", "/")]);
  rows.push(["幣別", a.currency]);
  rows.push(["更新時間", relative(a.observed_at)]);
  return (
    <>
      {rows.map(([k, v]) => (
        <div key={k}>
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
    </>
  );
}

function ManualFacts({ a }: { a: Account }) {
  const { viewing } = useApp();
  const [editing, setEditing] = useState(false);
  return (
    <>
      <div>
        <dt>類型</dt>
        <dd>{kinds[a.kind] ?? a.kind} · 手動管理</dd>
      </div>
      <div>
        <dt>幣別</dt>
        <dd>{a.currency}</dd>
      </div>
      <div>
        <dt>期初餘額</dt>
        <dd>
          <Money value={a.opening_balance} currency={a.currency} />
          {!viewing && (
            <button
              type="button"
              className="icon-btn"
              aria-label="編輯期初餘額"
              title="編輯期初餘額"
              onClick={() => setEditing(true)}
            >
              <Pencil size={15} />
            </button>
          )}
        </dd>
      </div>
      {a.kind === "investment" && (
        <>
          <div>
            <dt>帳戶現金</dt>
            <dd>
              <Money value={a.cash_balance} currency={a.currency} />
            </dd>
          </div>
          <div>
            <dt>持倉估值日</dt>
            <dd>{a.valuation_date ?? "尚無持倉估值"}</dd>
          </div>
        </>
      )}
      {editing && (
        <OpeningBalanceForm
          key={a.id}
          account={a}
          onClose={() => setEditing(false)}
        />
      )}
    </>
  );
}

function OpeningBalanceForm({
  account,
  onClose,
}: {
  account: Account;
  onClose: () => void;
}) {
  const { run, busy, error, setError } = useAction();
  const [original] = useState(account.opening_balance);
  const [value, setValue] = useState(new Decimal(original).toString());
  const parsed = value.trim() ? evaluate(value) : null;
  const preview =
    parsed == null
      ? null
      : new Decimal(account.cash_balance)
          .plus(parsed)
          .minus(original)
          .toString();
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy) return;
    if (parsed == null) {
      setError("請輸入有效的期初餘額");
      return;
    }
    if (
      await run(`/accounts/${account.id}/opening-balance`, {
        method: "PATCH",
        body: { opening_balance: parsed, expected_opening_balance: original },
        success: "期初餘額已更新",
      })
    )
      onClose();
  }
  return (
    <Sheet
      title="編輯期初餘額"
      description={account.name}
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <form className="form" onSubmit={submit}>
        <FormError>{error}</FormError>
        <Field
          label={`期初餘額（${account.currency}）`}
          hint="開始記帳前已持有的金額；欠款請填負數。"
        >
          <input
            autoFocus
            required
            inputMode="decimal"
            autoComplete="off"
            value={value}
            disabled={busy}
            onChange={(e) => setValue(e.target.value)}
          />
        </Field>
        <p className="muted small">
          修正期初餘額會更新目前餘額與資產走勢，不新增收入或支出，既有交易保持不變。
        </p>
        {preview != null && (
          <p>
            {account.kind === "investment"
              ? "更新後帳戶現金"
              : "更新後目前餘額"}
            ：<Money value={preview} currency={account.currency} />
          </p>
        )}
        <Button type="submit" variant="primary" busy={busy}>
          儲存期初餘額
        </Button>
        <Button type="button" disabled={busy} onClick={onClose}>
          取消
        </Button>
      </form>
    </Sheet>
  );
}

function RecentEntries({
  accountId,
  onClose,
}: {
  accountId: string;
  onClose: () => void;
}) {
  const { navigate } = useApp();
  const q = useQuery({
    queryKey: ["transactions", "account", accountId],
    queryFn: () => api<Entry[]>(`/activity-feed?account_id=${accountId}`),
  });
  const list = (q.data ?? []).slice(0, 5);
  if (q.isPending || !list.length) return null;
  return (
    <section className="asset-recent">
      <header>
        <strong>最近收支</strong>
        <button
          type="button"
          className="link"
          onClick={() => {
            onClose();
            navigate("activity", { account: accountId });
          }}
        >
          查看全部
        </button>
      </header>
      <ul>
        {list.map((e) => (
          <li key={e.id}>
            <CategoryIcon category={e.category} kind={e.kind} size={30} />
            <span className="asset-recent-main">
              <strong>{e.description || e.category}</strong>
              <small>
                {shortDate(e.occurred_on)} · {e.category}
              </small>
            </span>
            <Money value={e.amount} currency={e.currency} sign tone />
          </li>
        ))}
      </ul>
    </section>
  );
}

function SyncActions({ a, onClose }: { a: SourceAsset; onClose: () => void }) {
  const { navigate, viewing } = useApp();
  const { run, busy } = useAction();
  const holding = a.kind === "holding";
  const [included, setIncluded] = useState(a.included);
  if (a.reference_only) return <AccountLink a={a} />;
  return (
    <div className="action-list">
      <div className="action-switch">
        <span>
          計入淨資產
          <small>
            {holding
              ? "同一來源的持股會一起切換"
              : "如果你另外手動記了同一個帳戶，關閉這裡避免重複計算"}
          </small>
        </span>
        <Switch
          checked={included}
          disabled={busy || Boolean(viewing)}
          label={`${a.name}計入淨資產`}
          onChange={async (v) => {
            if (
              await run(`/source-assets/${a.id}`, {
                method: "PATCH",
                body: { included: v },
                success: v
                  ? `「${a.name}」已計入淨資產`
                  : `「${a.name}」不再計入淨資產`,
                toastError: true,
              })
            )
              setIncluded(v);
          }}
        />
      </div>
      <button
        type="button"
        onClick={() => {
          onClose();
          navigate("sources");
        }}
      >
        <PlugZap size={18} />
        <span>
          資料來源設定
          <small>立即同步、查看同步紀錄</small>
        </span>
      </button>
    </div>
  );
}

type LinkCandidate = {
  value: string;
  name: string;
  bank: string;
  mask: string | null;
  meta: string;
  amount: string | null | undefined;
  currency: string;
  logo: (string | null | undefined)[];
  manual: boolean;
  excluded: boolean;
  type?: string | null;
  score: number;
  reasons: string[];
};

/** 帳號末四碼：優先用 mask 欄位，其次從名稱找 ****1234 */
const last4 = (...v: (string | null | undefined)[]) => {
  for (const s of v) {
    const m = s?.normalize("NFKC").match(/(\d{4})\D*$/);
    if (m) return m[1];
  }
  return null;
};
const tidyName = (s: string) => s.normalize("NFKC").replace(/\s+/g, " ").trim();

/**
 * 集保參考帳戶：連結到實際的銀行帳戶。
 * 依末碼、銀行、餘額自動找出最可能的帳戶，點一下就連結。
 */
function AccountLink({ a }: { a: SourceAsset }) {
  const sources = useSourceAssets();
  const accounts = useAccounts();
  const { viewing } = useApp();
  const { run, busy, error } = useAction();
  const [pending, setPending] = useState<string>();
  const [changing, setChanging] = useState(false);
  const [q, setQ] = useState("");
  const current = sources.data?.find((r) => r.id === a.id) ?? a;
  const linked = current.linked_account;
  const myBank = bankKey(a.institution);
  const myMask = last4(a.mask, a.name);

  const candidates = useMemo(() => {
    const score = (c: Omit<LinkCandidate, "score" | "reasons">) => {
      const reasons: string[] = [];
      let n = 0;
      if (myMask && c.mask === myMask) {
        n += 10;
        reasons.push(`末碼 ${myMask} 相同`);
      }
      if (myBank && bankKey(...c.logo) === myBank) {
        n += 4;
        reasons.push("同一家銀行");
      }
      if (
        a.amount != null &&
        c.amount != null &&
        Number(a.amount) === Number(c.amount)
      ) {
        n += 3;
        reasons.push("餘額相同");
      }
      // 綜合存款的定存與活存共用同一個帳號末碼，要靠類型分辨
      const td = (t?: string | null) => t === "time_deposit";
      if (td(a.account_type) && td(c.type)) {
        n += 2;
        reasons.push("同為定存");
      } else if (td(a.account_type) !== td(c.type) && !c.manual) {
        n -= 8;
      }
      return { ...c, score: n, reasons };
    };
    const list: LinkCandidate[] = [
      ...(sources.data ?? [])
        .filter(
          (r) =>
            r.kind === "account" &&
            !r.reference_only &&
            r.connector !== "tdcc" &&
            r.currency === a.currency &&
            ["savings", "checking", "settlement_cash", "time_deposit"].includes(
              r.account_type ?? "savings",
            ),
        )
        .map((r) =>
          score({
            value: `source:${r.id}`,
            name: tidyName(r.name),
            bank: sourceNames[r.connector] ?? r.institution ?? r.connector,
            mask: last4(r.mask, r.name),
            meta: [accountTypes[r.account_type ?? ""], "同步"]
              .filter(Boolean)
              .join(" · "),
            amount: r.amount,
            currency: r.currency,
            logo: [r.institution, sourceNames[r.connector], r.connector],
            manual: false,
            excluded: !r.included,
            type: r.account_type,
          }),
        ),
      ...(accounts.data ?? [])
        .filter((r) => r.kind === "bank" && r.currency === a.currency)
        .map((r) =>
          score({
            value: `manual:${r.id}`,
            name: tidyName(r.name),
            bank: r.institution?.trim() || "手動帳戶",
            mask: last4(r.name),
            meta: "手動",
            amount: r.balance,
            currency: r.currency,
            logo: [r.institution],
            manual: true,
            excluded: false,
          }),
        ),
    ];
    return list.sort((x, y) => y.score - x.score);
  }, [sources.data, accounts.data, a, myBank, myMask]);

  const best = candidates.filter((c) => c.score >= 7).slice(0, 2);
  const needle = q.trim().normalize("NFKC").toLowerCase();
  const groups = useMemo(() => {
    const map = new Map<string, LinkCandidate[]>();
    for (const c of candidates) {
      if (
        needle &&
        !`${c.bank} ${c.name} ${c.mask ?? ""}`.toLowerCase().includes(needle)
      )
        continue;
      map.set(c.bank, [...(map.get(c.bank) ?? []), c]);
    }
    // 同一家銀行排最前面
    return [...map].sort(
      (x, y) =>
        Number(bankKey(...y[1][0].logo) === myBank) -
          Number(bankKey(...x[1][0].logo) === myBank) ||
        x[0].localeCompare(y[0], "zh-TW"),
    );
  }, [candidates, needle, myBank]);

  async function link(value: string) {
    const [primary_kind, primary_id] = value.split(":");
    setPending(value);
    const ok = await run(`/source-assets/${a.id}/link`, {
      method: "PUT",
      body: { primary_kind, primary_id },
      success: "已連結銀行帳戶",
    });
    setPending(undefined);
    if (ok) {
      setChanging(false);
      setQ("");
    }
  }

  const loading = sources.isPending || accounts.isPending;
  const linkedCandidate = linked
    ? candidates.find((c) => c.value === `${linked.kind}:${linked.id}`)
    : undefined;
  const choosing = !viewing && (!linked || changing);

  const row = (c: LinkCandidate, suggested = false) => (
    <button
      type="button"
      key={c.value}
      className={`link-option ${suggested ? "suggested" : ""}`}
      disabled={busy}
      onClick={() => link(c.value)}
    >
      <BankLogo names={c.logo} size={suggested ? 40 : 32} />
      <span className="link-option-main">
        <strong>
          {c.name}
          {c.mask && !c.name.includes(c.mask) && (
            <span className="dim"> ****{c.mask}</span>
          )}
        </strong>
        <small>
          {suggested ? `${c.bank} · ${c.meta}` : c.meta}
          {c.excluded && " · 未計入淨資產"}
        </small>
        {suggested && c.reasons.length > 0 && (
          <span className="link-reasons">
            {c.reasons.map((r) => (
              <em key={r}>
                <Check size={11} /> {r}
              </em>
            ))}
          </span>
        )}
      </span>
      <span className="link-option-side">
        <Money value={c.amount} currency={c.currency} />
        {pending === c.value ? (
          <Loader2 size={16} className="spin" />
        ) : (
          <span className="link-cta">連結</span>
        )}
      </span>
    </button>
  );

  return (
    <section className="acct-link">
      <div className="acct-link-intro">
        <Link2 size={16} />
        <p>
          這是集保看到的<strong>{a.institution ?? "銀行"}</strong>
          帳戶，只供參考、不計入淨資產。連結到你的銀行帳戶後，餘額與收支以銀行為準；集保持股照常計入。
        </p>
      </div>

      {linked && !changing && (
        <div className="acct-link-current">
          <BankLogo
            names={linkedCandidate?.logo ?? [linked.connector]}
            size={40}
          />
          <span className="link-option-main">
            <small className="dim">已連結</small>
            <strong>{tidyName(linked.name)}</strong>
            {(!linked.available || !linked.included) && (
              <small className="warn-text">
                {!linked.available
                  ? "銀行來源目前沒有這筆帳戶"
                  : "這個銀行帳戶目前未計入淨資產"}
              </small>
            )}
          </span>
          {!viewing && (
            <span className="acct-link-actions">
              <Button size="sm" onClick={() => setChanging(true)}>
                更換
              </Button>
              <Button
                size="sm"
                variant="ghost"
                busy={busy && !pending}
                onClick={() =>
                  run(`/source-assets/${a.id}/link`, {
                    method: "DELETE",
                    success: "已解除連結，集保帳戶仍僅供參考",
                  })
                }
              >
                解除
              </Button>
            </span>
          )}
        </div>
      )}

      {choosing && (
        <>
          {loading ? (
            <p className="dim small">正在讀取你的銀行帳戶…</p>
          ) : sources.isError || accounts.isError ? (
            <p className="bad small">帳戶清單讀取失敗，請重新開啟此帳戶。</p>
          ) : !candidates.length ? (
            <p className="dim small">
              還沒有 {a.currency} 的銀行帳戶。請先連結{a.institution ?? "銀行"}
              資料來源，或新增手動銀行帳戶。
            </p>
          ) : (
            <>
              {best.length > 0 && !needle && (
                <div className="link-best">
                  <span className="link-label">
                    <Sparkles size={13} /> 看起來是這個帳戶
                  </span>
                  {best.map((c) => row(c, true))}
                </div>
              )}
              <div className="link-all">
                <div className="link-all-head">
                  <span className="link-label">
                    {best.length ? "或從所有帳戶選擇" : "選擇對應的銀行帳戶"}
                  </span>
                  {linked && (
                    <button
                      type="button"
                      className="link small"
                      onClick={() => setChanging(false)}
                    >
                      取消更換
                    </button>
                  )}
                </div>
                {candidates.length > 6 && (
                  <label className="search">
                    <Search size={15} />
                    <input
                      type="search"
                      placeholder="搜尋銀行、帳戶名稱或末碼"
                      value={q}
                      onChange={(e) => setQ(e.target.value)}
                    />
                  </label>
                )}
                {groups.map(([bank, list]) => (
                  <div key={bank} className="link-group">
                    <span className="link-group-name">{bank}</span>
                    {list.map((c) => row(c))}
                  </div>
                ))}
                {!groups.length && (
                  <p className="dim small">找不到符合「{q}」的帳戶</p>
                )}
              </div>
            </>
          )}
          <FormError>{error}</FormError>
        </>
      )}
    </section>
  );
}

function ManualActions({ a, onClose }: { a: Account; onClose: () => void }) {
  const { navigate, compose } = useApp();
  const { run, busy, error } = useAction();
  const [confirm, setConfirm] = useState(false);
  return (
    <div className="action-list">
      <button
        type="button"
        onClick={() => {
          onClose();
          navigate("activity", { account: a.id });
        }}
      >
        <ListOrdered size={18} />
        <span>查看這個帳戶的收支</span>
      </button>
      <Writable>
        <button
          type="button"
          onClick={() => {
            onClose();
            compose({ accountId: a.id });
          }}
        >
          <Plus size={18} />
          <span>在這個帳戶記一筆</span>
        </button>
        <button
          type="button"
          onClick={() => {
            onClose();
            compose({ accountId: a.id, kind: "transfer" });
          }}
        >
          <ArrowLeftRight size={18} />
          <span>
            轉帳
            {a.kind === "credit" && <small>繳卡費請從銀行帳戶轉到這張卡</small>}
          </span>
        </button>
        <button
          type="button"
          onClick={() => {
            onClose();
            navigate("household", { tab: "settings" });
          }}
        >
          <Share2 size={18} />
          <span>
            分享給家人
            <small>對方可以唯讀查看你整個帳本</small>
          </span>
        </button>
        {!confirm ? (
          <button
            type="button"
            className="danger"
            onClick={() => setConfirm(true)}
          >
            <Trash2 size={18} />
            <span>
              刪除帳戶
              <small>只有沒有任何交易的帳戶可以刪除</small>
            </span>
          </button>
        ) : (
          <div className="confirm-box">
            <FormError>{error}</FormError>
            <p>確定刪除「{a.name}」？無法復原。</p>
            <div className="form-actions">
              <Button onClick={() => setConfirm(false)} disabled={busy}>
                取消
              </Button>
              <Button
                variant="danger"
                busy={busy}
                onClick={async () => {
                  if (
                    await run(`/accounts/${a.id}`, {
                      method: "DELETE",
                      success: `已刪除「${a.name}」`,
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
      </Writable>
    </div>
  );
}
