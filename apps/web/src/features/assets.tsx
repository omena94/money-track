"use client";
import { useEffect, useMemo, useState } from "react";
import {
  ChevronRight,
  CircleAlert,
  PlugZap,
  Plus,
  RefreshCw,
  Wallet,
} from "lucide-react";
import Decimal from "decimal.js";
import { type Account, type SourceAsset } from "@/lib/api";
import {
  kinds,
  liabilityKinds,
  quantity,
  relative,
  shortDate,
  sourceNames,
} from "@/lib/format";
import { BankLogo, HoldingLogo } from "@/components/bank-logo";
import {
  useAccounts,
  useFx,
  useNetWorth,
  useSourceAssets,
} from "@/lib/queries";
import { useAction, useApp } from "@/components/app-context";
import {
  Badge,
  Button,
  Empty,
  ErrorLine,
  Money,
  PageHead,
  Sheet,
  Skeleton,
  Switch,
  Writable,
} from "@/components/ui";
import { AccountForm, kindIcons, kindTones } from "./account-form";
import { categoryLabels, ForeignBlock } from "./net-worth";
import { AssetSheet, type AssetRef } from "./asset-sheet";
/** 明細無法並排時改用抽屜；與 globals.css 的 .ledger 媒體查詢同步 */
const narrowLedger =
  "(max-width: 900px), (min-width: 1024px) and (max-width: 1199px)";

const accountTypes: Record<string, string> = {
  savings: "活期",
  checking: "活期",
  time_deposit: "定期",
  settlement_cash: "交割",
  stored_value: "儲值",
};
const refOf = (i: Item): AssetRef =>
  i.type === "manual"
    ? { type: "manual", account: i.account }
    : { type: "sync", asset: i.asset };
const stampFull = (v: string) =>
  new Date(v).toLocaleString("zh-TW", {
    timeZone: "Asia/Taipei",
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });

type Item =
  | {
      type: "manual";
      key: string;
      account: Account;
      debt: boolean;
      holding: false;
    }
  | {
      type: "sync";
      key: string;
      asset: SourceAsset;
      debt: boolean;
      holding: boolean;
    };
type Group = {
  key: string;
  title: string;
  items: Item[];
  assets: Decimal;
  debt: Decimal;
  unknownDebt: boolean;
  synced: boolean;
  manual: boolean;
  /** 群組只有同一種手動帳戶時的類型，用來選退回圖示 */
  kind?: string;
  foreign: string[];
};

/** 群組圖示：認得的機構用官方圖示，否則依帳戶類型（現金、房產…）顯示彩色圖示 */
const GroupLogo = ({ g, size }: { g: Group; size: number }) => (
  <BankLogo
    names={[g.title]}
    size={size}
    fallback={g.kind ? kindIcons[g.kind] : undefined}
    tone={g.kind ? kindTones[g.kind] : undefined}
  />
);

const manualTitle = (a: Account) =>
  a.institution?.trim() ||
  (a.kind === "investment" ? "投資帳戶" : (kinds[a.kind] ?? "其他"));
const syncTitle = (a: SourceAsset) =>
  sourceNames[a.connector] ?? a.institution ?? a.connector;

function buildGroups(accounts: Account[], sources: SourceAsset[]): Group[] {
  const map = new Map<string, Group>();
  const group = (title: string) => {
    let g = map.get(title);
    if (!g) {
      g = {
        key: title,
        title,
        items: [],
        assets: new Decimal(0),
        debt: new Decimal(0),
        unknownDebt: false,
        synced: false,
        manual: false,
        foreign: [],
      };
      map.set(title, g);
    }
    return g;
  };
  const tally = (
    g: Group,
    debt: boolean,
    amount: string | null | undefined,
    currency: string,
    counted: boolean,
  ) => {
    if (currency !== "TWD") {
      if (!g.foreign.includes(currency)) g.foreign.push(currency);
      return;
    }
    if (!counted) return;
    if (amount == null) {
      if (debt) g.unknownDebt = true;
      return;
    }
    if (debt) g.debt = g.debt.plus(new Decimal(amount).abs());
    else g.assets = g.assets.plus(amount);
  };
  for (const a of accounts) {
    const g = group(manualTitle(a));
    const debt = liabilityKinds.has(a.kind);
    g.kind = !g.manual || g.kind === a.kind ? a.kind : undefined;
    g.manual = true;
    g.items.push({
      type: "manual",
      key: a.id,
      account: a,
      debt,
      holding: false,
    });
    tally(g, debt, a.balance, a.currency, true);
  }
  for (const s of sources) {
    const g = group(syncTitle(s));
    const debt = s.category === "debt";
    g.synced = true;
    g.kind = undefined;
    g.items.push({
      type: "sync",
      key: s.id,
      asset: s,
      debt,
      holding: s.kind === "holding",
    });
    tally(g, debt, s.amount, s.currency, s.included);
  }
  return [...map.values()].sort(
    (a, b) =>
      b.assets.minus(a.assets).toNumber() ||
      b.debt.minus(a.debt).toNumber() ||
      a.title.localeCompare(b.title, "zh-TW"),
  );
}

export function AssetsPage() {
  const { route, navigate } = useApp();
  const accounts = useAccounts();
  const sources = useSourceAssets();
  const worth = useNetWorth();
  const [adding, setAdding] = useState(false);
  const [selected, setSelected] = useState<AssetRef>();
  const [active, setActive] = useState<string>();
  const [mobileGroup, setMobileGroup] = useState<string>();
  useEffect(() => {
    if (route.params.get("new")) {
      setAdding(true);
      navigate("assets");
    }
  }, [route.params, navigate]);
  const groups = useMemo(
    () => buildGroups(accounts.data ?? [], sources.data ?? []),
    [accounts.data, sources.data],
  );
  const current = groups.find((g) => g.key === active) ?? groups[0];
  const loading = accounts.isPending || sources.isPending;

  return (
    <>
      <PageHead
        title="資產"
        subtitle="銀行、信用卡、投資與其他資產；手動與自動同步的帳戶一起管理"
        actions={
          <>
            <Button icon={PlugZap} onClick={() => navigate("sources")}>
              連結銀行
            </Button>
            <Writable>
              <Button
                variant="primary"
                icon={Plus}
                onClick={() => setAdding(true)}
              >
                新增帳戶
              </Button>
            </Writable>
          </>
        }
      />
      {worth.data && groups.length > 0 && (
        <section className="card worth compact">
          <div className="worth-head">
            <span>淨資產</span>
          </div>
          <Money value={worth.data.net} className="hero-amount" />
          <p className="worth-note">
            {new Decimal(worth.data.liabilities).gt(0) ? (
              <>
                已扣除 <Money value={worth.data.liabilities} /> 負債
              </>
            ) : (
              "目前沒有負債"
            )}
          </p>
          <div className="worth-cats">
            {worth.data.categories.map((c) => (
              <div key={c.key}>
                <span>{categoryLabels[c.key]}</span>
                <Money
                  value={c.amount}
                  className={`lg ${c.key === "debt" ? "bad" : ""}`}
                />
                <small>{c.count} 項</small>
              </div>
            ))}
          </div>
          <ForeignBlock data={worth.data} />
        </section>
      )}
      {loading ? (
        <Skeleton rows={4} h={64} />
      ) : accounts.isError || sources.isError ? (
        <ErrorLine
          retry={() => {
            void accounts.refetch();
            void sources.refetch();
          }}
        >
          資產讀取失敗
        </ErrorLine>
      ) : !groups.length ? (
        <section className="card">
          <Empty
            icon={Wallet}
            title="把你的帳戶都放進來"
            action={
              <div className="inline-actions">
                <Button icon={PlugZap} onClick={() => navigate("sources")}>
                  連結銀行自動同步
                </Button>
                <Writable>
                  <Button
                    variant="primary"
                    icon={Plus}
                    onClick={() => setAdding(true)}
                  >
                    手動新增帳戶
                  </Button>
                </Writable>
              </div>
            }
          >
            連結銀行、集保後會自動帶入餘額與持倉；現金、房子、貸款等也可以手動記錄，看見完整的淨資產。
          </Empty>
        </section>
      ) : (
        <div className="ledger">
          <section className="card flush ledger-list">
            <header className="ledger-list-head">
              <div>
                <strong>帳戶與資產</strong>
                <small>同一金融機構的帳戶、信用卡與持倉合併顯示</small>
              </div>
              <span className="dim small">{groups.length} 項</span>
            </header>
            <ul>
              {groups.map((g) => (
                <li key={g.key}>
                  <button
                    type="button"
                    className={`inst ${current?.key === g.key ? "on" : ""}`}
                    onClick={() => {
                      setActive(g.key);
                      if (window.matchMedia(narrowLedger).matches)
                        setMobileGroup(g.key);
                    }}
                  >
                    <GroupLogo g={g} size={36} />
                    <span className="inst-main">
                      <strong>
                        {g.title}
                        {g.synced && <Badge tone="info">自動同步</Badge>}
                      </strong>
                      <small>{summary(g)}</small>
                    </span>
                    <span className="inst-amounts">
                      {g.items.some((i) => !i.debt) && (
                        <Money value={g.assets.toString()} className="asset" />
                      )}
                      {g.debt.gt(0) ? (
                        <small className="bad">
                          負債 <Money value={g.debt.negated().toString()} />
                        </small>
                      ) : g.unknownDebt ? (
                        <small className="dim">負債金額待更新</small>
                      ) : null}
                    </span>
                    <ChevronRight size={16} className="inst-chevron" />
                  </button>
                </li>
              ))}
            </ul>
          </section>
          {current && (
            <section className="card flush ledger-detail">
              <GroupDetail group={current} onOpen={setSelected} />
            </section>
          )}
        </div>
      )}
      {mobileGroup && groups.find((g) => g.key === mobileGroup) && (
        <Sheet
          title={mobileGroup}
          side="right"
          onClose={() => setMobileGroup(undefined)}
        >
          <GroupDetail
            group={groups.find((g) => g.key === mobileGroup)!}
            onOpen={(a) => {
              setMobileGroup(undefined);
              setSelected(a);
            }}
            bare
          />
        </Sheet>
      )}
      {adding && (
        <Sheet title="新增帳戶" onClose={() => setAdding(false)}>
          <AccountForm onDone={() => setAdding(false)} />
        </Sheet>
      )}
      {selected && (
        <AssetSheet item={selected} onClose={() => setSelected(undefined)} />
      )}
    </>
  );
}

function summary(g: Group) {
  const holdings = g.items.filter((i) => i.holding).length;
  const cards = g.items.filter((i) => i.debt).length;
  const accts = g.items.length - holdings - cards;
  const parts = [];
  if (accts) parts.push(`${accts} 帳戶`);
  if (cards) parts.push(`${cards} 卡片／貸款`);
  if (holdings) parts.push(`${holdings} 檔持倉`);
  if (g.foreign.length) parts.push(`含 ${g.foreign.join("、")}`);
  return parts.join(" · ");
}

function GroupDetail({
  group: g,
  onOpen,
  bare,
}: {
  group: Group;
  onOpen: (a: AssetRef) => void;
  bare?: boolean;
}) {
  const { run } = useAction();
  const { navigate } = useApp();
  const isTime = (i: Item) =>
    i.type === "sync" && i.asset.account_type === "time_deposit";
  const deposits = g.items.filter((i) => !i.debt && !i.holding && !isTime(i));
  const timeDeposits = g.items.flatMap((i) =>
    isTime(i) && i.type === "sync" ? [i.asset] : [],
  );
  const updated = g.items
    .flatMap((i) => (i.type === "sync" ? [i.asset.observed_at] : []))
    .sort()
    .at(-1);
  // 同步帳戶依實際銀行分組（集保交割帳戶分屬不同銀行）
  const banks = new Map<string, Item[]>();
  for (const i of deposits) {
    const bank = i.type === "sync" ? i.asset.institution || g.title : g.title;
    banks.set(bank, [...(banks.get(bank) ?? []), i]);
  }
  const byBank = banks.size > 1 || (banks.size === 1 && !banks.has(g.title));
  const debts = g.items.filter((i) => i.debt);
  const holdings = g.items.filter((i) => i.holding);
  const holdingItem =
    holdings[0]?.type === "sync" ? holdings[0].asset : undefined;
  const toggle = (a: SourceAsset, v: boolean) =>
    run(`/source-assets/${a.id}`, {
      method: "PATCH",
      body: { included: v },
      success: v ? `「${a.name}」已計入淨資產` : `「${a.name}」不再計入淨資產`,
      toastError: true,
    });
  return (
    <>
      {!bare && (
        <header className="ledger-detail-head">
          <GroupLogo g={g} size={44} />
          <span className="dim small">金融機構</span>
          <h2>{g.title}</h2>
          <p className="dim small">
            {g.synced && g.manual
              ? "包含手動與自動同步的帳戶"
              : g.synced
                ? "資料由自動同步帶入"
                : "手動管理的帳戶"}
          </p>
        </header>
      )}
      {updated && (
        <p className="sync-fresh">
          <RefreshCw size={13} />
          資料更新於 {stampFull(updated)}（{relative(updated)}）
          <button
            type="button"
            className="link"
            onClick={() => navigate("sources")}
          >
            同步設定
          </button>
        </p>
      )}
      <div className="detail-stats">
        <div>
          <span>資產</span>
          <Money value={g.assets.toString()} className="lg asset" />
        </div>
        <div>
          <span>負債</span>
          {g.debt.gt(0) ? (
            <Money value={g.debt.negated().toString()} className="lg bad" />
          ) : (
            <span className="lg dim">—</span>
          )}
        </div>
      </div>
      {g.synced && g.manual && (
        <div className="callout warn slim">
          <CircleAlert size={16} />
          <span>
            這家機構同時有手動與同步帳戶。如果是同一個帳戶，請關閉同步項目的「計入淨資產」，避免重複計算。
          </span>
        </div>
      )}
      {deposits.length > 0 &&
        (byBank ? (
          <div className="detail-section">
            <header>
              <strong>
                {g.title === "集保 e 存摺" ? "交割帳戶" : "銀行帳戶"}
              </strong>
              <span className="dim small">
                {banks.size} 家銀行 · {deposits.length} 個帳戶
              </span>
            </header>
            <div className="bank-blocks">
              {[...banks.entries()].map(([bank, items]) => (
                <BankBlock
                  key={bank}
                  bank={bank}
                  items={items}
                  onOpen={onOpen}
                  onToggle={toggle}
                />
              ))}
            </div>
          </div>
        ) : (
          <Section title="銀行帳戶與現金" count={`${deposits.length} 個帳戶`}>
            {deposits.map((i) => (
              <ItemRow key={i.key} item={i} onOpen={onOpen} onToggle={toggle} />
            ))}
          </Section>
        ))}
      {timeDeposits.length > 0 && (
        <TimeDeposits items={timeDeposits} onOpen={onOpen} />
      )}
      {debts.length > 0 && (
        <Section title="信用卡與貸款" count={`${debts.length} 筆`}>
          {debts.map((i) => (
            <ItemRow key={i.key} item={i} onOpen={onOpen} onToggle={toggle} />
          ))}
        </Section>
      )}
      {holdings.length > 0 && (
        <Section
          title="持股"
          count={`${holdings.length} 檔`}
          action={
            holdingItem && (
              <Writable>
                <label className="switch-line">
                  計入淨資產
                  <Switch
                    checked={holdingItem.included}
                    label={`${g.title}持倉計入淨資產`}
                    onChange={(v) => toggle(holdingItem, v)}
                  />
                </label>
              </Writable>
            )
          }
        >
          {holdings.map((i) => (
            <ItemRow key={i.key} item={i} onOpen={onOpen} onToggle={toggle} />
          ))}
        </Section>
      )}
    </>
  );
}

/** 定期存款：本金、利率、期別與到期日 */
function TimeDeposits({
  items,
  onOpen,
}: {
  items: SourceAsset[];
  onOpen: (a: AssetRef) => void;
}) {
  const total = items
    .filter((a) => a.included && a.currency === "TWD" && a.amount != null)
    .reduce((s, a) => s.plus(a.amount!), new Decimal(0));
  const todayMs = Date.now();
  return (
    <div className="detail-section">
      <header>
        <strong>定期存款</strong>
        <span className="dim small">
          {items.length} 筆 ·{" "}
          {items.every((a) => a.reference_only) ? (
            "僅供參考，不計入淨資產"
          ) : (
            <>
              合計 <Money value={total.toString()} />
            </>
          )}
        </span>
      </header>
      <ul className="rows td-list">
        {items.map((a) => {
          const due = a.maturity_on?.slice(0, 10);
          const days = due
            ? Math.ceil((Date.parse(due) - todayMs) / 86400000)
            : null;
          const rate =
            a.interest_rate != null
              ? `年利率 ${new Decimal(a.interest_rate).times(100).toDecimalPlaces(3).toString()}%`
              : null;
          return (
            <li key={a.id}>
              <button
                type="button"
                className={`row ${a.included ? "" : "excluded"}`}
                onClick={() => onOpen({ type: "sync", asset: a })}
              >
                <BankLogo names={[a.institution]} size={40} />
                <span className="row-main">
                  <span className="row-title">
                    <span className="row-text">
                      {a.institution ?? "定期存款"}
                    </span>
                    <Badge tone="info">定存</Badge>
                    {!a.included && (
                      <Badge>
                        {a.reference_only ? "集保參考帳戶" : "未計入"}
                      </Badge>
                    )}
                    {a.linked_account && <Badge tone="info">已連結</Badge>}
                  </span>
                  <span className="row-meta">
                    {[a.mask, rate, a.term && `${a.term}期`]
                      .filter(Boolean)
                      .join(" · ")}
                  </span>
                  {due && (
                    <span className="row-meta">
                      {a.opened_on &&
                        `${a.opened_on.slice(0, 10).replaceAll("-", "/")} 起 · `}
                      到期 {due.replaceAll("-", "/")}
                      {days != null &&
                        (days > 0 ? `（剩 ${days} 天）` : "（已到期）")}
                    </span>
                  )}
                </span>
                <Money
                  value={a.amount}
                  currency={a.currency}
                  className="row-amount"
                />
                <ChevronRight size={16} className="row-chevron" />
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function BankBlock({
  bank,
  items,
  onOpen,
  onToggle,
}: {
  bank: string;
  items: Item[];
  onOpen: (a: AssetRef) => void;
  onToggle: (a: SourceAsset, v: boolean) => void;
}) {
  let total = new Decimal(0);
  const foreign = new Set<string>();
  for (const i of items) {
    const [amount, currency, counted] =
      i.type === "sync"
        ? [i.asset.amount, i.asset.currency, i.asset.included]
        : [i.account.balance, i.account.currency, true];
    if (currency !== "TWD") foreign.add(currency);
    else if (counted && amount != null) total = total.plus(amount);
  }
  return (
    <section className="bank-block">
      <header>
        <BankLogo names={[bank]} size={30} />
        <span className="bank-name">
          <strong>{bank}</strong>
          <small>
            {items.length} 個帳戶
            {foreign.size > 0 && ` · 含 ${[...foreign].join("、")}`}
          </small>
        </span>
        {items.every((i) => i.type === "sync" && i.asset.reference_only) ? (
          <span className="dim small">不計入淨資產</span>
        ) : (
          <Money value={total.toString()} className="bank-total" />
        )}
      </header>
      <ul className="rows">
        {items.map((i) => (
          <ItemRow key={i.key} item={i} onOpen={onOpen} onToggle={onToggle} />
        ))}
      </ul>
    </section>
  );
}

function Section({
  title,
  count,
  action,
  children,
}: {
  title: string;
  count: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="detail-section">
      <header>
        <strong>{title}</strong>
        {action ?? <span className="dim small">{count}</span>}
      </header>
      <ul className="rows">{children}</ul>
    </div>
  );
}

/** 外幣金額依即期買入匯率換算的台幣；台幣或無匯率時回傳 undefined */
function useTwd(currency: string, amount: string | null | undefined) {
  const foreign = currency !== "TWD" && amount != null && !new Decimal(amount).isZero();
  const fx = useFx(foreign);
  const rate = fx.data?.rates.find((r) => r.currency === currency)?.buy;
  if (!foreign || !rate) return undefined;
  return new Decimal(amount).mul(rate).abs().toDecimalPlaces(0).toString();
}

function ItemRow({
  item,
  onOpen,
  onToggle,
}: {
  item: Item;
  onOpen: (a: AssetRef) => void;
  onToggle?: (a: SourceAsset, v: boolean) => void;
}) {
  const [amount, currency] =
    item.type === "manual"
      ? [item.account.balance, item.account.currency]
      : [item.asset.amount, item.asset.currency];
  const twd = useTwd(currency, amount);
  if (item.type === "manual") {
    const a = item.account;
    const Icon = kindIcons[a.kind] ?? Wallet;
    return (
      <li>
        <button
          type="button"
          className="row"
          onClick={() => onOpen(refOf(item))}
        >
          <BankLogo
            names={[a.institution]}
            size={40}
            fallback={Icon}
            tone={kindTones[a.kind]}
          />
          <span className="row-main">
            <span className="row-title">
              <span className="row-text">{a.name}</span>
            </span>
            <span className="row-meta">
              {kinds[a.kind]} · {a.currency} · 手動
              {twd && <> · ≈ <Money value={twd} /></>}
            </span>
          </span>
          <Money
            value={a.balance}
            currency={a.currency}
            className="row-amount"
          />
          <ChevronRight size={16} className="row-chevron" />
        </button>
      </li>
    );
  }
  const s = item.asset;
  const meta = item.holding
    ? [s.symbol, s.quantity && `${quantity(s.quantity)} ${s.asset_type === "fund" ? "單位" : "股"}`].filter(Boolean)
    : [
        s.mask,
        accountTypes[s.account_type ?? ""],
        s.currency !== "TWD" && s.currency,
        item.debt && s.balance_from === "bill" && "以最新帳單計",
        item.debt &&
          s.payment_due &&
          `繳款 ${shortDate(s.payment_due.slice(0, 10))}`,
      ].filter(Boolean);
  return (
    <li>
      <button
        type="button"
        className={`row ${s.included ? "" : "excluded"}`}
        onClick={() => onOpen(refOf(item))}
      >
        {item.holding ? (
          <HoldingLogo name={s.name} size={40} />
        ) : (
          <BankLogo
            names={[s.institution, sourceNames[s.connector]]}
            size={40}
          />
        )}
        <span className="row-main">
          <span className="row-title">
            <span className="row-text">{s.name}</span>
            {s.account_type === "time_deposit" && (
              <Badge tone="info">定存</Badge>
            )}
            {!s.included && (
              <Badge>{s.reference_only ? "集保參考帳戶" : "未計入"}</Badge>
            )}
            {s.linked_account && <Badge tone="info">已連結</Badge>}
          </span>
          <span className="row-meta">
            {meta.join(" · ")}
            {twd && <> · ≈ <Money value={twd} /></>}
          </span>
        </span>
        <Money
          value={
            s.amount == null
              ? null
              : item.debt
                ? new Decimal(s.amount).abs().negated().toString()
                : s.amount
          }
          currency={s.currency}
          className={`row-amount ${item.debt ? "bad" : ""}`}
        />
        <ChevronRight size={16} className="row-chevron" />
      </button>
    </li>
  );
}
